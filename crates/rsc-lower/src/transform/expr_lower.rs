//! Expression lowering.
//!
//! Transforms `RustScript` AST expressions into Rust IR expressions. Handles
//! literals, identifiers, binary/unary operators, function calls, method calls,
//! closures, struct literals, template literals, `new` expressions, `await`,
//! optional chaining, nullish coalescing, and more.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{
    ParamMode, RustClosureBody, RustClosureParam, RustExpr, RustExprKind, RustType, SpreadOp,
};

use crate::context::LoweringContext;
use crate::ownership::{self, UseMap};

use super::{
    Transform, capitalize_first, detect_compound_assign, element_type_is_copy, extract_named_type,
    lower_binary_op, lower_unary_op,
};

impl Transform {
    /// Lower an expression.
    #[allow(clippy::too_many_lines)]
    // Expression lowering covers all AST expression kinds; splitting would obscure the match
    pub(super) fn lower_expr(
        &self,
        expr: &ast::Expr,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        match &expr.kind {
            ast::ExprKind::IntLit(v) => RustExpr::new(RustExprKind::IntLit(*v), expr.span),
            ast::ExprKind::FloatLit(v) => RustExpr::new(RustExprKind::FloatLit(*v), expr.span),
            ast::ExprKind::StringLit(s) => {
                // In Rust, string literals are &str. RustScript's `string` type is
                // String (owned). Wrap in .to_string() so the expression produces
                // an owned String. The exception is when this literal ends up inside
                // a println! format position — but that's handled by the builtin
                // registry which constructs its own StringLit for the format string.
                let lit = RustExpr::new(RustExprKind::StringLit(s.clone()), expr.span);
                RustExpr::synthetic(RustExprKind::ToString(Box::new(lit)))
            }
            ast::ExprKind::BoolLit(v) => RustExpr::new(RustExprKind::BoolLit(*v), expr.span),
            ast::ExprKind::Ident(ident) => {
                Self::lower_ident_ref(ident, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Binary(bin) => {
                // Special handling for exponentiation: `a ** b` → `a.pow(b as u32)` or `a.powf(b)`
                if bin.op == ast::BinaryOp::Pow {
                    return self.lower_pow_expr(
                        &bin.left, &bin.right, expr.span, ctx, use_map, stmt_index,
                    );
                }

                let left = self.lower_expr(&bin.left, ctx, use_map, stmt_index);
                let right = self.lower_expr(&bin.right, ctx, use_map, stmt_index);
                let op = lower_binary_op(bin.op);
                RustExpr::new(
                    RustExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::Unary(un) => {
                let operand = self.lower_expr(&un.operand, ctx, use_map, stmt_index);
                let op = lower_unary_op(un.op);
                RustExpr::new(
                    RustExprKind::Unary {
                        op,
                        operand: Box::new(operand),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::Call(call) => {
                self.lower_call_expr(call, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::MethodCall(mc) => {
                self.lower_method_call(mc, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Paren(inner) => {
                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                RustExpr::new(RustExprKind::Paren(Box::new(lowered)), expr.span)
            }
            ast::ExprKind::Assign(assign) => {
                // Detect compound assignment pattern: x = x op rhs
                if let Some((compound_op, rhs)) =
                    detect_compound_assign(&assign.target.name, &assign.value)
                {
                    let lowered_rhs = self.lower_expr(rhs, ctx, use_map, stmt_index);
                    return RustExpr::new(
                        RustExprKind::CompoundAssign {
                            target: assign.target.name.clone(),
                            op: compound_op,
                            value: Box::new(lowered_rhs),
                        },
                        expr.span,
                    );
                }
                let value = self.lower_expr(&assign.value, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::Assign {
                        target: assign.target.name.clone(),
                        value: Box::new(value),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::StructLit(slit) => {
                self.lower_struct_lit(slit, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::FieldAccess(fa) => {
                // Strip `#` prefix from hash-private field access
                let field_name = fa.field.name.trim_start_matches('#');

                // `this.field` or `this.#field` → `self.field`
                if matches!(fa.object.kind, ast::ExprKind::This) {
                    return RustExpr::new(
                        RustExprKind::SelfFieldAccess {
                            field: field_name.to_owned(),
                        },
                        expr.span,
                    );
                }

                // External `obj.#field` access — emit diagnostic
                if fa.field.name.starts_with('#') {
                    ctx.emit_diagnostic(Diagnostic::error(format!(
                        "cannot access private field `{}`",
                        fa.field.name
                    )));
                }

                // Math.PI / Math.E → std::f64::consts::PI / E
                if let ast::ExprKind::Ident(obj_ident) = &fa.object.kind
                    && let Some(constant_expr) =
                        crate::builtins::lower_math_constant(&obj_ident.name, field_name)
                {
                    return constant_expr;
                }

                // `.length` / `.size` on strings/arrays/maps/sets → `.len() as i64`
                // The cast to i64 matches RustScript's default numeric type and avoids
                // type mismatches when assigning to numeric fields (usize vs i64/u32).
                // `.size` is the Map/Set equivalent of `.length`.
                if field_name == "length" || field_name == "size" {
                    let object = self.lower_expr(&fa.object, ctx, use_map, stmt_index);
                    let len_call = RustExpr::new(
                        RustExprKind::MethodCall {
                            receiver: Box::new(object),
                            method: "len".into(),
                            type_args: vec![],
                            args: vec![],
                        },
                        expr.span,
                    );
                    return RustExpr::new(
                        RustExprKind::Cast(Box::new(len_call), RustType::I64),
                        expr.span,
                    );
                }

                let object = self.lower_expr(&fa.object, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::FieldAccess {
                        object: Box::new(object),
                        field: field_name.to_owned(),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::TemplateLit(tpl) => {
                self.lower_template_lit(tpl, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::ArrayLit(elements) => {
                self.lower_array_lit(elements, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::New(new_expr) => {
                self.lower_new_expr(new_expr, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Index(index_expr) => {
                // Check if the object is a tuple-typed variable with a literal index.
                // If so, emit tuple field access (e.g., `pair.0`) instead of index (`pair[0]`).
                if let ast::ExprKind::Ident(ident) = &index_expr.object.kind
                    && ctx
                        .lookup_variable(&ident.name)
                        .is_some_and(|info| matches!(info.ty, RustType::Tuple(_)))
                    && let ast::ExprKind::IntLit(n) = &index_expr.index.kind
                {
                    let object = self.lower_expr(&index_expr.object, ctx, use_map, stmt_index);
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    // Tuple indices from RustScript source are always small non-negative literals
                    let idx = (*n).max(0) as usize;
                    return RustExpr::new(
                        RustExprKind::TupleField {
                            object: Box::new(object),
                            index: idx,
                        },
                        expr.span,
                    );
                }

                let object = self.lower_expr(&index_expr.object, ctx, use_map, stmt_index);
                let index = self.lower_expr(&index_expr.index, ctx, use_map, stmt_index);

                // Rust arrays/Vecs require usize for indexing. When the index
                // is a non-literal expression (e.g., a variable of type i64),
                // wrap it in `as usize` to satisfy the type requirement.
                // Literal integers are left as-is since Rust infers usize.
                let index = if matches!(index.kind, RustExprKind::IntLit(_)) {
                    index
                } else {
                    RustExpr::synthetic(RustExprKind::Cast(
                        Box::new(index),
                        RustType::Named("usize".to_owned()),
                    ))
                };

                RustExpr::new(
                    RustExprKind::Index {
                        object: Box::new(object),
                        index: Box::new(index),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::NullLit => RustExpr::new(RustExprKind::None, expr.span),
            ast::ExprKind::OptionalChain(chain) => {
                let object = self.lower_expr(&chain.object, ctx, use_map, stmt_index);
                match &chain.access {
                    ast::OptionalAccess::Field(field) => RustExpr::new(
                        RustExprKind::OptionMap {
                            expr: Box::new(object),
                            closure_param: "v".to_owned(),
                            closure_body: Box::new(RustExpr::synthetic(
                                RustExprKind::FieldAccess {
                                    object: Box::new(RustExpr::synthetic(RustExprKind::Ident(
                                        "v".to_owned(),
                                    ))),
                                    field: field.name.clone(),
                                },
                            )),
                        },
                        expr.span,
                    ),
                    ast::OptionalAccess::Method(method, args) => {
                        let lowered_args: Vec<RustExpr> = args
                            .iter()
                            .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                            .collect();
                        RustExpr::new(
                            RustExprKind::OptionMap {
                                expr: Box::new(object),
                                closure_param: "v".to_owned(),
                                closure_body: Box::new(RustExpr::synthetic(
                                    RustExprKind::MethodCall {
                                        receiver: Box::new(RustExpr::synthetic(
                                            RustExprKind::Ident("v".to_owned()),
                                        )),
                                        method: method.name.clone(),
                                        type_args: vec![],
                                        args: lowered_args,
                                    },
                                )),
                            },
                            expr.span,
                        )
                    }
                }
            }
            ast::ExprKind::NullishCoalescing(nc) => {
                let left = self.lower_expr(&nc.left, ctx, use_map, stmt_index);
                let right = self.lower_expr(&nc.right, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::UnwrapOr {
                        expr: Box::new(left),
                        default: Box::new(right),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::Throw(value) => {
                let lowered = self.lower_expr(value, ctx, use_map, stmt_index);
                RustExpr::synthetic(RustExprKind::Err(Box::new(lowered)))
            }
            ast::ExprKind::Closure(closure) => {
                self.lower_closure(closure, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Await(inner) => {
                // Check for `await Promise.XXX([...])` patterns.
                if let ast::ExprKind::MethodCall(mc) = &inner.kind
                    && let ast::ExprKind::Ident(obj) = &mc.object.kind
                    && obj.name == "Promise"
                    && mc.args.len() == 1
                    && let ast::ExprKind::ArrayLit(elements) = &mc.args[0].kind
                {
                    let lowered_elements: Vec<RustExpr> = elements
                        .iter()
                        .filter_map(|e| match e {
                            ast::ArrayElement::Expr(e) => {
                                Some(self.lower_expr(e, ctx, use_map, stmt_index))
                            }
                            ast::ArrayElement::Spread(_) => None,
                        })
                        .collect();

                    match mc.method.name.as_str() {
                        // `await Promise.all([...])` → `tokio::join!(...)`
                        "all" => {
                            return RustExpr::new(
                                RustExprKind::TokioJoin(lowered_elements),
                                expr.span,
                            );
                        }
                        // `await Promise.race([...])` → `tokio::select! { ... }`
                        "race" => {
                            return RustExpr::new(
                                RustExprKind::TokioSelect(lowered_elements),
                                expr.span,
                            );
                        }
                        // `await Promise.any([...])` → `futures::future::select_ok(...)`
                        "any" => {
                            return RustExpr::new(
                                RustExprKind::FuturesSelectOk(lowered_elements),
                                expr.span,
                            );
                        }
                        _ => {}
                    }
                }

                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                // If the inner expression already has `?` (known throws function),
                // the call is already unwrapped — just await it without extra `?`.
                let inner_already_has_question_mark =
                    matches!(&lowered.kind, RustExprKind::QuestionMark(_));
                let await_expr = RustExpr::new(RustExprKind::Await(Box::new(lowered)), expr.span);
                // When inside a `throws` function and the awaited call is to an
                // unknown (external) function, add `?` after `.await` to propagate
                // the Result. Most Rust async APIs return Result, so this is the
                // correct default for external calls.
                if ctx.is_fn_throws() && !inner_already_has_question_mark {
                    RustExpr::new(RustExprKind::QuestionMark(Box::new(await_expr)), expr.span)
                } else {
                    await_expr
                }
            }
            ast::ExprKind::This => RustExpr::new(RustExprKind::SelfRef, expr.span),
            ast::ExprKind::FieldAssign(fa) => {
                let assign_field_name = fa.field.name.trim_start_matches('#').to_owned();
                // Check if this is `this.field = value` → `self.field = value`
                if matches!(fa.object.kind, ast::ExprKind::This) {
                    let value = self.lower_expr(&fa.value, ctx, use_map, stmt_index);
                    RustExpr::new(
                        RustExprKind::SelfFieldAssign {
                            field: assign_field_name,
                            value: Box::new(value),
                        },
                        expr.span,
                    )
                } else {
                    // General field assignment: lower the value only.
                    // Non-`this` field assignments lower to simple assignment.
                    let value = self.lower_expr(&fa.value, ctx, use_map, stmt_index);
                    RustExpr::new(
                        RustExprKind::Assign {
                            target: fa.field.name.clone(),
                            value: Box::new(value),
                        },
                        expr.span,
                    )
                }
            }
            ast::ExprKind::Shared(inner) => {
                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                RustExpr::new(RustExprKind::ArcMutexNew(Box::new(lowered)), expr.span)
            }
            ast::ExprKind::SpreadArg(inner) => {
                // SpreadArg is handled at the call-site level. If we reach here,
                // it's a spread used outside a function call context — just lower
                // the inner expression.
                self.lower_expr(inner, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Ternary(condition, consequent, alternate) => {
                let cond = self.lower_expr(condition, ctx, use_map, stmt_index);
                let then_expr = self.lower_expr(consequent, ctx, use_map, stmt_index);
                let else_expr = self.lower_expr(alternate, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::IfExpr {
                        condition: Box::new(cond),
                        then_expr: Box::new(then_expr),
                        else_expr: Box::new(else_expr),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::NonNullAssert(inner) => {
                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::MethodCall {
                        receiver: Box::new(lowered),
                        method: "unwrap".to_owned(),
                        type_args: vec![],
                        args: vec![],
                    },
                    expr.span,
                )
            }
            ast::ExprKind::Cast(inner, type_ann) => {
                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                let mut diags = Vec::new();
                let ty =
                    rsc_typeck::resolve::resolve_type_annotation_to_rust_type(type_ann, &mut diags);
                for d in diags {
                    ctx.emit_diagnostic(d);
                }
                RustExpr::new(RustExprKind::Cast(Box::new(lowered), ty), expr.span)
            }
            ast::ExprKind::TypeOf(inner) => {
                // Resolve typeof statically based on the expression's literal type.
                let type_str = match &inner.kind {
                    ast::ExprKind::IntLit(_) | ast::ExprKind::FloatLit(_) => "number",
                    ast::ExprKind::StringLit(_) | ast::ExprKind::TemplateLit(_) => "string",
                    ast::ExprKind::BoolLit(_) => "boolean",
                    _ => "object",
                };
                let lit = RustExpr::new(RustExprKind::StringLit(type_str.to_owned()), expr.span);
                RustExpr::synthetic(RustExprKind::ToString(Box::new(lit)))
            }
            ast::ExprKind::LogicalAssign(la) => {
                // Logical assignments are primarily handled at statement level.
                // In expression context, lower the value as a fallback — the
                // statement lowering path intercepts these before reaching here.
                ctx.emit_diagnostic(Diagnostic::error(
                    "logical assignment operators (??=, ||=, &&=) can only be used as statements",
                ));
                self.lower_expr(&la.value, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Satisfies(inner, _) => {
                // `satisfies` is a compile-time assertion — strip it entirely
                self.lower_expr(inner, ctx, use_map, stmt_index)
            }
        }
    }

    /// Lower a function call expression, handling optional, default, and rest parameters.
    ///
    /// When the callee has:
    /// - **Optional parameters**: missing args are filled with `None`.
    /// - **Default parameters**: missing args are filled with the default value.
    /// - **Rest parameters**: excess positional args are collected into `vec![...]`.
    /// - **Spread arguments**: `...arr` passes the array directly to a rest param.
    #[allow(clippy::too_many_lines)]
    // Call-site lowering handles enum resolution, borrow transforms, optional/default/rest
    // param filling, and throws wrapping — splitting would fragment the coherent logic.
    fn lower_call_expr(
        &self,
        call: &ast::CallExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Check for builtin free functions (e.g., `spawn`)
        if let Some(lowering_fn) = self.builtins.lookup_function(&call.callee.name).copied() {
            let lowered_args: Vec<RustExpr> = call
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return lowering_fn(lowered_args, span);
        }

        let sig = self.fn_signatures.get(&call.callee.name);
        let callee_modes = sig.and_then(|s| s.param_modes.as_ref());

        // Determine the number of non-rest parameters
        let non_rest_count = sig.map_or(0, |s| {
            if s.has_rest_param {
                s.param_count.saturating_sub(1)
            } else {
                s.param_count
            }
        });

        let has_rest = sig.is_some_and(|s| s.has_rest_param);
        let supplied_count = call.args.len();

        // Lower supplied arguments
        let mut args: Vec<RustExpr> = Vec::new();

        if has_rest {
            // Lower non-rest arguments normally
            for (i, a) in call.args.iter().enumerate() {
                if i < non_rest_count {
                    args.push(self.lower_single_arg(
                        a,
                        i,
                        sig,
                        callee_modes,
                        ctx,
                        use_map,
                        stmt_index,
                    ));
                }
            }

            // Fill in any missing optional/default non-rest args
            if let Some(s) = sig {
                for i in supplied_count.min(non_rest_count)..non_rest_count {
                    if let Some(default_expr) = s.default_values.get(i).and_then(|d| d.as_ref()) {
                        args.push(default_expr.clone());
                    } else if s.optional_params.get(i).copied().unwrap_or(false) {
                        args.push(RustExpr::synthetic(RustExprKind::None));
                    }
                }
            }

            // Collect rest arguments into a vec![]
            if supplied_count > non_rest_count {
                // Check if the only rest arg is a SpreadArg — pass directly
                let rest_args: Vec<&ast::Expr> = call.args[non_rest_count..].iter().collect();
                if rest_args.len() == 1 {
                    if let ast::ExprKind::SpreadArg(inner) = &rest_args[0].kind {
                        args.push(self.lower_expr(inner, ctx, use_map, stmt_index));
                    } else {
                        let vec_elements: Vec<RustExpr> = rest_args
                            .iter()
                            .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                            .collect();
                        args.push(RustExpr::synthetic(RustExprKind::VecLit(vec_elements)));
                    }
                } else {
                    let vec_elements: Vec<RustExpr> = rest_args
                        .iter()
                        .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                        .collect();
                    args.push(RustExpr::synthetic(RustExprKind::VecLit(vec_elements)));
                }
            } else {
                // No rest args supplied — pass empty vec
                args.push(RustExpr::synthetic(RustExprKind::VecLit(Vec::new())));
            }
        } else {
            // No rest param: lower args normally with enum/borrow transforms
            for (i, a) in call.args.iter().enumerate() {
                args.push(self.lower_single_arg(a, i, sig, callee_modes, ctx, use_map, stmt_index));
            }

            // Fill in missing optional/default args
            if let Some(s) = sig {
                for i in supplied_count..s.param_count {
                    if let Some(default_expr) = s.default_values.get(i).and_then(|d| d.as_ref()) {
                        args.push(default_expr.clone());
                    } else if s.optional_params.get(i).copied().unwrap_or(false) {
                        args.push(RustExpr::synthetic(RustExprKind::None));
                    }
                }
            }
        }

        let call_expr = RustExpr::new(
            RustExprKind::Call {
                func: call.callee.name.clone(),
                args,
            },
            span,
        );
        // If the callee is a throws function and we're inside a throws function,
        // wrap with `?` operator.
        let callee_throws = sig.is_some_and(|sig| sig.throws);
        if callee_throws && ctx.is_fn_throws() {
            RustExpr::new(RustExprKind::QuestionMark(Box::new(call_expr)), span)
        } else {
            call_expr
        }
    }

    /// Lower a single argument at a call site, handling enum resolution and borrow transforms.
    #[allow(clippy::too_many_arguments)]
    // Parameters mirror the call-site context needed for enum resolution and borrow transforms
    fn lower_single_arg(
        &self,
        a: &ast::Expr,
        i: usize,
        sig: Option<&super::FnSignature>,
        callee_modes: Option<&Vec<ParamMode>>,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Lower array literals as tuples when the parameter type is a tuple.
        if let ast::ExprKind::ArrayLit(elements) = &a.kind
            && let Some(param_ty) = sig.and_then(|s| s.param_types.get(i))
            && matches!(param_ty, RustType::Tuple(_))
        {
            let lowered: Vec<RustExpr> = elements
                .iter()
                .map(|e| match e {
                    ast::ArrayElement::Expr(expr) | ast::ArrayElement::Spread(expr) => {
                        self.lower_expr(expr, ctx, use_map, stmt_index)
                    }
                })
                .collect();
            return RustExpr::new(RustExprKind::Tuple(lowered), a.span);
        }

        // Resolve string literals as enum variants when the parameter type is an enum
        if let ast::ExprKind::StringLit(s) = &a.kind
            && let Some(param_ty) = sig.and_then(|s| s.param_types.get(i))
            && let Some(type_name) = extract_named_type(param_ty)
            && let Some(td) = self.type_registry.lookup(&type_name)
            && matches!(
                &td.kind,
                rsc_typeck::registry::TypeDefKind::SimpleEnum(_)
                    | rsc_typeck::registry::TypeDefKind::DataEnum(_)
            )
        {
            return RustExpr::new(
                RustExprKind::EnumVariant {
                    enum_name: type_name,
                    variant_name: capitalize_first(s),
                },
                a.span,
            );
        }
        // When the parameter is a union type, wrap the argument with `.into()`
        // so the From impl converts it to the generated union enum.
        if let Some(param_ty) = sig.and_then(|s| s.param_types.get(i))
            && matches!(param_ty, RustType::GeneratedUnion { .. })
        {
            let lowered = self.lower_expr(a, ctx, use_map, stmt_index);
            return RustExpr::synthetic(RustExprKind::MethodCall {
                receiver: Box::new(lowered),
                method: "into".to_owned(),
                type_args: vec![],
                args: vec![],
            });
        }

        let lowered = self.lower_expr(a, ctx, use_map, stmt_index);

        // Tier 2: apply callsite borrow transform
        if let Some(modes) = callee_modes
            && let Some(mode) = modes.get(i)
        {
            match mode {
                ParamMode::BorrowedStr => {
                    // String literal → &str: unwrap .to_string() wrapper
                    if let RustExprKind::ToString(inner) = &lowered.kind
                        && matches!(inner.kind, RustExprKind::StringLit(_))
                    {
                        return *inner.clone();
                    }
                    // Variable → &var (auto-deref handles &String → &str)
                    if !matches!(lowered.kind, RustExprKind::Borrow(_)) {
                        return RustExpr::synthetic(RustExprKind::Borrow(Box::new(lowered)));
                    }
                }
                ParamMode::Borrowed => {
                    // Wrap in & unless already a borrow
                    if !matches!(lowered.kind, RustExprKind::Borrow(_)) {
                        return RustExpr::synthetic(RustExprKind::Borrow(Box::new(lowered)));
                    }
                }
                ParamMode::Owned => {
                    // When the argument is a reference variable (e.g., an
                    // iterator closure param or for-of loop variable) and the
                    // callee expects an owned value, clone to convert &T → T.
                    if let RustExprKind::Ident(name) = &lowered.kind
                        && ctx.is_reference_variable(name)
                    {
                        return RustExpr::synthetic(RustExprKind::Clone(Box::new(lowered)));
                    }
                }
            }
        }

        // Even without callee mode info, clone reference variables passed
        // to function calls since the default is Owned.
        if let RustExprKind::Ident(name) = &lowered.kind
            && ctx.is_reference_variable(name)
            && callee_modes.is_none()
        {
            return RustExpr::synthetic(RustExprKind::Clone(Box::new(lowered)));
        }

        // For external function calls (no signature info), pass string
        // literals as &str rather than String. Most Rust APIs expect &str.
        if callee_modes.is_none()
            && let RustExprKind::ToString(inner) = &lowered.kind
            && matches!(inner.kind, RustExprKind::StringLit(_))
        {
            return *inner.clone();
        }

        lowered
    }

    /// Lower a closure / arrow function expression.
    ///
    /// Maps `(x: i32): i32 => x * 2` to `|x: i32| -> i32 { x * 2 }`.
    fn lower_closure(
        &self,
        closure: &ast::ClosureExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let mut diags = Vec::new();

        // Lower parameters
        let params: Vec<RustClosureParam> = closure
            .params
            .iter()
            .map(|p| {
                // Inferred types (from omitted annotations) produce ty: None
                if matches!(p.type_ann.kind, ast::TypeKind::Inferred) {
                    return RustClosureParam {
                        name: p.name.name.clone(),
                        ty: None,
                    };
                }
                let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                    &p.type_ann,
                    &self.type_registry,
                    &[],
                    &mut diags,
                );
                let rust_ty = rsc_typeck::bridge::type_to_rust_type(&ty);
                RustClosureParam {
                    name: p.name.name.clone(),
                    ty: Some(rust_ty),
                }
            })
            .collect();

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        // Lower return type
        let return_type = closure.return_type.as_ref().map(|rt| {
            let mut diags = Vec::new();
            let ty = rsc_typeck::resolve::resolve_type_annotation_with_generics(
                rt,
                &self.type_registry,
                &[],
                &mut diags,
            );
            for d in diags {
                ctx.emit_diagnostic(d);
            }
            rsc_typeck::bridge::type_to_rust_type(&ty)
        });

        // Lower body
        let body = match &closure.body {
            ast::ClosureBody::Expr(expr) => {
                let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
                RustClosureBody::Expr(Box::new(lowered))
            }
            ast::ClosureBody::Block(block) => {
                // Use an empty reassigned set for closure bodies — they are opaque
                let reassigned = std::collections::HashSet::new();
                let lowered = self.lower_block(block, ctx, use_map, 0, &reassigned);
                RustClosureBody::Block(lowered)
            }
        };

        RustExpr::new(
            RustExprKind::Closure {
                is_async: closure.is_async,
                is_move: closure.is_move,
                params,
                return_type,
                body,
            },
            span,
        )
    }

    /// Lower exponentiation: `a ** b`.
    ///
    /// For integer bases: `a.pow(b as u32)`.
    /// For float bases: `a.powf(b)`.
    /// When the base type is ambiguous, defaults to `.pow(b as u32)`.
    fn lower_pow_expr(
        &self,
        left: &ast::Expr,
        right: &ast::Expr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let base = self.lower_expr(left, ctx, use_map, stmt_index);
        let exp = self.lower_expr(right, ctx, use_map, stmt_index);

        let is_float_literal = matches!(left.kind, ast::ExprKind::FloatLit(_));
        let is_float_var = if let ast::ExprKind::Ident(ident) = &left.kind {
            ctx.lookup_variable(&ident.name)
                .is_some_and(|info| matches!(info.ty, RustType::F32 | RustType::F64))
        } else {
            false
        };
        let is_float = is_float_literal || is_float_var;

        if is_float {
            // `a.powf(b)`
            RustExpr::new(
                RustExprKind::MethodCall {
                    receiver: Box::new(base),
                    method: "powf".to_owned(),
                    type_args: vec![],
                    args: vec![exp],
                },
                span,
            )
        } else {
            // `a.pow(b as u32)`
            let cast_exp = RustExpr::new(RustExprKind::Cast(Box::new(exp), RustType::U32), span);
            RustExpr::new(
                RustExprKind::MethodCall {
                    receiver: Box::new(base),
                    method: "pow".to_owned(),
                    type_args: vec![],
                    args: vec![cast_exp],
                },
                span,
            )
        }
    }

    /// Lower an identifier reference, inserting a clone if ownership analysis requires it.
    fn lower_ident_ref(
        ident: &ast::Ident,
        span: rsc_syntax::span::Span,
        ctx: &LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let base = RustExpr::new(RustExprKind::Ident(ident.name.clone()), span);

        // Check if clone is needed
        let var_type = ctx.lookup_variable(&ident.name).map(|info| info.ty.clone());

        if let Some(ty) = var_type
            && ownership::needs_clone(&ident.name, stmt_index, use_map, &ty)
        {
            return RustExpr::synthetic(RustExprKind::Clone(Box::new(base)));
        }

        base
    }

    /// Lower a method call expression.
    ///
    /// First checks if the method call matches a builtin. If so, lowers
    /// the arguments first then delegates to the builtin lowering function.
    /// Then checks static method calls on class names (`ClassName.method()`
    /// → `ClassName::method()`). Then checks string methods registered in
    /// the builtin registry.
    #[allow(clippy::too_many_lines)]
    // Method call lowering checks builtins, static methods, string methods,
    // collection methods, map/set, shared<T> sugar, and fallback; splitting would
    // fragment the dispatch logic.
    fn lower_method_call(
        &self,
        mc: &ast::MethodCallExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Try to match as a builtin: extract object name from Ident
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && let Some(lowering_fn) = self
                .builtins
                .lookup_method(&obj_ident.name, &mc.method.name)
        {
            // Lower arguments first, then pass to builtin
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return lowering_fn(lowered_args, span);
        }

        // Check for static method call: `ClassName.method()` → `ClassName::method()`
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && self
                .type_registry
                .has_static_method(&obj_ident.name, &mc.method.name)
        {
            let args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return RustExpr::new(
                RustExprKind::StaticCall {
                    type_name: obj_ident.name.clone(),
                    method: mc.method.name.clone(),
                    args,
                },
                span,
            );
        }

        // Check for Map/Set method: type-aware dispatch for methods like
        // get/set/has/delete/clear/keys/values/entries/add that would conflict
        // with user-defined class methods if dispatched by name alone.
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && let Some(var_info) = ctx.lookup_variable(&obj_ident.name)
            && matches!(
                &var_info.ty,
                RustType::Generic(base, _)
                    if matches!(base.as_ref(), RustType::Named(n) if n == "HashMap" || n == "HashSet")
            )
            && let Some(lowering_fn) = self.builtins.lookup_map_set_method(&mc.method.name)
        {
            let is_set = matches!(
                &var_info.ty,
                RustType::Generic(base, _)
                    if matches!(base.as_ref(), RustType::Named(n) if n == "HashSet")
            );
            let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            // HashSet.has() uses .contains() instead of HashMap's .contains_key()
            if is_set && mc.method.name == "has" {
                return crate::builtins::lower_set_has(receiver, lowered_args, span);
            }
            return lowering_fn(receiver, lowered_args, span);
        }

        // Check for string method: if the method name matches a registered
        // string method, lower via the string method lowering function.
        if let Some(lowering_fn) = self.builtins.lookup_string_method(&mc.method.name) {
            let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return lowering_fn(receiver, lowered_args, span);
        }

        // Check for collection method: if the method name matches a registered
        // collection method, lower to an iterator chain. If the receiver is
        // already an iterator chain (from a chained call like arr.map().filter()),
        // merge the outer operation into the existing chain.
        if self
            .builtins
            .lookup_collection_method(&mc.method.name)
            .is_some()
        {
            let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);

            // Mark iterator closure parameters as reference variables.
            // In `.iter()` chains, closures receive `&T`. When the closure body
            // uses the parameter in an owned context (e.g., passing to a function
            // that takes T), it needs to be cloned.
            let mut ref_params: Vec<String> = Vec::new();
            for a in &mc.args {
                if let ast::ExprKind::Closure(c) = &a.kind {
                    for p in &c.params {
                        // Only mark non-Copy types as references
                        let is_copy_param = if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
                            && let Some(var_info) = ctx.lookup_variable(&obj_ident.name)
                        {
                            element_type_is_copy(&var_info.ty)
                        } else {
                            false
                        };
                        if !is_copy_param {
                            ctx.mark_as_reference(p.name.name.clone());
                            ref_params.push(p.name.name.clone());
                        }
                    }
                }
            }

            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();

            // Unmark the closure parameters after lowering
            for name in &ref_params {
                ctx.unmark_reference(name);
            }

            // Try to merge into an existing chain first (for chained calls)
            if matches!(receiver.kind, RustExprKind::IteratorChain { .. })
                && let Some(merged) = crate::builtins::merge_into_chain(
                    receiver.clone(),
                    &mc.method.name,
                    &lowered_args,
                    span,
                )
            {
                return merged;
            }

            // Not a chain merge — create a new iterator chain via the lowering function.
            // The lookup is guaranteed to succeed since we checked `is_some()` above.
            if let Some(lowering_fn) = self.builtins.lookup_collection_method(&mc.method.name) {
                return lowering_fn(receiver, lowered_args, span);
            }
        }

        // Check for `.lock()` on shared<T> (Arc<Mutex<T>>) — auto-unwrap
        if mc.method.name == "lock"
            && mc.args.is_empty()
            && let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && let Some(var_info) = ctx.lookup_variable(&obj_ident.name)
            && matches!(var_info.ty, rsc_syntax::rust_ir::RustType::ArcMutex(_))
        {
            let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
            // .lock() → .lock().unwrap()
            let lock_call = RustExpr::new(
                RustExprKind::MethodCall {
                    receiver: Box::new(receiver),
                    method: "lock".to_owned(),
                    type_args: vec![],
                    args: vec![],
                },
                span,
            );
            return RustExpr::new(
                RustExprKind::MethodCall {
                    receiver: Box::new(lock_call),
                    method: "unwrap".to_owned(),
                    type_args: vec![],
                    args: vec![],
                },
                span,
            );
        }

        // Check for static method call on an imported or otherwise known type:
        // `TypeName.method()` → `TypeName::method()` when the receiver is not a variable.
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && self.is_type_name(&obj_ident.name, ctx)
        {
            let args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| {
                    let lowered = self.lower_expr(a, ctx, use_map, stmt_index);
                    // Strip .to_string() from string literal args for external calls
                    if let RustExprKind::ToString(inner) = &lowered.kind
                        && matches!(inner.kind, RustExprKind::StringLit(_))
                    {
                        return *inner.clone();
                    }
                    lowered
                })
                .collect();
            return RustExpr::new(
                RustExprKind::StaticCall {
                    type_name: obj_ident.name.clone(),
                    method: mc.method.name.clone(),
                    args,
                },
                span,
            );
        }

        // Not a builtin — lower as a regular method call.
        // For external method calls (no known signature), strip .to_string()
        // from string literal args since most Rust APIs expect &str.
        let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
        let args: Vec<RustExpr> = mc
            .args
            .iter()
            .map(|a| {
                let lowered = self.lower_expr(a, ctx, use_map, stmt_index);
                if let RustExprKind::ToString(inner) = &lowered.kind
                    && matches!(inner.kind, RustExprKind::StringLit(_))
                {
                    return *inner.clone();
                }
                lowered
            })
            .collect();

        RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(receiver),
                method: mc.method.name.clone(),
                type_args: vec![],
                args,
            },
            span,
        )
    }

    /// Lower a struct literal expression.
    ///
    /// If the struct literal has no explicit type name, attempts to resolve it
    /// from the surrounding variable declaration context. The lowering pass
    /// stores the current expected type when processing `VarDecl` with a
    /// struct literal initializer.
    ///
    /// Handles spread syntax: `{ ...base, field: value }` lowers to Rust
    /// struct update syntax `Type { field: value, ..base.clone() }`.
    fn lower_struct_lit(
        &self,
        slit: &ast::StructLitExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let type_name = slit
            .type_name
            .as_ref()
            .map(|n| n.name.clone())
            .or_else(|| ctx.current_struct_type_name().map(String::from))
            .unwrap_or_else(|| {
                ctx.emit_diagnostic(Diagnostic::error("cannot infer struct type for literal"));
                "_UnknownStruct".to_owned()
            });

        let fields: Vec<(String, RustExpr)> = slit
            .fields
            .iter()
            .map(|f| {
                let value = self.lower_expr(&f.value, ctx, use_map, stmt_index);
                (f.name.name.clone(), value)
            })
            .collect();

        // Handle spread: `{ ...base, field: value }` → struct update syntax
        if let Some(spread_expr) = &slit.spread {
            // If there are no field overrides, it's a simple clone
            if fields.is_empty() {
                let base = self.lower_expr(spread_expr, ctx, use_map, stmt_index);
                return RustExpr::new(RustExprKind::Clone(Box::new(base)), span);
            }

            let base = self.lower_expr(spread_expr, ctx, use_map, stmt_index);
            return RustExpr::new(
                RustExprKind::StructUpdate {
                    type_name,
                    fields,
                    base: Box::new(RustExpr::new(
                        RustExprKind::Clone(Box::new(base)),
                        spread_expr.span,
                    )),
                },
                span,
            );
        }

        RustExpr::new(RustExprKind::StructLit { type_name, fields }, span)
    }

    /// Lower an array literal expression.
    ///
    /// Without spread elements, lowers to `vec![a, b, c]`.
    /// With spread elements, lowers to a `SpreadArray` IR node that emits
    /// a block expression with extend/push operations.
    fn lower_array_lit(
        &self,
        elements: &[ast::ArrayElement],
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let has_spread = elements
            .iter()
            .any(|e| matches!(e, ast::ArrayElement::Spread(_)));

        if !has_spread {
            // No spread — plain vec literal
            let lowered: Vec<RustExpr> = elements
                .iter()
                .map(|e| match e {
                    ast::ArrayElement::Expr(expr) => {
                        self.lower_expr(expr, ctx, use_map, stmt_index)
                    }
                    ast::ArrayElement::Spread(_) => unreachable!(),
                })
                .collect();
            return RustExpr::new(RustExprKind::VecLit(lowered), span);
        }

        // Simple copy case: `[...arr]` → `arr.clone()`
        if elements.len() == 1
            && let ast::ArrayElement::Spread(expr) = &elements[0]
        {
            let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
            return RustExpr::new(RustExprKind::Clone(Box::new(lowered)), span);
        }

        // Spread case: collect initial elements (before first spread)
        // and then build push/extend operations.
        let mut initial = Vec::new();
        let mut ops = Vec::new();
        let mut seen_spread = false;

        for elem in elements {
            match elem {
                ast::ArrayElement::Expr(expr) => {
                    let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
                    if seen_spread {
                        ops.push(SpreadOp::Push(lowered));
                    } else {
                        initial.push(lowered);
                    }
                }
                ast::ArrayElement::Spread(expr) => {
                    let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
                    if !seen_spread && initial.is_empty() {
                        // First spread is at the beginning — clone it as the base
                        ops.push(SpreadOp::Extend(RustExpr::new(
                            RustExprKind::Clone(Box::new(lowered)),
                            expr.span,
                        )));
                    } else {
                        ops.push(SpreadOp::Extend(lowered));
                    }
                    seen_spread = true;
                }
            }
        }

        RustExpr::new(RustExprKind::SpreadArray { initial, ops }, span)
    }

    /// Lower a template literal expression.
    ///
    /// - No interpolation: lowers to `"text".to_string()`
    /// - With interpolation: lowers to `format!("text{}text", expr, ...)`
    fn lower_template_lit(
        &self,
        tpl: &ast::TemplateLitExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Separate string parts and expression parts
        let mut strings: Vec<&str> = Vec::new();
        let mut exprs: Vec<&ast::Expr> = Vec::new();

        for part in &tpl.parts {
            match part {
                ast::TemplatePart::String(s, _) => strings.push(s),
                ast::TemplatePart::Expr(e) => exprs.push(e),
            }
        }

        // No interpolation: just a plain string
        if exprs.is_empty() {
            let text = strings.join("");
            let lit = RustExpr::new(RustExprKind::StringLit(text), span);
            return RustExpr::synthetic(RustExprKind::ToString(Box::new(lit)));
        }

        // Build the format string by joining string segments with `{}`
        let mut format_str = String::new();
        for (i, s) in strings.iter().enumerate() {
            format_str.push_str(s);
            if i < exprs.len() {
                format_str.push_str("{}");
            }
        }

        // Build the format! arguments: format string + lowered expressions
        let mut args = vec![RustExpr::synthetic(RustExprKind::StringLit(format_str))];
        for expr in &exprs {
            args.push(self.lower_expr(expr, ctx, use_map, stmt_index));
        }

        RustExpr::new(
            RustExprKind::Macro {
                name: "format".to_owned(),
                args,
            },
            span,
        )
    }

    /// Lower a `new` expression to a Rust static method call or vec literal.
    ///
    /// `new Map()` → `HashMap::new()`, `new Set()` → `HashSet::new()`,
    /// `new Array()` → `vec![]` (empty vec).
    /// `new ClassName(args)` → `ClassName::new(args)` (class constructor).
    fn lower_new_expr(
        &self,
        new_expr: &ast::NewExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let rust_type_name =
            rsc_typeck::resolve::map_collection_type_name(&new_expr.type_name.name);

        let args: Vec<RustExpr> = new_expr
            .args
            .iter()
            .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
            .collect();

        match rust_type_name.as_str() {
            "Vec" => {
                // `new Array()` → `vec![]` (empty vec literal)
                RustExpr::new(RustExprKind::VecLit(args), span)
            }
            _ => {
                // `new Map()` → `HashMap::new()`, `new Set()` → `HashSet::new()`
                // `new ClassName(args)` → `ClassName::new(args)` (class constructor)
                RustExpr::new(
                    RustExprKind::StaticCall {
                        type_name: rust_type_name,
                        method: "new".to_owned(),
                        args,
                    },
                    span,
                )
            }
        }
    }
}

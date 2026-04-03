//! Expression lowering.
//!
//! Transforms `RustScript` AST expressions into Rust IR expressions. Handles
//! literals, identifiers, binary/unary operators, function calls, method calls,
//! closures, struct literals, template literals, `new` expressions, `await`,
//! optional chaining, nullish coalescing, and more.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{
    ParamMode, RustBinaryOp, RustBlock, RustClosureBody, RustClosureParam, RustExpr, RustExprKind,
    RustLetStmt, RustStmt, RustType, SpreadOp,
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

                // Special handling for `in` operator: `"key" in map` → `map.contains_key(&"key".to_string())`
                if bin.op == ast::BinaryOp::In {
                    let left = self.lower_expr(&bin.left, ctx, use_map, stmt_index);
                    let right = self.lower_expr(&bin.right, ctx, use_map, stmt_index);
                    // Wrap the key in a borrow for `contains_key(&key)`
                    let key_arg = RustExpr::synthetic(RustExprKind::Borrow(Box::new(left)));
                    return RustExpr::new(
                        RustExprKind::MethodCall {
                            receiver: Box::new(right),
                            method: "contains_key".to_owned(),
                            type_args: vec![],
                            args: vec![key_arg],
                        },
                        expr.span,
                    );
                }

                let left = self.lower_expr(&bin.left, ctx, use_map, stmt_index);
                let right = self.lower_expr(&bin.right, ctx, use_map, stmt_index);
                let op = lower_binary_op(bin.op);

                // Insert widening casts when both sides are numeric but different types
                let (left, right) = if is_numeric_widenable_op(op) {
                    if let (Some(left_ty), Some(right_ty)) = (
                        infer_rust_expr_type(&left, ctx),
                        infer_rust_expr_type(&right, ctx),
                    ) {
                        if let Some(wider) = wider_numeric_type(&left_ty, &right_ty) {
                            (
                                maybe_widen(left, &left_ty, &wider),
                                maybe_widen(right, &right_ty, &wider),
                            )
                        } else {
                            (left, right)
                        }
                    } else {
                        (left, right)
                    }
                } else {
                    (left, right)
                };

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

                    // Insert widening cast if the target variable is wider than the rhs
                    let lowered_rhs = if let Some(target_ty) = ctx
                        .lookup_variable(&assign.target.name)
                        .map(|info| info.ty.clone())
                    {
                        if let Some(rhs_ty) = infer_rust_expr_type(&lowered_rhs, ctx) {
                            maybe_widen(lowered_rhs, &rhs_ty, &target_ty)
                        } else {
                            lowered_rhs
                        }
                    } else {
                        lowered_rhs
                    };

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

                // Insert widening cast if the target variable is wider than the value
                let value = if let Some(target_ty) = ctx
                    .lookup_variable(&assign.target.name)
                    .map(|info| info.ty.clone())
                {
                    if let Some(val_ty) = infer_rust_expr_type(&value, ctx) {
                        maybe_widen(value, &val_ty, &target_ty)
                    } else {
                        value
                    }
                } else {
                    value
                };

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
                // `super.field` → `self.field` (fields are copied from base class)
                if matches!(fa.object.kind, ast::ExprKind::This | ast::ExprKind::Super) {
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
                        "cannot access private field `{}` from outside the class",
                        fa.field.name
                    )));
                }

                // Math.PI / Math.E / Math.LN2 / … → std::f64::consts::*
                // Number.MAX_SAFE_INTEGER / Number.MIN_SAFE_INTEGER → integer literals
                if let ast::ExprKind::Ident(obj_ident) = &fa.object.kind {
                    if let Some(constant_expr) =
                        crate::builtins::lower_math_constant(&obj_ident.name, field_name)
                    {
                        return constant_expr;
                    }
                    if let Some(constant_expr) =
                        crate::builtins::lower_number_constant(&obj_ident.name, field_name)
                    {
                        return constant_expr;
                    }
                }

                // Error property access on catch variables:
                // `e.message` → `e.clone()` (the caught error IS the message string)
                // `e.name` → `"Error"` (errors are strings, name is always "Error")
                if let ast::ExprKind::Ident(obj_ident) = &fa.object.kind
                    && ctx.is_catch_variable(&obj_ident.name)
                {
                    if field_name == "message" {
                        // The caught error is already a String — `.message` is identity.
                        let object = self.lower_expr(&fa.object, ctx, use_map, stmt_index);
                        return RustExpr::new(RustExprKind::Clone(Box::new(object)), expr.span);
                    }
                    if field_name == "name" {
                        return RustExpr::new(
                            RustExprKind::StringLit("Error".to_owned()),
                            expr.span,
                        );
                    }
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
            ast::ExprKind::TaggedTemplate {
                tag,
                quasis,
                expressions,
            } => self.lower_tagged_template(
                tag,
                quasis,
                expressions,
                expr.span,
                ctx,
                use_map,
                stmt_index,
            ),
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

                // Check if the object is a HashMap-typed variable.
                // HashMap uses string/typed keys, not usize indices.
                let is_hashmap = if let ast::ExprKind::Ident(ident) = &index_expr.object.kind {
                    ctx.lookup_variable(&ident.name)
                        .is_some_and(|info| is_hashmap_type(&info.ty))
                } else {
                    false
                };

                let object = self.lower_expr(&index_expr.object, ctx, use_map, stmt_index);
                let index = self.lower_expr(&index_expr.index, ctx, use_map, stmt_index);

                if is_hashmap {
                    // HashMap read: `map["key"]` → `map[&"key".to_string()]`
                    // Rust's HashMap Index trait accepts &K, and for String keys
                    // it also accepts &str via Borrow trait. String literals work
                    // directly as &str. For non-literal keys, pass as-is.
                    RustExpr::new(
                        RustExprKind::Index {
                            object: Box::new(object),
                            index: Box::new(index),
                        },
                        expr.span,
                    )
                } else {
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
                // Check for `await Promise.XXX(...)` patterns.
                if let ast::ExprKind::MethodCall(mc) = &inner.kind
                    && let ast::ExprKind::Ident(obj) = &mc.object.kind
                    && obj.name == "Promise"
                    && mc.args.len() == 1
                {
                    // Handle array-argument patterns: all, allSettled, race, any
                    if let ast::ExprKind::ArrayLit(elements) = &mc.args[0].kind {
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
                                // tokio::join! takes futures directly — the `?` must
                                // happen *after* the join, not inside it. Strip any
                                // QuestionMark wrappers that lower_expr added for
                                // throwing calls and record which positions need
                                // unwrapping.
                                let mut throwing_elements = vec![false; lowered_elements.len()];
                                let stripped: Vec<RustExpr> = lowered_elements
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, e)| {
                                        if let RustExprKind::QuestionMark(inner) = e.kind {
                                            throwing_elements[i] = true;
                                            *inner
                                        } else {
                                            e
                                        }
                                    })
                                    .collect();
                                return RustExpr::new(
                                    RustExprKind::TokioJoin {
                                        elements: stripped,
                                        throwing_elements,
                                    },
                                    expr.span,
                                );
                            }
                            // `await Promise.allSettled([...])` → `tokio::join!(...)`
                            // Like `all` but always strips `?` and never unwraps —
                            // each result is kept as-is (Ok or Err).
                            "allSettled" => {
                                let stripped: Vec<RustExpr> = lowered_elements
                                    .into_iter()
                                    .map(|e| {
                                        if let RustExprKind::QuestionMark(inner) = e.kind {
                                            *inner
                                        } else {
                                            e
                                        }
                                    })
                                    .collect();
                                return RustExpr::new(
                                    RustExprKind::TokioJoinSettled(stripped),
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

                    // Handle single-argument patterns: resolve, reject
                    match mc.method.name.as_str() {
                        // `await Promise.resolve(x)` → just `x`
                        "resolve" => {
                            return self.lower_expr(&mc.args[0], ctx, use_map, stmt_index);
                        }
                        // `await Promise.reject(msg)` → `panic!("rejected: {}", msg)`
                        "reject" => {
                            let lowered_arg =
                                self.lower_expr(&mc.args[0], ctx, use_map, stmt_index);
                            return RustExpr::new(
                                RustExprKind::Macro {
                                    name: "panic".to_owned(),
                                    args: vec![
                                        RustExpr::synthetic(RustExprKind::StringLit(
                                            "rejected: {}".to_owned(),
                                        )),
                                        lowered_arg,
                                    ],
                                },
                                expr.span,
                            );
                        }
                        _ => {}
                    }
                }

                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                // For async throws functions: the call returns Future<Output=Result<T,E>>.
                // We need `.await?` (await first, then unwrap), not `?.await`.
                // If lower_expr already added `?` on the call, unwrap it, add .await,
                // then re-wrap with `?`.
                if let RustExprKind::QuestionMark(inner_call) = lowered.kind {
                    let await_expr = RustExpr::new(RustExprKind::Await(inner_call), expr.span);
                    RustExpr::new(RustExprKind::QuestionMark(Box::new(await_expr)), expr.span)
                } else if ctx.is_fn_throws() {
                    // External async call in throws context: add .await?
                    let await_expr =
                        RustExpr::new(RustExprKind::Await(Box::new(lowered)), expr.span);
                    RustExpr::new(RustExprKind::QuestionMark(Box::new(await_expr)), expr.span)
                } else {
                    let await_expr =
                        RustExpr::new(RustExprKind::Await(Box::new(lowered)), expr.span);
                    // In non-throws context, if the awaited call is to a throws
                    // function (returns Result), add .unwrap() to crash on error.
                    // This matches TypeScript semantics where unhandled rejections crash.
                    let callee_throws = match &inner.kind {
                        ast::ExprKind::Call(call) => self
                            .fn_signatures
                            .get(&call.callee.name)
                            .is_some_and(|sig| sig.throws),
                        ast::ExprKind::MethodCall(mc) => {
                            // Check Type::method or bare method name
                            if let ast::ExprKind::Ident(obj) = &mc.object.kind {
                                let key = format!("{}::{}", obj.name, mc.method.name);
                                self.fn_signatures
                                    .get(&key)
                                    .or_else(|| self.fn_signatures.get(&mc.method.name))
                                    .is_some_and(|sig| sig.throws)
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                    if callee_throws {
                        RustExpr::synthetic(RustExprKind::MethodCall {
                            receiver: Box::new(await_expr),
                            method: "unwrap".to_owned(),
                            type_args: vec![],
                            args: vec![],
                        })
                    } else {
                        await_expr
                    }
                }
            }
            ast::ExprKind::This => RustExpr::new(RustExprKind::SelfRef, expr.span),
            ast::ExprKind::Super => {
                // `super` as a bare expression should not appear outside of
                // `super.method()` which is handled in lower_method_call.
                ctx.emit_diagnostic(rsc_syntax::diagnostic::Diagnostic::error(
                    "`super` must be used as `super.method()` or `super(args)`".to_owned(),
                ));
                RustExpr::new(
                    RustExprKind::Ident("/* super error */".to_owned()),
                    expr.span,
                )
            }
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
                    "logical assignment operators (??=, ||=, &&=) can only be used as statements, not inside expressions",
                ));
                self.lower_expr(&la.value, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Satisfies(inner, _) => {
                // `satisfies` is a compile-time assertion — strip it entirely
                self.lower_expr(inner, ctx, use_map, stmt_index)
            }
            ast::ExprKind::IndexAssign(ia) => {
                // Check if the object is a HashMap-typed variable.
                let is_hashmap = if let ast::ExprKind::Ident(ident) = &ia.object.kind {
                    ctx.lookup_variable(&ident.name)
                        .is_some_and(|info| is_hashmap_type(&info.ty))
                } else {
                    false
                };

                let object = self.lower_expr(&ia.object, ctx, use_map, stmt_index);
                let index = self.lower_expr(&ia.index, ctx, use_map, stmt_index);
                let value = self.lower_expr(&ia.value, ctx, use_map, stmt_index);

                if is_hashmap {
                    // HashMap write: `map["key"] = value` → `map.insert(key, value)`
                    // If key is a string literal, convert to `.to_string()` for owned String key
                    let key = if matches!(&index.kind, RustExprKind::StringLit(_)) {
                        RustExpr::synthetic(RustExprKind::ToString(Box::new(index)))
                    } else {
                        index
                    };
                    RustExpr::new(
                        RustExprKind::MethodCall {
                            receiver: Box::new(object),
                            method: "insert".to_owned(),
                            type_args: vec![],
                            args: vec![key, value],
                        },
                        expr.span,
                    )
                } else {
                    // Standard index assignment: `arr[i] = value` → `arr[i] = value`
                    let index = if matches!(index.kind, RustExprKind::IntLit(_)) {
                        index
                    } else {
                        RustExpr::synthetic(RustExprKind::Cast(
                            Box::new(index),
                            RustType::Named("usize".to_owned()),
                        ))
                    };
                    RustExpr::new(
                        RustExprKind::IndexAssign {
                            object: Box::new(object),
                            index: Box::new(index),
                            value: Box::new(value),
                        },
                        expr.span,
                    )
                }
            }
            ast::ExprKind::Yield(_) => {
                // Yield expressions are handled at the function level during generator
                // lowering (state machine transformation). If we reach here, it means
                // yield appeared in a non-generator context — emit a placeholder that
                // will produce a clear compile error in the generated Rust.
                RustExpr::new(
                    RustExprKind::Ident("compile_error!(\"yield outside generator\")".to_owned()),
                    expr.span,
                )
            }
            ast::ExprKind::Delete(operand) => {
                // `delete map["key"]` → `map.remove("key")`
                // The operand must be an index expression.
                if let ast::ExprKind::Index(idx) = &operand.kind {
                    let object = self.lower_expr(&idx.object, ctx, use_map, stmt_index);
                    let key = self.lower_expr(&idx.index, ctx, use_map, stmt_index);
                    RustExpr::new(
                        RustExprKind::MethodCall {
                            receiver: Box::new(object),
                            method: "remove".to_owned(),
                            type_args: vec![],
                            args: vec![key],
                        },
                        expr.span,
                    )
                } else {
                    // Unsupported delete target — emit a compile error placeholder
                    RustExpr::new(
                        RustExprKind::Ident(
                            "compile_error!(\"delete requires an index expression\")".to_owned(),
                        ),
                        expr.span,
                    )
                }
            }
            ast::ExprKind::Void(inner) => {
                // `void expr` → `{ expr; }` (block expression that evaluates and discards)
                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                let block = RustBlock {
                    stmts: vec![RustStmt::Semi(lowered)],
                    expr: None,
                };
                RustExpr::new(RustExprKind::BlockExpr(block), expr.span)
            }
            ast::ExprKind::Comma(exprs) => {
                // `(a, b, c)` → `{ a; b; c }` — all but last are statements, last is value
                let mut stmts = Vec::new();
                let last = exprs.len() - 1;
                for (i, e) in exprs.iter().enumerate() {
                    let lowered = self.lower_expr(e, ctx, use_map, stmt_index);
                    if i < last {
                        stmts.push(RustStmt::Semi(lowered));
                    } else {
                        // Last expression is the block value (trailing expression)
                        let block = RustBlock {
                            stmts,
                            expr: Some(Box::new(lowered)),
                        };
                        return RustExpr::new(RustExprKind::BlockExpr(block), expr.span);
                    }
                }
                // Should never reach here — comma expressions always have at least one element
                unreachable!("comma expression should have at least one element")
            }
            ast::ExprKind::DynamicImport(module) => {
                // Dynamic imports are not supported in compiled Rust.
                // Emit a diagnostic warning and lower to a panic to keep code compiling.
                ctx.emit_diagnostic(Diagnostic::warning(format!(
                    "dynamic import(\"{module}\") is not supported in RustScript; use a static import declaration instead"
                )));
                RustExpr::new(
                    RustExprKind::Macro {
                        name: "panic".to_owned(),
                        args: vec![RustExpr::synthetic(RustExprKind::StringLit(format!(
                            "dynamic import not supported: {module}"
                        )))],
                    },
                    expr.span,
                )
            }
            ast::ExprKind::NewTarget => {
                // `new.target` is not meaningful in Rust — constructors are just functions.
                // Emit a warning and lower to an empty string literal.
                ctx.emit_diagnostic(Diagnostic::warning(
                    "new.target is not supported in RustScript; constructors are regular functions and do not have a `new.target` equivalent",
                ));
                RustExpr::new(RustExprKind::StringLit(String::new()), expr.span)
            }
            ast::ExprKind::ImportMeta => {
                // `import.meta` → `module_path!()` as a reasonable approximation.
                ctx.emit_diagnostic(Diagnostic::warning(
                    "import.meta is partially supported; lowering to module_path!()",
                ));
                RustExpr::new(
                    RustExprKind::Macro {
                        name: "module_path".to_owned(),
                        args: vec![],
                    },
                    expr.span,
                )
            }
            // Increment/decrement: lower to compound assignment
            ast::ExprKind::PostfixIncrement(operand) | ast::ExprKind::PrefixIncrement(operand) => {
                if let ast::ExprKind::Ident(ident) = &operand.kind {
                    RustExpr::new(
                        RustExprKind::CompoundAssign {
                            target: ident.name.clone(),
                            op: rsc_syntax::rust_ir::RustCompoundAssignOp::AddAssign,
                            value: Box::new(RustExpr::new(RustExprKind::IntLit(1), expr.span)),
                        },
                        expr.span,
                    )
                } else {
                    self.lower_expr(operand, ctx, use_map, stmt_index)
                }
            }
            ast::ExprKind::PostfixDecrement(operand) | ast::ExprKind::PrefixDecrement(operand) => {
                if let ast::ExprKind::Ident(ident) = &operand.kind {
                    RustExpr::new(
                        RustExprKind::CompoundAssign {
                            target: ident.name.clone(),
                            op: rsc_syntax::rust_ir::RustCompoundAssignOp::SubAssign,
                            value: Box::new(RustExpr::new(RustExprKind::IntLit(1), expr.span)),
                        },
                        expr.span,
                    )
                } else {
                    self.lower_expr(operand, ctx, use_map, stmt_index)
                }
            }
            ast::ExprKind::AsConst(inner) => {
                // `as const` on array literals → static slice literal `&[elem1, elem2, ...]`
                // `as const` on other expressions → lower the inner expression unchanged
                if let ast::ExprKind::ArrayLit(elements) = &inner.kind {
                    let has_spread = elements
                        .iter()
                        .any(|e| matches!(e, ast::ArrayElement::Spread(_)));
                    if !has_spread {
                        let lowered: Vec<RustExpr> = elements
                            .iter()
                            .map(|e| match e {
                                ast::ArrayElement::Expr(el) => {
                                    // In `as const` context, string literals stay as `&str`
                                    // (no `.to_string()` wrapping) since static slices
                                    // hold `&str` references, not owned `String`.
                                    if let ast::ExprKind::StringLit(s) = &el.kind {
                                        RustExpr::new(RustExprKind::StringLit(s.clone()), el.span)
                                    } else {
                                        self.lower_expr(el, ctx, use_map, stmt_index)
                                    }
                                }
                                ast::ArrayElement::Spread(_) => unreachable!(),
                            })
                            .collect();
                        return RustExpr::new(RustExprKind::SliceLit(lowered), expr.span);
                    }
                }
                // For non-array (objects, literals): strip the `as const`, lower inner
                self.lower_expr(inner, ctx, use_map, stmt_index)
            }
            ast::ExprKind::ClassExpr(class_def) => {
                // Class expressions in arbitrary expression positions produce
                // the class name as an identifier. The class itself is hoisted
                // during top-level const lowering.
                RustExpr::new(RustExprKind::Ident(class_def.name.name.clone()), expr.span)
            }
            ast::ExprKind::RegexLit { pattern, flags } => {
                // `/pattern/flags` → `Regex::new("(?flags)pattern").unwrap()`
                // Same lowering as `new RegExp("pattern", "flags")`.
                let mut rust_flags = String::new();
                for ch in flags.chars() {
                    match ch {
                        'i' | 'm' | 's' => rust_flags.push(ch),
                        _ => {} // g, u, y, d ignored
                    }
                }
                let pattern_str = if rust_flags.is_empty() {
                    pattern.clone()
                } else {
                    format!("(?{rust_flags}){pattern}")
                };
                let pattern_arg = RustExpr::new(RustExprKind::StringLit(pattern_str), expr.span);
                let new_call = RustExpr::new(
                    RustExprKind::StaticCall {
                        type_name: "Regex".to_owned(),
                        method: "new".to_owned(),
                        args: vec![pattern_arg],
                    },
                    expr.span,
                );
                RustExpr::new(
                    RustExprKind::MethodCall {
                        receiver: Box::new(new_call),
                        method: "unwrap".to_owned(),
                        type_args: vec![],
                        args: vec![],
                    },
                    expr.span,
                )
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
        // Type-only imports cannot be called as functions.
        if self.is_type_only_import(&call.callee.name) {
            ctx.emit_diagnostic(Diagnostic::error(format!(
                "cannot use type-only import `{}` as a value",
                call.callee.name
            )));
        }

        // Check for generator function calls — rewrite to StructName::new(args)
        if let Some(struct_name) = self.generator_structs.get(&call.callee.name) {
            let args: Vec<RustExpr> = call
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return RustExpr::new(
                RustExprKind::StaticCall {
                    type_name: struct_name.clone(),
                    method: "new".to_owned(),
                    args,
                },
                span,
            );
        }

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

        // When the parameter type is &dyn Trait or &[T], add & before the argument
        if let Some(param_ty) = sig.and_then(|s| s.param_types.get(i))
            && matches!(
                param_ty,
                RustType::DynRef(_) | RustType::Slice(_) | RustType::Reference(_)
            )
        {
            let lowered = self.lower_expr(a, ctx, use_map, stmt_index);
            if !matches!(lowered.kind, RustExprKind::Borrow(_)) {
                return RustExpr::synthetic(RustExprKind::Borrow(Box::new(lowered)));
            }
            return lowered;
        }

        // Infer struct type for object literals from the parameter type.
        // When the argument is an untyped struct literal and the parameter type
        // is a named struct, propagate the type name so `lower_struct_lit` can
        // resolve the struct type instead of emitting "cannot infer struct type".
        if matches!(&a.kind, ast::ExprKind::StructLit(slit) if slit.type_name.is_none()) {
            if let Some(param_ty) = sig.and_then(|s| s.param_types.get(i)) {
                if let Some(type_name) = extract_named_type(param_ty) {
                    let prev = ctx.current_struct_type_name().map(String::from);
                    ctx.set_struct_type_name(Some(type_name));
                    let lowered = self.lower_expr(a, ctx, use_map, stmt_index);
                    ctx.set_struct_type_name(prev);
                    return lowered;
                }
            }
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
        // Handle `super.method(args)` → construct temporary base class, call method.
        if matches!(mc.object.kind, ast::ExprKind::Super) {
            return self.lower_super_method_call(mc, span, ctx, use_map, stmt_index);
        }

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
            // HashSet.forEach() uses single-param callback, not Map's (value, key) swap
            if is_set && mc.method.name == "forEach" {
                return crate::builtins::lower_set_for_each(receiver, lowered_args, span);
            }
            return lowering_fn(receiver, lowered_args, span);
        }

        // Check for array-specific method: type-aware dispatch for methods like
        // indexOf, lastIndexOf, at, concat, slice, includes that have different
        // semantics on arrays vs strings.
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && let Some(var_info) = ctx.lookup_variable(&obj_ident.name)
            && matches!(
                &var_info.ty,
                RustType::Generic(base, _)
                    if matches!(base.as_ref(), RustType::Named(n) if n == "Vec")
            )
            && let Some(lowering_fn) = self.builtins.lookup_array_method(&mc.method.name)
        {
            let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return lowering_fn(receiver, lowered_args, span);
        }

        // Check for Date instance method: type-aware dispatch for methods like
        // getTime/toISOString/toString that are specific to Date (SystemTime) values.
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && let Some(var_info) = ctx.lookup_variable(&obj_ident.name)
            && crate::builtins::is_date_type(&var_info.ty)
            && let Some(lowering_fn) = self.builtins.lookup_date_method(&mc.method.name)
        {
            let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return lowering_fn(receiver, lowered_args, span);
        }

        // Check for Regex method: type-aware dispatch for methods like
        // test/exec that would conflict with user-defined methods if
        // dispatched by name alone.
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && let Some(var_info) = ctx.lookup_variable(&obj_ident.name)
            && matches!(&var_info.ty, RustType::Named(n) if n == "Regex")
            && let Some(lowering_fn) = self.builtins.lookup_regex_method(&mc.method.name)
        {
            let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return lowering_fn(receiver, lowered_args, span);
        }

        // Check for Number instance method: type-aware dispatch for methods like
        // toFixed/toPrecision/toString that are specific to numeric types.
        if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind
            && let Some(var_info) = ctx.lookup_variable(&obj_ident.name)
            && crate::builtins::is_number_type(&var_info.ty)
            && let Some(lowering_fn) = self.builtins.lookup_number_method(&mc.method.name)
        {
            let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
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
            // Type-only imports cannot be used as values (e.g., static method calls).
            if self.is_type_only_import(&obj_ident.name) {
                ctx.emit_diagnostic(Diagnostic::error(format!(
                    "cannot use type-only import `{}` as a value",
                    obj_ident.name
                )));
            }
            // Look up external signature by "TypeName::method_name"
            let sig_key = format!("{}::{}", obj_ident.name, mc.method.name);
            let sig = self.fn_signatures.get(&sig_key);
            let callee_modes = sig.and_then(|s| s.param_modes.as_ref());

            let args: Vec<RustExpr> = mc
                .args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    if sig.is_some() {
                        self.lower_single_arg(a, i, sig, callee_modes, ctx, use_map, stmt_index)
                    } else {
                        let lowered = self.lower_expr(a, ctx, use_map, stmt_index);
                        // Strip .to_string() from string literal args for external calls
                        if let RustExprKind::ToString(inner) = &lowered.kind
                            && matches!(inner.kind, RustExprKind::StringLit(_))
                        {
                            return *inner.clone();
                        }
                        lowered
                    }
                })
                .collect();

            let call_expr = RustExpr::new(
                RustExprKind::StaticCall {
                    type_name: obj_ident.name.clone(),
                    method: mc.method.name.clone(),
                    args,
                },
                span,
            );

            // Wrap with `?` if the external method throws and we're in a throws context
            if sig.is_some_and(|s| s.throws) && ctx.is_fn_throws() {
                return RustExpr::new(RustExprKind::QuestionMark(Box::new(call_expr)), span);
            }
            return call_expr;
        }

        // Not a builtin — lower as a regular method call.
        // Try to look up an external method signature by "TypeName::method_name"
        // using the receiver's type from context.
        let method_sig = if let ast::ExprKind::Ident(obj_ident) = &mc.object.kind {
            ctx.lookup_variable(&obj_ident.name)
                .and_then(|var_info| extract_named_type(&var_info.ty))
                .and_then(|type_name| {
                    let key = format!("{}::{}", type_name, mc.method.name);
                    self.fn_signatures.get(&key)
                })
        } else {
            None
        };
        let callee_modes = method_sig.and_then(|s| s.param_modes.as_ref());

        let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
        let args: Vec<RustExpr> = mc
            .args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if method_sig.is_some() {
                    self.lower_single_arg(a, i, method_sig, callee_modes, ctx, use_map, stmt_index)
                } else {
                    // No known signature — strip .to_string() from string literal args
                    // since most Rust APIs expect &str.
                    let lowered = self.lower_expr(a, ctx, use_map, stmt_index);
                    if let RustExprKind::ToString(inner) = &lowered.kind
                        && matches!(inner.kind, RustExprKind::StringLit(_))
                    {
                        return *inner.clone();
                    }
                    lowered
                }
            })
            .collect();

        let call_expr = RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(receiver),
                method: mc.method.name.clone(),
                type_args: vec![],
                args,
            },
            span,
        );

        // Wrap with `?` if the external method throws and we're in a throws context
        if method_sig.is_some_and(|s| s.throws) && ctx.is_fn_throws() {
            return RustExpr::new(RustExprKind::QuestionMark(Box::new(call_expr)), span);
        }
        call_expr
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
    ///
    /// When any field has a computed key (`[expr]: value`), the entire object
    /// literal is lowered to a `HashMap<String, _>` with `.insert()` calls.
    fn lower_struct_lit(
        &self,
        slit: &ast::StructLitExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Check if any field uses a computed property name.
        let has_computed = slit.fields.iter().any(|f| f.computed_key.is_some());

        if has_computed {
            return self.lower_computed_object_lit(slit, span, ctx, use_map, stmt_index);
        }

        let type_name = slit
            .type_name
            .as_ref()
            .map(|n| n.name.clone())
            .or_else(|| ctx.current_struct_type_name().map(String::from))
            .unwrap_or_else(|| {
                ctx.emit_diagnostic(Diagnostic::error("cannot infer struct type for literal; specify the type explicitly, e.g., `const x: MyType = { ... }`"));
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

    /// Lower an object literal with computed property names to a `HashMap`.
    ///
    /// Produces a block expression:
    /// ```text
    /// {
    ///     let mut __obj = HashMap::new();
    ///     __obj.insert("static_key".to_string(), value);
    ///     __obj.insert(computed_key_expr, value);
    ///     __obj
    /// }
    /// ```
    fn lower_computed_object_lit(
        &self,
        slit: &ast::StructLitExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let obj_name = "__obj";
        let mut stmts = Vec::new();

        // `let mut __obj = HashMap::new();`
        stmts.push(RustStmt::Let(RustLetStmt {
            mutable: true,
            name: obj_name.to_owned(),
            ty: None,
            init: RustExpr::new(
                RustExprKind::StaticCall {
                    type_name: "HashMap".to_owned(),
                    method: "new".to_owned(),
                    args: vec![],
                },
                span,
            ),
            span: Some(span),
        }));

        // Generate `.insert()` calls for each field
        for field in &slit.fields {
            let value = self.lower_expr(&field.value, ctx, use_map, stmt_index);

            let key_expr = if let Some(computed) = &field.computed_key {
                // Computed key: use the expression directly
                self.lower_expr(computed, ctx, use_map, stmt_index)
            } else {
                // Static key: convert name to a string literal with `.to_string()`
                RustExpr::new(
                    RustExprKind::ToString(Box::new(RustExpr::new(
                        RustExprKind::StringLit(field.name.name.clone()),
                        field.span,
                    ))),
                    field.span,
                )
            };

            // `__obj.insert(key, value);`
            stmts.push(RustStmt::Semi(RustExpr::new(
                RustExprKind::MethodCall {
                    receiver: Box::new(RustExpr::new(
                        RustExprKind::Ident(obj_name.to_owned()),
                        span,
                    )),
                    method: "insert".to_owned(),
                    type_args: vec![],
                    args: vec![key_expr, value],
                },
                field.span,
            )));
        }

        // Trailing expression: `__obj`
        let trailing = RustExpr::new(RustExprKind::Ident(obj_name.to_owned()), span);

        RustExpr::new(
            RustExprKind::BlockExpr(RustBlock {
                stmts,
                expr: Some(Box::new(trailing)),
            }),
            span,
        )
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
            // Check if the expected element type is a tuple — if so, inner array
            // literals should be lowered as tuple constructors, not vec literals.
            // This handles `Array<[T, U]>` where `["hello", 1]` → `("hello".to_string(), 1)`.
            let expected_elem = ctx.take_expected_element_type();
            let is_tuple_element = matches!(&expected_elem, Some(RustType::Tuple(_)));

            let lowered: Vec<RustExpr> = elements
                .iter()
                .map(|e| match e {
                    ast::ArrayElement::Expr(expr) => {
                        if is_tuple_element {
                            if let ast::ExprKind::ArrayLit(inner_elements) = &expr.kind {
                                // Lower inner array literal as a tuple constructor
                                let tuple_parts: Vec<RustExpr> = inner_elements
                                    .iter()
                                    .map(|ie| match ie {
                                        ast::ArrayElement::Expr(inner_expr)
                                        | ast::ArrayElement::Spread(inner_expr) => {
                                            self.lower_expr(inner_expr, ctx, use_map, stmt_index)
                                        }
                                    })
                                    .collect();
                                return RustExpr::new(RustExprKind::Tuple(tuple_parts), expr.span);
                            }
                        }
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

    /// Lower a tagged template literal to a function call.
    ///
    /// `` tag`hello ${name} world` `` lowers to:
    /// `tag(&["hello ", " world"], vec![name.clone()])`
    ///
    /// The tag function receives:
    /// 1. A static string slice of the template's string segments.
    /// 2. A `vec![]` of the interpolated expression values.
    #[allow(clippy::too_many_arguments)]
    fn lower_tagged_template(
        &self,
        tag: &ast::Expr,
        quasis: &[String],
        expressions: &[ast::Expr],
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Extract the tag function name from the expression.
        let func_name = if let ast::ExprKind::Ident(ident) = &tag.kind {
            ident.name.clone()
        } else {
            ctx.emit_diagnostic(Diagnostic::error(
                "tagged template literal tag must be a simple identifier, e.g., `html`...``",
            ));
            "unknown_tag".to_owned()
        };

        // Build first arg: &["quasi0", "quasi1", ...]
        let quasi_exprs: Vec<RustExpr> = quasis
            .iter()
            .map(|s| RustExpr::synthetic(RustExprKind::StringLit(s.clone())))
            .collect();
        let strings_arg = RustExpr::synthetic(RustExprKind::SliceLit(quasi_exprs));

        // Build second arg: vec![expr1, expr2, ...]
        let value_exprs: Vec<RustExpr> = expressions
            .iter()
            .map(|e| self.lower_expr(e, ctx, use_map, stmt_index))
            .collect();
        let values_arg = RustExpr::synthetic(RustExprKind::VecLit(value_exprs));

        RustExpr::new(
            RustExprKind::Call {
                func: func_name,
                args: vec![strings_arg, values_arg],
            },
            span,
        )
    }

    /// Lower a `new` expression to a Rust static method call or vec literal.
    ///
    /// `new Map()` → `HashMap::new()`, `new Set()` → `HashSet::new()`,
    /// `new Array()` → `vec![]` (empty vec).
    /// `new Error("msg")` → `"msg".to_string()` (error as string).
    /// `new TypeError("msg")` → `"TypeError: msg".to_string()` (prefixed error string).
    /// `new ClassName(args)` → `ClassName::new(args)` (class constructor).
    fn lower_new_expr(
        &self,
        new_expr: &ast::NewExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        // Type-only imports cannot be constructed as values.
        if self.is_type_only_import(&new_expr.type_name.name) {
            ctx.emit_diagnostic(Diagnostic::error(format!(
                "cannot use type-only import `{}` as a value",
                new_expr.type_name.name
            )));
        }

        // Error class hierarchy: `new Error("msg")` → string expression.
        // `new TypeError("msg")` → `"TypeError: msg"` etc.
        let type_name = &new_expr.type_name.name;
        if is_error_class(type_name) {
            let args: Vec<RustExpr> = new_expr
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();
            return lower_error_constructor(type_name, args, span);
        }

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
            "Date" => {
                // `new Date()` → `std::time::SystemTime::now()`
                RustExpr::new(
                    RustExprKind::Raw("std::time::SystemTime::now()".to_owned()),
                    span,
                )
            }
            "Regex" => {
                // `new RegExp("pattern")` → `Regex::new("pattern").unwrap()`
                // `new RegExp("pattern", "gi")` → `Regex::new("(?i)pattern").unwrap()`
                // Flags: i → (?i), m → (?m), s → (?s), g → ignored (affects method choice)
                let pattern_arg = Self::build_regex_pattern_arg(&new_expr.args, &args, span);
                let new_call = RustExpr::new(
                    RustExprKind::StaticCall {
                        type_name: "Regex".to_owned(),
                        method: "new".to_owned(),
                        args: vec![pattern_arg],
                    },
                    span,
                );
                RustExpr::new(
                    RustExprKind::MethodCall {
                        receiver: Box::new(new_call),
                        method: "unwrap".to_owned(),
                        type_args: vec![],
                        args: vec![],
                    },
                    span,
                )
            }
            _ => {
                // `new Map()` → `HashMap::new()`, `new Set()` → `HashSet::new()`
                // `new ClassName(args)` → `ClassName::new(args)` (class constructor)
                //
                // Fill in default values for omitted constructor arguments.
                let mut args = args;
                let sig_key = format!("{rust_type_name}::new");
                if let Some(sig) = self.fn_signatures.get(&sig_key) {
                    let supplied_count = args.len();
                    for i in supplied_count..sig.param_count {
                        if let Some(default_expr) =
                            sig.default_values.get(i).and_then(|d| d.as_ref())
                        {
                            args.push(default_expr.clone());
                        } else if sig.optional_params.get(i).copied().unwrap_or(false) {
                            args.push(RustExpr::synthetic(RustExprKind::None));
                        }
                    }
                }
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

    /// Build the pattern argument for `Regex::new()`, incorporating flags.
    ///
    /// If the second argument to `new RegExp(pattern, flags)` is a string literal
    /// containing JS regex flags, converts them to Rust regex inline flags:
    /// `i` → `(?i)`, `m` → `(?m)`, `s` → `(?s)`, `g` → ignored.
    /// The flags are prepended to the pattern string.
    fn build_regex_pattern_arg(
        raw_args: &[rsc_syntax::ast::Expr],
        lowered_args: &[RustExpr],
        span: rsc_syntax::span::Span,
    ) -> RustExpr {
        // Strip ToString wrapper — Regex::new() takes &str, not String
        let pattern = if let Some(expr) = lowered_args.first().cloned() {
            if let RustExprKind::ToString(inner) = expr.kind {
                *inner
            } else {
                expr
            }
        } else {
            RustExpr::synthetic(RustExprKind::StringLit(String::new()))
        };

        // Check for flags in the second argument (raw AST, not lowered)
        let flags_str = raw_args.get(1).and_then(|expr| {
            if let rsc_syntax::ast::ExprKind::StringLit(s) = &expr.kind {
                Some(s.as_str())
            } else {
                None
            }
        });

        if let Some(flags) = flags_str {
            // Convert JS flags to Rust regex inline flags
            let mut rust_flags = String::new();
            for ch in flags.chars() {
                match ch {
                    'i' | 'm' | 's' => rust_flags.push(ch),
                    _ => {} // Ignored: g=global, u=unicode, y=sticky, d=indices
                }
            }

            if !rust_flags.is_empty() {
                // If the pattern is a string literal, prepend the flags inline
                if let RustExprKind::StringLit(ref pat) = pattern.kind {
                    return RustExpr::new(
                        RustExprKind::StringLit(format!("(?{rust_flags}){pat}")),
                        span,
                    );
                }
                // For non-literal patterns, use format! to prepend flags
                return RustExpr::new(
                    RustExprKind::Macro {
                        name: "format".into(),
                        args: vec![
                            RustExpr::synthetic(RustExprKind::StringLit(format!(
                                "(?{rust_flags}){{}}"
                            ))),
                            pattern,
                        ],
                    },
                    span,
                );
            }
        }

        pattern
    }

    /// Lower a `super.method(args)` call.
    ///
    /// Constructs a temporary base class instance from the inherited fields
    /// (which are copied into the derived class) and calls the method on it.
    /// Emits: `Base { field1: self.field1.clone(), ... }.method(args)`
    fn lower_super_method_call(
        &self,
        mc: &ast::MethodCallExpr,
        span: rsc_syntax::span::Span,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
    ) -> RustExpr {
        let Some(base_class) = ctx.current_base_class().map(String::from) else {
            ctx.emit_diagnostic(rsc_syntax::diagnostic::Diagnostic::error(
                "`super` can only be used in a class that extends another class".to_owned(),
            ));
            return RustExpr::new(RustExprKind::Ident("/* super error */".to_owned()), span);
        };

        // Lower the arguments
        let args: Vec<RustExpr> = mc
            .args
            .iter()
            .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
            .collect();

        // `super(args)` (constructor delegation) → `Base::new(args)`
        if mc.method.name == "new" {
            return RustExpr::new(
                RustExprKind::StaticCall {
                    type_name: base_class,
                    method: "new".to_owned(),
                    args,
                },
                span,
            );
        }

        // `super.method(args)` → construct temporary base class, call method.
        // Since fields are copied (not composed), we build a temporary Base
        // instance from the inherited fields and call the method on it.
        let base_fields: Vec<(String, RustType)> = self
            .type_registry
            .get_class_fields(&base_class)
            .map(|fields| {
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), rsc_typeck::bridge::type_to_rust_type(ty)))
                    .collect()
            })
            .unwrap_or_default();

        // Build field initializers: `field: self.field.clone()`
        let struct_fields: Vec<(String, RustExpr)> = base_fields
            .iter()
            .map(|(name, _ty)| {
                let field_access = RustExpr::synthetic(RustExprKind::SelfFieldAccess {
                    field: name.clone(),
                });
                let cloned = RustExpr::synthetic(RustExprKind::Clone(Box::new(field_access)));
                (name.clone(), cloned)
            })
            .collect();

        // Construct: `Base { field1: self.field1.clone(), ... }`
        let base_instance = RustExpr::new(
            RustExprKind::StructLit {
                type_name: base_class,
                fields: struct_fields,
            },
            span,
        );

        // Emit: `Base { ... }.method(args)`
        RustExpr::new(
            RustExprKind::MethodCall {
                receiver: Box::new(base_instance),
                method: mc.method.name.clone(),
                type_args: vec![],
                args,
            },
            span,
        )
    }
}

/// Return the numeric promotion rank for a `RustType`.
///
/// Smaller integers get lower ranks; floats rank above all integers.
/// Returns `None` for non-numeric types.
///
/// Rank assignments:
/// - `i8`, `u8` → 1
/// - `i16`, `u16` → 2
/// - `i32`, `u32` → 3
/// - `i64`, `u64` → 4
/// - `f32` → 5
/// - `f64` → 6
fn numeric_rank(ty: &RustType) -> Option<u8> {
    match ty {
        RustType::I8 | RustType::U8 => Some(1),
        RustType::I16 | RustType::U16 => Some(2),
        RustType::I32 | RustType::U32 => Some(3),
        RustType::I64 | RustType::U64 => Some(4),
        RustType::F32 => Some(5),
        RustType::F64 => Some(6),
        _ => None,
    }
}

/// Determine the wider of two numeric types for automatic promotion.
///
/// Returns `Some(wider_type)` when one type can be safely widened to the other.
/// Returns `None` when:
/// - Either type is not numeric
/// - The types are identical (no widening needed)
/// - Widening would cross signedness boundaries at the same rank
///   (e.g., `u32` → `i32` is not safe)
///
/// Special cases:
/// - Any integer → `f32` or `f64` is allowed (mirrors TypeScript behavior)
/// - When both are integer types of different ranks, the higher rank wins
///   but we pick the specific type (not just the rank) of the wider operand
pub(super) fn wider_numeric_type(a: &RustType, b: &RustType) -> Option<RustType> {
    let rank_a = numeric_rank(a)?;
    let rank_b = numeric_rank(b)?;

    if a == b {
        return None; // same type, no widening needed
    }

    if rank_a == rank_b {
        // Same rank but different types (e.g., i32 vs u32).
        // Don't auto-widen — this could be lossy in either direction.
        return None;
    }

    if rank_a > rank_b {
        Some(a.clone())
    } else {
        Some(b.clone())
    }
}

/// Infer the `RustType` of a lowered `RustExpr` from its structure and context.
///
/// Returns `Some(ty)` for expressions whose type is concretely known:
/// - Identifiers → looked up in the lowering context
/// - Cast expressions → the target type of the cast
/// - Clone/Paren → delegates to the inner expression
///
/// Literals (`IntLit`, `FloatLit`) intentionally return `None` because they are
/// polymorphic in Rust (a literal `2` adapts to `i32`, `i64`, etc. based on
/// context). Widening should only occur between expressions with fixed types.
pub(super) fn infer_rust_expr_type(expr: &RustExpr, ctx: &LoweringContext) -> Option<RustType> {
    match &expr.kind {
        RustExprKind::Ident(name) => ctx.lookup_variable(name).map(|info| info.ty.clone()),
        RustExprKind::Cast(_, target_ty) => Some(target_ty.clone()),
        RustExprKind::Clone(inner) | RustExprKind::Paren(inner) => infer_rust_expr_type(inner, ctx),
        _ => None,
    }
}

/// Wrap `expr` in a `Cast` to `target_type` if it would be a safe numeric widening.
///
/// Returns the expression unchanged if `expr_type` is already the target type
/// or if no safe widening exists between the two types.
fn maybe_widen(expr: RustExpr, expr_type: &RustType, target_type: &RustType) -> RustExpr {
    if expr_type == target_type {
        return expr;
    }
    // Check that target_type is actually wider
    let Some(rank_expr) = numeric_rank(expr_type) else {
        return expr;
    };
    let Some(rank_target) = numeric_rank(target_type) else {
        return expr;
    };
    if rank_target > rank_expr {
        RustExpr::synthetic(RustExprKind::Cast(Box::new(expr), target_type.clone()))
    } else {
        expr
    }
}

/// Check whether a `RustBinaryOp` is a numeric operation that benefits from widening.
///
/// Arithmetic (`+`, `-`, `*`, `/`, `%`) and comparison (`<`, `>`, `<=`, `>=`, `==`, `!=`)
/// operators return `true`. Logical and bitwise operators return `false`.
fn is_numeric_widenable_op(op: RustBinaryOp) -> bool {
    matches!(
        op,
        RustBinaryOp::Add
            | RustBinaryOp::Sub
            | RustBinaryOp::Mul
            | RustBinaryOp::Div
            | RustBinaryOp::Rem
            | RustBinaryOp::Eq
            | RustBinaryOp::Ne
            | RustBinaryOp::Lt
            | RustBinaryOp::Gt
            | RustBinaryOp::Le
            | RustBinaryOp::Ge
    )
}

/// Check whether a `RustType` is a `HashMap` type.
///
/// Returns `true` for `Generic(Named("HashMap"), _)` which is the lowered form
/// of index signature types and `Map<K, V>` / `new Map()`.
pub(super) fn is_hashmap_type(ty: &RustType) -> bool {
    matches!(
        ty,
        RustType::Generic(base, _) if matches!(base.as_ref(), RustType::Named(name) if name == "HashMap")
    )
}

/// Check whether a type name is a JavaScript `Error` class or subclass.
///
/// Recognizes `Error`, `TypeError`, `RangeError`, `ReferenceError`, and `SyntaxError`.
pub(super) fn is_error_class(name: &str) -> bool {
    matches!(
        name,
        "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
    )
}

/// Lower an error constructor to a string expression.
///
/// - `new Error("msg")` → `"msg".to_string()`
/// - `new TypeError("msg")` → `"TypeError: msg".to_string()` (when static string literal)
///   or `format!("TypeError: {}", msg)` (when dynamic)
/// - No-arg: `new Error()` → `"Error".to_string()`
fn lower_error_constructor(
    type_name: &str,
    args: Vec<RustExpr>,
    span: rsc_syntax::span::Span,
) -> RustExpr {
    let is_base_error = type_name == "Error";
    let msg_arg = args.into_iter().next();

    match (is_base_error, msg_arg) {
        // `new Error("msg")` → `"msg".to_string()`
        (true, Some(msg)) => msg,
        // `new Error()` → `"Error".to_string()`
        (true, None) => RustExpr::new(
            RustExprKind::ToString(Box::new(RustExpr::new(
                RustExprKind::StringLit("Error".to_owned()),
                span,
            ))),
            span,
        ),
        // `new TypeError("msg")` → `"TypeError: msg".to_string()`
        (false, Some(msg)) => {
            // Extract the string from `ToString(StringLit(s))` wrapper
            // (string literals are lowered as `"s".to_string()`)
            let static_str = match &msg.kind {
                RustExprKind::StringLit(s) => Some(s.clone()),
                RustExprKind::ToString(inner) => {
                    if let RustExprKind::StringLit(s) = &inner.kind {
                        Some(s.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(s) = static_str {
                return RustExpr::new(
                    RustExprKind::ToString(Box::new(RustExpr::new(
                        RustExprKind::StringLit(format!("{type_name}: {s}")),
                        span,
                    ))),
                    span,
                );
            }
            // For dynamic expressions, use format!
            RustExpr::new(
                RustExprKind::Macro {
                    name: "format".to_owned(),
                    args: vec![
                        RustExpr::synthetic(RustExprKind::StringLit(format!("{type_name}: {{}}"))),
                        msg,
                    ],
                },
                span,
            )
        }
        // `new TypeError()` → `"TypeError".to_string()`
        (false, None) => RustExpr::new(
            RustExprKind::ToString(Box::new(RustExpr::new(
                RustExprKind::StringLit(type_name.to_owned()),
                span,
            ))),
            span,
        ),
    }
}

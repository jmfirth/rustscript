//! Expression lowering.
//!
//! Transforms `RustScript` AST expressions into Rust IR expressions. Handles
//! literals, identifiers, binary/unary operators, function calls, method calls,
//! closures, struct literals, template literals, `new` expressions, `await`,
//! optional chaining, nullish coalescing, and more.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{RustClosureBody, RustClosureParam, RustExpr, RustExprKind};

use crate::context::LoweringContext;
use crate::ownership::{self, UseMap};

use super::{
    Transform, capitalize_first, detect_compound_assign, extract_named_type, lower_binary_op,
    lower_unary_op,
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
                // Check for builtin free functions (e.g., `spawn`)
                if let Some(lowering_fn) = self.builtins.lookup_function(&call.callee.name).copied()
                {
                    let lowered_args: Vec<RustExpr> = call
                        .args
                        .iter()
                        .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                        .collect();
                    return lowering_fn(lowered_args, expr.span);
                }

                let sig = self.fn_signatures.get(&call.callee.name);
                let args: Vec<RustExpr> = call
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
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
                        self.lower_expr(a, ctx, use_map, stmt_index)
                    })
                    .collect();
                let call_expr = RustExpr::new(
                    RustExprKind::Call {
                        func: call.callee.name.clone(),
                        args,
                    },
                    expr.span,
                );
                // If the callee is a throws function and we're inside a throws function,
                // wrap with `?` operator.
                let callee_throws = sig.is_some_and(|sig| sig.throws);
                if callee_throws && ctx.is_fn_throws() {
                    RustExpr::new(RustExprKind::QuestionMark(Box::new(call_expr)), expr.span)
                } else {
                    call_expr
                }
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
                // `this.field` → `self.field`
                if matches!(fa.object.kind, ast::ExprKind::This) {
                    return RustExpr::new(
                        RustExprKind::SelfFieldAccess {
                            field: fa.field.name.clone(),
                        },
                        expr.span,
                    );
                }

                // `.length` on strings/arrays → `.len()` method call
                if fa.field.name == "length" {
                    let object = self.lower_expr(&fa.object, ctx, use_map, stmt_index);
                    return RustExpr::new(
                        RustExprKind::MethodCall {
                            receiver: Box::new(object),
                            method: "len".into(),
                            type_args: vec![],
                            args: vec![],
                        },
                        expr.span,
                    );
                }

                let object = self.lower_expr(&fa.object, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::FieldAccess {
                        object: Box::new(object),
                        field: fa.field.name.clone(),
                    },
                    expr.span,
                )
            }
            ast::ExprKind::TemplateLit(tpl) => {
                self.lower_template_lit(tpl, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::ArrayLit(elements) => {
                let lowered: Vec<RustExpr> = elements
                    .iter()
                    .map(|e| self.lower_expr(e, ctx, use_map, stmt_index))
                    .collect();
                RustExpr::new(RustExprKind::VecLit(lowered), expr.span)
            }
            ast::ExprKind::New(new_expr) => {
                self.lower_new_expr(new_expr, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Index(index_expr) => {
                let object = self.lower_expr(&index_expr.object, ctx, use_map, stmt_index);
                let index = self.lower_expr(&index_expr.index, ctx, use_map, stmt_index);
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
                // Check for `await Promise.all([...])` pattern.
                // Lowers to `tokio::join!(expr1, expr2, ...)` without a separate `.await`.
                if let ast::ExprKind::MethodCall(mc) = &inner.kind
                    && let ast::ExprKind::Ident(obj) = &mc.object.kind
                    && obj.name == "Promise"
                    && mc.method.name == "all"
                    && mc.args.len() == 1
                    && let ast::ExprKind::ArrayLit(elements) = &mc.args[0].kind
                {
                    let lowered_elements: Vec<RustExpr> = elements
                        .iter()
                        .map(|e| self.lower_expr(e, ctx, use_map, stmt_index))
                        .collect();
                    return RustExpr::new(RustExprKind::TokioJoin(lowered_elements), expr.span);
                }

                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                RustExpr::new(RustExprKind::Await(Box::new(lowered)), expr.span)
            }
            ast::ExprKind::This => RustExpr::new(RustExprKind::SelfRef, expr.span),
            ast::ExprKind::FieldAssign(fa) => {
                // Check if this is `this.field = value` → `self.field = value`
                if matches!(fa.object.kind, ast::ExprKind::This) {
                    let value = self.lower_expr(&fa.value, ctx, use_map, stmt_index);
                    RustExpr::new(
                        RustExprKind::SelfFieldAssign {
                            field: fa.field.name.clone(),
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
        }
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
    /// Then checks string methods registered in the builtin registry.
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
            let lowered_args: Vec<RustExpr> = mc
                .args
                .iter()
                .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                .collect();

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

        // Not a builtin — lower as a regular method call
        let receiver = self.lower_expr(&mc.object, ctx, use_map, stmt_index);
        let args: Vec<RustExpr> = mc
            .args
            .iter()
            .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
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

        let fields = slit
            .fields
            .iter()
            .map(|f| {
                let value = self.lower_expr(&f.value, ctx, use_map, stmt_index);
                (f.name.name.clone(), value)
            })
            .collect();

        RustExpr::new(RustExprKind::StructLit { type_name, fields }, span)
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

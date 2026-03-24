//! AST-to-IR transformation.
//!
//! Consumes the `RustScript` AST and produces Rust IR, using the types,
//! ownership, and builtins modules for type resolution, clone insertion,
//! and builtin method lowering respectively.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::{
    RustBinaryOp, RustBlock, RustElse, RustExpr, RustExprKind, RustFile, RustFnDecl, RustIfStmt,
    RustItem, RustLetStmt, RustParam, RustReturnStmt, RustStmt, RustType, RustUnaryOp,
    RustWhileStmt,
};

use crate::builtins::BuiltinRegistry;
use crate::context::LoweringContext;
use crate::ownership::{self, UseMap};
use crate::types;

/// The AST-to-IR transformer.
///
/// Holds the builtin registry and drives the lowering of an entire module.
pub(crate) struct Transform {
    builtins: BuiltinRegistry,
}

impl Transform {
    /// Create a new transformer with the default builtin registry.
    pub fn new() -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
        }
    }

    /// Lower a complete `RustScript` module to a Rust file.
    pub fn lower_module(&self, module: &ast::Module) -> (RustFile, Vec<Diagnostic>) {
        let mut ctx = LoweringContext::new();

        let items = module
            .items
            .iter()
            .map(|item| match item {
                ast::Item::Function(f) => RustItem::Function(self.lower_fn(f, &mut ctx)),
            })
            .collect();

        let diagnostics = ctx.into_diagnostics();
        (RustFile { items }, diagnostics)
    }

    /// Lower a function declaration.
    ///
    /// Performs two-pass analysis: first finds reassigned variables and builds
    /// a use map, then lowers the body with that context.
    pub fn lower_fn(&self, f: &ast::FnDecl, ctx: &mut LoweringContext) -> RustFnDecl {
        ctx.push_scope();

        // Phase 1: find reassigned variables for mutability analysis
        let reassigned = ownership::find_reassigned_variables(&f.body);

        // Phase 2: build use map for ownership analysis
        let use_map = UseMap::analyze(&f.body, |obj, method| {
            self.builtins.is_ref_args(obj, method)
        });

        // Declare parameters in scope
        let params: Vec<RustParam> = f
            .params
            .iter()
            .map(|p| {
                let ty = types::resolve_type_annotation(&p.type_ann, &mut Vec::new());
                ctx.declare_variable(p.name.name.clone(), ty.clone(), false);
                RustParam {
                    name: p.name.name.clone(),
                    ty,
                    span: Some(p.span),
                }
            })
            .collect();

        let return_type = f.return_type.as_ref().and_then(|ann| {
            let ty = types::resolve_type_annotation(ann, &mut Vec::new());
            if ty == RustType::Unit {
                return None;
            }
            Some(ty)
        });

        // Lower the body
        let body = self.lower_block(&f.body, ctx, &use_map, 0, &reassigned);

        ctx.pop_scope();

        RustFnDecl {
            name: f.name.name.clone(),
            params,
            return_type,
            body,
            span: Some(f.span),
        }
    }

    /// Lower a block of statements.
    #[allow(clippy::only_used_in_recursion)]
    fn lower_block(
        &self,
        block: &ast::Block,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        current_base: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustBlock {
        let stmts = block
            .stmts
            .iter()
            .enumerate()
            .map(|(i, stmt)| self.lower_stmt(stmt, ctx, use_map, current_base + i, reassigned))
            .collect();

        RustBlock { stmts, expr: None }
    }

    /// Lower a single statement.
    fn lower_stmt(
        &self,
        stmt: &ast::Stmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        match stmt {
            ast::Stmt::VarDecl(decl) => {
                self.lower_var_decl(decl, ctx, use_map, stmt_index, reassigned)
            }
            ast::Stmt::Expr(expr) => {
                let lowered = self.lower_expr(expr, ctx, use_map, stmt_index);
                RustStmt::Semi(lowered)
            }
            ast::Stmt::Return(ret) => {
                let value = ret
                    .value
                    .as_ref()
                    .map(|v| self.lower_expr(v, ctx, use_map, stmt_index));
                RustStmt::Return(RustReturnStmt {
                    value,
                    span: Some(ret.span),
                })
            }
            ast::Stmt::If(if_stmt) => {
                RustStmt::If(self.lower_if(if_stmt, ctx, use_map, stmt_index, reassigned))
            }
            ast::Stmt::While(while_stmt) => {
                RustStmt::While(self.lower_while(while_stmt, ctx, use_map, stmt_index, reassigned))
            }
        }
    }

    /// Lower a variable declaration.
    fn lower_var_decl(
        &self,
        decl: &ast::VarDecl,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustStmt {
        // Resolve the type from annotation or infer from literal
        let mut diags = Vec::new();
        let ty = if let Some(ann) = &decl.type_ann {
            types::resolve_type_annotation(ann, &mut diags)
        } else {
            types::infer_literal_type(&decl.init).unwrap_or(RustType::I64)
        };

        for d in diags {
            ctx.emit_diagnostic(d);
        }

        // Determine mutability:
        // - `const` declarations are never mutable
        // - `let` declarations are mutable only if the variable is reassigned
        let mutable = decl.binding == ast::VarBinding::Let && reassigned.contains(&decl.name.name);

        ctx.declare_variable(decl.name.name.clone(), ty.clone(), mutable);

        let init = self.lower_expr(&decl.init, ctx, use_map, stmt_index);

        RustStmt::Let(RustLetStmt {
            mutable,
            name: decl.name.name.clone(),
            ty: Some(ty),
            init,
            span: Some(decl.span),
        })
    }

    /// Lower an if statement.
    fn lower_if(
        &self,
        if_stmt: &ast::IfStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustIfStmt {
        let condition = self.lower_expr(&if_stmt.condition, ctx, use_map, stmt_index);
        let then_block =
            self.lower_block(&if_stmt.then_block, ctx, use_map, stmt_index, reassigned);
        let else_clause = if_stmt.else_clause.as_ref().map(|ec| match ec {
            ast::ElseClause::Block(block) => {
                RustElse::Block(self.lower_block(block, ctx, use_map, stmt_index, reassigned))
            }
            ast::ElseClause::ElseIf(nested_if) => RustElse::ElseIf(Box::new(
                self.lower_if(nested_if, ctx, use_map, stmt_index, reassigned),
            )),
        });

        RustIfStmt {
            condition,
            then_block,
            else_clause,
            span: Some(if_stmt.span),
        }
    }

    /// Lower a while statement.
    fn lower_while(
        &self,
        while_stmt: &ast::WhileStmt,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        stmt_index: usize,
        reassigned: &std::collections::HashSet<String>,
    ) -> RustWhileStmt {
        let condition = self.lower_expr(&while_stmt.condition, ctx, use_map, stmt_index);
        let body = self.lower_block(&while_stmt.body, ctx, use_map, stmt_index, reassigned);

        RustWhileStmt {
            condition,
            body,
            span: Some(while_stmt.span),
        }
    }

    /// Lower an expression.
    fn lower_expr(
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
                self.lower_ident_ref(ident, expr.span, ctx, use_map, stmt_index)
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
                let args: Vec<RustExpr> = call
                    .args
                    .iter()
                    .map(|a| self.lower_expr(a, ctx, use_map, stmt_index))
                    .collect();
                RustExpr::new(
                    RustExprKind::Call {
                        func: call.callee.name.clone(),
                        args,
                    },
                    expr.span,
                )
            }
            ast::ExprKind::MethodCall(mc) => {
                self.lower_method_call(mc, expr.span, ctx, use_map, stmt_index)
            }
            ast::ExprKind::Paren(inner) => {
                let lowered = self.lower_expr(inner, ctx, use_map, stmt_index);
                RustExpr::new(RustExprKind::Paren(Box::new(lowered)), expr.span)
            }
            ast::ExprKind::Assign(assign) => {
                let value = self.lower_expr(&assign.value, ctx, use_map, stmt_index);
                RustExpr::new(
                    RustExprKind::Assign {
                        target: assign.target.name.clone(),
                        value: Box::new(value),
                    },
                    expr.span,
                )
            }
        }
    }

    /// Lower an identifier reference, inserting a clone if needed.
    #[allow(clippy::unused_self)] // Method for consistency with other lower_* methods
    fn lower_ident_ref(
        &self,
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
                args,
            },
            span,
        )
    }
}

/// Map a `RustScript` binary operator to a Rust binary operator.
fn lower_binary_op(op: ast::BinaryOp) -> RustBinaryOp {
    match op {
        ast::BinaryOp::Add => RustBinaryOp::Add,
        ast::BinaryOp::Sub => RustBinaryOp::Sub,
        ast::BinaryOp::Mul => RustBinaryOp::Mul,
        ast::BinaryOp::Div => RustBinaryOp::Div,
        ast::BinaryOp::Mod => RustBinaryOp::Rem,
        ast::BinaryOp::Eq => RustBinaryOp::Eq,
        ast::BinaryOp::Ne => RustBinaryOp::Ne,
        ast::BinaryOp::Lt => RustBinaryOp::Lt,
        ast::BinaryOp::Gt => RustBinaryOp::Gt,
        ast::BinaryOp::Le => RustBinaryOp::Le,
        ast::BinaryOp::Ge => RustBinaryOp::Ge,
        ast::BinaryOp::And => RustBinaryOp::And,
        ast::BinaryOp::Or => RustBinaryOp::Or,
    }
}

/// Map a `RustScript` unary operator to a Rust unary operator.
fn lower_unary_op(op: ast::UnaryOp) -> RustUnaryOp {
    match op {
        ast::UnaryOp::Neg => RustUnaryOp::Neg,
        ast::UnaryOp::Not => RustUnaryOp::Not,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::ast::*;
    use rsc_syntax::span::Span;

    fn span(start: u32, end: u32) -> Span {
        Span::new(start, end)
    }

    fn ident(name: &str, start: u32, end: u32) -> Ident {
        Ident {
            name: name.to_owned(),
            span: span(start, end),
        }
    }

    fn int_expr(value: i64, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::IntLit(value),
            span: span(start, end),
        }
    }

    fn ident_expr(name: &str, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::Ident(ident(name, start, end)),
            span: span(start, end),
        }
    }

    fn string_expr(s: &str, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::StringLit(s.to_owned()),
            span: span(start, end),
        }
    }

    fn make_module(items: Vec<Item>) -> Module {
        Module {
            items,
            span: span(0, 100),
        }
    }

    fn make_fn(
        name: &str,
        params: Vec<Param>,
        return_type: Option<TypeAnnotation>,
        body: Vec<Stmt>,
    ) -> FnDecl {
        FnDecl {
            name: ident(name, 0, name.len() as u32),
            params,
            return_type,
            body: Block {
                stmts: body,
                span: span(0, 100),
            },
            span: span(0, 100),
        }
    }

    fn make_param(name: &str, type_name: &str) -> Param {
        Param {
            name: ident(name, 0, name.len() as u32),
            type_ann: TypeAnnotation {
                kind: TypeKind::Named(ident(type_name, 0, type_name.len() as u32)),
                span: span(0, type_name.len() as u32),
            },
            span: span(0, 10),
        }
    }

    // Test 15: Lower empty function main()
    #[test]
    fn test_lower_empty_main_function() {
        let f = make_fn("main", vec![], None, vec![]);
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        assert_eq!(file.items.len(), 1);
        let RustItem::Function(func) = &file.items[0];
        assert_eq!(func.name, "main");
        assert!(func.params.is_empty());
        assert!(func.return_type.is_none());
        assert!(func.body.stmts.is_empty());
        assert!(func.span.is_some());
    }

    // Test 16: Lower function params (a: i32, b: string): bool
    #[test]
    fn test_lower_function_params_and_return_type() {
        let f = make_fn(
            "test",
            vec![make_param("a", "i32"), make_param("b", "string")],
            Some(TypeAnnotation {
                kind: TypeKind::Named(ident("bool", 0, 4)),
                span: span(0, 4),
            }),
            vec![],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0];
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.params[0].name, "a");
        assert_eq!(func.params[0].ty, RustType::I32);
        assert_eq!(func.params[1].name, "b");
        assert_eq!(func.params[1].ty, RustType::String);
        assert_eq!(func.return_type, Some(RustType::Bool));
    }

    // Test 17: Lower const x: i32 = 42
    #[test]
    fn test_lower_const_with_type_annotation() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("i32", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        assert_eq!(func.body.stmts.len(), 1);
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(!let_stmt.mutable);
                assert_eq!(let_stmt.ty, Some(RustType::I32));
                assert_eq!(let_stmt.name, "x");
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test 18: Lower let x = 42; x = 10; → let mut
    #[test]
    fn test_lower_let_with_reassignment_becomes_mut() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![
                Stmt::VarDecl(VarDecl {
                    binding: VarBinding::Let,
                    name: ident("x", 4, 5),
                    type_ann: None,
                    init: int_expr(42, 8, 10),
                    span: span(0, 11),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident("x", 12, 13),
                        value: Box::new(int_expr(10, 16, 18)),
                    }),
                    span: span(12, 19),
                }),
            ],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        assert_eq!(func.body.stmts.len(), 2);
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert!(
                    let_stmt.mutable,
                    "x should be mutable since it's reassigned"
                );
            }
            other => panic!("expected Let, got {other:?}"),
        }
        // Second stmt should be Semi(Assign)
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Assign { target, .. } => assert_eq!(target, "x"),
                other => panic!("expected Assign, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 19: Lower const x = 42; (no type ann) → infer i64
    #[test]
    fn test_lower_const_no_type_annotation_infers_i64() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: None,
                init: int_expr(42, 10, 12),
                span: span(0, 13),
            })],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(let_stmt.ty, Some(RustType::I64));
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    // Test 20: Lower console.log("hello") → println! via builtin registry
    #[test]
    fn test_lower_console_log_single_arg_produces_println_macro() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![string_expr("hello", 12, 19)],
                }),
                span: span(0, 20),
            })],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    assert_eq!(args.len(), 2);
                    match &args[0].kind {
                        RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{}"),
                        other => panic!("expected StringLit format, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 21: Lower console.log(x, y) → println! with format string "{} {}"
    #[test]
    fn test_lower_console_log_two_args_format_string() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    object: Box::new(ident_expr("console", 0, 7)),
                    method: ident("log", 8, 11),
                    args: vec![ident_expr("x", 12, 13), ident_expr("y", 15, 16)],
                }),
                span: span(0, 17),
            })],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    assert_eq!(args.len(), 3); // format string + 2 args
                    match &args[0].kind {
                        RustExprKind::StringLit(fmt) => assert_eq!(fmt, "{} {}"),
                        other => panic!("expected StringLit format, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 22: Lower if (x > 0) { ... } → RustIfStmt sans parens
    #[test]
    fn test_lower_if_statement_with_condition() {
        let f = make_fn(
            "main",
            vec![make_param("x", "i32")],
            None,
            vec![Stmt::If(IfStmt {
                condition: Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Gt,
                        left: Box::new(ident_expr("x", 4, 5)),
                        right: Box::new(int_expr(0, 8, 9)),
                    }),
                    span: span(4, 9),
                },
                then_block: Block {
                    stmts: vec![],
                    span: span(11, 13),
                },
                else_clause: None,
                span: span(0, 13),
            })],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        match &func.body.stmts[0] {
            RustStmt::If(if_stmt) => {
                assert!(if_stmt.span.is_some());
                match &if_stmt.condition.kind {
                    RustExprKind::Binary { op, .. } => {
                        assert_eq!(*op, RustBinaryOp::Gt);
                    }
                    other => panic!("expected Binary, got {other:?}"),
                }
                assert!(if_stmt.else_clause.is_none());
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    // Test 23: Lower binary % → RustBinaryOp::Rem
    #[test]
    fn test_lower_binary_mod_to_rem() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Expr(Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Mod,
                    left: Box::new(int_expr(10, 0, 2)),
                    right: Box::new(int_expr(3, 5, 6)),
                }),
                span: span(0, 6),
            })],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Binary { op, .. } => assert_eq!(*op, RustBinaryOp::Rem),
                other => panic!("expected Binary, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Test 24: Lower return; → RustStmt::Return with value: None
    #[test]
    fn test_lower_bare_return() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::Return(ReturnStmt {
                value: None,
                span: span(0, 7),
            })],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        match &func.body.stmts[0] {
            RustStmt::Return(ret) => {
                assert!(ret.value.is_none());
                assert!(ret.span.is_some());
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // Test 25: Unknown type name → diagnostic emitted
    #[test]
    fn test_lower_unknown_type_emits_diagnostic() {
        let f = make_fn(
            "main",
            vec![],
            None,
            vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: Some(TypeAnnotation {
                    kind: TypeKind::Named(ident("Foo", 9, 12)),
                    span: span(9, 12),
                }),
                init: int_expr(42, 15, 17),
                span: span(0, 18),
            })],
        );
        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (_, diags) = transform.lower_module(&module);

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown type"));
        assert!(diags[0].message.contains("Foo"));
    }

    // Correctness Scenario 1: Fibonacci lowering
    #[test]
    fn test_correctness_fibonacci_lowering() {
        // function fib(n: i32): i32 {
        //   if (n <= 1) { return n; }
        //   return fib(n - 1) + fib(n - 2);
        // }
        let f = FnDecl {
            name: ident("fib", 0, 3),
            params: vec![make_param("n", "i32")],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Named(ident("i32", 0, 3)),
                span: span(0, 3),
            }),
            body: Block {
                stmts: vec![
                    Stmt::If(IfStmt {
                        condition: Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Le,
                                left: Box::new(ident_expr("n", 10, 11)),
                                right: Box::new(int_expr(1, 15, 16)),
                            }),
                            span: span(10, 16),
                        },
                        then_block: Block {
                            stmts: vec![Stmt::Return(ReturnStmt {
                                value: Some(ident_expr("n", 20, 21)),
                                span: span(18, 22),
                            })],
                            span: span(17, 23),
                        },
                        else_clause: None,
                        span: span(7, 23),
                    }),
                    Stmt::Return(ReturnStmt {
                        value: Some(Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Add,
                                left: Box::new(Expr {
                                    kind: ExprKind::Call(CallExpr {
                                        callee: ident("fib", 30, 33),
                                        args: vec![Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: BinaryOp::Sub,
                                                left: Box::new(ident_expr("n", 34, 35)),
                                                right: Box::new(int_expr(1, 38, 39)),
                                            }),
                                            span: span(34, 39),
                                        }],
                                    }),
                                    span: span(30, 40),
                                }),
                                right: Box::new(Expr {
                                    kind: ExprKind::Call(CallExpr {
                                        callee: ident("fib", 43, 46),
                                        args: vec![Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: BinaryOp::Sub,
                                                left: Box::new(ident_expr("n", 47, 48)),
                                                right: Box::new(int_expr(2, 51, 52)),
                                            }),
                                            span: span(47, 52),
                                        }],
                                    }),
                                    span: span(43, 53),
                                }),
                            }),
                            span: span(30, 53),
                        }),
                        span: span(24, 54),
                    }),
                ],
                span: span(5, 55),
            },
            span: span(0, 55),
        };

        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, diags) = transform.lower_module(&module);

        assert!(diags.is_empty());
        let RustItem::Function(func) = &file.items[0];
        assert_eq!(func.name, "fib");
        assert_eq!(func.params.len(), 1);
        assert_eq!(func.params[0].name, "n");
        assert_eq!(func.params[0].ty, RustType::I32);
        assert_eq!(func.return_type, Some(RustType::I32));
        assert_eq!(func.body.stmts.len(), 2);
        assert!(func.span.is_some());

        // Verify all spans are Some
        assert!(func.params[0].span.is_some());
        match &func.body.stmts[0] {
            RustStmt::If(if_stmt) => {
                assert!(if_stmt.span.is_some());
                assert!(if_stmt.condition.span.is_some());
            }
            other => panic!("expected If, got {other:?}"),
        }
        match &func.body.stmts[1] {
            RustStmt::Return(ret) => {
                assert!(ret.span.is_some());
                assert!(ret.value.as_ref().unwrap().span.is_some());
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    // Correctness Scenario 2: String - no clones for println! args
    #[test]
    fn test_correctness_no_clones_for_println_args() {
        // function example(name: string): void {
        //   console.log(name);   // stmt 0: NOT a move position
        //   console.log(name);   // stmt 1: NOT a move position
        // }
        let f = FnDecl {
            name: ident("example", 0, 7),
            params: vec![make_param("name", "string")],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            }),
            body: Block {
                stmts: vec![
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 30, 37)),
                            method: ident("log", 38, 41),
                            args: vec![ident_expr("name", 42, 46)],
                        }),
                        span: span(30, 47),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 50, 57)),
                            method: ident("log", 58, 61),
                            args: vec![ident_expr("name", 62, 66)],
                        }),
                        span: span(50, 67),
                    }),
                ],
                span: span(28, 68),
            },
            span: span(0, 68),
        };

        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        // Both statements should be println! macros with NO clones
        for (i, stmt) in func.body.stmts.iter().enumerate() {
            match stmt {
                RustStmt::Semi(expr) => match &expr.kind {
                    RustExprKind::Macro { name, args } => {
                        assert_eq!(name, "println");
                        // The second arg should be an Ident, not a Clone
                        assert!(
                            args.len() >= 2,
                            "stmt {i}: expected at least 2 args in println!"
                        );
                        match &args[1].kind {
                            RustExprKind::Ident(n) => assert_eq!(n, "name"),
                            RustExprKind::Clone(_) => {
                                panic!("stmt {i}: name should NOT be cloned for println!")
                            }
                            other => panic!("stmt {i}: expected Ident, got {other:?}"),
                        }
                    }
                    other => panic!("stmt {i}: expected Macro, got {other:?}"),
                },
                other => panic!("stmt {i}: expected Semi, got {other:?}"),
            }
        }
    }

    // Correctness Scenario 3: String clone when actually needed
    #[test]
    fn test_correctness_string_clone_at_move_point() {
        // function example(name: string): void {
        //   greet(name);          // stmt 0: move position, name used later → clone
        //   console.log(name);    // stmt 1: not a move position, no clone
        // }
        let f = FnDecl {
            name: ident("example", 0, 7),
            params: vec![make_param("name", "string")],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            }),
            body: Block {
                stmts: vec![
                    Stmt::Expr(Expr {
                        kind: ExprKind::Call(CallExpr {
                            callee: ident("greet", 30, 35),
                            args: vec![ident_expr("name", 36, 40)],
                        }),
                        span: span(30, 41),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::MethodCall(MethodCallExpr {
                            object: Box::new(ident_expr("console", 45, 52)),
                            method: ident("log", 53, 56),
                            args: vec![ident_expr("name", 57, 61)],
                        }),
                        span: span(45, 62),
                    }),
                ],
                span: span(28, 63),
            },
            span: span(0, 63),
        };

        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        assert_eq!(func.body.stmts.len(), 2);

        // stmt 0: greet(name.clone()) — name is in move position and used later
        match &func.body.stmts[0] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Call { func: f, args } => {
                    assert_eq!(f, "greet");
                    assert_eq!(args.len(), 1);
                    match &args[0].kind {
                        RustExprKind::Clone(inner) => match &inner.kind {
                            RustExprKind::Ident(n) => assert_eq!(n, "name"),
                            other => panic!("expected Ident inside Clone, got {other:?}"),
                        },
                        other => panic!("expected Clone, got {other:?}"),
                    }
                }
                other => panic!("expected Call, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }

        // stmt 1: println! — name is NOT cloned
        match &func.body.stmts[1] {
            RustStmt::Semi(expr) => match &expr.kind {
                RustExprKind::Macro { name, args } => {
                    assert_eq!(name, "println");
                    match &args[1].kind {
                        RustExprKind::Ident(n) => assert_eq!(n, "name"),
                        RustExprKind::Clone(_) => panic!("name should NOT be cloned in println!"),
                        other => panic!("expected Ident, got {other:?}"),
                    }
                }
                other => panic!("expected Macro, got {other:?}"),
            },
            other => panic!("expected Semi, got {other:?}"),
        }
    }

    // Correctness Scenario 4: Mutability detection
    #[test]
    fn test_correctness_mutability_detection() {
        // function counter(): void {
        //   let x = 0;
        //   const y = 10;
        //   x = x + 1;
        // }
        let f = FnDecl {
            name: ident("counter", 0, 7),
            params: vec![],
            return_type: Some(TypeAnnotation {
                kind: TypeKind::Void,
                span: span(0, 4),
            }),
            body: Block {
                stmts: vec![
                    Stmt::VarDecl(VarDecl {
                        binding: VarBinding::Let,
                        name: ident("x", 20, 21),
                        type_ann: None,
                        init: int_expr(0, 24, 25),
                        span: span(16, 26),
                    }),
                    Stmt::VarDecl(VarDecl {
                        binding: VarBinding::Const,
                        name: ident("y", 33, 34),
                        type_ann: None,
                        init: int_expr(10, 37, 39),
                        span: span(27, 40),
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::Assign(AssignExpr {
                            target: ident("x", 41, 42),
                            value: Box::new(Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Add,
                                    left: Box::new(ident_expr("x", 45, 46)),
                                    right: Box::new(int_expr(1, 49, 50)),
                                }),
                                span: span(45, 50),
                            }),
                        }),
                        span: span(41, 51),
                    }),
                ],
                span: span(14, 52),
            },
            span: span(0, 52),
        };

        let module = make_module(vec![Item::Function(f)]);
        let transform = Transform::new();
        let (file, _) = transform.lower_module(&module);

        let RustItem::Function(func) = &file.items[0];
        assert_eq!(func.body.stmts.len(), 3);

        // x is let mut (reassigned)
        match &func.body.stmts[0] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(let_stmt.name, "x");
                assert!(let_stmt.mutable, "x should be let mut (reassigned)");
            }
            other => panic!("expected Let for x, got {other:?}"),
        }

        // y is let (const, not reassigned)
        match &func.body.stmts[1] {
            RustStmt::Let(let_stmt) => {
                assert_eq!(let_stmt.name, "y");
                assert!(!let_stmt.mutable, "y should be let (const)");
            }
            other => panic!("expected Let for y, got {other:?}"),
        }
    }
}

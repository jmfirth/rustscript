//! Standard library dependency detection.
//!
//! Scans the AST for usage of standard library builtins that require
//! external crate dependencies: `JSON.stringify`/`JSON.parse` (`serde_json`)
//! and `Math.random()` (rand).

use rsc_syntax::ast;

/// Check if a module uses `JSON.stringify()` or `JSON.parse()`.
pub(super) fn module_needs_serde_json(module: &ast::Module) -> bool {
    module.items.iter().any(|item| match &item.kind {
        ast::ItemKind::Function(f) => block_uses_json(&f.body),
        ast::ItemKind::Class(cls) => cls.members.iter().any(|m| {
            if let ast::ClassMember::Method(method) = m {
                block_uses_json(&method.body)
            } else {
                false
            }
        }),
        _ => false,
    })
}

/// Check if a module uses `Math.random()`.
pub(super) fn module_needs_rand(module: &ast::Module) -> bool {
    module.items.iter().any(|item| match &item.kind {
        ast::ItemKind::Function(f) => block_uses_math_random(&f.body),
        ast::ItemKind::Class(cls) => cls.members.iter().any(|m| {
            if let ast::ClassMember::Method(method) = m {
                block_uses_math_random(&method.body)
            } else {
                false
            }
        }),
        _ => false,
    })
}

/// Recursively scan a block for `JSON.stringify` or `JSON.parse` usage.
fn block_uses_json(block: &ast::Block) -> bool {
    block.stmts.iter().any(stmt_uses_json)
}

/// Check if a statement uses JSON methods.
fn stmt_uses_json(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Expr(expr)
        | ast::Stmt::Return(ast::ReturnStmt {
            value: Some(expr), ..
        }) => expr_uses_json(expr),
        ast::Stmt::VarDecl(decl) => expr_uses_json(&decl.init),
        ast::Stmt::If(if_stmt) => {
            expr_uses_json(&if_stmt.condition)
                || block_uses_json(&if_stmt.then_block)
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::Block(b)) if block_uses_json(b))
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::ElseIf(elif)) if stmt_uses_json(&ast::Stmt::If(*elif.clone())))
        }
        ast::Stmt::While(w) => expr_uses_json(&w.condition) || block_uses_json(&w.body),
        ast::Stmt::For(f) => expr_uses_json(&f.iterable) || block_uses_json(&f.body),
        ast::Stmt::TryCatch(tc) => {
            block_uses_json(&tc.try_block)
                || tc.catch_block.as_ref().is_some_and(block_uses_json)
                || tc.finally_block.as_ref().is_some_and(block_uses_json)
        }
        _ => false,
    }
}

/// Check if an expression uses `JSON.stringify` or `JSON.parse`.
fn expr_uses_json(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::MethodCall(mc) => {
            if let ast::ExprKind::Ident(obj) = &mc.object.kind
                && crate::builtins::needs_serde_json(&obj.name, &mc.method.name)
            {
                return true;
            }
            // Check receiver and args recursively
            expr_uses_json(&mc.object) || mc.args.iter().any(expr_uses_json)
        }
        ast::ExprKind::Call(call) => call.args.iter().any(expr_uses_json),
        ast::ExprKind::Binary(bin) => expr_uses_json(&bin.left) || expr_uses_json(&bin.right),
        ast::ExprKind::Unary(un) => expr_uses_json(&un.operand),
        ast::ExprKind::Paren(inner) | ast::ExprKind::Await(inner) => expr_uses_json(inner),
        ast::ExprKind::Assign(assign) => expr_uses_json(&assign.value),
        _ => false,
    }
}

/// Recursively scan a block for `Math.random()` usage.
fn block_uses_math_random(block: &ast::Block) -> bool {
    block.stmts.iter().any(stmt_uses_math_random)
}

/// Check if a statement uses `Math.random()`.
fn stmt_uses_math_random(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Expr(expr)
        | ast::Stmt::Return(ast::ReturnStmt {
            value: Some(expr), ..
        }) => expr_uses_math_random(expr),
        ast::Stmt::VarDecl(decl) => expr_uses_math_random(&decl.init),
        ast::Stmt::If(if_stmt) => {
            expr_uses_math_random(&if_stmt.condition)
                || block_uses_math_random(&if_stmt.then_block)
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::Block(b)) if block_uses_math_random(b))
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::ElseIf(elif)) if stmt_uses_math_random(&ast::Stmt::If(*elif.clone())))
        }
        ast::Stmt::While(w) => {
            expr_uses_math_random(&w.condition) || block_uses_math_random(&w.body)
        }
        ast::Stmt::For(f) => expr_uses_math_random(&f.iterable) || block_uses_math_random(&f.body),
        ast::Stmt::TryCatch(tc) => {
            block_uses_math_random(&tc.try_block)
                || tc.catch_block.as_ref().is_some_and(block_uses_math_random)
                || tc
                    .finally_block
                    .as_ref()
                    .is_some_and(block_uses_math_random)
        }
        _ => false,
    }
}

/// Check if an expression uses `Math.random()`.
fn expr_uses_math_random(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::MethodCall(mc) => {
            if let ast::ExprKind::Ident(obj) = &mc.object.kind
                && crate::builtins::needs_rand_crate(&obj.name, &mc.method.name)
            {
                return true;
            }
            expr_uses_math_random(&mc.object) || mc.args.iter().any(expr_uses_math_random)
        }
        ast::ExprKind::Call(call) => call.args.iter().any(expr_uses_math_random),
        ast::ExprKind::Binary(bin) => {
            expr_uses_math_random(&bin.left) || expr_uses_math_random(&bin.right)
        }
        ast::ExprKind::Unary(un) => expr_uses_math_random(&un.operand),
        ast::ExprKind::Paren(inner) | ast::ExprKind::Await(inner) => expr_uses_math_random(inner),
        ast::ExprKind::Assign(assign) => expr_uses_math_random(&assign.value),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::ast::*;
    use rsc_syntax::span::Span;

    fn span() -> Span {
        Span::new(0, 10)
    }

    fn ident(name: &str) -> Ident {
        Ident {
            name: name.to_owned(),
            span: span(),
        }
    }

    fn ident_expr(name: &str) -> Expr {
        Expr {
            kind: ExprKind::Ident(ident(name)),
            span: span(),
        }
    }

    fn make_method_call(object: &str, method: &str) -> Stmt {
        Stmt::Expr(Expr {
            kind: ExprKind::MethodCall(MethodCallExpr {
                object: Box::new(ident_expr(object)),
                method: ident(method),
                args: vec![ident_expr("x")],
            }),
            span: span(),
        })
    }

    fn make_module_with_stmts(stmts: Vec<Stmt>) -> Module {
        Module {
            items: vec![Item {
                kind: ItemKind::Function(FnDecl {
                    is_async: false,
                    name: ident("main"),
                    type_params: None,
                    params: vec![],
                    return_type: None,
                    body: Block {
                        stmts,
                        span: span(),
                    },
                    doc_comment: None,
                    span: span(),
                }),
                exported: false,
                span: span(),
            }],
            span: span(),
        }
    }

    #[test]
    fn test_module_needs_serde_json_for_json_stringify() {
        let module = make_module_with_stmts(vec![make_method_call("JSON", "stringify")]);
        assert!(module_needs_serde_json(&module));
    }

    #[test]
    fn test_module_needs_serde_json_for_json_parse() {
        let module = make_module_with_stmts(vec![make_method_call("JSON", "parse")]);
        assert!(module_needs_serde_json(&module));
    }

    #[test]
    fn test_module_does_not_need_serde_json_for_console_log() {
        let module = make_module_with_stmts(vec![make_method_call("console", "log")]);
        assert!(!module_needs_serde_json(&module));
    }

    #[test]
    fn test_module_needs_rand_for_math_random() {
        let module = make_module_with_stmts(vec![make_method_call("Math", "random")]);
        assert!(module_needs_rand(&module));
    }

    #[test]
    fn test_module_does_not_need_rand_for_math_floor() {
        let module = make_module_with_stmts(vec![make_method_call("Math", "floor")]);
        assert!(!module_needs_rand(&module));
    }
}

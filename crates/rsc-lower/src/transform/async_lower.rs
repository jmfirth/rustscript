//! Async runtime detection utilities.
//!
//! Free functions that scan the AST for patterns requiring the tokio async
//! runtime: `spawn()` calls, `Promise.all()` usage, and `await` expressions.
//! Used by the top-level module lowering to determine whether the generated
//! Cargo.toml needs a tokio dependency.

use rsc_syntax::ast;

/// Check if a module contains `spawn()` calls or `Promise.all()` usage
/// that requires the async runtime, beyond just `async function` declarations.
pub(super) fn module_needs_async_runtime(module: &ast::Module) -> bool {
    for item in &module.items {
        if let ast::ItemKind::Function(f) = &item.kind
            && block_needs_async_runtime(&f.body)
        {
            return true;
        }
    }
    false
}

/// Recursively scan a block for `spawn()` calls or `Promise.all()` usage.
fn block_needs_async_runtime(block: &ast::Block) -> bool {
    for stmt in &block.stmts {
        if stmt_needs_async_runtime(stmt) {
            return true;
        }
    }
    false
}

/// Check if a statement contains async runtime patterns.
fn stmt_needs_async_runtime(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Expr(expr)
        | ast::Stmt::Return(ast::ReturnStmt {
            value: Some(expr), ..
        }) => expr_needs_async_runtime(expr),
        ast::Stmt::VarDecl(decl) => expr_needs_async_runtime(&decl.init),
        ast::Stmt::ArrayDestructure(adestr) => expr_needs_async_runtime(&adestr.init),
        ast::Stmt::If(if_stmt) => {
            expr_needs_async_runtime(&if_stmt.condition)
                || block_needs_async_runtime(&if_stmt.then_block)
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::Block(b)) if block_needs_async_runtime(b))
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::ElseIf(elif)) if stmt_needs_async_runtime(&ast::Stmt::If(*elif.clone())))
        }
        ast::Stmt::While(w) => {
            expr_needs_async_runtime(&w.condition) || block_needs_async_runtime(&w.body)
        }
        ast::Stmt::DoWhile(dw) => {
            block_needs_async_runtime(&dw.body) || expr_needs_async_runtime(&dw.condition)
        }
        ast::Stmt::For(f) => {
            f.is_await
                || expr_needs_async_runtime(&f.iterable)
                || block_needs_async_runtime(&f.body)
        }
        ast::Stmt::ForIn(f) => {
            expr_needs_async_runtime(&f.iterable) || block_needs_async_runtime(&f.body)
        }
        ast::Stmt::TryCatch(tc) => {
            block_needs_async_runtime(&tc.try_block)
                || tc
                    .catch_block
                    .as_ref()
                    .is_some_and(block_needs_async_runtime)
                || tc
                    .finally_block
                    .as_ref()
                    .is_some_and(block_needs_async_runtime)
        }
        _ => false,
    }
}

/// Check if an expression contains `spawn(...)` or `Promise.all(...)`.
fn expr_needs_async_runtime(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Call(call) if call.callee.name == "spawn" => true,
        ast::ExprKind::Await(inner) => {
            // Check for Promise.all/race/any patterns
            if let ast::ExprKind::MethodCall(mc) = &inner.kind
                && let ast::ExprKind::Ident(obj) = &mc.object.kind
                && obj.name == "Promise"
                && matches!(mc.method.name.as_str(), "all" | "race" | "any")
            {
                return true;
            }
            expr_needs_async_runtime(inner)
        }
        ast::ExprKind::Call(call) => call.args.iter().any(expr_needs_async_runtime),
        ast::ExprKind::MethodCall(mc) => {
            expr_needs_async_runtime(&mc.object) || mc.args.iter().any(expr_needs_async_runtime)
        }
        ast::ExprKind::Binary(bin) => {
            expr_needs_async_runtime(&bin.left) || expr_needs_async_runtime(&bin.right)
        }
        ast::ExprKind::Unary(un) => expr_needs_async_runtime(&un.operand),
        ast::ExprKind::Paren(inner) => expr_needs_async_runtime(inner),
        _ => false,
    }
}

/// Check if a module uses `for await` or `Promise.any` patterns that require
/// the `futures` crate dependency.
pub(super) fn module_needs_futures_crate(module: &ast::Module) -> bool {
    for item in &module.items {
        if let ast::ItemKind::Function(f) = &item.kind
            && block_needs_futures(&f.body)
        {
            return true;
        }
    }
    false
}

/// Recursively scan a block for patterns requiring the futures crate.
fn block_needs_futures(block: &ast::Block) -> bool {
    block.stmts.iter().any(stmt_needs_futures)
}

/// Check if a statement contains `for await` or `Promise.any` usage.
fn stmt_needs_futures(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::For(f) => f.is_await || block_needs_futures(&f.body),
        ast::Stmt::ForIn(f) => block_needs_futures(&f.body),
        ast::Stmt::Expr(expr)
        | ast::Stmt::Return(ast::ReturnStmt {
            value: Some(expr), ..
        }) => expr_needs_futures(expr),
        ast::Stmt::VarDecl(decl) => expr_needs_futures(&decl.init),
        ast::Stmt::ArrayDestructure(adestr) => expr_needs_futures(&adestr.init),
        ast::Stmt::If(if_stmt) => {
            expr_needs_futures(&if_stmt.condition)
                || block_needs_futures(&if_stmt.then_block)
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::Block(b)) if block_needs_futures(b))
        }
        ast::Stmt::While(w) => expr_needs_futures(&w.condition) || block_needs_futures(&w.body),
        ast::Stmt::DoWhile(dw) => {
            block_needs_futures(&dw.body) || expr_needs_futures(&dw.condition)
        }
        ast::Stmt::TryCatch(tc) => {
            block_needs_futures(&tc.try_block)
                || tc.catch_block.as_ref().is_some_and(block_needs_futures)
                || tc.finally_block.as_ref().is_some_and(block_needs_futures)
        }
        _ => false,
    }
}

/// Check if an expression contains `Promise.any(...)`.
fn expr_needs_futures(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Await(inner) => {
            if let ast::ExprKind::MethodCall(mc) = &inner.kind
                && let ast::ExprKind::Ident(obj) = &mc.object.kind
                && obj.name == "Promise"
                && mc.method.name == "any"
            {
                return true;
            }
            expr_needs_futures(inner)
        }
        ast::ExprKind::Call(call) => call.args.iter().any(expr_needs_futures),
        ast::ExprKind::MethodCall(mc) => {
            expr_needs_futures(&mc.object) || mc.args.iter().any(expr_needs_futures)
        }
        ast::ExprKind::Binary(bin) => {
            expr_needs_futures(&bin.left) || expr_needs_futures(&bin.right)
        }
        ast::ExprKind::Unary(un) => expr_needs_futures(&un.operand),
        ast::ExprKind::Paren(inner) => expr_needs_futures(inner),
        _ => false,
    }
}

/// Check if a block contains any `await` expression.
///
/// Used to determine if a try/catch IIFE needs to be an async closure.
pub(super) fn block_contains_await(block: &ast::Block) -> bool {
    block.stmts.iter().any(stmt_contains_await)
}

/// Check if a statement contains any `await` expression.
fn stmt_contains_await(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Expr(expr)
        | ast::Stmt::Return(ast::ReturnStmt {
            value: Some(expr), ..
        }) => expr_contains_await(expr),
        ast::Stmt::VarDecl(decl) => expr_contains_await(&decl.init),
        ast::Stmt::ArrayDestructure(adestr) => expr_contains_await(&adestr.init),
        ast::Stmt::If(if_stmt) => {
            expr_contains_await(&if_stmt.condition)
                || block_contains_await(&if_stmt.then_block)
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::Block(b)) if block_contains_await(b))
                || matches!(&if_stmt.else_clause, Some(ast::ElseClause::ElseIf(elif)) if stmt_contains_await(&ast::Stmt::If(*elif.clone())))
        }
        ast::Stmt::While(w) => expr_contains_await(&w.condition) || block_contains_await(&w.body),
        ast::Stmt::DoWhile(dw) => {
            block_contains_await(&dw.body) || expr_contains_await(&dw.condition)
        }
        ast::Stmt::For(f) => {
            f.is_await || expr_contains_await(&f.iterable) || block_contains_await(&f.body)
        }
        ast::Stmt::ForIn(f) => expr_contains_await(&f.iterable) || block_contains_await(&f.body),
        ast::Stmt::TryCatch(tc) => {
            block_contains_await(&tc.try_block)
                || tc.catch_block.as_ref().is_some_and(block_contains_await)
                || tc.finally_block.as_ref().is_some_and(block_contains_await)
        }
        _ => false,
    }
}

/// Check if an expression contains any `await` expression.
fn expr_contains_await(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Await(_) => true,
        ast::ExprKind::Call(call) => call.args.iter().any(expr_contains_await),
        ast::ExprKind::MethodCall(mc) => {
            expr_contains_await(&mc.object) || mc.args.iter().any(expr_contains_await)
        }
        ast::ExprKind::Binary(bin) => {
            expr_contains_await(&bin.left) || expr_contains_await(&bin.right)
        }
        ast::ExprKind::Unary(un) => expr_contains_await(&un.operand),
        ast::ExprKind::Paren(inner) => expr_contains_await(inner),
        _ => false,
    }
}

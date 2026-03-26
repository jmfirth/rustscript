//! Use declaration collection.
//!
//! Walks the generated Rust IR tree to find which `use` statements are needed,
//! currently scanning for `HashMap` and `HashSet` usage.

use rsc_syntax::rust_ir::{
    RustBlock, RustClosureBody, RustElse, RustExpr, RustExprKind, RustItem, RustMethod, RustStmt,
    RustType, RustUseDecl,
};

/// Scan generated items for usage of `HashMap`, `HashSet`, `Arc`, and `Mutex`
/// types and produce the corresponding `use` declarations.
pub(super) fn collect_use_declarations(items: &[RustItem]) -> Vec<RustUseDecl> {
    let mut needs_hashmap = false;
    let mut needs_hashset = false;
    let mut needs_arc_mutex = false;

    for item in items {
        scan_item_for_collections(item, &mut needs_hashmap, &mut needs_hashset);
        scan_item_for_arc_mutex(item, &mut needs_arc_mutex);
    }

    let mut uses = Vec::new();
    if needs_hashmap {
        uses.push(RustUseDecl {
            path: "std::collections::HashMap".to_owned(),
            public: false,
            span: None,
        });
    }
    if needs_hashset {
        uses.push(RustUseDecl {
            path: "std::collections::HashSet".to_owned(),
            public: false,
            span: None,
        });
    }
    if needs_arc_mutex {
        uses.push(RustUseDecl {
            path: "std::sync::Arc".to_owned(),
            public: false,
            span: None,
        });
        uses.push(RustUseDecl {
            path: "std::sync::Mutex".to_owned(),
            public: false,
            span: None,
        });
    }
    uses
}

/// Scan a single item for references to `HashMap` or `HashSet`.
fn scan_item_for_collections(item: &RustItem, needs_hashmap: &mut bool, needs_hashset: &mut bool) {
    match item {
        RustItem::Function(f) => {
            for p in &f.params {
                scan_type_for_collections(&p.ty, needs_hashmap, needs_hashset);
            }
            if let Some(ret) = &f.return_type {
                scan_type_for_collections(ret, needs_hashmap, needs_hashset);
            }
            scan_block_for_collections(&f.body, needs_hashmap, needs_hashset);
        }
        RustItem::Struct(s) => {
            for field in &s.fields {
                scan_type_for_collections(&field.ty, needs_hashmap, needs_hashset);
            }
        }
        RustItem::Enum(e) => {
            for variant in &e.variants {
                for field in &variant.fields {
                    scan_type_for_collections(&field.ty, needs_hashmap, needs_hashset);
                }
            }
        }
        RustItem::Trait(t) => {
            for method in &t.methods {
                for p in &method.params {
                    scan_type_for_collections(&p.ty, needs_hashmap, needs_hashset);
                }
                if let Some(ret) = &method.return_type {
                    scan_type_for_collections(ret, needs_hashmap, needs_hashset);
                }
            }
        }
        RustItem::Impl(imp) => {
            for method in &imp.methods {
                scan_method_for_collections(method, needs_hashmap, needs_hashset);
            }
        }
        RustItem::TraitImpl(ti) => {
            for method in &ti.methods {
                scan_method_for_collections(method, needs_hashmap, needs_hashset);
            }
        }
        RustItem::RawRust(_) => {
            // Raw Rust is opaque — we don't scan it for collection usage.
        }
        RustItem::Const(c) => {
            scan_type_for_collections(&c.ty, needs_hashmap, needs_hashset);
            scan_expr_for_collections(&c.init, needs_hashmap, needs_hashset);
        }
    }
}

/// Scan a method for `HashMap` or `HashSet` references.
fn scan_method_for_collections(
    method: &RustMethod,
    needs_hashmap: &mut bool,
    needs_hashset: &mut bool,
) {
    for p in &method.params {
        scan_type_for_collections(&p.ty, needs_hashmap, needs_hashset);
    }
    if let Some(ret) = &method.return_type {
        scan_type_for_collections(ret, needs_hashmap, needs_hashset);
    }
    scan_block_for_collections(&method.body, needs_hashmap, needs_hashset);
}

/// Scan a single item for references to `Arc<Mutex<T>>`.
fn scan_item_for_arc_mutex(item: &RustItem, needs_arc_mutex: &mut bool) {
    match item {
        RustItem::Function(f) => {
            for p in &f.params {
                scan_type_for_arc_mutex(&p.ty, needs_arc_mutex);
            }
            if let Some(ret) = &f.return_type {
                scan_type_for_arc_mutex(ret, needs_arc_mutex);
            }
            scan_block_for_arc_mutex(&f.body, needs_arc_mutex);
        }
        RustItem::Struct(s) => {
            for field in &s.fields {
                scan_type_for_arc_mutex(&field.ty, needs_arc_mutex);
            }
        }
        RustItem::Enum(e) => {
            for variant in &e.variants {
                for field in &variant.fields {
                    scan_type_for_arc_mutex(&field.ty, needs_arc_mutex);
                }
            }
        }
        RustItem::Trait(t) => {
            for method in &t.methods {
                for p in &method.params {
                    scan_type_for_arc_mutex(&p.ty, needs_arc_mutex);
                }
                if let Some(ret) = &method.return_type {
                    scan_type_for_arc_mutex(ret, needs_arc_mutex);
                }
            }
        }
        RustItem::Impl(imp) => {
            for method in &imp.methods {
                scan_method_for_arc_mutex(method, needs_arc_mutex);
            }
        }
        RustItem::TraitImpl(ti) => {
            for method in &ti.methods {
                scan_method_for_arc_mutex(method, needs_arc_mutex);
            }
        }
        RustItem::RawRust(_) => {}
        RustItem::Const(c) => {
            scan_type_for_arc_mutex(&c.ty, needs_arc_mutex);
            scan_expr_for_arc_mutex(&c.init, needs_arc_mutex);
        }
    }
}

/// Scan a method for `Arc<Mutex<T>>` references.
fn scan_method_for_arc_mutex(method: &RustMethod, needs_arc_mutex: &mut bool) {
    for p in &method.params {
        scan_type_for_arc_mutex(&p.ty, needs_arc_mutex);
    }
    if let Some(ret) = &method.return_type {
        scan_type_for_arc_mutex(ret, needs_arc_mutex);
    }
    scan_block_for_arc_mutex(&method.body, needs_arc_mutex);
}

/// Scan a type for `ArcMutex` references.
fn scan_type_for_arc_mutex(ty: &RustType, needs_arc_mutex: &mut bool) {
    match ty {
        RustType::ArcMutex(inner) => {
            *needs_arc_mutex = true;
            scan_type_for_arc_mutex(inner, needs_arc_mutex);
        }
        RustType::Generic(base, args) => {
            scan_type_for_arc_mutex(base, needs_arc_mutex);
            for arg in args {
                scan_type_for_arc_mutex(arg, needs_arc_mutex);
            }
        }
        RustType::Option(inner) => {
            scan_type_for_arc_mutex(inner, needs_arc_mutex);
        }
        RustType::Result(ok, err) => {
            scan_type_for_arc_mutex(ok, needs_arc_mutex);
            scan_type_for_arc_mutex(err, needs_arc_mutex);
        }
        _ => {}
    }
}

/// Scan a block for `Arc<Mutex<T>>` usage.
fn scan_block_for_arc_mutex(block: &RustBlock, needs_arc_mutex: &mut bool) {
    for stmt in &block.stmts {
        scan_stmt_for_arc_mutex(stmt, needs_arc_mutex);
    }
    if let Some(expr) = &block.expr {
        scan_expr_for_arc_mutex(expr, needs_arc_mutex);
    }
}

/// Scan a statement for `Arc<Mutex<T>>` usage.
fn scan_stmt_for_arc_mutex(stmt: &RustStmt, needs_arc_mutex: &mut bool) {
    match stmt {
        RustStmt::Let(let_stmt) => {
            if let Some(ty) = &let_stmt.ty {
                scan_type_for_arc_mutex(ty, needs_arc_mutex);
            }
            scan_expr_for_arc_mutex(&let_stmt.init, needs_arc_mutex);
        }
        RustStmt::Expr(expr) | RustStmt::Semi(expr) => {
            scan_expr_for_arc_mutex(expr, needs_arc_mutex);
        }
        RustStmt::Return(ret) => {
            if let Some(val) = &ret.value {
                scan_expr_for_arc_mutex(val, needs_arc_mutex);
            }
        }
        _ => {}
    }
}

/// Scan an expression for `ArcMutexNew` usage.
fn scan_expr_for_arc_mutex(expr: &RustExpr, needs_arc_mutex: &mut bool) {
    match &expr.kind {
        RustExprKind::ArcMutexNew(inner) => {
            *needs_arc_mutex = true;
            scan_expr_for_arc_mutex(inner, needs_arc_mutex);
        }
        RustExprKind::Binary { left, right, .. } => {
            scan_expr_for_arc_mutex(left, needs_arc_mutex);
            scan_expr_for_arc_mutex(right, needs_arc_mutex);
        }
        RustExprKind::Call { args, .. } | RustExprKind::Macro { args, .. } => {
            for arg in args {
                scan_expr_for_arc_mutex(arg, needs_arc_mutex);
            }
        }
        RustExprKind::MethodCall { receiver, args, .. } => {
            scan_expr_for_arc_mutex(receiver, needs_arc_mutex);
            for arg in args {
                scan_expr_for_arc_mutex(arg, needs_arc_mutex);
            }
        }
        RustExprKind::Paren(inner)
        | RustExprKind::Clone(inner)
        | RustExprKind::Borrow(inner)
        | RustExprKind::ToString(inner)
        | RustExprKind::Some(inner)
        | RustExprKind::QuestionMark(inner)
        | RustExprKind::Ok(inner)
        | RustExprKind::Err(inner)
        | RustExprKind::Await(inner)
        | RustExprKind::Cast(inner, _) => {
            scan_expr_for_arc_mutex(inner, needs_arc_mutex);
        }
        RustExprKind::Assign { value, .. }
        | RustExprKind::CompoundAssign { value, .. }
        | RustExprKind::SelfFieldAssign { value, .. } => {
            scan_expr_for_arc_mutex(value, needs_arc_mutex);
        }
        _ => {}
    }
}

/// Scan a type for `HashMap` or `HashSet` references.
fn scan_type_for_collections(ty: &RustType, needs_hashmap: &mut bool, needs_hashset: &mut bool) {
    match ty {
        RustType::Named(name) => {
            if name == "HashMap" {
                *needs_hashmap = true;
            } else if name == "HashSet" {
                *needs_hashset = true;
            }
        }
        RustType::Generic(base, args) => {
            scan_type_for_collections(base, needs_hashmap, needs_hashset);
            for arg in args {
                scan_type_for_collections(arg, needs_hashmap, needs_hashset);
            }
        }
        RustType::Option(inner) | RustType::ArcMutex(inner) => {
            scan_type_for_collections(inner, needs_hashmap, needs_hashset);
        }
        RustType::Result(ok, err) => {
            scan_type_for_collections(ok, needs_hashmap, needs_hashset);
            scan_type_for_collections(err, needs_hashmap, needs_hashset);
        }
        _ => {}
    }
}

/// Scan a block for `HashMap` or `HashSet` usage in expressions and statements.
fn scan_block_for_collections(
    block: &RustBlock,
    needs_hashmap: &mut bool,
    needs_hashset: &mut bool,
) {
    for stmt in &block.stmts {
        scan_stmt_for_collections(stmt, needs_hashmap, needs_hashset);
    }
    if let Some(expr) = &block.expr {
        scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
    }
}

/// Scan a statement for `HashMap` or `HashSet` usage.
fn scan_stmt_for_collections(stmt: &RustStmt, needs_hashmap: &mut bool, needs_hashset: &mut bool) {
    match stmt {
        RustStmt::Let(let_stmt) => {
            if let Some(ty) = &let_stmt.ty {
                scan_type_for_collections(ty, needs_hashmap, needs_hashset);
            }
            scan_expr_for_collections(&let_stmt.init, needs_hashmap, needs_hashset);
        }
        RustStmt::Expr(expr) | RustStmt::Semi(expr) => {
            scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
        }
        RustStmt::Return(ret) => {
            if let Some(val) = &ret.value {
                scan_expr_for_collections(val, needs_hashmap, needs_hashset);
            }
        }
        RustStmt::If(if_stmt) => {
            scan_expr_for_collections(&if_stmt.condition, needs_hashmap, needs_hashset);
            scan_block_for_collections(&if_stmt.then_block, needs_hashmap, needs_hashset);
            if let Some(else_clause) = &if_stmt.else_clause {
                match else_clause {
                    RustElse::Block(block) => {
                        scan_block_for_collections(block, needs_hashmap, needs_hashset);
                    }
                    RustElse::ElseIf(nested_if) => {
                        scan_expr_for_collections(
                            &nested_if.condition,
                            needs_hashmap,
                            needs_hashset,
                        );
                        scan_block_for_collections(
                            &nested_if.then_block,
                            needs_hashmap,
                            needs_hashset,
                        );
                    }
                }
            }
        }
        RustStmt::While(while_stmt) => {
            scan_expr_for_collections(&while_stmt.condition, needs_hashmap, needs_hashset);
            scan_block_for_collections(&while_stmt.body, needs_hashmap, needs_hashset);
        }
        RustStmt::Destructure(destr) => {
            scan_expr_for_collections(&destr.init, needs_hashmap, needs_hashset);
        }
        RustStmt::Match(match_stmt) => {
            scan_expr_for_collections(&match_stmt.scrutinee, needs_hashmap, needs_hashset);
            for arm in &match_stmt.arms {
                scan_block_for_collections(&arm.body, needs_hashmap, needs_hashset);
            }
        }
        RustStmt::IfLet(if_let) => {
            scan_expr_for_collections(&if_let.expr, needs_hashmap, needs_hashset);
            scan_block_for_collections(&if_let.then_block, needs_hashmap, needs_hashset);
            if let Some(else_block) = &if_let.else_block {
                scan_block_for_collections(else_block, needs_hashmap, needs_hashset);
            }
        }
        RustStmt::MatchResult(match_result) => {
            scan_expr_for_collections(&match_result.expr, needs_hashmap, needs_hashset);
            scan_block_for_collections(&match_result.ok_block, needs_hashmap, needs_hashset);
            scan_block_for_collections(&match_result.err_block, needs_hashmap, needs_hashset);
            for s in &match_result.finally_stmts {
                scan_stmt_for_collections(s, needs_hashmap, needs_hashset);
            }
        }
        RustStmt::ForIn(for_in) => {
            scan_expr_for_collections(&for_in.iterable, needs_hashmap, needs_hashset);
            scan_block_for_collections(&for_in.body, needs_hashmap, needs_hashset);
        }
        RustStmt::LetElse(let_else) => {
            scan_expr_for_collections(&let_else.expr, needs_hashmap, needs_hashset);
            scan_block_for_collections(&let_else.else_block, needs_hashmap, needs_hashset);
        }
        RustStmt::TupleDestructure(td) => {
            scan_expr_for_collections(&td.init, needs_hashmap, needs_hashset);
        }
        RustStmt::TryFinally(tf) => {
            scan_block_for_collections(&tf.try_block, needs_hashmap, needs_hashset);
            for s in &tf.finally_stmts {
                scan_stmt_for_collections(s, needs_hashmap, needs_hashset);
            }
        }
        RustStmt::Break(_) | RustStmt::Continue(_) | RustStmt::RawRust(_) => {}
    }
}

#[allow(clippy::too_many_lines)]
// Expression scanning covers all IR node kinds; splitting would obscure the match structure
/// Scan an expression for `HashMap` or `HashSet` usage.
fn scan_expr_for_collections(expr: &RustExpr, needs_hashmap: &mut bool, needs_hashset: &mut bool) {
    match &expr.kind {
        RustExprKind::StaticCall {
            type_name, args, ..
        } => {
            if type_name == "HashMap" {
                *needs_hashmap = true;
            } else if type_name == "HashSet" {
                *needs_hashset = true;
            }
            for arg in args {
                scan_expr_for_collections(arg, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::VecLit(elems) => {
            for elem in elems {
                scan_expr_for_collections(elem, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::Index { object, index } => {
            scan_expr_for_collections(object, needs_hashmap, needs_hashset);
            scan_expr_for_collections(index, needs_hashmap, needs_hashset);
        }
        RustExprKind::Binary { left, right, .. } => {
            scan_expr_for_collections(left, needs_hashmap, needs_hashset);
            scan_expr_for_collections(right, needs_hashmap, needs_hashset);
        }
        RustExprKind::Unary { operand, .. } => {
            scan_expr_for_collections(operand, needs_hashmap, needs_hashset);
        }
        RustExprKind::Call { args, .. } | RustExprKind::Macro { args, .. } => {
            for arg in args {
                scan_expr_for_collections(arg, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::MethodCall { receiver, args, .. } => {
            scan_expr_for_collections(receiver, needs_hashmap, needs_hashset);
            for arg in args {
                scan_expr_for_collections(arg, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::Paren(inner)
        | RustExprKind::Clone(inner)
        | RustExprKind::Borrow(inner)
        | RustExprKind::ToString(inner)
        | RustExprKind::Some(inner)
        | RustExprKind::QuestionMark(inner)
        | RustExprKind::Ok(inner)
        | RustExprKind::Err(inner)
        | RustExprKind::Await(inner)
        | RustExprKind::ArcMutexNew(inner)
        | RustExprKind::Cast(inner, _) => {
            scan_expr_for_collections(inner, needs_hashmap, needs_hashset);
        }
        RustExprKind::Assign { value, .. }
        | RustExprKind::CompoundAssign { value, .. }
        | RustExprKind::SelfFieldAssign { value, .. } => {
            scan_expr_for_collections(value, needs_hashmap, needs_hashset);
        }
        RustExprKind::StructLit { fields, .. } | RustExprKind::SelfStructLit { fields } => {
            for (_, val) in fields {
                scan_expr_for_collections(val, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::FieldAccess { object, .. } => {
            scan_expr_for_collections(object, needs_hashmap, needs_hashset);
        }
        RustExprKind::IntLit(_)
        | RustExprKind::FloatLit(_)
        | RustExprKind::StringLit(_)
        | RustExprKind::BoolLit(_)
        | RustExprKind::Ident(_)
        | RustExprKind::EnumVariant { .. }
        | RustExprKind::None
        | RustExprKind::SelfRef
        | RustExprKind::SelfFieldAccess { .. } => {}
        RustExprKind::UnwrapOr { expr, default } => {
            scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
            scan_expr_for_collections(default, needs_hashmap, needs_hashset);
        }
        RustExprKind::OptionMap {
            expr, closure_body, ..
        } => {
            scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
            scan_expr_for_collections(closure_body, needs_hashmap, needs_hashset);
        }
        RustExprKind::ClosureCall { body, .. } | RustExprKind::AsyncBlock { body, .. } => {
            scan_block_for_collections(body, needs_hashmap, needs_hashset);
        }
        RustExprKind::Closure { body, .. } => match body {
            RustClosureBody::Expr(expr) => {
                scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
            }
            RustClosureBody::Block(block) => {
                scan_block_for_collections(block, needs_hashmap, needs_hashset);
            }
        },
        RustExprKind::TokioJoin(exprs) => {
            for expr in exprs {
                scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
            }
        }
        RustExprKind::IfExpr {
            condition,
            then_expr,
            else_expr,
        } => {
            scan_expr_for_collections(condition, needs_hashmap, needs_hashset);
            scan_expr_for_collections(then_expr, needs_hashmap, needs_hashset);
            scan_expr_for_collections(else_expr, needs_hashmap, needs_hashset);
        }
        RustExprKind::IteratorChain {
            source,
            ops,
            terminal,
        } => {
            scan_expr_for_collections(source, needs_hashmap, needs_hashset);
            for op in ops {
                match op {
                    rsc_syntax::rust_ir::IteratorOp::Map(_, body)
                    | rsc_syntax::rust_ir::IteratorOp::Filter(_, body) => {
                        scan_expr_for_collections(body, needs_hashmap, needs_hashset);
                    }
                    rsc_syntax::rust_ir::IteratorOp::MapFnRef(fn_expr)
                    | rsc_syntax::rust_ir::IteratorOp::FilterFnRef(fn_expr) => {
                        scan_expr_for_collections(fn_expr, needs_hashmap, needs_hashset);
                    }
                    rsc_syntax::rust_ir::IteratorOp::Cloned => {}
                }
            }
            match terminal {
                rsc_syntax::rust_ir::IteratorTerminal::CollectVec => {}
                rsc_syntax::rust_ir::IteratorTerminal::Fold { init, body, .. } => {
                    scan_expr_for_collections(init, needs_hashmap, needs_hashset);
                    match body {
                        RustClosureBody::Expr(expr) => {
                            scan_expr_for_collections(expr, needs_hashmap, needs_hashset);
                        }
                        RustClosureBody::Block(block) => {
                            scan_block_for_collections(block, needs_hashmap, needs_hashset);
                        }
                    }
                }
                rsc_syntax::rust_ir::IteratorTerminal::Find(_, body)
                | rsc_syntax::rust_ir::IteratorTerminal::Any(_, body)
                | rsc_syntax::rust_ir::IteratorTerminal::All(_, body)
                | rsc_syntax::rust_ir::IteratorTerminal::ForEach(_, body) => {
                    scan_expr_for_collections(body, needs_hashmap, needs_hashset);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::rust_ir::{
        RustBlock, RustExpr, RustExprKind, RustFnDecl, RustLetStmt, RustStmt, RustType,
    };

    /// Helper: make a minimal function item with the given body statements.
    fn make_fn_with_stmts(stmts: Vec<RustStmt>) -> RustItem {
        RustItem::Function(RustFnDecl {
            attributes: vec![],
            is_async: false,
            public: false,
            name: "test".to_owned(),
            type_params: vec![],
            params: vec![],
            return_type: None,
            body: RustBlock { stmts, expr: None },
            doc_comment: None,
            span: None,
        })
    }

    #[test]
    fn test_use_collector_detects_arc_mutex_type_in_let_stmt() {
        let items = vec![make_fn_with_stmts(vec![RustStmt::Let(RustLetStmt {
            mutable: false,
            name: "counter".to_owned(),
            ty: Some(RustType::ArcMutex(Box::new(RustType::I32))),
            init: RustExpr::synthetic(RustExprKind::ArcMutexNew(Box::new(RustExpr::synthetic(
                RustExprKind::IntLit(0),
            )))),
            span: None,
        })])];

        let uses = collect_use_declarations(&items);
        let paths: Vec<&str> = uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            paths.contains(&"std::sync::Arc"),
            "expected Arc use, got: {paths:?}"
        );
        assert!(
            paths.contains(&"std::sync::Mutex"),
            "expected Mutex use, got: {paths:?}"
        );
    }

    #[test]
    fn test_use_collector_no_arc_mutex_when_not_used() {
        let items = vec![make_fn_with_stmts(vec![RustStmt::Let(RustLetStmt {
            mutable: false,
            name: "x".to_owned(),
            ty: Some(RustType::I32),
            init: RustExpr::synthetic(RustExprKind::IntLit(42)),
            span: None,
        })])];

        let uses = collect_use_declarations(&items);
        let paths: Vec<&str> = uses.iter().map(|u| u.path.as_str()).collect();
        assert!(
            !paths.contains(&"std::sync::Arc"),
            "unexpected Arc use when not needed"
        );
    }
}

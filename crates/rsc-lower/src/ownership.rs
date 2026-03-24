//! Ownership analysis for Tier 1 clone insertion.
//!
//! Implements last-use analysis to determine when a `String`-typed variable
//! needs to be cloned at a move point. This module is the future extraction
//! seam for the full `rsc-ownership` crate.

use std::collections::{HashMap, HashSet};

use rsc_syntax::ast;
use rsc_syntax::rust_ir::RustType;

/// A record of where a variable is used within a function body.
pub(crate) struct UseLocation {
    /// Index of the statement within the enclosing block.
    pub stmt_index: usize,
    /// Whether this use is in a "move position" (function call argument
    /// where the callee takes ownership). `println!` args are NOT move
    /// positions because `println!` takes references.
    pub is_move_position: bool,
}

/// Map from variable name to ordered list of use locations.
pub(crate) struct UseMap {
    uses: HashMap<String, Vec<UseLocation>>,
}

impl UseMap {
    /// Build a `UseMap` by scanning a function body.
    ///
    /// `is_ref_call` is a predicate that returns true if a method call's
    /// arguments are passed by reference (not moved). This decouples ownership
    /// analysis from the builtin registry.
    pub fn analyze(body: &ast::Block, is_ref_call: impl Fn(&str, &str) -> bool) -> Self {
        let mut uses: HashMap<String, Vec<UseLocation>> = HashMap::new();

        for (stmt_index, stmt) in body.stmts.iter().enumerate() {
            Self::collect_stmt_uses(stmt, stmt_index, &is_ref_call, &mut uses);
        }

        Self { uses }
    }

    /// Look up all uses of a variable.
    pub fn get_uses(&self, var_name: &str) -> Option<&Vec<UseLocation>> {
        self.uses.get(var_name)
    }

    /// Collect variable uses from a statement.
    fn collect_stmt_uses(
        stmt: &ast::Stmt,
        stmt_index: usize,
        is_ref_call: &impl Fn(&str, &str) -> bool,
        uses: &mut HashMap<String, Vec<UseLocation>>,
    ) {
        match stmt {
            ast::Stmt::VarDecl(decl) => {
                // The initializer expression may reference variables
                Self::collect_expr_uses(&decl.init, stmt_index, false, is_ref_call, uses);
            }
            ast::Stmt::Expr(expr) => {
                Self::collect_expr_uses(expr, stmt_index, false, is_ref_call, uses);
            }
            ast::Stmt::Return(ret) => {
                if let Some(value) = &ret.value {
                    Self::collect_expr_uses(value, stmt_index, false, is_ref_call, uses);
                }
            }
            ast::Stmt::If(if_stmt) => {
                Self::collect_if_uses(if_stmt, stmt_index, is_ref_call, uses);
            }
            ast::Stmt::While(while_stmt) => {
                Self::collect_expr_uses(
                    &while_stmt.condition,
                    stmt_index,
                    false,
                    is_ref_call,
                    uses,
                );
                for inner_stmt in &while_stmt.body.stmts {
                    Self::collect_stmt_uses(inner_stmt, stmt_index, is_ref_call, uses);
                }
            }
        }
    }

    /// Collect uses from an if statement.
    fn collect_if_uses(
        if_stmt: &ast::IfStmt,
        stmt_index: usize,
        is_ref_call: &impl Fn(&str, &str) -> bool,
        uses: &mut HashMap<String, Vec<UseLocation>>,
    ) {
        Self::collect_expr_uses(&if_stmt.condition, stmt_index, false, is_ref_call, uses);
        for inner_stmt in &if_stmt.then_block.stmts {
            Self::collect_stmt_uses(inner_stmt, stmt_index, is_ref_call, uses);
        }
        if let Some(else_clause) = &if_stmt.else_clause {
            match else_clause {
                ast::ElseClause::Block(block) => {
                    for inner_stmt in &block.stmts {
                        Self::collect_stmt_uses(inner_stmt, stmt_index, is_ref_call, uses);
                    }
                }
                ast::ElseClause::ElseIf(nested_if) => {
                    Self::collect_if_uses(nested_if, stmt_index, is_ref_call, uses);
                }
            }
        }
    }

    /// Collect variable uses from an expression.
    ///
    /// `in_move_position` is true when this expression is a function call
    /// argument in a non-ref call.
    fn collect_expr_uses(
        expr: &ast::Expr,
        stmt_index: usize,
        in_move_position: bool,
        is_ref_call: &impl Fn(&str, &str) -> bool,
        uses: &mut HashMap<String, Vec<UseLocation>>,
    ) {
        match &expr.kind {
            ast::ExprKind::Ident(ident) => {
                uses.entry(ident.name.clone())
                    .or_default()
                    .push(UseLocation {
                        stmt_index,
                        is_move_position: in_move_position,
                    });
            }
            ast::ExprKind::Binary(bin) => {
                Self::collect_expr_uses(&bin.left, stmt_index, false, is_ref_call, uses);
                Self::collect_expr_uses(&bin.right, stmt_index, false, is_ref_call, uses);
            }
            ast::ExprKind::Unary(un) => {
                Self::collect_expr_uses(&un.operand, stmt_index, false, is_ref_call, uses);
            }
            ast::ExprKind::Call(call) => {
                // Regular function calls: arguments are in move position
                for arg in &call.args {
                    Self::collect_expr_uses(arg, stmt_index, true, is_ref_call, uses);
                }
            }
            ast::ExprKind::MethodCall(mc) => {
                // Check if this is a builtin that takes refs
                let obj_name = match &mc.object.kind {
                    ast::ExprKind::Ident(ident) => Some(ident.name.as_str()),
                    _ => None,
                };
                let is_ref = obj_name.is_some_and(|obj| is_ref_call(obj, &mc.method.name));
                let arg_move = !is_ref;

                // The object itself may be a variable reference
                Self::collect_expr_uses(&mc.object, stmt_index, false, is_ref_call, uses);
                for arg in &mc.args {
                    Self::collect_expr_uses(arg, stmt_index, arg_move, is_ref_call, uses);
                }
            }
            ast::ExprKind::Paren(inner) => {
                Self::collect_expr_uses(inner, stmt_index, in_move_position, is_ref_call, uses);
            }
            ast::ExprKind::Assign(assign) => {
                // The target is not a "use" in the ownership sense (it's being written to)
                // The value side may reference variables
                Self::collect_expr_uses(&assign.value, stmt_index, false, is_ref_call, uses);
            }
            ast::ExprKind::IntLit(_)
            | ast::ExprKind::FloatLit(_)
            | ast::ExprKind::StringLit(_)
            | ast::ExprKind::BoolLit(_) => {}
        }
    }
}

/// Determine whether a variable reference at the given position needs cloning.
///
/// A clone is needed when:
/// 1. The variable's type is not `Copy` (i.e., it's `String`)
/// 2. The current use is in a move position
/// 3. There exists a later use of the same variable
pub(crate) fn needs_clone(
    var_name: &str,
    current_stmt_index: usize,
    use_map: &UseMap,
    var_type: &RustType,
) -> bool {
    // Copy types never need cloning
    if is_copy_type(var_type) {
        return false;
    }

    // Look up the current use in the map to check if it's a move position
    let Some(locations) = use_map.get_uses(var_name) else {
        return false;
    };

    // Find the current use — we need to check if IT is a move position
    let current_is_move = locations
        .iter()
        .any(|loc| loc.stmt_index == current_stmt_index && loc.is_move_position);

    if !current_is_move {
        return false;
    }

    // Check if there are any later uses
    locations
        .iter()
        .any(|loc| loc.stmt_index > current_stmt_index)
}

/// Determine which `let` variables are reassigned in a block.
///
/// Walks the block looking for assignment expressions and returns the set
/// of variable names that appear as assignment targets.
pub(crate) fn find_reassigned_variables(body: &ast::Block) -> HashSet<String> {
    let mut reassigned = HashSet::new();
    for stmt in &body.stmts {
        collect_assignments(stmt, &mut reassigned);
    }
    reassigned
}

/// Check whether a Rust type implements `Copy`.
fn is_copy_type(ty: &RustType) -> bool {
    matches!(
        ty,
        RustType::I32 | RustType::I64 | RustType::F64 | RustType::Bool | RustType::Unit
    )
}

/// Recursively collect assignment targets from a statement.
fn collect_assignments(stmt: &ast::Stmt, reassigned: &mut HashSet<String>) {
    match stmt {
        ast::Stmt::Expr(expr) => collect_assignments_from_expr(expr, reassigned),
        ast::Stmt::If(if_stmt) => {
            collect_if_assignments(if_stmt, reassigned);
        }
        ast::Stmt::While(while_stmt) => {
            for inner in &while_stmt.body.stmts {
                collect_assignments(inner, reassigned);
            }
        }
        ast::Stmt::VarDecl(_) | ast::Stmt::Return(_) => {}
    }
}

/// Collect assignment targets from if statements.
fn collect_if_assignments(if_stmt: &ast::IfStmt, reassigned: &mut HashSet<String>) {
    for inner in &if_stmt.then_block.stmts {
        collect_assignments(inner, reassigned);
    }
    if let Some(else_clause) = &if_stmt.else_clause {
        match else_clause {
            ast::ElseClause::Block(block) => {
                for inner in &block.stmts {
                    collect_assignments(inner, reassigned);
                }
            }
            ast::ElseClause::ElseIf(nested_if) => {
                collect_if_assignments(nested_if, reassigned);
            }
        }
    }
}

/// Extract assignment targets from an expression.
fn collect_assignments_from_expr(expr: &ast::Expr, reassigned: &mut HashSet<String>) {
    if let ast::ExprKind::Assign(assign) = &expr.kind {
        reassigned.insert(assign.target.name.clone());
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

    fn ident_expr(name: &str, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::Ident(ident(name, start, end)),
            span: span(start, end),
        }
    }

    fn int_expr(value: i64, start: u32, end: u32) -> Expr {
        Expr {
            kind: ExprKind::IntLit(value),
            span: span(start, end),
        }
    }

    fn no_ref_call(_obj: &str, _method: &str) -> bool {
        false
    }

    fn console_log_ref(obj: &str, method: &str) -> bool {
        obj == "console" && method == "log"
    }

    // Test 5: UseMap::analyze with two uses of variable x
    #[test]
    fn test_use_map_analyze_two_uses_correct_indices() {
        // Block: { greet(x); greet(x); foo(); }
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 0, 5),
                        args: vec![ident_expr("x", 6, 7)],
                    }),
                    span: span(0, 8),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 10, 15),
                        args: vec![ident_expr("x", 16, 17)],
                    }),
                    span: span(10, 18),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("foo", 20, 23),
                        args: vec![],
                    }),
                    span: span(20, 25),
                }),
            ],
            span: span(0, 25),
        };

        let use_map = UseMap::analyze(&block, no_ref_call);
        let x_uses = use_map.get_uses("x").unwrap();
        assert_eq!(x_uses.len(), 2);
        assert_eq!(x_uses[0].stmt_index, 0);
        assert_eq!(x_uses[1].stmt_index, 1);
        assert!(x_uses[0].is_move_position);
        assert!(x_uses[1].is_move_position);
    }

    // Test 6: needs_clone for String at stmt 0 with later use at stmt 2
    #[test]
    fn test_needs_clone_string_with_later_use_returns_true() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 0, 5),
                        args: vec![ident_expr("x", 6, 7)],
                    }),
                    span: span(0, 8),
                }),
                Stmt::Expr(int_expr(42, 10, 12)),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 20, 25),
                        args: vec![ident_expr("x", 26, 27)],
                    }),
                    span: span(20, 28),
                }),
            ],
            span: span(0, 28),
        };

        let use_map = UseMap::analyze(&block, no_ref_call);
        assert!(needs_clone("x", 0, &use_map, &RustType::String));
    }

    // Test 7: needs_clone at last use returns false
    #[test]
    fn test_needs_clone_string_at_last_use_returns_false() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 0, 5),
                        args: vec![ident_expr("x", 6, 7)],
                    }),
                    span: span(0, 8),
                }),
                Stmt::Expr(int_expr(42, 10, 12)),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("greet", 20, 25),
                        args: vec![ident_expr("x", 26, 27)],
                    }),
                    span: span(20, 28),
                }),
            ],
            span: span(0, 28),
        };

        let use_map = UseMap::analyze(&block, no_ref_call);
        assert!(!needs_clone("x", 2, &use_map, &RustType::String));
    }

    // Test 8: needs_clone for Copy type returns false
    #[test]
    fn test_needs_clone_copy_type_returns_false() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("foo", 0, 3),
                        args: vec![ident_expr("x", 4, 5)],
                    }),
                    span: span(0, 6),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Call(CallExpr {
                        callee: ident("foo", 10, 13),
                        args: vec![ident_expr("x", 14, 15)],
                    }),
                    span: span(10, 16),
                }),
            ],
            span: span(0, 16),
        };

        let use_map = UseMap::analyze(&block, no_ref_call);
        assert!(!needs_clone("x", 0, &use_map, &RustType::I32));
        assert!(!needs_clone("x", 0, &use_map, &RustType::I64));
        assert!(!needs_clone("x", 0, &use_map, &RustType::F64));
        assert!(!needs_clone("x", 0, &use_map, &RustType::Bool));
    }

    // Test 9: println! args are NOT move positions
    #[test]
    fn test_needs_clone_println_args_not_move_position() {
        let block = Block {
            stmts: vec![
                Stmt::Expr(Expr {
                    kind: ExprKind::MethodCall(MethodCallExpr {
                        object: Box::new(ident_expr("console", 0, 7)),
                        method: ident("log", 8, 11),
                        args: vec![ident_expr("x", 12, 13)],
                    }),
                    span: span(0, 14),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::MethodCall(MethodCallExpr {
                        object: Box::new(ident_expr("console", 20, 27)),
                        method: ident("log", 28, 31),
                        args: vec![ident_expr("x", 32, 33)],
                    }),
                    span: span(20, 34),
                }),
            ],
            span: span(0, 34),
        };

        let use_map = UseMap::analyze(&block, console_log_ref);
        // Even though x is String and used later, println! is not a move position
        assert!(!needs_clone("x", 0, &use_map, &RustType::String));
    }

    // Test 10: find_reassigned_variables with x = 10
    #[test]
    fn test_find_reassigned_variables_with_assignment() {
        let block = Block {
            stmts: vec![
                Stmt::VarDecl(VarDecl {
                    binding: VarBinding::Let,
                    name: ident("x", 4, 5),
                    type_ann: None,
                    init: int_expr(0, 8, 9),
                    span: span(0, 10),
                }),
                Stmt::Expr(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        target: ident("x", 11, 12),
                        value: Box::new(int_expr(10, 15, 17)),
                    }),
                    span: span(11, 18),
                }),
            ],
            span: span(0, 18),
        };

        let reassigned = find_reassigned_variables(&block);
        assert!(reassigned.contains("x"));
        assert_eq!(reassigned.len(), 1);
    }

    // Test 11: find_reassigned_variables on block with no assignments
    #[test]
    fn test_find_reassigned_variables_none() {
        let block = Block {
            stmts: vec![Stmt::VarDecl(VarDecl {
                binding: VarBinding::Const,
                name: ident("x", 6, 7),
                type_ann: None,
                init: int_expr(42, 10, 12),
                span: span(0, 13),
            })],
            span: span(0, 13),
        };

        let reassigned = find_reassigned_variables(&block);
        assert!(reassigned.is_empty());
    }
}

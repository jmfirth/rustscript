//! Test block lowering.
//!
//! Transforms `RustScript` `test()`, `describe()`, and `it()` blocks into
//! Rust test infrastructure: `#[cfg(test)] mod tests { #[test] fn ... }`.
//! Also handles `assert()` → `assert!()` / `assert_eq!()` / `assert_ne!()`
//! mapping and `expect().toBe()` → `assert_eq!()` chains.

use std::collections::HashSet;

use rustscript_syntax::ast;
use rustscript_syntax::rust_ir::{
    RustBlock, RustDescribeModule, RustExpr, RustExprKind, RustStmt, RustTestFn, RustTestItem,
    RustTestModule,
};

use crate::context::LoweringContext;
use crate::ownership::UseMap;

use super::Transform;

impl Transform {
    /// Collect all test blocks from a module and produce a [`RustTestModule`].
    ///
    /// Returns `None` if the module contains no test blocks.
    pub(super) fn collect_test_module(
        &self,
        module: &ast::Module,
        ctx: &mut LoweringContext,
    ) -> Option<RustTestModule> {
        let test_blocks: Vec<&ast::TestBlock> = module
            .items
            .iter()
            .filter_map(|item| {
                if let ast::ItemKind::TestBlock(tb) = &item.kind {
                    Some(tb)
                } else {
                    None
                }
            })
            .collect();

        if test_blocks.is_empty() {
            return None;
        }

        let use_map = UseMap::empty();
        let reassigned = HashSet::new();
        let mut items = Vec::new();

        for tb in test_blocks {
            self.lower_test_block(tb, ctx, &use_map, &reassigned, &mut items);
        }

        Some(RustTestModule { items })
    }

    /// Lower a single test block into one or more [`RustTestItem`]s.
    fn lower_test_block(
        &self,
        tb: &ast::TestBlock,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        reassigned: &HashSet<String>,
        items: &mut Vec<RustTestItem>,
    ) {
        match tb.kind {
            ast::TestBlockKind::Test | ast::TestBlockKind::It => {
                let name = sanitize_test_name(&tb.description);
                if let ast::TestBody::Stmts(block) = &tb.body {
                    let body = self.lower_test_body(block, ctx, use_map, reassigned);
                    items.push(RustTestItem::TestFn(RustTestFn { name, body }));
                }
            }
            ast::TestBlockKind::Describe => {
                let name = sanitize_test_name(&tb.description);
                if let ast::TestBody::Items(nested) = &tb.body {
                    let mut nested_items = Vec::new();
                    for nested_tb in nested {
                        self.lower_test_block(
                            nested_tb,
                            ctx,
                            use_map,
                            reassigned,
                            &mut nested_items,
                        );
                    }
                    items.push(RustTestItem::DescribeModule(RustDescribeModule {
                        name,
                        items: nested_items,
                    }));
                }
            }
        }
    }

    /// Lower the body of a test function, transforming `assert()` calls.
    fn lower_test_body(
        &self,
        block: &ast::Block,
        ctx: &mut LoweringContext,
        use_map: &UseMap,
        reassigned: &HashSet<String>,
    ) -> RustBlock {
        let stmts = block
            .stmts
            .iter()
            .enumerate()
            .map(|(i, stmt)| {
                let lowered = self.lower_stmt(stmt, ctx, use_map, i, reassigned);
                rewrite_assert_stmts(lowered)
            })
            .collect();

        RustBlock { stmts, expr: None }
    }
}

/// Sanitize a test description string to a valid Rust function/module name.
///
/// Rules:
/// - Lowercase the entire string
/// - Replace spaces, hyphens, and special characters with underscores
/// - Remove leading digits
/// - Collapse multiple underscores
/// - Truncate to 100 characters
pub(crate) fn sanitize_test_name(description: &str) -> String {
    let lowered = description.to_lowercase();
    let mut result = String::with_capacity(lowered.len());

    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            result.push(ch);
        } else {
            // Replace spaces, hyphens, and special chars with underscore
            result.push('_');
        }
    }

    // Remove leading digits/underscores
    let trimmed = result.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_');
    let mut result = trimmed.to_owned();

    // Collapse multiple underscores
    while result.contains("__") {
        result = result.replace("__", "_");
    }

    // Trim trailing underscores
    let result = result.trim_end_matches('_').to_owned();

    // Truncate to 100 chars
    if result.len() > 100 {
        result[..100].to_owned()
    } else if result.is_empty() {
        "unnamed_test".to_owned()
    } else {
        result
    }
}

/// Rewrite `assert()` function calls in a lowered statement.
///
/// Transforms:
/// - `assert(x == y)` → `assert_eq!(x, y)`
/// - `assert(x != y)` → `assert_ne!(x, y)`
/// - `assert(expr)` → `assert!(expr)`
fn rewrite_assert_stmts(stmt: RustStmt) -> RustStmt {
    match stmt {
        RustStmt::Semi(expr) => RustStmt::Semi(rewrite_assert_expr(expr)),
        other => other,
    }
}

/// Rewrite an `assert()` call expression into the appropriate Rust macro.
fn rewrite_assert_expr(expr: RustExpr) -> RustExpr {
    match &expr.kind {
        RustExprKind::Call { func, args, .. } if func == "assert" && args.len() == 1 => {
            let span = expr.span;
            let arg = &args[0];
            match &arg.kind {
                // assert(a == b) → assert_eq!(a, b)
                RustExprKind::Binary {
                    op: rustscript_syntax::rust_ir::RustBinaryOp::Eq,
                    left,
                    right,
                } => RustExpr {
                    kind: RustExprKind::Macro {
                        name: "assert_eq".to_owned(),
                        args: vec![(**left).clone(), (**right).clone()],
                    },
                    span,
                },
                // assert(a != b) → assert_ne!(a, b)
                RustExprKind::Binary {
                    op: rustscript_syntax::rust_ir::RustBinaryOp::Ne,
                    left,
                    right,
                } => RustExpr {
                    kind: RustExprKind::Macro {
                        name: "assert_ne".to_owned(),
                        args: vec![(**left).clone(), (**right).clone()],
                    },
                    span,
                },
                // assert(expr) → assert!(expr) for any other expression
                _ => RustExpr {
                    kind: RustExprKind::Macro {
                        name: "assert".to_owned(),
                        args: vec![arg.clone()],
                    },
                    span,
                },
            }
        }
        _ => expr,
    }
}

/// Rewrite `expect(x).toBe(y)` method chain into assert macros.
///
/// - `expect(x).toBe(y)` becomes `assert_eq!(x, y)`
/// - `expect(x).toBeTruthy()` becomes `assert!(x)`
/// - `expect(x).toBeFalsy()` becomes `assert!(!x)`
#[allow(dead_code)]
fn rewrite_expect_chain(expr: RustExpr) -> RustExpr {
    let span = expr.span;
    let (receiver, method, args) = match &expr.kind {
        RustExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => (receiver, method.as_str(), args),
        _ => return expr,
    };

    // Check if receiver is `expect(x)` call with exactly one argument
    let subject = match &receiver.kind {
        RustExprKind::Call { func, args, .. } if func == "expect" && args.len() == 1 => &args[0],
        _ => return expr,
    };

    match method {
        "toBe" if args.len() == 1 => RustExpr {
            kind: RustExprKind::Macro {
                name: "assert_eq".to_owned(),
                args: vec![subject.clone(), args[0].clone()],
            },
            span,
        },
        "toBeTruthy" if args.is_empty() => RustExpr {
            kind: RustExprKind::Macro {
                name: "assert".to_owned(),
                args: vec![subject.clone()],
            },
            span,
        },
        "toBeFalsy" if args.is_empty() => RustExpr {
            kind: RustExprKind::Macro {
                name: "assert".to_owned(),
                args: vec![RustExpr::synthetic(RustExprKind::Unary {
                    op: rustscript_syntax::rust_ir::RustUnaryOp::Not,
                    operand: Box::new(subject.clone()),
                })],
            },
            span,
        },
        _ => expr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_test_name_simple() {
        assert_eq!(sanitize_test_name("adds two numbers"), "adds_two_numbers");
    }

    #[test]
    fn test_sanitize_test_name_with_hyphens() {
        assert_eq!(sanitize_test_name("my-test-case"), "my_test_case");
    }

    #[test]
    fn test_sanitize_test_name_with_special_chars() {
        assert_eq!(
            sanitize_test_name("should handle (edge) cases!"),
            "should_handle_edge_cases"
        );
    }

    #[test]
    fn test_sanitize_test_name_leading_digits() {
        assert_eq!(
            sanitize_test_name("123 starts with digits"),
            "starts_with_digits"
        );
    }

    #[test]
    fn test_sanitize_test_name_multiple_underscores() {
        assert_eq!(sanitize_test_name("lots   of   spaces"), "lots_of_spaces");
    }

    #[test]
    fn test_sanitize_test_name_empty() {
        assert_eq!(sanitize_test_name(""), "unnamed_test");
    }

    #[test]
    fn test_sanitize_test_name_truncation() {
        let long_name = "a".repeat(150);
        let result = sanitize_test_name(&long_name);
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn test_sanitize_test_name_uppercase() {
        assert_eq!(
            sanitize_test_name("Should Add Numbers"),
            "should_add_numbers"
        );
    }

    #[test]
    fn test_sanitize_test_name_already_valid() {
        assert_eq!(sanitize_test_name("simple"), "simple");
    }
}

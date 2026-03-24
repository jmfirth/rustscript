//! Type resolution and literal type inference for the lowering pass.
//!
//! Maps `RustScript` type names to Rust IR types and infers types from
//! literal expressions. This module is the future extraction seam for
//! the full `rsc-typeck` crate.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::RustType;

/// Resolve a `RustScript` type name to a Rust type.
///
/// Returns `None` for unknown type names.
pub(crate) fn resolve_type(name: &str) -> Option<RustType> {
    match name {
        "i32" => Some(RustType::I32),
        "i64" => Some(RustType::I64),
        "f64" => Some(RustType::F64),
        "bool" => Some(RustType::Bool),
        "string" => Some(RustType::String),
        "void" => Some(RustType::Unit),
        _ => None,
    }
}

/// Infer the type of a literal expression.
///
/// Returns `None` for non-literal expressions.
pub(crate) fn infer_literal_type(expr: &ast::Expr) -> Option<RustType> {
    match &expr.kind {
        ast::ExprKind::IntLit(_) => Some(RustType::I64),
        ast::ExprKind::FloatLit(_) => Some(RustType::F64),
        ast::ExprKind::StringLit(_) => Some(RustType::String),
        ast::ExprKind::BoolLit(_) => Some(RustType::Bool),
        _ => None,
    }
}

/// Resolve a type annotation to a Rust type, emitting a diagnostic for unknown types.
///
/// Unknown type names produce a diagnostic and default to `i64`.
pub(crate) fn resolve_type_annotation(
    ann: &ast::TypeAnnotation,
    diagnostics: &mut Vec<Diagnostic>,
) -> RustType {
    match &ann.kind {
        ast::TypeKind::Void => RustType::Unit,
        ast::TypeKind::Named(ident) => resolve_type(&ident.name).unwrap_or_else(|| {
            diagnostics.push(Diagnostic::error(format!("unknown type `{}`", ident.name)));
            RustType::I64
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::ast::{Expr, ExprKind, Ident, TypeAnnotation, TypeKind};
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

    // Test 1: All 6 known types resolve correctly
    #[test]
    fn test_resolve_type_all_known_types() {
        assert_eq!(resolve_type("i32"), Some(RustType::I32));
        assert_eq!(resolve_type("i64"), Some(RustType::I64));
        assert_eq!(resolve_type("f64"), Some(RustType::F64));
        assert_eq!(resolve_type("bool"), Some(RustType::Bool));
        assert_eq!(resolve_type("string"), Some(RustType::String));
        assert_eq!(resolve_type("void"), Some(RustType::Unit));
    }

    // Test 2: Unknown type returns None
    #[test]
    fn test_resolve_type_unknown_returns_none() {
        assert_eq!(resolve_type("Foo"), None);
        assert_eq!(resolve_type("Bar"), None);
        assert_eq!(resolve_type(""), None);
    }

    // Test 3: Literal type inference
    #[test]
    fn test_infer_literal_type_int_lit() {
        let expr = Expr {
            kind: ExprKind::IntLit(42),
            span: span(0, 2),
        };
        assert_eq!(infer_literal_type(&expr), Some(RustType::I64));
    }

    #[test]
    fn test_infer_literal_type_float_lit() {
        let expr = Expr {
            kind: ExprKind::FloatLit(3.14),
            span: span(0, 4),
        };
        assert_eq!(infer_literal_type(&expr), Some(RustType::F64));
    }

    #[test]
    fn test_infer_literal_type_string_lit() {
        let expr = Expr {
            kind: ExprKind::StringLit("hello".to_owned()),
            span: span(0, 7),
        };
        assert_eq!(infer_literal_type(&expr), Some(RustType::String));
    }

    #[test]
    fn test_infer_literal_type_bool_lit() {
        let expr = Expr {
            kind: ExprKind::BoolLit(true),
            span: span(0, 4),
        };
        assert_eq!(infer_literal_type(&expr), Some(RustType::Bool));
    }

    #[test]
    fn test_infer_literal_type_non_literal_returns_none() {
        let expr = Expr {
            kind: ExprKind::Ident(ident("x", 0, 1)),
            span: span(0, 1),
        };
        assert_eq!(infer_literal_type(&expr), None);
    }

    // Test 4: resolve_type_annotation with unknown type emits diagnostic
    #[test]
    fn test_resolve_type_annotation_known_type() {
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("i32", 0, 3)),
            span: span(0, 3),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, RustType::I32);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_resolve_type_annotation_void() {
        let ann = TypeAnnotation {
            kind: TypeKind::Void,
            span: span(0, 4),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, RustType::Unit);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_resolve_type_annotation_unknown_emits_diagnostic_defaults_i64() {
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("Foo", 0, 3)),
            span: span(0, 3),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, RustType::I64);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown type"));
        assert!(diags[0].message.contains("Foo"));
    }
}

//! Type name resolution and literal type inference.
//!
//! Maps `RustScript` type names to [`Type`] values and infers types from
//! literal expressions. This module was extracted from `rsc-lower/src/types.rs`
//! during Phase 1.

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;

use crate::bridge::type_to_rust_type;
use crate::types::{PrimitiveType, Type};

/// Resolve a `RustScript` type name to a [`Type`].
///
/// Handles primitives, `string`, `void`, and the collection type names.
/// User-defined type names are resolved by the type environment (later tasks).
/// Unknown names return `None` — the caller decides whether to treat as
/// user-defined or error.
#[must_use]
pub fn resolve_type_name(name: &str) -> Option<Type> {
    match name {
        "i8" => Some(Type::Primitive(PrimitiveType::I8)),
        "i16" => Some(Type::Primitive(PrimitiveType::I16)),
        "i32" => Some(Type::Primitive(PrimitiveType::I32)),
        "i64" => Some(Type::Primitive(PrimitiveType::I64)),
        "u8" => Some(Type::Primitive(PrimitiveType::U8)),
        "u16" => Some(Type::Primitive(PrimitiveType::U16)),
        "u32" => Some(Type::Primitive(PrimitiveType::U32)),
        "u64" => Some(Type::Primitive(PrimitiveType::U64)),
        "f32" => Some(Type::Primitive(PrimitiveType::F32)),
        "f64" => Some(Type::Primitive(PrimitiveType::F64)),
        "bool" => Some(Type::Primitive(PrimitiveType::Bool)),
        "string" => Some(Type::String),
        "void" => Some(Type::Unit),
        _ => None,
    }
}

/// Infer the type of a literal expression.
///
/// Returns `None` for non-literal expressions.
#[must_use]
pub fn infer_literal_type(expr: &ast::Expr) -> Option<Type> {
    match &expr.kind {
        ast::ExprKind::IntLit(_) => Some(Type::Primitive(PrimitiveType::I64)),
        ast::ExprKind::FloatLit(_) => Some(Type::Primitive(PrimitiveType::F64)),
        ast::ExprKind::StringLit(_) => Some(Type::String),
        ast::ExprKind::BoolLit(_) => Some(Type::Primitive(PrimitiveType::Bool)),
        _ => None,
    }
}

/// Resolve a type annotation to a [`Type`], emitting a diagnostic for unknown types.
///
/// Unknown type names produce a diagnostic and default to `i64`.
pub fn resolve_type_annotation(
    ann: &ast::TypeAnnotation,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    match &ann.kind {
        ast::TypeKind::Void => Type::Unit,
        ast::TypeKind::Named(ident) => resolve_type_name(&ident.name).unwrap_or_else(|| {
            diagnostics.push(Diagnostic::error(format!("unknown type `{}`", ident.name)));
            Type::Primitive(PrimitiveType::I64)
        }),
    }
}

/// Resolve a type annotation to a `RustType`, emitting diagnostics for unknown types.
///
/// This is a convenience function that resolves through [`Type`] and converts
/// to [`rsc_syntax::rust_ir::RustType`] via the bridge. It preserves the exact
/// behavior of the original `rsc-lower/types.rs` resolution.
pub fn resolve_type_annotation_to_rust_type(
    ann: &ast::TypeAnnotation,
    diagnostics: &mut Vec<Diagnostic>,
) -> rsc_syntax::rust_ir::RustType {
    let ty = resolve_type_annotation(ann, diagnostics);
    type_to_rust_type(&ty)
}

/// Infer the `RustType` of a literal expression.
///
/// Convenience function that infers through [`Type`] and converts via the bridge.
#[must_use]
pub fn infer_literal_rust_type(expr: &ast::Expr) -> Option<rsc_syntax::rust_ir::RustType> {
    infer_literal_type(expr).map(|ty| type_to_rust_type(&ty))
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

    // Test 1: All known types resolve correctly (expanded for Phase 1)
    #[test]
    fn test_resolve_type_name_all_known_types() {
        assert_eq!(
            resolve_type_name("i8"),
            Some(Type::Primitive(PrimitiveType::I8))
        );
        assert_eq!(
            resolve_type_name("i16"),
            Some(Type::Primitive(PrimitiveType::I16))
        );
        assert_eq!(
            resolve_type_name("i32"),
            Some(Type::Primitive(PrimitiveType::I32))
        );
        assert_eq!(
            resolve_type_name("i64"),
            Some(Type::Primitive(PrimitiveType::I64))
        );
        assert_eq!(
            resolve_type_name("u8"),
            Some(Type::Primitive(PrimitiveType::U8))
        );
        assert_eq!(
            resolve_type_name("u16"),
            Some(Type::Primitive(PrimitiveType::U16))
        );
        assert_eq!(
            resolve_type_name("u32"),
            Some(Type::Primitive(PrimitiveType::U32))
        );
        assert_eq!(
            resolve_type_name("u64"),
            Some(Type::Primitive(PrimitiveType::U64))
        );
        assert_eq!(
            resolve_type_name("f32"),
            Some(Type::Primitive(PrimitiveType::F32))
        );
        assert_eq!(
            resolve_type_name("f64"),
            Some(Type::Primitive(PrimitiveType::F64))
        );
        assert_eq!(
            resolve_type_name("bool"),
            Some(Type::Primitive(PrimitiveType::Bool))
        );
        assert_eq!(resolve_type_name("string"), Some(Type::String));
        assert_eq!(resolve_type_name("void"), Some(Type::Unit));
    }

    // Test 2: Unknown type returns None
    #[test]
    fn test_resolve_type_name_unknown_returns_none() {
        assert_eq!(resolve_type_name("Foo"), None);
        assert_eq!(resolve_type_name("Bar"), None);
        assert_eq!(resolve_type_name(""), None);
        assert_eq!(resolve_type_name("UnknownType"), None);
    }

    // Test 3: New types not in Phase 0
    #[test]
    fn test_resolve_type_name_u8_new_type() {
        assert_eq!(
            resolve_type_name("u8"),
            Some(Type::Primitive(PrimitiveType::U8))
        );
    }

    #[test]
    fn test_resolve_type_name_f32_new_type() {
        assert_eq!(
            resolve_type_name("f32"),
            Some(Type::Primitive(PrimitiveType::F32))
        );
    }

    // Test 4: Literal type inference
    #[test]
    fn test_infer_literal_type_int_lit() {
        let expr = Expr {
            kind: ExprKind::IntLit(42),
            span: span(0, 2),
        };
        assert_eq!(
            infer_literal_type(&expr),
            Some(Type::Primitive(PrimitiveType::I64))
        );
    }

    #[test]
    fn test_infer_literal_type_float_lit() {
        let expr = Expr {
            kind: ExprKind::FloatLit(3.14),
            span: span(0, 4),
        };
        assert_eq!(
            infer_literal_type(&expr),
            Some(Type::Primitive(PrimitiveType::F64))
        );
    }

    #[test]
    fn test_infer_literal_type_string_lit() {
        let expr = Expr {
            kind: ExprKind::StringLit("hello".to_owned()),
            span: span(0, 7),
        };
        assert_eq!(infer_literal_type(&expr), Some(Type::String));
    }

    #[test]
    fn test_infer_literal_type_bool_lit() {
        let expr = Expr {
            kind: ExprKind::BoolLit(true),
            span: span(0, 4),
        };
        assert_eq!(
            infer_literal_type(&expr),
            Some(Type::Primitive(PrimitiveType::Bool))
        );
    }

    #[test]
    fn test_infer_literal_type_non_literal_returns_none() {
        let expr = Expr {
            kind: ExprKind::Ident(ident("x", 0, 1)),
            span: span(0, 1),
        };
        assert_eq!(infer_literal_type(&expr), None);
    }

    // Test 5: resolve_type_annotation with known type
    #[test]
    fn test_resolve_type_annotation_known_type() {
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("i32", 0, 3)),
            span: span(0, 3),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation(&ann, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::I32));
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
        assert_eq!(ty, Type::Unit);
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
        assert_eq!(ty, Type::Primitive(PrimitiveType::I64));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown type"));
        assert!(diags[0].message.contains("Foo"));
    }
}

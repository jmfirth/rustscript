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

/// Resolve a union type annotation (`T | null`) to `Option<T>`.
///
/// Only `T | null` unions are supported in Phase 1. The resolver identifies
/// which member is `null` and wraps the other member in `Type::Option`.
/// Multi-member non-null unions are not supported and resolve to the first
/// non-null member.
fn resolve_union_type(
    members: &[ast::TypeAnnotation],
    mut resolve_member: impl FnMut(&ast::TypeAnnotation) -> Type,
) -> Type {
    let mut non_null_types = Vec::new();
    let mut has_null = false;

    for member in members {
        if let ast::TypeKind::Named(ident) = &member.kind
            && ident.name == "null"
        {
            has_null = true;
            continue;
        }
        non_null_types.push(resolve_member(member));
    }

    if has_null {
        let inner = non_null_types.into_iter().next().unwrap_or(Type::Unit);
        Type::Option(Box::new(inner))
    } else {
        // No null member — just return the first type (unsupported union)
        non_null_types.into_iter().next().unwrap_or(Type::Unit)
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
        ast::ExprKind::StringLit(_) | ast::ExprKind::TemplateLit(_) => Some(Type::String),
        ast::ExprKind::BoolLit(_) => Some(Type::Primitive(PrimitiveType::Bool)),
        ast::ExprKind::NullLit => Some(Type::Option(Box::new(Type::Error))),
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
        ast::TypeKind::Generic(ident, args) => {
            let resolved_args: Vec<Type> = args
                .iter()
                .map(|a| resolve_type_annotation(a, diagnostics))
                .collect();
            // Map collection type aliases to their Rust equivalents
            let rust_name = map_collection_type_name(&ident.name);
            Type::Generic(rust_name, resolved_args)
        }
        ast::TypeKind::Union(members) => {
            resolve_union_type(members, |m| resolve_type_annotation(m, diagnostics))
        }
        ast::TypeKind::Function(param_types, return_type) => {
            let params: Vec<Type> = param_types
                .iter()
                .map(|p| resolve_type_annotation(p, diagnostics))
                .collect();
            let ret = resolve_type_annotation(return_type, diagnostics);
            Type::Function(params, Box::new(ret))
        }
        ast::TypeKind::Intersection(members) => {
            // Intersection types are used for trait bounds — resolve each member
            // individually. The lowering pass handles them specially for function parameters.
            // For type resolution, treat the first member as the resolved type.
            members
                .first()
                .map_or(Type::Unit, |m| resolve_type_annotation(m, diagnostics))
        }
    }
}

/// Resolve a type annotation to a [`Type`], treating unknown names as user-defined
/// types rather than errors.
///
/// Uses the [`crate::registry::TypeRegistry`] to confirm that the type is registered.
/// Unknown names that are also not in the registry fall back to diagnostics.
/// Type parameter names from the current generic scope resolve to `Type::TypeVar`.
pub fn resolve_type_annotation_with_registry(
    ann: &ast::TypeAnnotation,
    registry: &crate::registry::TypeRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    resolve_type_annotation_with_generics(ann, registry, &[], diagnostics)
}

/// Resolve a type annotation to a [`Type`], with generic type parameter scope.
///
/// `generic_param_names` is the list of type parameter names currently in scope
/// (e.g., `["T", "U"]` inside `function foo<T, U>(...)`). Names matching a
/// generic parameter resolve to `Type::TypeVar` rather than being looked up
/// in the registry.
pub fn resolve_type_annotation_with_generics(
    ann: &ast::TypeAnnotation,
    registry: &crate::registry::TypeRegistry,
    generic_param_names: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    match &ann.kind {
        ast::TypeKind::Void => Type::Unit,
        ast::TypeKind::Named(ident) => {
            // `Self` is a special type used in interface method return types.
            // It passes through to the lowering pass which handles it natively.
            if ident.name == "Self" {
                return Type::Named("Self".to_owned());
            }
            // Check if this is a generic type parameter in scope
            if generic_param_names.contains(&ident.name) {
                return Type::TypeVar(ident.name.clone());
            }
            // Try primitive/built-in first
            if let Some(ty) = resolve_type_name(&ident.name) {
                return ty;
            }
            // Try user-defined type from registry
            if registry.lookup(&ident.name).is_some() {
                return Type::Named(ident.name.clone());
            }
            // Unknown type
            diagnostics.push(Diagnostic::error(format!("unknown type `{}`", ident.name)));
            Type::Primitive(PrimitiveType::I64)
        }
        ast::TypeKind::Generic(ident, args) => {
            let resolved_args: Vec<Type> = args
                .iter()
                .map(|a| {
                    resolve_type_annotation_with_generics(
                        a,
                        registry,
                        generic_param_names,
                        diagnostics,
                    )
                })
                .collect();
            // Map collection type aliases to their Rust equivalents
            let rust_name = map_collection_type_name(&ident.name);
            Type::Generic(rust_name, resolved_args)
        }
        ast::TypeKind::Union(members) => resolve_union_type(members, |m| {
            resolve_type_annotation_with_generics(m, registry, generic_param_names, diagnostics)
        }),
        ast::TypeKind::Function(param_types, return_type) => {
            let params: Vec<Type> = param_types
                .iter()
                .map(|p| {
                    resolve_type_annotation_with_generics(
                        p,
                        registry,
                        generic_param_names,
                        diagnostics,
                    )
                })
                .collect();
            let ret = resolve_type_annotation_with_generics(
                return_type,
                registry,
                generic_param_names,
                diagnostics,
            );
            Type::Function(params, Box::new(ret))
        }
        ast::TypeKind::Intersection(members) => {
            // Intersection types are used for trait bounds — resolve each member.
            // For type resolution, treat the first member as the resolved type.
            members.first().map_or(Type::Unit, |m| {
                resolve_type_annotation_with_generics(m, registry, generic_param_names, diagnostics)
            })
        }
    }
}

/// Map `RustScript` collection type names to their Rust equivalents.
///
/// `Array` → `Vec`, `Map` → `HashMap`, `Set` → `HashSet`.
/// All other names pass through unchanged.
#[must_use]
pub fn map_collection_type_name(name: &str) -> String {
    match name {
        "Array" => "Vec".to_owned(),
        "Map" => "HashMap".to_owned(),
        "Set" => "HashSet".to_owned(),
        other => other.to_owned(),
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

    // Test T14-14: Type registry resolves user-defined types
    #[test]
    fn test_resolve_type_annotation_with_registry_known_user_type() {
        let mut registry = crate::registry::TypeRegistry::new();
        registry.register(
            "User".to_owned(),
            vec![
                ("name".to_owned(), Type::String),
                ("age".to_owned(), Type::Primitive(PrimitiveType::U32)),
            ],
        );

        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("User", 0, 4)),
            span: span(0, 4),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert_eq!(ty, Type::Named("User".to_owned()));
        assert!(diags.is_empty());
    }

    // Test T14-15: Type registry still resolves primitives
    #[test]
    fn test_resolve_type_annotation_with_registry_primitive_type() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("i32", 0, 3)),
            span: span(0, 3),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::I32));
        assert!(diags.is_empty());
    }

    // Test T14-16: Unknown type with registry emits diagnostic
    #[test]
    fn test_resolve_type_annotation_with_registry_unknown_type() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("Unknown", 0, 7)),
            span: span(0, 7),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_registry(&ann, &registry, &mut diags);
        assert_eq!(ty, Type::Primitive(PrimitiveType::I64));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown type"));
    }

    // ---- Task 016: Generic type resolution ----

    // Test T16-13: Type parameter T in function body resolves to TypeVar
    #[test]
    fn test_resolve_type_param_in_generic_scope() {
        let registry = crate::registry::TypeRegistry::new();
        let generic_names = vec!["T".to_owned()];
        let ann = TypeAnnotation {
            kind: TypeKind::Named(ident("T", 0, 1)),
            span: span(0, 1),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_generics(&ann, &registry, &generic_names, &mut diags);
        assert_eq!(ty, Type::TypeVar("T".to_owned()));
        assert!(diags.is_empty());
    }

    // Test T16-14: Generic type annotation resolves to Type::Generic
    // Array<string> maps to Vec<String> via collection type alias mapping
    #[test]
    fn test_resolve_generic_type_annotation() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Generic(
                ident("Array", 0, 5),
                vec![TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }],
            ),
            span: span(0, 13),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_generics(&ann, &registry, &[], &mut diags);
        match &ty {
            Type::Generic(name, args) => {
                assert_eq!(name, "Vec");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], Type::String);
            }
            _ => panic!("expected Generic type, got {ty:?}"),
        }
    }

    // ---- Task 017: Collection type mapping ----

    // Test T17-10b: Map<string, u32> resolves to Generic("HashMap", [String, u32])
    #[test]
    fn test_resolve_map_type_to_hashmap() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Generic(
                ident("Map", 0, 3),
                vec![
                    TypeAnnotation {
                        kind: TypeKind::Named(ident("string", 0, 6)),
                        span: span(0, 6),
                    },
                    TypeAnnotation {
                        kind: TypeKind::Named(ident("u32", 0, 3)),
                        span: span(0, 3),
                    },
                ],
            ),
            span: span(0, 20),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_generics(&ann, &registry, &[], &mut diags);
        match &ty {
            Type::Generic(name, args) => {
                assert_eq!(name, "HashMap");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], Type::String);
                assert_eq!(args[1], Type::Primitive(PrimitiveType::U32));
            }
            _ => panic!("expected Generic type, got {ty:?}"),
        }
    }

    // Test T17-10c: Set<string> resolves to Generic("HashSet", [String])
    #[test]
    fn test_resolve_set_type_to_hashset() {
        let registry = crate::registry::TypeRegistry::new();
        let ann = TypeAnnotation {
            kind: TypeKind::Generic(
                ident("Set", 0, 3),
                vec![TypeAnnotation {
                    kind: TypeKind::Named(ident("string", 0, 6)),
                    span: span(0, 6),
                }],
            ),
            span: span(0, 13),
        };
        let mut diags = Vec::new();
        let ty = resolve_type_annotation_with_generics(&ann, &registry, &[], &mut diags);
        match &ty {
            Type::Generic(name, args) => {
                assert_eq!(name, "HashSet");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], Type::String);
            }
            _ => panic!("expected Generic type, got {ty:?}"),
        }
    }

    // Test T17-10d: map_collection_type_name passes through unknown names
    #[test]
    fn test_map_collection_type_name_passthrough() {
        assert_eq!(map_collection_type_name("Array"), "Vec");
        assert_eq!(map_collection_type_name("Map"), "HashMap");
        assert_eq!(map_collection_type_name("Set"), "HashSet");
        assert_eq!(map_collection_type_name("Container"), "Container");
    }
}

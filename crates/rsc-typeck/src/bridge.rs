//! Bridging between the typeck [`Type`] and the IR [`RustType`].
//!
//! Converts the canonical type representation to the Rust IR type used
//! by the lowering pass and emitter.

use rsc_syntax::rust_ir::RustType;

use crate::types::{PrimitiveType, Type};

/// Convert a typeck [`Type`] to a [`RustType`] for the IR.
///
/// Types that don't yet have IR representations produce `RustType::Unit` as
/// a placeholder.
#[must_use]
pub fn type_to_rust_type(ty: &Type) -> RustType {
    match ty {
        Type::Primitive(prim) => primitive_to_rust_type(*prim),
        Type::String => RustType::String,
        Type::Named(name) => RustType::Named(name.clone()),
        Type::TypeVar(name) => RustType::TypeParam(name.clone()),
        Type::Generic(name, args) => {
            let base = RustType::Named(name.clone());
            let rust_args: Vec<RustType> = args.iter().map(type_to_rust_type).collect();
            RustType::Generic(Box::new(base), rust_args)
        }
        Type::Option(inner) => RustType::Option(Box::new(type_to_rust_type(inner))),
        Type::Function(params, ret) => {
            let rust_params: Vec<RustType> = params.iter().map(type_to_rust_type).collect();
            let rust_ret = type_to_rust_type(ret);
            RustType::ImplFn(rust_params, Box::new(rust_ret))
        }
        Type::Result(ok, err) => RustType::Result(
            Box::new(type_to_rust_type(ok)),
            Box::new(type_to_rust_type(err)),
        ),
        Type::ArcMutex(inner) => RustType::ArcMutex(Box::new(type_to_rust_type(inner))),
        Type::Tuple(types) => RustType::Tuple(types.iter().map(type_to_rust_type).collect()),
        Type::Union(members) => {
            let variants: Vec<(String, RustType)> = members
                .iter()
                .map(|m| {
                    let rust_ty = type_to_rust_type(m);
                    let variant_name = union_variant_name(&rust_ty);
                    (variant_name, rust_ty)
                })
                .collect();
            let name = union_enum_name(&variants);
            RustType::GeneratedUnion { name, variants }
        }
        Type::Unknown => RustType::BoxDynAny,
        Type::Unit | Type::Error => RustType::Unit,
    }
}

/// Generate the `PascalCase` variant name for a union member type.
///
/// Maps `RustType::String` → `"String"`, `RustType::I32` → `"I32"`, etc.
fn union_variant_name(ty: &RustType) -> String {
    match ty {
        RustType::I8 => "I8".to_owned(),
        RustType::I16 => "I16".to_owned(),
        RustType::I32 => "I32".to_owned(),
        RustType::I64 => "I64".to_owned(),
        RustType::U8 => "U8".to_owned(),
        RustType::U16 => "U16".to_owned(),
        RustType::U32 => "U32".to_owned(),
        RustType::U64 => "U64".to_owned(),
        RustType::F32 => "F32".to_owned(),
        RustType::F64 => "F64".to_owned(),
        RustType::Bool => "Bool".to_owned(),
        RustType::String => "String".to_owned(),
        RustType::Unit => "Unit".to_owned(),
        RustType::Named(name)
        | RustType::TypeParam(name)
        | RustType::GeneratedUnion { name, .. } => name.clone(),
        RustType::Option(inner) => format!("Option{}", union_variant_name(inner)),
        RustType::Generic(base, _) => union_variant_name(base),
        RustType::Tuple(types) => {
            let parts: Vec<String> = types.iter().map(union_variant_name).collect();
            format!("Tuple{}", parts.join(""))
        }
        RustType::Result(ok, err) => {
            format!(
                "Result{}{}",
                union_variant_name(ok),
                union_variant_name(err)
            )
        }
        RustType::ImplFn(_, _) => "Fn".to_owned(),
        RustType::SelfType => "Self_".to_owned(),
        RustType::Infer => "Infer".to_owned(),
        RustType::ArcMutex(inner) => format!("Shared{}", union_variant_name(inner)),
        RustType::DynRef(name) => format!("Dyn{name}"),
        RustType::BoxDynAny => "Unknown".to_owned(),
    }
}

/// Generate the deterministic enum name for a union type.
///
/// Sorts variant names alphabetically and joins with `"Or"`.
/// E.g., variants `[("String", _), ("I32", _)]` → `"I32OrString"`.
fn union_enum_name(variants: &[(String, RustType)]) -> String {
    let mut names: Vec<&str> = variants.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    names.join("Or")
}

/// Convert a primitive type to the corresponding `RustType`.
fn primitive_to_rust_type(prim: PrimitiveType) -> RustType {
    match prim {
        PrimitiveType::I8 => RustType::I8,
        PrimitiveType::I16 => RustType::I16,
        PrimitiveType::I32 => RustType::I32,
        PrimitiveType::I64 => RustType::I64,
        PrimitiveType::U8 => RustType::U8,
        PrimitiveType::U16 => RustType::U16,
        PrimitiveType::U32 => RustType::U32,
        PrimitiveType::U64 => RustType::U64,
        PrimitiveType::F32 => RustType::F32,
        PrimitiveType::F64 => RustType::F64,
        PrimitiveType::Bool => RustType::Bool,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_all_primitives() {
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::I8)),
            RustType::I8
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::I16)),
            RustType::I16
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::I32)),
            RustType::I32
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::I64)),
            RustType::I64
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::U8)),
            RustType::U8
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::U16)),
            RustType::U16
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::U32)),
            RustType::U32
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::U64)),
            RustType::U64
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::F32)),
            RustType::F32
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::F64)),
            RustType::F64
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::Bool)),
            RustType::Bool
        );
    }

    #[test]
    fn test_bridge_string() {
        assert_eq!(type_to_rust_type(&Type::String), RustType::String);
    }

    #[test]
    fn test_bridge_unit() {
        assert_eq!(type_to_rust_type(&Type::Unit), RustType::Unit);
    }

    #[test]
    fn test_bridge_named_type_produces_named() {
        assert_eq!(
            type_to_rust_type(&Type::Named("Foo".to_owned())),
            RustType::Named("Foo".to_owned())
        );
    }

    #[test]
    fn test_bridge_type_param_produces_type_param() {
        assert_eq!(
            type_to_rust_type(&Type::TypeVar("T".to_owned())),
            RustType::TypeParam("T".to_owned())
        );
    }

    #[test]
    fn test_bridge_generic_produces_generic() {
        assert_eq!(
            type_to_rust_type(&Type::Generic("Vec".to_owned(), vec![Type::String])),
            RustType::Generic(
                Box::new(RustType::Named("Vec".to_owned())),
                vec![RustType::String]
            )
        );
    }

    #[test]
    fn test_bridge_option_type_produces_option() {
        assert_eq!(
            type_to_rust_type(&Type::Option(Box::new(Type::String))),
            RustType::Option(Box::new(RustType::String))
        );
    }

    #[test]
    fn test_bridge_result_type_produces_result() {
        assert_eq!(
            type_to_rust_type(&Type::Result(Box::new(Type::String), Box::new(Type::Unit))),
            RustType::Result(Box::new(RustType::String), Box::new(RustType::Unit))
        );
    }

    #[test]
    fn test_bridge_error_type_produces_unit() {
        assert_eq!(type_to_rust_type(&Type::Error), RustType::Unit);
    }

    #[test]
    fn test_bridge_function_type_produces_impl_fn() {
        assert_eq!(
            type_to_rust_type(&Type::Function(
                vec![Type::Primitive(PrimitiveType::I32)],
                Box::new(Type::Primitive(PrimitiveType::I32))
            )),
            RustType::ImplFn(vec![RustType::I32], Box::new(RustType::I32))
        );
        assert_eq!(
            type_to_rust_type(&Type::Function(vec![], Box::new(Type::Unit))),
            RustType::ImplFn(vec![], Box::new(RustType::Unit))
        );
    }

    #[test]
    fn test_bridge_arc_mutex_type_produces_arc_mutex() {
        assert_eq!(
            type_to_rust_type(&Type::ArcMutex(Box::new(Type::Primitive(
                PrimitiveType::I32
            )))),
            RustType::ArcMutex(Box::new(RustType::I32))
        );
    }

    #[test]
    fn test_bridge_arc_mutex_string_produces_arc_mutex_string() {
        assert_eq!(
            type_to_rust_type(&Type::ArcMutex(Box::new(Type::String))),
            RustType::ArcMutex(Box::new(RustType::String))
        );
    }

    #[test]
    fn test_bridge_tuple_type_produces_tuple() {
        assert_eq!(
            type_to_rust_type(&Type::Tuple(vec![
                Type::String,
                Type::Primitive(PrimitiveType::I32)
            ])),
            RustType::Tuple(vec![RustType::String, RustType::I32])
        );
    }

    #[test]
    fn test_bridge_nested_tuple_type_produces_nested_tuple() {
        assert_eq!(
            type_to_rust_type(&Type::Tuple(vec![
                Type::String,
                Type::Tuple(vec![
                    Type::Primitive(PrimitiveType::I32),
                    Type::Primitive(PrimitiveType::Bool)
                ])
            ])),
            RustType::Tuple(vec![
                RustType::String,
                RustType::Tuple(vec![RustType::I32, RustType::Bool])
            ])
        );
    }

    // ---- Task 065: General union types ----

    #[test]
    fn test_bridge_union_two_types_produces_generated_union() {
        let ty = Type::Union(vec![Type::String, Type::Primitive(PrimitiveType::I32)]);
        let result = type_to_rust_type(&ty);
        match &result {
            RustType::GeneratedUnion { name, variants } => {
                // Name is sorted alphabetically: I32OrString
                assert_eq!(name, "I32OrString");
                assert_eq!(variants.len(), 2);
                assert_eq!(variants[0].0, "String");
                assert_eq!(variants[0].1, RustType::String);
                assert_eq!(variants[1].0, "I32");
                assert_eq!(variants[1].1, RustType::I32);
            }
            other => panic!("expected GeneratedUnion, got {other:?}"),
        }
    }

    #[test]
    fn test_bridge_union_three_types_produces_generated_union() {
        let ty = Type::Union(vec![
            Type::String,
            Type::Primitive(PrimitiveType::I32),
            Type::Primitive(PrimitiveType::Bool),
        ]);
        let result = type_to_rust_type(&ty);
        match &result {
            RustType::GeneratedUnion { name, variants } => {
                // Name sorted: BoolOrI32OrString
                assert_eq!(name, "BoolOrI32OrString");
                assert_eq!(variants.len(), 3);
            }
            other => panic!("expected GeneratedUnion, got {other:?}"),
        }
    }

    #[test]
    fn test_bridge_union_name_is_deterministic_regardless_of_input_order() {
        let ty1 = Type::Union(vec![Type::String, Type::Primitive(PrimitiveType::I32)]);
        let ty2 = Type::Union(vec![Type::Primitive(PrimitiveType::I32), Type::String]);
        let r1 = type_to_rust_type(&ty1);
        let r2 = type_to_rust_type(&ty2);
        match (&r1, &r2) {
            (
                RustType::GeneratedUnion { name: n1, .. },
                RustType::GeneratedUnion { name: n2, .. },
            ) => {
                assert_eq!(
                    n1, n2,
                    "same union in different order should produce same name"
                );
            }
            _ => panic!("expected GeneratedUnion"),
        }
    }

    #[test]
    fn test_bridge_union_display_shows_enum_name() {
        let ty = Type::Union(vec![Type::String, Type::Primitive(PrimitiveType::I32)]);
        let result = type_to_rust_type(&ty);
        assert_eq!(result.to_string(), "I32OrString");
    }

    #[test]
    fn test_union_variant_name_primitives() {
        assert_eq!(union_variant_name(&RustType::String), "String");
        assert_eq!(union_variant_name(&RustType::I32), "I32");
        assert_eq!(union_variant_name(&RustType::Bool), "Bool");
        assert_eq!(union_variant_name(&RustType::F64), "F64");
    }

    // ---- Task 117: unknown type ----

    #[test]
    fn test_bridge_unknown_type_produces_box_dyn_any() {
        assert_eq!(type_to_rust_type(&Type::Unknown), RustType::BoxDynAny);
    }

    #[test]
    fn test_bridge_unknown_display_shows_box_dyn_any() {
        assert_eq!(
            type_to_rust_type(&Type::Unknown).to_string(),
            "Box<dyn std::any::Any>"
        );
    }

    #[test]
    fn test_bridge_unknown_union_variant_name() {
        assert_eq!(union_variant_name(&RustType::BoxDynAny), "Unknown");
    }
}

//! Derive macro inference for generated Rust structs and enums.
//!
//! Analyzes field types to determine which `#[derive(...)]` macros are safe
//! to apply. `Debug` and `Clone` are always derived. Conditional derives
//! like `PartialEq`, `Eq`, `Hash`, and `Copy` are added only when all
//! field types support the corresponding trait.

use rsc_syntax::rust_ir::{RustEnumVariant, RustType};

/// Determine which derive macros are safe for a struct with the given field types.
///
/// Always includes `Debug` and `Clone`. Conditionally includes `PartialEq`,
/// `Eq`, and `Hash` based on whether all field types support those traits.
/// Generic structs (those with type parameters) only get `Debug` and `Clone`
/// since the type parameter may not satisfy the trait bounds.
pub(crate) fn infer_struct_derives(
    field_types: &[&RustType],
    has_type_params: bool,
) -> Vec<String> {
    let mut derives = vec!["Debug".to_owned(), "Clone".to_owned()];

    if has_type_params {
        return derives;
    }

    if field_types.iter().all(|t| supports_partial_eq(t)) {
        derives.push("PartialEq".to_owned());
    }

    if field_types.iter().all(|t| supports_eq(t)) {
        derives.push("Eq".to_owned());
    }

    derives
}

/// Determine which derive macros are safe for an enum with the given variants.
///
/// Simple enums (all variants fieldless) get `Copy`, `PartialEq`, `Eq`, and
/// `Hash` in addition to `Debug` and `Clone`. Data enums derive based on the
/// union of all variant field types.
pub(crate) fn infer_enum_derives(variants: &[RustEnumVariant]) -> Vec<String> {
    let is_simple = variants.iter().all(|v| v.fields.is_empty());

    if is_simple {
        return vec![
            "Debug".to_owned(),
            "Clone".to_owned(),
            "Copy".to_owned(),
            "PartialEq".to_owned(),
            "Eq".to_owned(),
            "Hash".to_owned(),
        ];
    }

    // Data enum: collect all field types across all variants
    let field_types: Vec<&RustType> = variants
        .iter()
        .flat_map(|v| v.fields.iter().map(|f| &f.ty))
        .collect();

    let mut derives = vec!["Debug".to_owned(), "Clone".to_owned()];

    if field_types.iter().all(|t| supports_partial_eq(t)) {
        derives.push("PartialEq".to_owned());
    }

    if field_types.iter().all(|t| supports_eq(t)) {
        derives.push("Eq".to_owned());
    }

    derives
}

/// Whether a type supports `PartialEq`.
///
/// Most types support `PartialEq`. Closures (`ImplFn`) and type parameters
/// do not, since we cannot verify trait support at compile time.
fn supports_partial_eq(ty: &RustType) -> bool {
    match ty {
        RustType::I8
        | RustType::I16
        | RustType::I32
        | RustType::I64
        | RustType::U8
        | RustType::U16
        | RustType::U32
        | RustType::U64
        | RustType::F32
        | RustType::F64
        | RustType::Bool
        | RustType::String
        | RustType::Unit
        | RustType::Named(_) => true,
        RustType::Option(inner) => supports_partial_eq(inner),
        RustType::Result(ok, err) => supports_partial_eq(ok) && supports_partial_eq(err),
        RustType::Generic(base, args) => {
            supports_partial_eq(base) && args.iter().all(supports_partial_eq)
        }
        RustType::Tuple(types) => types.iter().all(supports_partial_eq),
        RustType::TypeParam(_)
        | RustType::ImplFn(_, _)
        | RustType::SelfType
        | RustType::Infer
        | RustType::ArcMutex(_) => false,
    }
}

/// Whether a type supports `Eq`.
///
/// Like `PartialEq`, but excludes floating-point types (`f32`, `f64`)
/// which only implement `PartialEq`, not `Eq`.
fn supports_eq(ty: &RustType) -> bool {
    match ty {
        RustType::I8
        | RustType::I16
        | RustType::I32
        | RustType::I64
        | RustType::U8
        | RustType::U16
        | RustType::U32
        | RustType::U64
        | RustType::Bool
        | RustType::String
        | RustType::Unit
        | RustType::Named(_) => true,
        RustType::F32
        | RustType::F64
        | RustType::TypeParam(_)
        | RustType::ImplFn(_, _)
        | RustType::SelfType
        | RustType::Infer
        | RustType::ArcMutex(_) => false,
        RustType::Tuple(types) => types.iter().all(supports_eq),
        RustType::Option(inner) => supports_eq(inner),
        RustType::Result(ok, err) => supports_eq(ok) && supports_eq(err),
        RustType::Generic(base, args) => supports_eq(base) && args.iter().all(supports_eq),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsc_syntax::rust_ir::{RustEnumVariant, RustFieldDef};

    // Test T049-10: infer_derives with [i32, String] includes PartialEq, Eq
    #[test]
    fn test_derive_inference_integer_and_string_includes_partial_eq_eq() {
        let types: Vec<&RustType> = vec![&RustType::I32, &RustType::String];
        let derives = infer_struct_derives(&types, false);
        assert!(derives.contains(&"Debug".to_owned()));
        assert!(derives.contains(&"Clone".to_owned()));
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(derives.contains(&"Eq".to_owned()));
    }

    // Test T049-11: infer_derives with [f64] includes PartialEq, no Eq
    #[test]
    fn test_derive_inference_float_includes_partial_eq_excludes_eq() {
        let types: Vec<&RustType> = vec![&RustType::F64];
        let derives = infer_struct_derives(&types, false);
        assert!(derives.contains(&"Debug".to_owned()));
        assert!(derives.contains(&"Clone".to_owned()));
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(!derives.contains(&"Eq".to_owned()));
    }

    // Test T049-12: infer_derives with TypeParam gets Debug and Clone only
    #[test]
    fn test_derive_inference_type_param_only_debug_clone() {
        let t_param = RustType::TypeParam("T".to_owned());
        let types: Vec<&RustType> = vec![&t_param];
        let derives = infer_struct_derives(&types, true);
        assert_eq!(derives, vec!["Debug".to_owned(), "Clone".to_owned()]);
    }

    #[test]
    fn test_derive_inference_simple_enum_gets_full_derives() {
        let variants = vec![
            RustEnumVariant {
                name: "North".to_owned(),
                fields: vec![],
                span: None,
            },
            RustEnumVariant {
                name: "South".to_owned(),
                fields: vec![],
                span: None,
            },
        ];
        let derives = infer_enum_derives(&variants);
        assert!(derives.contains(&"Debug".to_owned()));
        assert!(derives.contains(&"Clone".to_owned()));
        assert!(derives.contains(&"Copy".to_owned()));
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(derives.contains(&"Eq".to_owned()));
        assert!(derives.contains(&"Hash".to_owned()));
    }

    #[test]
    fn test_derive_inference_data_enum_with_float_excludes_eq() {
        let variants = vec![
            RustEnumVariant {
                name: "Circle".to_owned(),
                fields: vec![RustFieldDef {
                    public: true,
                    name: "radius".to_owned(),
                    ty: RustType::F64,
                    doc_comment: None,
                    span: None,
                }],
                span: None,
            },
            RustEnumVariant {
                name: "Square".to_owned(),
                fields: vec![RustFieldDef {
                    public: true,
                    name: "side".to_owned(),
                    ty: RustType::F64,
                    doc_comment: None,
                    span: None,
                }],
                span: None,
            },
        ];
        let derives = infer_enum_derives(&variants);
        assert!(derives.contains(&"Debug".to_owned()));
        assert!(derives.contains(&"Clone".to_owned()));
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(!derives.contains(&"Eq".to_owned()));
        assert!(!derives.contains(&"Copy".to_owned()));
    }

    #[test]
    fn test_derive_inference_option_of_eq_type_supports_eq() {
        let opt_i32 = RustType::Option(Box::new(RustType::I32));
        let types: Vec<&RustType> = vec![&opt_i32];
        let derives = infer_struct_derives(&types, false);
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(derives.contains(&"Eq".to_owned()));
    }

    #[test]
    fn test_derive_inference_option_of_float_excludes_eq() {
        let opt_f64 = RustType::Option(Box::new(RustType::F64));
        let types: Vec<&RustType> = vec![&opt_f64];
        let derives = infer_struct_derives(&types, false);
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(!derives.contains(&"Eq".to_owned()));
    }

    #[test]
    fn test_derive_inference_all_integers_includes_eq() {
        let types: Vec<&RustType> = vec![&RustType::I32, &RustType::U64, &RustType::Bool];
        let derives = infer_struct_derives(&types, false);
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(derives.contains(&"Eq".to_owned()));
    }

    #[test]
    fn test_derive_inference_mixed_float_and_int_excludes_eq() {
        let types: Vec<&RustType> = vec![&RustType::I32, &RustType::F32];
        let derives = infer_struct_derives(&types, false);
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(!derives.contains(&"Eq".to_owned()));
    }

    #[test]
    fn test_derive_inference_empty_fields_gets_all_derives() {
        let types: Vec<&RustType> = vec![];
        let derives = infer_struct_derives(&types, false);
        assert!(derives.contains(&"Debug".to_owned()));
        assert!(derives.contains(&"Clone".to_owned()));
        assert!(derives.contains(&"PartialEq".to_owned()));
        assert!(derives.contains(&"Eq".to_owned()));
    }

    #[test]
    fn test_derive_inference_impl_fn_excludes_partial_eq() {
        let closure = RustType::ImplFn(vec![RustType::I32], Box::new(RustType::I32));
        let types: Vec<&RustType> = vec![&closure];
        let derives = infer_struct_derives(&types, false);
        assert!(derives.contains(&"Debug".to_owned()));
        assert!(derives.contains(&"Clone".to_owned()));
        assert!(!derives.contains(&"PartialEq".to_owned()));
        assert!(!derives.contains(&"Eq".to_owned()));
    }
}

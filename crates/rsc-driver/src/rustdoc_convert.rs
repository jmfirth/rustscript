//! Conversion from rustdoc parsed types to external function info.
//!
//! Bridges the gap between `RustdocFunction`/`RustdocType` (from the rustdoc
//! JSON parser) and `ExternalFnInfo` (consumed by the lowering pass for
//! call-site analysis).

use std::collections::HashMap;

use rsc_syntax::external_fn::{ExternalFnInfo, ExternalParamInfo, ExternalReturnType};

use crate::rustdoc_parser::{RustdocCrate, RustdocFunction, RustdocItemKind, RustdocType};

/// Convert all functions in a rustdoc crate to external function info.
///
/// Returns a map keyed by qualified name (`"crate::function"` or
/// `"crate::Type::method"`). The `crate_name` is used as the prefix
/// for all qualified names.
#[must_use]
pub fn convert_crate_to_external_fns(
    crate_name: &str,
    crate_data: &RustdocCrate,
) -> HashMap<String, ExternalFnInfo> {
    let mut result = HashMap::new();

    for item in crate_data.items.values() {
        if let RustdocItemKind::Function(func) = &item.kind {
            let key = if let Some(ref parent) = func.parent_type {
                format!("{crate_name}::{parent}::{}", item.name)
            } else {
                format!("{crate_name}::{}", item.name)
            };

            let info = convert_function(crate_name, &item.name, func);
            result.insert(key, info);
        }
    }

    result
}

/// Convert a single `RustdocFunction` to an `ExternalFnInfo`.
#[must_use]
pub fn convert_function(crate_name: &str, name: &str, func: &RustdocFunction) -> ExternalFnInfo {
    let params = func
        .params
        .iter()
        .map(|(param_name, param_type)| classify_param(param_name, param_type))
        .collect();

    let return_type = func
        .return_type
        .as_ref()
        .map_or(ExternalReturnType::Unit, classify_return_type);

    ExternalFnInfo {
        name: name.to_owned(),
        crate_name: crate_name.to_owned(),
        params,
        return_type,
        is_async: func.is_async,
        is_method: func.has_self,
        parent_type: func.parent_type.clone(),
    }
}

/// Classify a parameter type for external function info.
fn classify_param(name: &str, ty: &RustdocType) -> ExternalParamInfo {
    match ty {
        RustdocType::BorrowedRef { is_mutable, ty } => {
            let is_str_ref = is_str_type(ty);
            ExternalParamInfo {
                name: name.to_owned(),
                is_ref: true,
                is_str_ref,
                is_mut_ref: *is_mutable,
            }
        }
        _ => ExternalParamInfo {
            name: name.to_owned(),
            is_ref: false,
            is_str_ref: false,
            is_mut_ref: false,
        },
    }
}

/// Check if a type is `str` (the primitive, not `String`).
fn is_str_type(ty: &RustdocType) -> bool {
    matches!(ty, RustdocType::Primitive(name) if name == "str")
}

/// Classify a return type for external function info.
fn classify_return_type(ty: &RustdocType) -> ExternalReturnType {
    match ty {
        RustdocType::ResolvedPath { name, .. } => {
            if name == "Result" {
                ExternalReturnType::Result
            } else if name == "Option" {
                ExternalReturnType::Option
            } else {
                ExternalReturnType::Value
            }
        }
        RustdocType::Tuple(types) if types.is_empty() => ExternalReturnType::Unit,
        _ => ExternalReturnType::Value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rustdoc_parser::{RustdocItem, RustdocType};

    #[test]
    fn test_convert_function_simple() {
        let func = RustdocFunction {
            generics: vec![],
            params: vec![(
                "path".to_owned(),
                RustdocType::BorrowedRef {
                    is_mutable: false,
                    ty: Box::new(RustdocType::Primitive("str".to_owned())),
                },
            )],
            return_type: Some(RustdocType::ResolvedPath {
                name: "String".to_owned(),
                args: vec![],
            }),
            is_async: false,
            is_unsafe: false,
            has_self: false,
            parent_type: None,
        };

        let info = convert_function("my_crate", "do_thing", &func);
        assert_eq!(info.name, "do_thing");
        assert_eq!(info.crate_name, "my_crate");
        assert_eq!(info.params.len(), 1);
        assert!(info.params[0].is_ref);
        assert!(info.params[0].is_str_ref);
        assert!(!info.params[0].is_mut_ref);
        assert!(matches!(info.return_type, ExternalReturnType::Value));
        assert!(!info.is_async);
        assert!(!info.is_method);
        assert!(info.parent_type.is_none());
    }

    #[test]
    fn test_convert_function_async_method_result() {
        let func = RustdocFunction {
            generics: vec![],
            params: vec![(
                "data".to_owned(),
                RustdocType::BorrowedRef {
                    is_mutable: true,
                    ty: Box::new(RustdocType::ResolvedPath {
                        name: "Vec".to_owned(),
                        args: vec![RustdocType::Primitive("u8".to_owned())],
                    }),
                },
            )],
            return_type: Some(RustdocType::ResolvedPath {
                name: "Result".to_owned(),
                args: vec![
                    RustdocType::Tuple(vec![]),
                    RustdocType::ResolvedPath {
                        name: "Error".to_owned(),
                        args: vec![],
                    },
                ],
            }),
            is_async: true,
            is_unsafe: false,
            has_self: true,
            parent_type: Some("Server".to_owned()),
        };

        let info = convert_function("axum", "serve", &func);
        assert_eq!(info.name, "serve");
        assert!(info.is_async);
        assert!(info.is_method);
        assert_eq!(info.parent_type.as_deref(), Some("Server"));
        assert!(matches!(info.return_type, ExternalReturnType::Result));
        assert!(info.params[0].is_mut_ref);
        assert!(!info.params[0].is_str_ref);
    }

    #[test]
    fn test_convert_function_option_return() {
        let func = RustdocFunction {
            generics: vec![],
            params: vec![],
            return_type: Some(RustdocType::ResolvedPath {
                name: "Option".to_owned(),
                args: vec![RustdocType::Primitive("i32".to_owned())],
            }),
            is_async: false,
            is_unsafe: false,
            has_self: false,
            parent_type: None,
        };

        let info = convert_function("std", "find", &func);
        assert!(matches!(info.return_type, ExternalReturnType::Option));
    }

    #[test]
    fn test_convert_function_unit_return() {
        let func = RustdocFunction {
            generics: vec![],
            params: vec![],
            return_type: None,
            is_async: false,
            is_unsafe: false,
            has_self: false,
            parent_type: None,
        };

        let info = convert_function("test", "noop", &func);
        assert!(matches!(info.return_type, ExternalReturnType::Unit));
    }

    #[test]
    fn test_convert_function_owned_param() {
        let func = RustdocFunction {
            generics: vec![],
            params: vec![(
                "value".to_owned(),
                RustdocType::ResolvedPath {
                    name: "String".to_owned(),
                    args: vec![],
                },
            )],
            return_type: None,
            is_async: false,
            is_unsafe: false,
            has_self: false,
            parent_type: None,
        };

        let info = convert_function("test", "take", &func);
        assert!(!info.params[0].is_ref);
        assert!(!info.params[0].is_str_ref);
        assert!(!info.params[0].is_mut_ref);
    }

    #[test]
    fn test_convert_crate_to_external_fns() {
        let mut crate_data = RustdocCrate::default();

        // Add a free function
        let func_item = RustdocItem {
            id: "0:1".to_owned(),
            name: "parse".to_owned(),
            docs: None,
            kind: RustdocItemKind::Function(RustdocFunction {
                generics: vec![],
                params: vec![(
                    "input".to_owned(),
                    RustdocType::BorrowedRef {
                        is_mutable: false,
                        ty: Box::new(RustdocType::Primitive("str".to_owned())),
                    },
                )],
                return_type: Some(RustdocType::ResolvedPath {
                    name: "Result".to_owned(),
                    args: vec![],
                }),
                is_async: false,
                is_unsafe: false,
                has_self: false,
                parent_type: None,
            }),
        };
        crate_data.items.insert("0:1".to_owned(), func_item);

        // Add a method
        let method_item = RustdocItem {
            id: "0:2".to_owned(),
            name: "route".to_owned(),
            docs: None,
            kind: RustdocItemKind::Function(RustdocFunction {
                generics: vec![],
                params: vec![],
                return_type: Some(RustdocType::ResolvedPath {
                    name: "Router".to_owned(),
                    args: vec![],
                }),
                is_async: false,
                is_unsafe: false,
                has_self: true,
                parent_type: Some("Router".to_owned()),
            }),
        };
        crate_data.items.insert("0:2".to_owned(), method_item);

        let result = convert_crate_to_external_fns("axum", &crate_data);

        assert_eq!(result.len(), 2);
        assert!(result.contains_key("axum::parse"));
        assert!(result.contains_key("axum::Router::route"));

        let parse_info = &result["axum::parse"];
        assert_eq!(parse_info.name, "parse");
        assert!(matches!(parse_info.return_type, ExternalReturnType::Result));
        assert!(parse_info.params[0].is_str_ref);

        let route_info = &result["axum::Router::route"];
        assert_eq!(route_info.name, "route");
        assert!(route_info.is_method);
        assert_eq!(route_info.parent_type.as_deref(), Some("Router"));
    }

    #[test]
    fn test_convert_crate_skips_non_functions() {
        let mut crate_data = RustdocCrate::default();

        // Add a struct (not a function)
        let struct_item = RustdocItem {
            id: "0:1".to_owned(),
            name: "MyStruct".to_owned(),
            docs: None,
            kind: RustdocItemKind::Struct(crate::rustdoc_parser::RustdocStruct {
                generics: vec![],
                fields: vec![],
                is_tuple: false,
                method_ids: vec![],
            }),
        };
        crate_data.items.insert("0:1".to_owned(), struct_item);

        let result = convert_crate_to_external_fns("test", &crate_data);
        assert!(result.is_empty());
    }

    #[test]
    fn test_classify_return_type_empty_tuple_is_unit() {
        let ty = RustdocType::Tuple(vec![]);
        assert!(matches!(
            classify_return_type(&ty),
            ExternalReturnType::Unit
        ));
    }

    #[test]
    fn test_classify_return_type_nonempty_tuple_is_value() {
        let ty = RustdocType::Tuple(vec![RustdocType::Primitive("i32".to_owned())]);
        assert!(matches!(
            classify_return_type(&ty),
            ExternalReturnType::Value
        ));
    }
}

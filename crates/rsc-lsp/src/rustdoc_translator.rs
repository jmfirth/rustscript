//! Translates rustdoc items to `RustScript` display syntax.
//!
//! Converts Rust function signatures, struct definitions, trait definitions,
//! and enum definitions from rustdoc JSON into `RustScript`-native syntax
//! for hover display. Uses the type translation rules from [`name_map`]
//! extended for full rustdoc type representations.
//!
//! [`name_map`]: crate::name_map

use rsc_driver::rustdoc_parser::{
    RustdocEnum, RustdocField, RustdocFunction, RustdocGenericParam, RustdocItem, RustdocItemKind,
    RustdocStruct, RustdocTrait, RustdocType, RustdocVariant, RustdocVariantKind,
};

/// Translate a rustdoc item to a `RustScript` hover display string.
///
/// Returns a markdown string suitable for LSP hover content, including
/// a `RustScript` code block and optional documentation.
#[must_use]
pub fn translate_item_to_hover(item: &RustdocItem) -> String {
    let code = match &item.kind {
        RustdocItemKind::Function(func) => translate_function(&item.name, func),
        RustdocItemKind::Struct(s) => translate_struct(&item.name, s),
        RustdocItemKind::Trait(t) => translate_trait(&item.name, t),
        RustdocItemKind::Enum(e) => translate_enum(&item.name, e),
    };

    let code_block = format!("```rustscript\n{code}\n```");

    match &item.docs {
        Some(docs) if !docs.is_empty() => format!("{docs}\n\n---\n\n{code_block}"),
        _ => code_block,
    }
}

/// Translate a function signature to `RustScript` syntax.
///
/// Produces output like:
/// ```text
/// function get<H extends Handler, T>(handler: H): MethodRouter
/// async function serve(): void
/// ```
#[must_use]
pub fn translate_function(name: &str, func: &RustdocFunction) -> String {
    let mut parts = Vec::new();

    if func.is_async {
        parts.push("async ".to_owned());
    }

    parts.push("function ".to_owned());

    // If this is a method, show it with dot notation.
    if let Some(parent) = &func.parent_type {
        parts.push(format!("{parent}."));
    }

    parts.push(name.to_owned());

    // Generic parameters.
    if !func.generics.is_empty() {
        parts.push(translate_generic_params(&func.generics));
    }

    // Parameters.
    let params_str: Vec<String> = func
        .params
        .iter()
        .map(|(pname, ty)| format!("{pname}: {}", translate_type(ty)))
        .collect();
    parts.push(format!("({})", params_str.join(", ")));

    // Return type.
    let ret = func
        .return_type
        .as_ref()
        .map_or_else(|| "void".to_owned(), translate_type);
    parts.push(format!(": {ret}"));

    parts.join("")
}

/// Translate a struct to `RustScript` syntax.
///
/// Produces output like:
/// ```text
/// class Router { ... }
/// type Point = { x: number, y: number }
/// ```
#[must_use]
pub fn translate_struct(name: &str, s: &RustdocStruct) -> String {
    let generics = if s.generics.is_empty() {
        String::new()
    } else {
        translate_generic_params(&s.generics)
    };

    if s.fields.is_empty() {
        format!("class {name}{generics}")
    } else {
        let fields: Vec<String> = s
            .fields
            .iter()
            .map(|f| format!("  {}: {}", f.name, translate_type(&f.ty)))
            .collect();
        format!("class {name}{generics} {{\n{}\n}}", fields.join("\n"))
    }
}

/// Translate a trait to `RustScript` syntax.
///
/// Produces output like:
/// ```text
/// interface Handler<T> { ... }
/// ```
#[must_use]
pub fn translate_trait(name: &str, t: &RustdocTrait) -> String {
    let generics = if t.generics.is_empty() {
        String::new()
    } else {
        translate_generic_params(&t.generics)
    };

    let methods_hint = if t.method_ids.is_empty() {
        String::new()
    } else {
        format!(" // {} methods", t.method_ids.len())
    };

    format!("interface {name}{generics}{methods_hint}")
}

/// Translate an enum to `RustScript` syntax.
///
/// For simple enums (all unit variants), produces a string union:
/// ```text
/// type Method = "Get" | "Post" | "Put" | "Delete"
/// ```
///
/// For complex enums, produces a discriminated union or enum notation.
#[must_use]
pub fn translate_enum(name: &str, e: &RustdocEnum) -> String {
    let generics = if e.generics.is_empty() {
        String::new()
    } else {
        translate_generic_params(&e.generics)
    };

    let all_plain = e
        .variants
        .iter()
        .all(|v| matches!(v.kind, RustdocVariantKind::Plain));

    if all_plain && !e.variants.is_empty() {
        let variant_strs: Vec<String> = e
            .variants
            .iter()
            .map(|v| format!("\"{}\"", v.name))
            .collect();
        format!("type {name}{generics} = {}", variant_strs.join(" | "))
    } else {
        let variant_strs: Vec<String> = e.variants.iter().map(translate_variant).collect();
        format!("enum {name}{generics} {{\n{}\n}}", variant_strs.join("\n"))
    }
}

/// Translate an enum variant to display syntax.
fn translate_variant(variant: &RustdocVariant) -> String {
    match &variant.kind {
        RustdocVariantKind::Plain => format!("  {}", variant.name),
        RustdocVariantKind::Tuple(types) => {
            let types_str: Vec<String> = types.iter().map(translate_type).collect();
            format!("  {}({})", variant.name, types_str.join(", "))
        }
        RustdocVariantKind::Struct(fields) => {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|f| format!("{}: {}", f.name, translate_type(&f.ty)))
                .collect();
            format!("  {} {{ {} }}", variant.name, fields_str.join(", "))
        }
    }
}

/// Translate generic parameters to `RustScript` syntax.
///
/// Converts `<T: Display + Clone>` to `<T extends Display & Clone>`.
fn translate_generic_params(params: &[RustdocGenericParam]) -> String {
    let parts: Vec<String> = params
        .iter()
        .map(|p| {
            if p.bounds.is_empty() {
                p.name.clone()
            } else {
                let bounds_str: Vec<String> = p.bounds.iter().map(|b| translate_bound(b)).collect();
                format!("{} extends {}", p.name, bounds_str.join(" & "))
            }
        })
        .collect();

    format!("<{}>", parts.join(", "))
}

/// Translate a trait bound name, applying known mappings.
fn translate_bound(bound: &str) -> String {
    // Apply known Rust-to-RTS trait name mappings.
    match bound {
        "Iterator" | "IntoIterator" => "Iterable".to_owned(),
        "Fn" | "FnMut" | "FnOnce" => "Function".to_owned(),
        other => other.to_owned(),
    }
}

/// Translate a rustdoc type to `RustScript` display syntax.
///
/// This is the core type translation function. It applies the mapping rules:
/// - `String` / `&str` -> `string`
/// - `Vec<T>` -> `Array<T>`
/// - `HashMap<K,V>` -> `Map<K,V>`
/// - `HashSet<T>` -> `Set<T>`
/// - `Option<T>` -> `T | null`
/// - `Result<T, E>` -> `T throws E`
/// - `Box<dyn Trait>` -> `Trait`
/// - `Arc<Mutex<T>>` -> `shared<T>`
/// - `&T` / `&mut T` -> `T` (borrows hidden)
/// - Lifetimes stripped
/// - `Self` -> `this`
#[must_use]
pub fn translate_type(ty: &RustdocType) -> String {
    match ty {
        RustdocType::Primitive(name) => translate_primitive(name),

        RustdocType::Generic(name) => {
            if name == "Self" {
                "this".to_owned()
            } else {
                name.clone()
            }
        }

        RustdocType::ResolvedPath { name, args } => translate_resolved_path(name, args),

        // Borrows are transparent in RustScript -- just show the inner type.
        RustdocType::BorrowedRef { ty, .. } => translate_type(ty),

        RustdocType::Tuple(types) => {
            if types.is_empty() {
                "void".to_owned()
            } else {
                let inner: Vec<String> = types.iter().map(translate_type).collect();
                format!("[{}]", inner.join(", "))
            }
        }

        RustdocType::Slice(inner) => {
            format!("Array<{}>", translate_type(inner))
        }

        RustdocType::Array { ty, len } => {
            format!("Array<{}> /* len: {len} */", translate_type(ty))
        }

        RustdocType::RawPointer { ty, .. } => {
            // Raw pointers shown as the inner type with a marker.
            format!("/* ptr */ {}", translate_type(ty))
        }

        RustdocType::ImplTrait(bounds) => {
            if bounds.is_empty() {
                "unknown".to_owned()
            } else {
                let translated: Vec<String> = bounds.iter().map(|b| translate_bound(b)).collect();
                translated.join(" & ")
            }
        }

        RustdocType::FnPointer {
            params,
            return_type,
        } => {
            let params_str: Vec<String> = params.iter().map(translate_type).collect();
            format!(
                "({}) => {}",
                params_str.join(", "),
                translate_type(return_type)
            )
        }

        RustdocType::QualifiedPath { name } => name.clone(),

        RustdocType::Infer => "(inferred)".to_owned(),

        RustdocType::Unknown(s) => s.clone(),
    }
}

/// Translate a primitive Rust type name to `RustScript` syntax.
fn translate_primitive(name: &str) -> String {
    match name {
        "str" | "String" | "char" => "string".to_owned(),
        "bool" => "boolean".to_owned(),
        "()" => "void".to_owned(),
        "never" | "!" => "never".to_owned(),
        other => other.to_owned(),
    }
}

/// Translate a resolved path type, handling known container types.
#[allow(clippy::too_many_lines)]
// All container type mappings belong in one match for clarity
fn translate_resolved_path(name: &str, args: &[RustdocType]) -> String {
    match name {
        // String types
        "String" => "string".to_owned(),

        // Container types
        "Vec" => {
            let inner = args.first().map_or("unknown", |_| "");
            if inner == "unknown" {
                "Array<unknown>".to_owned()
            } else {
                format!("Array<{}>", translate_type(&args[0]))
            }
        }
        "HashMap" | "BTreeMap" => {
            let key = args
                .first()
                .map_or_else(|| "unknown".to_owned(), translate_type);
            let val = args
                .get(1)
                .map_or_else(|| "unknown".to_owned(), translate_type);
            format!("Map<{key}, {val}>")
        }
        "HashSet" | "BTreeSet" => {
            let inner = args
                .first()
                .map_or_else(|| "unknown".to_owned(), translate_type);
            format!("Set<{inner}>")
        }

        // Option -> T | null
        "Option" => {
            let inner = args
                .first()
                .map_or_else(|| "unknown".to_owned(), translate_type);
            format!("{inner} | null")
        }

        // Result -> T throws E
        "Result" => {
            let ok = args
                .first()
                .map_or_else(|| "unknown".to_owned(), translate_type);
            let err = args
                .get(1)
                .map_or_else(|| "Error".to_owned(), translate_type);
            format!("{ok} throws {err}")
        }

        // Box<dyn Trait> -> just Trait
        "Box" => args
            .first()
            .map_or_else(|| "unknown".to_owned(), translate_type),

        // Arc<Mutex<T>> -> shared<T>
        "Arc" => {
            if let Some(RustdocType::ResolvedPath {
                name: inner_name,
                args: inner_args,
            }) = args.first()
                && inner_name == "Mutex"
            {
                let t = inner_args
                    .first()
                    .map_or_else(|| "unknown".to_owned(), translate_type);
                return format!("shared<{t}>");
            }
            args.first()
                .map_or_else(|| "unknown".to_owned(), translate_type)
        }

        // Rc is similar to Arc for display.
        "Rc" => args
            .first()
            .map_or_else(|| "unknown".to_owned(), translate_type),

        // Cow -> T (last arg, after lifetime)
        "Cow" => args
            .last()
            .map_or_else(|| "unknown".to_owned(), translate_type),

        // Pin<Box<dyn Future<Output = T>>> and similar
        "Pin" => args
            .first()
            .map_or_else(|| "unknown".to_owned(), translate_type),

        // Self -> this
        "Self" => "this".to_owned(),

        // User-defined types with generic args
        _ => {
            if args.is_empty() {
                name.to_owned()
            } else {
                let args_str: Vec<String> = args.iter().map(translate_type).collect();
                format!("{name}<{}>", args_str.join(", "))
            }
        }
    }
}

/// Translate a struct field for display.
#[must_use]
pub fn translate_field(field: &RustdocField) -> String {
    format!("{}: {}", field.name, translate_type(&field.ty))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Type translation tests -----

    #[test]
    fn test_translate_type_string_resolved_path() {
        let ty = RustdocType::ResolvedPath {
            name: "String".to_owned(),
            args: vec![],
        };
        assert_eq!(translate_type(&ty), "string");
    }

    #[test]
    fn test_translate_type_str_primitive() {
        let ty = RustdocType::Primitive("str".to_owned());
        assert_eq!(translate_type(&ty), "string");
    }

    #[test]
    fn test_translate_type_bool_primitive() {
        let ty = RustdocType::Primitive("bool".to_owned());
        assert_eq!(translate_type(&ty), "boolean");
    }

    #[test]
    fn test_translate_type_unit_tuple() {
        let ty = RustdocType::Tuple(vec![]);
        assert_eq!(translate_type(&ty), "void");
    }

    #[test]
    fn test_translate_type_vec_to_array() {
        let ty = RustdocType::ResolvedPath {
            name: "Vec".to_owned(),
            args: vec![RustdocType::Primitive("i32".to_owned())],
        };
        assert_eq!(translate_type(&ty), "Array<i32>");
    }

    #[test]
    fn test_translate_type_hashmap_to_map() {
        let ty = RustdocType::ResolvedPath {
            name: "HashMap".to_owned(),
            args: vec![
                RustdocType::ResolvedPath {
                    name: "String".to_owned(),
                    args: vec![],
                },
                RustdocType::Primitive("i32".to_owned()),
            ],
        };
        assert_eq!(translate_type(&ty), "Map<string, i32>");
    }

    #[test]
    fn test_translate_type_hashset_to_set() {
        let ty = RustdocType::ResolvedPath {
            name: "HashSet".to_owned(),
            args: vec![RustdocType::ResolvedPath {
                name: "String".to_owned(),
                args: vec![],
            }],
        };
        assert_eq!(translate_type(&ty), "Set<string>");
    }

    #[test]
    fn test_translate_type_option_to_nullable() {
        let ty = RustdocType::ResolvedPath {
            name: "Option".to_owned(),
            args: vec![RustdocType::ResolvedPath {
                name: "String".to_owned(),
                args: vec![],
            }],
        };
        assert_eq!(translate_type(&ty), "string | null");
    }

    #[test]
    fn test_translate_type_result_to_throws() {
        let ty = RustdocType::ResolvedPath {
            name: "Result".to_owned(),
            args: vec![
                RustdocType::Primitive("i32".to_owned()),
                RustdocType::ResolvedPath {
                    name: "String".to_owned(),
                    args: vec![],
                },
            ],
        };
        assert_eq!(translate_type(&ty), "i32 throws string");
    }

    #[test]
    fn test_translate_type_box_unwrapped() {
        let ty = RustdocType::ResolvedPath {
            name: "Box".to_owned(),
            args: vec![RustdocType::ImplTrait(vec!["Display".to_owned()])],
        };
        assert_eq!(translate_type(&ty), "Display");
    }

    #[test]
    fn test_translate_type_arc_mutex_to_shared() {
        let ty = RustdocType::ResolvedPath {
            name: "Arc".to_owned(),
            args: vec![RustdocType::ResolvedPath {
                name: "Mutex".to_owned(),
                args: vec![RustdocType::Primitive("i32".to_owned())],
            }],
        };
        assert_eq!(translate_type(&ty), "shared<i32>");
    }

    #[test]
    fn test_translate_type_borrow_hidden() {
        let ty = RustdocType::BorrowedRef {
            is_mutable: false,
            ty: Box::new(RustdocType::Primitive("str".to_owned())),
        };
        assert_eq!(translate_type(&ty), "string");
    }

    #[test]
    fn test_translate_type_mut_borrow_hidden() {
        let ty = RustdocType::BorrowedRef {
            is_mutable: true,
            ty: Box::new(RustdocType::ResolvedPath {
                name: "Vec".to_owned(),
                args: vec![RustdocType::Primitive("i32".to_owned())],
            }),
        };
        assert_eq!(translate_type(&ty), "Array<i32>");
    }

    #[test]
    fn test_translate_type_self_to_this() {
        let ty = RustdocType::Generic("Self".to_owned());
        assert_eq!(translate_type(&ty), "this");
    }

    #[test]
    fn test_translate_type_generic_passthrough() {
        let ty = RustdocType::Generic("T".to_owned());
        assert_eq!(translate_type(&ty), "T");
    }

    #[test]
    fn test_translate_type_fn_pointer() {
        let ty = RustdocType::FnPointer {
            params: vec![RustdocType::Primitive("i32".to_owned())],
            return_type: Box::new(RustdocType::Primitive("bool".to_owned())),
        };
        assert_eq!(translate_type(&ty), "(i32) => boolean");
    }

    #[test]
    fn test_translate_type_slice_to_array() {
        let ty = RustdocType::Slice(Box::new(RustdocType::Primitive("u8".to_owned())));
        assert_eq!(translate_type(&ty), "Array<u8>");
    }

    #[test]
    fn test_translate_type_tuple_to_tuple() {
        let ty = RustdocType::Tuple(vec![
            RustdocType::Primitive("i32".to_owned()),
            RustdocType::Primitive("bool".to_owned()),
        ]);
        assert_eq!(translate_type(&ty), "[i32, boolean]");
    }

    #[test]
    fn test_translate_type_impl_trait() {
        let ty = RustdocType::ImplTrait(vec!["Display".to_owned(), "Clone".to_owned()]);
        assert_eq!(translate_type(&ty), "Display & Clone");
    }

    #[test]
    fn test_translate_type_cow_unwrapped() {
        let ty = RustdocType::ResolvedPath {
            name: "Cow".to_owned(),
            args: vec![
                RustdocType::Generic("'a".to_owned()),
                RustdocType::Primitive("str".to_owned()),
            ],
        };
        assert_eq!(translate_type(&ty), "string");
    }

    #[test]
    fn test_translate_type_rc_unwrapped() {
        let ty = RustdocType::ResolvedPath {
            name: "Rc".to_owned(),
            args: vec![RustdocType::ResolvedPath {
                name: "String".to_owned(),
                args: vec![],
            }],
        };
        assert_eq!(translate_type(&ty), "string");
    }

    #[test]
    fn test_translate_type_user_defined_generic() {
        let ty = RustdocType::ResolvedPath {
            name: "Response".to_owned(),
            args: vec![RustdocType::Generic("T".to_owned())],
        };
        assert_eq!(translate_type(&ty), "Response<T>");
    }

    #[test]
    fn test_translate_type_user_defined_no_args() {
        let ty = RustdocType::ResolvedPath {
            name: "Router".to_owned(),
            args: vec![],
        };
        assert_eq!(translate_type(&ty), "Router");
    }

    // ----- Function translation tests -----

    #[test]
    fn test_translate_function_simple() {
        let func = RustdocFunction {
            generics: vec![],
            params: vec![(
                "name".to_owned(),
                RustdocType::ResolvedPath {
                    name: "String".to_owned(),
                    args: vec![],
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
        assert_eq!(
            translate_function("greet", &func),
            "function greet(name: string): string"
        );
    }

    #[test]
    fn test_translate_function_async() {
        let func = RustdocFunction {
            generics: vec![],
            params: vec![],
            return_type: None,
            is_async: true,
            is_unsafe: false,
            has_self: false,
            parent_type: None,
        };
        assert_eq!(
            translate_function("serve", &func),
            "async function serve(): void"
        );
    }

    #[test]
    fn test_translate_function_with_generics() {
        let func = RustdocFunction {
            generics: vec![
                RustdocGenericParam {
                    name: "H".to_owned(),
                    bounds: vec!["Handler".to_owned()],
                },
                RustdocGenericParam {
                    name: "T".to_owned(),
                    bounds: vec![],
                },
            ],
            params: vec![("handler".to_owned(), RustdocType::Generic("H".to_owned()))],
            return_type: Some(RustdocType::ResolvedPath {
                name: "MethodRouter".to_owned(),
                args: vec![],
            }),
            is_async: false,
            is_unsafe: false,
            has_self: false,
            parent_type: None,
        };
        assert_eq!(
            translate_function("get", &func),
            "function get<H extends Handler, T>(handler: H): MethodRouter"
        );
    }

    #[test]
    fn test_translate_function_method_with_parent() {
        let func = RustdocFunction {
            generics: vec![],
            params: vec![
                (
                    "path".to_owned(),
                    RustdocType::BorrowedRef {
                        is_mutable: false,
                        ty: Box::new(RustdocType::Primitive("str".to_owned())),
                    },
                ),
                (
                    "handler".to_owned(),
                    RustdocType::ResolvedPath {
                        name: "MethodRouter".to_owned(),
                        args: vec![],
                    },
                ),
            ],
            return_type: Some(RustdocType::Generic("Self".to_owned())),
            is_async: false,
            is_unsafe: false,
            has_self: true,
            parent_type: Some("Router".to_owned()),
        };
        assert_eq!(
            translate_function("route", &func),
            "function Router.route(path: string, handler: MethodRouter): this"
        );
    }

    // ----- Struct translation tests -----

    #[test]
    fn test_translate_struct_empty() {
        let s = RustdocStruct {
            generics: vec![],
            fields: vec![],
            is_tuple: false,
            method_ids: vec![],
        };
        assert_eq!(translate_struct("Router", &s), "class Router");
    }

    #[test]
    fn test_translate_struct_with_fields() {
        let s = RustdocStruct {
            generics: vec![],
            fields: vec![
                RustdocField {
                    name: "name".to_owned(),
                    ty: RustdocType::ResolvedPath {
                        name: "String".to_owned(),
                        args: vec![],
                    },
                },
                RustdocField {
                    name: "age".to_owned(),
                    ty: RustdocType::Primitive("u32".to_owned()),
                },
            ],
            is_tuple: false,
            method_ids: vec![],
        };
        assert_eq!(
            translate_struct("User", &s),
            "class User {\n  name: string\n  age: u32\n}"
        );
    }

    #[test]
    fn test_translate_struct_with_generics() {
        let s = RustdocStruct {
            generics: vec![RustdocGenericParam {
                name: "T".to_owned(),
                bounds: vec![],
            }],
            fields: vec![],
            is_tuple: false,
            method_ids: vec![],
        };
        assert_eq!(translate_struct("Wrapper", &s), "class Wrapper<T>");
    }

    // ----- Trait translation tests -----

    #[test]
    fn test_translate_trait_simple() {
        let t = RustdocTrait {
            generics: vec![],
            method_ids: vec!["0:1".to_owned(), "0:2".to_owned()],
        };
        assert_eq!(
            translate_trait("Handler", &t),
            "interface Handler // 2 methods"
        );
    }

    #[test]
    fn test_translate_trait_with_generics() {
        let t = RustdocTrait {
            generics: vec![RustdocGenericParam {
                name: "T".to_owned(),
                bounds: vec!["Send".to_owned()],
            }],
            method_ids: vec![],
        };
        assert_eq!(
            translate_trait("Handler", &t),
            "interface Handler<T extends Send>"
        );
    }

    // ----- Enum translation tests -----

    #[test]
    fn test_translate_enum_all_plain_variants() {
        let e = RustdocEnum {
            generics: vec![],
            variants: vec![
                RustdocVariant {
                    name: "Get".to_owned(),
                    kind: RustdocVariantKind::Plain,
                },
                RustdocVariant {
                    name: "Post".to_owned(),
                    kind: RustdocVariantKind::Plain,
                },
                RustdocVariant {
                    name: "Put".to_owned(),
                    kind: RustdocVariantKind::Plain,
                },
                RustdocVariant {
                    name: "Delete".to_owned(),
                    kind: RustdocVariantKind::Plain,
                },
            ],
        };
        assert_eq!(
            translate_enum("Method", &e),
            "type Method = \"Get\" | \"Post\" | \"Put\" | \"Delete\""
        );
    }

    #[test]
    fn test_translate_enum_with_data() {
        let e = RustdocEnum {
            generics: vec![],
            variants: vec![
                RustdocVariant {
                    name: "Ok".to_owned(),
                    kind: RustdocVariantKind::Tuple(vec![RustdocType::Generic("T".to_owned())]),
                },
                RustdocVariant {
                    name: "Err".to_owned(),
                    kind: RustdocVariantKind::Tuple(vec![RustdocType::Generic("E".to_owned())]),
                },
            ],
        };
        assert_eq!(
            translate_enum("Result", &e),
            "enum Result {\n  Ok(T)\n  Err(E)\n}"
        );
    }

    // ----- Full item hover tests -----

    #[test]
    fn test_translate_item_to_hover_with_docs() {
        let item = RustdocItem {
            id: "0:1".to_owned(),
            name: "greet".to_owned(),
            docs: Some("Greets a user.".to_owned()),
            kind: RustdocItemKind::Function(RustdocFunction {
                generics: vec![],
                params: vec![(
                    "name".to_owned(),
                    RustdocType::ResolvedPath {
                        name: "String".to_owned(),
                        args: vec![],
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
            }),
        };
        let hover = translate_item_to_hover(&item);
        assert!(hover.contains("Greets a user."));
        assert!(hover.contains("```rustscript"));
        assert!(hover.contains("function greet(name: string): string"));
    }

    #[test]
    fn test_translate_item_to_hover_without_docs() {
        let item = RustdocItem {
            id: "0:1".to_owned(),
            name: "Router".to_owned(),
            docs: None,
            kind: RustdocItemKind::Struct(RustdocStruct {
                generics: vec![],
                fields: vec![],
                is_tuple: false,
                method_ids: vec![],
            }),
        };
        let hover = translate_item_to_hover(&item);
        assert!(hover.starts_with("```rustscript\nclass Router\n```"));
        assert!(!hover.contains("---"));
    }

    #[test]
    fn test_translate_type_nested_option_vec() {
        // Option<Vec<String>> -> Array<string> | null
        let ty = RustdocType::ResolvedPath {
            name: "Option".to_owned(),
            args: vec![RustdocType::ResolvedPath {
                name: "Vec".to_owned(),
                args: vec![RustdocType::ResolvedPath {
                    name: "String".to_owned(),
                    args: vec![],
                }],
            }],
        };
        assert_eq!(translate_type(&ty), "Array<string> | null");
    }

    #[test]
    fn test_translate_type_result_option() {
        let ty = RustdocType::ResolvedPath {
            name: "Result".to_owned(),
            args: vec![
                RustdocType::ResolvedPath {
                    name: "Option".to_owned(),
                    args: vec![RustdocType::Primitive("i32".to_owned())],
                },
                RustdocType::ResolvedPath {
                    name: "String".to_owned(),
                    args: vec![],
                },
            ],
        };
        assert_eq!(translate_type(&ty), "i32 | null throws string");
    }

    #[test]
    fn test_translate_type_btreemap() {
        let ty = RustdocType::ResolvedPath {
            name: "BTreeMap".to_owned(),
            args: vec![
                RustdocType::ResolvedPath {
                    name: "String".to_owned(),
                    args: vec![],
                },
                RustdocType::Primitive("i32".to_owned()),
            ],
        };
        assert_eq!(translate_type(&ty), "Map<string, i32>");
    }

    #[test]
    fn test_translate_type_btreeset() {
        let ty = RustdocType::ResolvedPath {
            name: "BTreeSet".to_owned(),
            args: vec![RustdocType::Primitive("i32".to_owned())],
        };
        assert_eq!(translate_type(&ty), "Set<i32>");
    }

    #[test]
    fn test_translate_type_char_to_string() {
        let ty = RustdocType::Primitive("char".to_owned());
        assert_eq!(translate_type(&ty), "string");
    }

    #[test]
    fn test_translate_type_never() {
        let ty = RustdocType::Primitive("never".to_owned());
        assert_eq!(translate_type(&ty), "never");
    }

    #[test]
    fn test_translate_type_infer() {
        let ty = RustdocType::Infer;
        assert_eq!(translate_type(&ty), "(inferred)");
    }

    #[test]
    fn test_translate_generic_params_single() {
        let params = vec![RustdocGenericParam {
            name: "T".to_owned(),
            bounds: vec![],
        }];
        assert_eq!(translate_generic_params(&params), "<T>");
    }

    #[test]
    fn test_translate_generic_params_with_bounds() {
        let params = vec![RustdocGenericParam {
            name: "T".to_owned(),
            bounds: vec!["Display".to_owned(), "Clone".to_owned()],
        }];
        assert_eq!(
            translate_generic_params(&params),
            "<T extends Display & Clone>"
        );
    }

    #[test]
    fn test_translate_generic_params_multiple() {
        let params = vec![
            RustdocGenericParam {
                name: "K".to_owned(),
                bounds: vec!["Eq".to_owned()],
            },
            RustdocGenericParam {
                name: "V".to_owned(),
                bounds: vec![],
            },
        ];
        assert_eq!(translate_generic_params(&params), "<K extends Eq, V>");
    }
}

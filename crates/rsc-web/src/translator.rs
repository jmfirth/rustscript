//! Translates rustdoc items to `RustScript` display syntax.
//!
//! Converts Rust function signatures, struct definitions, trait definitions,
//! and enum definitions from rustdoc JSON into `RustScript`-native syntax
//! for hover display. This is a copy of the translator from `rsc-lsp` to
//! avoid pulling in WASM-incompatible dependencies (tower-lsp, tokio).

use rsc_driver::rustdoc_parser::{
    RustdocEnum, RustdocField, RustdocFunction, RustdocGenericParam, RustdocItem, RustdocItemKind,
    RustdocStruct, RustdocTrait, RustdocType, RustdocVariant, RustdocVariantKind,
};

/// Translate a rustdoc item to a `RustScript` hover display string.
///
/// Returns a markdown string suitable for display, including a `RustScript`
/// code block and optional documentation.
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
    match bound {
        "Iterator" | "IntoIterator" => "Iterable".to_owned(),
        "Fn" | "FnMut" | "FnOnce" => "Function".to_owned(),
        other => other.to_owned(),
    }
}

/// Translate a rustdoc type to `RustScript` display syntax.
///
/// Applies the mapping rules:
/// - `String` / `&str` -> `string`
/// - `Vec<T>` -> `Array<T>`
/// - `HashMap<K,V>` -> `Map<K,V>`
/// - `Option<T>` -> `T | null`
/// - `Result<T, E>` -> `T throws E`
/// - `Box<dyn Trait>` -> `Trait`
/// - `Arc<Mutex<T>>` -> `shared<T>`
/// - `&T` / `&mut T` -> `T` (borrows hidden)
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

        RustdocType::QualifiedPath {
            name,
            self_type,
            trait_name: _,
        } => {
            match self_type.as_deref() {
                // <Self as Trait>::Item -> just "Item"
                Some(RustdocType::Generic(g)) if g == "Self" => name.clone(),
                // <T as Trait>::Item -> "T.Item" (RustScript dot notation)
                Some(RustdocType::Generic(g)) => format!("{g}.{name}"),
                // Anything else -> just the name
                _ => name.clone(),
            }
        }

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

        // Infallible -> never
        "Infallible" => "never".to_owned(),

        // User-defined types with generic args
        _ => {
            // Strip module paths: "std::convert::Infallible" -> "Infallible",
            // "crate::routing::MethodFilter" -> "MethodFilter"
            let short_name = name.rsplit("::").next().unwrap_or(name);

            // Check the short name for known translations
            if short_name == "Infallible" {
                return "never".to_owned();
            }

            if args.is_empty() {
                short_name.to_owned()
            } else {
                let args_str: Vec<String> = args.iter().map(translate_type).collect();
                format!("{short_name}<{}>", args_str.join(", "))
            }
        }
    }
}

/// Translate a struct field for display.
#[must_use]
pub fn translate_field(field: &RustdocField) -> String {
    format!("{}: {}", field.name, translate_type(&field.ty))
}

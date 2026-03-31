//! Reverse name mapping from Rust identifiers to `RustScript` identifiers.
//!
//! When rust-analyzer returns completions or type information using Rust names
//! (e.g., `to_uppercase`, `Vec<T>`), this module translates them back to the
//! `RustScript` equivalents (e.g., `toUpperCase`, `Array<T>`).
//!
//! Also provides [`rust_type_to_rts_display`] for converting the compiler's
//! [`RustType`](rsc_syntax::rust_ir::RustType) enum to `RustScript` display syntax,
//! ensuring that the developer never sees Rust types in the editor.

use std::collections::HashMap;
use std::sync::LazyLock;

use rsc_syntax::rust_ir::RustType;

/// Mapping from Rust method/field names to their `RustScript` equivalents.
static REVERSE_METHOD_NAMES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("to_uppercase", "toUpperCase");
    m.insert("to_lowercase", "toLowerCase");
    m.insert("starts_with", "startsWith");
    m.insert("ends_with", "endsWith");
    m.insert("contains", "includes");
    m.insert("len", "length");
    m.insert("push", "push");
    m.insert("pop", "pop");
    m.insert("is_empty", "isEmpty");
    m.insert("trim", "trim");
    m.insert("split", "split");
    m.insert("join", "join");
    m.insert("chars", "chars");
    m.insert("to_string", "toString");
    m
});

/// Mapping from Rust type names to their `RustScript` equivalents.
static REVERSE_TYPE_NAMES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("Vec", "Array");
    m.insert("HashMap", "Map");
    m.insert("HashSet", "Set");
    m.insert("String", "string");
    m.insert("bool", "boolean");
    m.insert("i32", "number");
    m.insert("f64", "number");
    m.insert("()", "void");
    m
});

/// Translate a Rust method name to its `RustScript` equivalent.
///
/// Returns the `RustScript` name if a mapping exists, or the original name if not.
#[must_use]
pub fn translate_method_name(rust_name: &str) -> &str {
    REVERSE_METHOD_NAMES
        .get(rust_name)
        .copied()
        .unwrap_or(rust_name)
}

/// Translate a Rust type name to its `RustScript` equivalent.
///
/// Returns the `RustScript` name if a mapping exists, or the original name if not.
#[must_use]
pub fn translate_type_name(rust_name: &str) -> &str {
    REVERSE_TYPE_NAMES
        .get(rust_name)
        .copied()
        .unwrap_or(rust_name)
}

/// Translate a Rust type string to `RustScript` syntax.
///
/// Handles generic types like `Vec<String>` -> `Array<string>` and
/// nested generics like `HashMap<String, Vec<i32>>` -> `Map<string, Array<number>>`.
/// Also handles `Option<T>` -> `T | null`, `Result<T, E>` -> `T throws E`,
/// `fn(A) -> B` / `impl Fn(A) -> B` -> `(A) => B`, and
/// `Arc<Mutex<T>>` -> `shared<T>`.
#[must_use]
pub fn translate_type_string(rust_type: &str) -> String {
    let trimmed = rust_type.trim();

    // Handle `fn(...) -> T` or `impl Fn(...) -> T` patterns
    if let Some(rest) = trimmed
        .strip_prefix("fn(")
        .or_else(|| trimmed.strip_prefix("impl Fn("))
    {
        return translate_fn_type_string(rest);
    }

    // Handle generic types: look for `<` to split name from args.
    if let Some(bracket_pos) = trimmed.find('<') {
        let name = &trimmed[..bracket_pos];
        let rest = &trimmed[bracket_pos + 1..];
        // Strip trailing `>`.
        let inner = rest.strip_suffix('>').unwrap_or(rest);

        // Special cases for Rust types with different RTS representations
        match name {
            "Option" => {
                let inner_translated = translate_type_string(inner);
                return format!("{inner_translated} | null");
            }
            "Result" => {
                let args = split_generic_args(inner);
                if args.len() == 2 {
                    let ok = translate_type_string(&args[0]);
                    let err = translate_type_string(&args[1]);
                    return format!("{ok} throws {err}");
                }
            }
            "Arc" => {
                // Arc<Mutex<T>> -> shared<T>
                let inner_trimmed = inner.trim();
                if let Some(mutex_inner) = inner_trimmed
                    .strip_prefix("Mutex<")
                    .and_then(|s| s.strip_suffix('>'))
                {
                    let t = translate_type_string(mutex_inner);
                    return format!("shared<{t}>");
                }
            }
            _ => {}
        }

        let translated_name = translate_type_name(name);
        let translated_args = translate_generic_args(inner);
        format!("{translated_name}<{translated_args}>")
    } else {
        translate_type_name(trimmed).to_owned()
    }
}

/// Translate a Rust `fn(...) -> T` string (after stripping the `fn(` or `impl Fn(` prefix).
///
/// Expects input like `A, B) -> T`.
fn translate_fn_type_string(rest: &str) -> String {
    // Find the closing `)` for the params, handling nested parens
    let mut depth = 1;
    let mut split_pos = None;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    split_pos = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    if let Some(pos) = split_pos {
        let params_str = &rest[..pos];
        let after_paren = rest[pos + 1..].trim();

        let translated_params = if params_str.trim().is_empty() {
            String::new()
        } else {
            split_generic_args(params_str)
                .iter()
                .map(|p| translate_type_string(p))
                .collect::<Vec<_>>()
                .join(", ")
        };

        let ret_type = if let Some(ret) = after_paren.strip_prefix("->") {
            translate_type_string(ret.trim())
        } else {
            "void".to_owned()
        };

        format!("({translated_params}) => {ret_type}")
    } else {
        // Malformed — pass through
        format!("fn({rest}")
    }
}

/// Split comma-separated generic arguments respecting nesting depth.
fn split_generic_args(args: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut current = String::new();

    for c in args.chars() {
        match c {
            '<' | '(' => {
                depth += 1;
                current.push(c);
            }
            '>' | ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                result.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        result.push(current.trim().to_owned());
    }

    result
}

/// Translate comma-separated generic arguments, handling nested generics.
fn translate_generic_args(args: &str) -> String {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut current = String::new();

    for c in args.chars() {
        match c {
            '<' => {
                depth += 1;
                current.push(c);
            }
            '>' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                result.push(translate_type_string(current.trim()));
                current.clear();
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        result.push(translate_type_string(current.trim()));
    }

    result.join(", ")
}

/// Translate a completion label that may be a method or type name.
///
/// Checks method names first, then type names. Returns the translated label.
#[must_use]
pub fn translate_completion_label(label: &str) -> String {
    // Check if it's a method name.
    if let Some(&rts_name) = REVERSE_METHOD_NAMES.get(label) {
        return rts_name.to_owned();
    }

    // Check if it's a type with generics.
    if label.contains('<') {
        return translate_type_string(label);
    }

    // Check if it's a plain type name.
    if let Some(&rts_name) = REVERSE_TYPE_NAMES.get(label) {
        return rts_name.to_owned();
    }

    // No mapping — pass through.
    label.to_owned()
}

/// Convert a Rust IR [`RustType`] to `RustScript` display syntax.
///
/// This is the authoritative translation layer ensuring the developer never sees
/// Rust types in the editor. All LSP hover output, diagnostics, and completion
/// details should use this function.
///
/// # Examples
///
/// | Rust IR | RustScript display |
/// |---------|--------------------|
/// | `RustType::String` | `string` |
/// | `RustType::Bool` | `boolean` |
/// | `RustType::Unit` | `void` |
/// | `RustType::Option(T)` | `T \| null` |
/// | `RustType::Result(T, E)` | `T throws E` |
/// | `RustType::ImplFn([i32], i32)` | `(i32) => i32` |
/// | `RustType::ArcMutex(T)` | `shared<T>` |
/// | `Generic(Named("Vec"), [String])` | `Array<string>` |
/// | `Generic(Named("HashMap"), [K, V])` | `Map<K, V>` |
/// | `Generic(Named("HashSet"), [T])` | `Set<T>` |
#[must_use]
pub fn rust_type_to_rts_display(ty: &RustType) -> String {
    match ty {
        // Primitive numerics — pass through as-is (RustScript keeps Rust numeric names)
        RustType::I8 => "i8".to_owned(),
        RustType::I16 => "i16".to_owned(),
        RustType::I32 => "i32".to_owned(),
        RustType::I64 => "i64".to_owned(),
        RustType::U8 => "u8".to_owned(),
        RustType::U16 => "u16".to_owned(),
        RustType::U32 => "u32".to_owned(),
        RustType::U64 => "u64".to_owned(),
        RustType::F32 => "f32".to_owned(),
        RustType::F64 => "f64".to_owned(),

        // Rust `bool` -> RustScript `boolean`
        RustType::Bool => "boolean".to_owned(),

        // Rust `String` -> RustScript `string`
        RustType::String => "string".to_owned(),

        // Rust `()` -> RustScript `void`
        RustType::Unit => "void".to_owned(),

        // Rust `!` -> RustScript `never`
        RustType::Never => "never".to_owned(),

        // User-defined named types and type parameters pass through
        RustType::Named(name) | RustType::TypeParam(name) => name.clone(),

        // Generic types: translate container names
        RustType::Generic(base, args) => {
            let base_name = rust_type_base_to_rts(base);
            let args_str: Vec<String> = args.iter().map(rust_type_to_rts_display).collect();
            format!("{base_name}<{}>", args_str.join(", "))
        }

        // `Option<T>` -> `T | null`
        RustType::Option(inner) => {
            let inner_str = rust_type_to_rts_display(inner);
            format!("{inner_str} | null")
        }

        // `Result<T, E>` -> `T throws E`
        RustType::Result(ok, err) => {
            let ok_str = rust_type_to_rts_display(ok);
            let err_str = rust_type_to_rts_display(err);
            format!("{ok_str} throws {err_str}")
        }

        // `impl Fn(A, B) -> R` -> `(A, B) => R`
        RustType::ImplFn(params, ret) => {
            let params_str: Vec<String> = params.iter().map(rust_type_to_rts_display).collect();
            let ret_str = rust_type_to_rts_display(ret);
            format!("({}) => {ret_str}", params_str.join(", "))
        }

        // `Self` type — displayed as `this` in RustScript
        RustType::SelfType => "this".to_owned(),

        // Inferred type
        RustType::Infer => "(inferred)".to_owned(),

        // `Arc<Mutex<T>>` -> `shared<T>`
        RustType::ArcMutex(inner) => {
            let inner_str = rust_type_to_rts_display(inner);
            format!("shared<{inner_str}>")
        }

        // `(T1, T2, ...)` -> `[T1, T2, ...]`
        RustType::Tuple(types) => {
            let types_str: Vec<String> = types.iter().map(rust_type_to_rts_display).collect();
            format!("[{}]", types_str.join(", "))
        }

        // Generated union enum -> `T1 | T2 | ...`
        RustType::GeneratedUnion { variants, .. } => {
            let types_str: Vec<String> = variants
                .iter()
                .map(|(_, ty)| rust_type_to_rts_display(ty))
                .collect();
            types_str.join(" | ")
        }

        // `&dyn TraitName` -> display as the trait name (for polymorphic class params)
        RustType::DynRef(trait_name) => trait_name.clone(),

        // `Box<dyn std::any::Any>` -> `unknown`
        RustType::BoxDynAny => "unknown".to_owned(),

        // `&T` — reference type from `as const`
        RustType::Reference(inner) => {
            let inner_str = rust_type_to_rts_display(inner);
            format!("&{inner_str}")
        }

        // `[T]` — slice type
        RustType::Slice(inner) => {
            let inner_str = rust_type_to_rts_display(inner);
            format!("[{inner_str}]")
        }

        // `&str` — string slice reference
        RustType::StrRef => "&str".to_owned(),
    }
}

/// Translate a `RustType` used as a generic base name to its `RustScript` equivalent.
///
/// Maps `Vec` -> `Array`, `HashMap` -> `Map`, `HashSet` -> `Set`.
/// Other types pass through their normal display.
fn rust_type_base_to_rts(base: &RustType) -> String {
    match base {
        RustType::Named(name) => match name.as_str() {
            "Vec" => "Array".to_owned(),
            "HashMap" => "Map".to_owned(),
            "HashSet" => "Set".to_owned(),
            other => other.to_owned(),
        },
        other => rust_type_to_rts_display(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 5: Reverse name mapping
    #[test]
    fn test_name_map_to_uppercase_maps_to_to_upper_case() {
        assert_eq!(translate_method_name("to_uppercase"), "toUpperCase");
    }

    #[test]
    fn test_name_map_to_lowercase_maps_to_to_lower_case() {
        assert_eq!(translate_method_name("to_lowercase"), "toLowerCase");
    }

    #[test]
    fn test_name_map_starts_with_maps_to_starts_with() {
        assert_eq!(translate_method_name("starts_with"), "startsWith");
    }

    #[test]
    fn test_name_map_ends_with_maps_to_ends_with() {
        assert_eq!(translate_method_name("ends_with"), "endsWith");
    }

    #[test]
    fn test_name_map_contains_maps_to_includes() {
        assert_eq!(translate_method_name("contains"), "includes");
    }

    #[test]
    fn test_name_map_len_maps_to_length() {
        assert_eq!(translate_method_name("len"), "length");
    }

    #[test]
    fn test_name_map_unknown_method_passes_through() {
        assert_eq!(translate_method_name("custom_method"), "custom_method");
    }

    // Test 6: Reverse type mapping
    #[test]
    fn test_name_map_vec_string_maps_to_array_string() {
        assert_eq!(translate_type_string("Vec<String>"), "Array<string>");
    }

    #[test]
    fn test_name_map_hashmap_maps_to_map() {
        assert_eq!(translate_type_name("HashMap"), "Map");
    }

    #[test]
    fn test_name_map_hashset_maps_to_set() {
        assert_eq!(translate_type_name("HashSet"), "Set");
    }

    #[test]
    fn test_name_map_string_maps_to_string() {
        assert_eq!(translate_type_name("String"), "string");
    }

    #[test]
    fn test_name_map_bool_maps_to_boolean() {
        assert_eq!(translate_type_name("bool"), "boolean");
    }

    #[test]
    fn test_name_map_nested_generics() {
        assert_eq!(
            translate_type_string("HashMap<String, Vec<i32>>"),
            "Map<string, Array<number>>"
        );
    }

    #[test]
    fn test_name_map_plain_type_no_generics() {
        assert_eq!(translate_type_string("i32"), "number");
    }

    #[test]
    fn test_name_map_unknown_type_passes_through() {
        assert_eq!(translate_type_string("MyStruct"), "MyStruct");
    }

    // Correctness scenario 2: Name translation in completion
    #[test]
    fn test_name_map_correctness_completion_label_to_uppercase() {
        let translated = translate_completion_label("to_uppercase");
        assert_eq!(translated, "toUpperCase");
    }

    #[test]
    fn test_name_map_correctness_completion_label_vec_type() {
        let translated = translate_completion_label("Vec<String>");
        assert_eq!(translated, "Array<string>");
    }

    #[test]
    fn test_name_map_correctness_completion_label_passthrough() {
        let translated = translate_completion_label("my_field");
        assert_eq!(translated, "my_field");
    }

    // -----------------------------------------------------------------------
    // String-based translation: Option, Result, fn, Arc<Mutex>
    // -----------------------------------------------------------------------

    #[test]
    fn test_translate_type_string_option_to_union_null() {
        assert_eq!(translate_type_string("Option<String>"), "string | null");
    }

    #[test]
    fn test_translate_type_string_option_nested() {
        assert_eq!(
            translate_type_string("Option<Vec<String>>"),
            "Array<string> | null"
        );
    }

    #[test]
    fn test_translate_type_string_result_to_throws() {
        assert_eq!(
            translate_type_string("Result<i32, String>"),
            "number throws string"
        );
    }

    #[test]
    fn test_translate_type_string_fn_to_arrow() {
        assert_eq!(translate_type_string("fn() -> String"), "() => string");
    }

    #[test]
    fn test_translate_type_string_fn_with_params() {
        assert_eq!(
            translate_type_string("fn(i32, String) -> bool"),
            "(number, string) => boolean"
        );
    }

    #[test]
    fn test_translate_type_string_impl_fn_to_arrow() {
        assert_eq!(
            translate_type_string("impl Fn(i32) -> i32"),
            "(number) => number"
        );
    }

    #[test]
    fn test_translate_type_string_arc_mutex_to_shared() {
        assert_eq!(translate_type_string("Arc<Mutex<i32>>"), "shared<number>");
    }

    #[test]
    fn test_translate_type_string_unit_to_void() {
        assert_eq!(translate_type_string("()"), "void");
    }

    // -----------------------------------------------------------------------
    // rust_type_to_rts_display tests — RustType -> RustScript display syntax
    // -----------------------------------------------------------------------

    #[test]
    fn test_rts_display_string() {
        assert_eq!(rust_type_to_rts_display(&RustType::String), "string");
    }

    #[test]
    fn test_rts_display_bool() {
        assert_eq!(rust_type_to_rts_display(&RustType::Bool), "boolean");
    }

    #[test]
    fn test_rts_display_unit() {
        assert_eq!(rust_type_to_rts_display(&RustType::Unit), "void");
    }

    #[test]
    fn test_rts_display_i32() {
        assert_eq!(rust_type_to_rts_display(&RustType::I32), "i32");
    }

    #[test]
    fn test_rts_display_f64() {
        assert_eq!(rust_type_to_rts_display(&RustType::F64), "f64");
    }

    #[test]
    fn test_rts_display_named_type() {
        assert_eq!(
            rust_type_to_rts_display(&RustType::Named("User".to_owned())),
            "User"
        );
    }

    #[test]
    fn test_rts_display_type_param() {
        assert_eq!(
            rust_type_to_rts_display(&RustType::TypeParam("T".to_owned())),
            "T"
        );
    }

    #[test]
    fn test_rts_display_vec_string_to_array_string() {
        let ty = RustType::Generic(
            Box::new(RustType::Named("Vec".to_owned())),
            vec![RustType::String],
        );
        assert_eq!(rust_type_to_rts_display(&ty), "Array<string>");
    }

    #[test]
    fn test_rts_display_hashmap_to_map() {
        let ty = RustType::Generic(
            Box::new(RustType::Named("HashMap".to_owned())),
            vec![RustType::String, RustType::I32],
        );
        assert_eq!(rust_type_to_rts_display(&ty), "Map<string, i32>");
    }

    #[test]
    fn test_rts_display_hashset_to_set() {
        let ty = RustType::Generic(
            Box::new(RustType::Named("HashSet".to_owned())),
            vec![RustType::String],
        );
        assert_eq!(rust_type_to_rts_display(&ty), "Set<string>");
    }

    #[test]
    fn test_rts_display_option_to_union_null() {
        let ty = RustType::Option(Box::new(RustType::String));
        assert_eq!(rust_type_to_rts_display(&ty), "string | null");
    }

    #[test]
    fn test_rts_display_result_to_throws() {
        let ty = RustType::Result(Box::new(RustType::I32), Box::new(RustType::String));
        assert_eq!(rust_type_to_rts_display(&ty), "i32 throws string");
    }

    #[test]
    fn test_rts_display_impl_fn_to_arrow() {
        let ty = RustType::ImplFn(vec![RustType::I32], Box::new(RustType::I32));
        assert_eq!(rust_type_to_rts_display(&ty), "(i32) => i32");
    }

    #[test]
    fn test_rts_display_impl_fn_multi_params() {
        let ty = RustType::ImplFn(
            vec![RustType::String, RustType::I32],
            Box::new(RustType::Bool),
        );
        assert_eq!(rust_type_to_rts_display(&ty), "(string, i32) => boolean");
    }

    #[test]
    fn test_rts_display_arc_mutex_to_shared() {
        let ty = RustType::ArcMutex(Box::new(RustType::I32));
        assert_eq!(rust_type_to_rts_display(&ty), "shared<i32>");
    }

    #[test]
    fn test_rts_display_nested_option_vec() {
        // Option<Vec<String>> -> Array<string> | null
        let ty = RustType::Option(Box::new(RustType::Generic(
            Box::new(RustType::Named("Vec".to_owned())),
            vec![RustType::String],
        )));
        assert_eq!(rust_type_to_rts_display(&ty), "Array<string> | null");
    }

    #[test]
    fn test_rts_display_nested_hashmap_vec() {
        // HashMap<String, Vec<i32>> -> Map<string, Array<i32>>
        let ty = RustType::Generic(
            Box::new(RustType::Named("HashMap".to_owned())),
            vec![
                RustType::String,
                RustType::Generic(
                    Box::new(RustType::Named("Vec".to_owned())),
                    vec![RustType::I32],
                ),
            ],
        );
        assert_eq!(rust_type_to_rts_display(&ty), "Map<string, Array<i32>>");
    }

    #[test]
    fn test_rts_display_self_type() {
        assert_eq!(rust_type_to_rts_display(&RustType::SelfType), "this");
    }

    #[test]
    fn test_rts_display_infer() {
        assert_eq!(rust_type_to_rts_display(&RustType::Infer), "(inferred)");
    }

    #[test]
    fn test_rts_display_user_generic() {
        // Generic(Named("Response"), [Named("Data")]) -> Response<Data>
        let ty = RustType::Generic(
            Box::new(RustType::Named("Response".to_owned())),
            vec![RustType::Named("Data".to_owned())],
        );
        assert_eq!(rust_type_to_rts_display(&ty), "Response<Data>");
    }

    #[test]
    fn test_rts_display_all_numerics() {
        assert_eq!(rust_type_to_rts_display(&RustType::I8), "i8");
        assert_eq!(rust_type_to_rts_display(&RustType::I16), "i16");
        assert_eq!(rust_type_to_rts_display(&RustType::I64), "i64");
        assert_eq!(rust_type_to_rts_display(&RustType::U8), "u8");
        assert_eq!(rust_type_to_rts_display(&RustType::U16), "u16");
        assert_eq!(rust_type_to_rts_display(&RustType::U32), "u32");
        assert_eq!(rust_type_to_rts_display(&RustType::U64), "u64");
        assert_eq!(rust_type_to_rts_display(&RustType::F32), "f32");
    }

    #[test]
    fn test_rts_display_tuple_type() {
        let ty = RustType::Tuple(vec![RustType::String, RustType::I32]);
        assert_eq!(rust_type_to_rts_display(&ty), "[string, i32]");
    }

    #[test]
    fn test_rts_display_tuple_type_three_elements() {
        let ty = RustType::Tuple(vec![RustType::String, RustType::I32, RustType::Bool]);
        assert_eq!(rust_type_to_rts_display(&ty), "[string, i32, boolean]");
    }

    // ---- Task 065: General union type display ----

    #[test]
    fn test_rts_display_generated_union_two_types() {
        let ty = RustType::GeneratedUnion {
            name: "I32OrString".to_owned(),
            variants: vec![
                ("String".to_owned(), RustType::String),
                ("I32".to_owned(), RustType::I32),
            ],
        };
        assert_eq!(rust_type_to_rts_display(&ty), "string | i32");
    }

    #[test]
    fn test_rts_display_generated_union_three_types() {
        let ty = RustType::GeneratedUnion {
            name: "BoolOrI32OrString".to_owned(),
            variants: vec![
                ("String".to_owned(), RustType::String),
                ("I32".to_owned(), RustType::I32),
                ("Bool".to_owned(), RustType::Bool),
            ],
        };
        assert_eq!(rust_type_to_rts_display(&ty), "string | i32 | boolean");
    }
}

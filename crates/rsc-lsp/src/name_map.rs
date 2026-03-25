//! Reverse name mapping from Rust identifiers to `RustScript` identifiers.
//!
//! When rust-analyzer returns completions or type information using Rust names
//! (e.g., `to_uppercase`, `Vec<T>`), this module translates them back to the
//! `RustScript` equivalents (e.g., `toUpperCase`, `Array<T>`).

use std::collections::HashMap;
use std::sync::LazyLock;

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
#[must_use]
pub fn translate_type_string(rust_type: &str) -> String {
    // Handle generic types: look for `<` to split name from args.
    if let Some(bracket_pos) = rust_type.find('<') {
        let name = &rust_type[..bracket_pos];
        let rest = &rust_type[bracket_pos + 1..];
        // Strip trailing `>`.
        let inner = rest.strip_suffix('>').unwrap_or(rest);

        let translated_name = translate_type_name(name);
        let translated_args = translate_generic_args(inner);
        format!("{translated_name}<{translated_args}>")
    } else {
        translate_type_name(rust_type).to_owned()
    }
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
}

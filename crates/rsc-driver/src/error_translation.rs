//! Translation of `rustc` error messages into `RustScript`-friendly terminology.
//!
//! When cargo build/run fails on the generated `.rs` code, `rustc` emits error
//! messages referencing Rust types, lifetimes, and concepts. This module translates
//! those messages into `RustScript` equivalents so the developer sees familiar terms.

use regex::Regex;
use std::sync::LazyLock;

/// Header prepended to translated rustc error output.
const TRANSLATED_HEADER: &str = "RustScript compilation error (from rustc):";

/// Header prepended to raw (untranslated) rustc error output.
const RAW_HEADER: &str = "rustc error (in generated code):";

/// Compiled regex patterns for type name translations.
///
/// Each pattern targets a specific Rust type in "type position" contexts:
/// after colons, inside angle brackets, in error message descriptions.
struct TranslationPatterns {
    /// Matches `Vec<...>` with balanced angle brackets.
    vec_type: Regex,
    /// Matches `HashMap<...>` with balanced angle brackets.
    hashmap_type: Regex,
    /// Matches `HashSet<...>` with balanced angle brackets.
    hashset_type: Regex,
    /// Matches `Option<...>` with balanced angle brackets.
    option_type: Regex,
    /// Matches `Result<...>` with balanced angle brackets.
    result_type: Regex,
    /// Matches `String` as a standalone type (not part of another word).
    string_type: Regex,
    /// Matches `&str` as a type reference.
    str_ref_type: Regex,
    /// Matches `&String` as a type reference.
    string_ref_type: Regex,
    /// Matches `impl Fn(...)` / `impl FnMut(...)` / `impl FnOnce(...)` patterns.
    impl_fn_type: Regex,
    /// Matches `fn(...)` type syntax.
    fn_type: Regex,
    /// Matches `impl Trait` (not Fn-family).
    impl_trait: Regex,
    /// Matches `'static` lifetime annotations.
    static_lifetime: Regex,
}

static PATTERNS: LazyLock<TranslationPatterns> = LazyLock::new(|| TranslationPatterns {
    vec_type: Regex::new(r"\bVec<").expect("valid regex"),
    hashmap_type: Regex::new(r"\bHashMap<").expect("valid regex"),
    hashset_type: Regex::new(r"\bHashSet<").expect("valid regex"),
    option_type: Regex::new(r"\bOption<").expect("valid regex"),
    result_type: Regex::new(r"\bResult<").expect("valid regex"),
    string_type: Regex::new(r"\bString\b").expect("valid regex"),
    str_ref_type: Regex::new(r"&str\b").expect("valid regex"),
    string_ref_type: Regex::new(r"&String\b").expect("valid regex"),
    impl_fn_type: Regex::new(r"\bimpl\s+Fn(Mut|Once)?\(").expect("valid regex"),
    fn_type: Regex::new(r"\bfn\(").expect("valid regex"),
    impl_trait: Regex::new(r"\bimpl\s+([A-Z]\w+)\b").expect("valid regex"),
    static_lifetime: Regex::new(r"'static\s*").expect("valid regex"),
});

/// Translate `rustc` error output into `RustScript`-friendly terms.
///
/// Performs string substitution on type names in the error text. If the input
/// appears to contain recognized rustc error patterns, returns the translated
/// output with a descriptive header. Otherwise, returns the original text with
/// a fallback header.
#[must_use]
pub fn translate_rustc_errors(stderr: &str) -> String {
    if stderr.trim().is_empty() {
        return String::new();
    }

    let translated = translate_type_names(stderr);

    // If we actually changed something, use the translated header.
    // If nothing changed, use the raw header as fallback.
    if translated == stderr {
        format!("{RAW_HEADER}\n{stderr}")
    } else {
        format!("{TRANSLATED_HEADER}\n{translated}")
    }
}

/// Apply all type name translations to the given text.
///
/// Handles nested generics by processing from the inside out: first translates
/// the innermost type names, then works outward through generic wrappers.
fn translate_type_names(input: &str) -> String {
    let mut output = input.to_owned();

    // Order matters: translate inner types before outer wrappers.
    // 1. Simple leaf types first (String, &str, &String)
    output = translate_string_ref_type(&output);
    output = translate_str_ref_type(&output);
    output = translate_string_type(&output);

    // 2. Lifetime annotations
    output = translate_static_lifetime(&output);

    // 3. Function types
    output = translate_impl_fn_types(&output);
    output = translate_fn_types(&output);

    // 4. Generic wrapper types (inside-out: nested ones get translated first
    //    because we translate the inner type name, then the wrapper)
    output = translate_option_type(&output);
    output = translate_result_type(&output);
    output = translate_hashset_type(&output);
    output = translate_hashmap_type(&output);
    output = translate_vec_type(&output);

    // 5. impl Trait (after Fn translations to avoid double-matching)
    output = translate_impl_trait(&output);

    output
}

/// Translate `String` → `string` (only standalone occurrences, not inside other words).
fn translate_string_type(input: &str) -> String {
    PATTERNS
        .string_type
        .replace_all(input, "string")
        .into_owned()
}

/// Translate `&str` → `string (reference)`.
fn translate_str_ref_type(input: &str) -> String {
    PATTERNS
        .str_ref_type
        .replace_all(input, "string (reference)")
        .into_owned()
}

/// Translate `&String` → `string (reference)`.
fn translate_string_ref_type(input: &str) -> String {
    PATTERNS
        .string_ref_type
        .replace_all(input, "string (reference)")
        .into_owned()
}

/// Translate `'static` → empty (remove lifetime noise).
fn translate_static_lifetime(input: &str) -> String {
    PATTERNS.static_lifetime.replace_all(input, "").into_owned()
}

/// Translate `Vec<T>` → `Array<T>`.
///
/// Finds each `Vec<` and extracts the balanced generic content, then wraps
/// it as `Array<...>`.
fn translate_vec_type(input: &str) -> String {
    replace_generic_type(input, &PATTERNS.vec_type, "Vec<", "Array<")
}

/// Translate `HashMap<K, V>` → `Map<K, V>`.
fn translate_hashmap_type(input: &str) -> String {
    replace_generic_type(input, &PATTERNS.hashmap_type, "HashMap<", "Map<")
}

/// Translate `HashSet<T>` → `Set<T>`.
fn translate_hashset_type(input: &str) -> String {
    replace_generic_type(input, &PATTERNS.hashset_type, "HashSet<", "Set<")
}

/// Translate `Option<T>` → `T | null`.
///
/// This is special: `Option<i32>` becomes `i32 | null`, not `Option<i32>` with
/// a different wrapper name.
fn translate_option_type(input: &str) -> String {
    replace_option_type(input)
}

/// Translate `Result<T, E>` → mention of throws.
///
/// `Result<i32, E>` becomes `i32 (throws E)`.
fn translate_result_type(input: &str) -> String {
    replace_result_type(input)
}

/// Translate `impl Fn(T) -> U` / `impl FnMut(T) -> U` / `impl FnOnce(T) -> U`
/// into `(T) => U`.
fn translate_impl_fn_types(input: &str) -> String {
    replace_impl_fn_type(input)
}

/// Translate bare `fn(T) -> U` into `(T) => U`.
fn translate_fn_types(input: &str) -> String {
    replace_bare_fn_type(input)
}

/// Translate `impl Trait` → `extends Trait` (but not `impl Fn*`).
fn translate_impl_trait(input: &str) -> String {
    // Only match impl followed by a non-Fn trait name.
    let re = &PATTERNS.impl_trait;
    re.replace_all(input, |caps: &regex::Captures| {
        let trait_name = &caps[1];
        // Skip Fn/FnMut/FnOnce — those were already handled.
        if trait_name.starts_with("Fn") {
            caps[0].to_owned()
        } else {
            format!("extends {trait_name}")
        }
    })
    .into_owned()
}

/// Replace a generic type like `Vec<T>` → `Array<T>` with balanced bracket matching.
///
/// The inner content is recursively translated via [`translate_type_names`] so that
/// nested generics like `Vec<Vec<String>>` become `Array<Array<string>>`.
fn replace_generic_type(input: &str, pattern: &Regex, prefix: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        // Add everything before the match.
        result.push_str(&remaining[..m.start()]);

        // Find the balanced closing `>` after the opening `<`.
        let after_open = &remaining[m.end()..];
        if let Some(close_pos) = find_balanced_close(after_open) {
            let inner = &after_open[..close_pos];
            let translated_inner = translate_type_names(inner);
            result.push_str(replacement);
            result.push_str(&translated_inner);
            result.push('>');
            remaining = &after_open[close_pos + 1..];
        } else {
            // No balanced close found — leave the original text.
            result.push_str(prefix);
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Replace `Option<T>` with `T | null`.
///
/// The inner content is recursively translated so that `Option<Vec<String>>`
/// becomes `Array<string> | null`.
fn replace_option_type(input: &str) -> String {
    let pattern = &PATTERNS.option_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        let after_open = &remaining[m.end()..];
        if let Some(close_pos) = find_balanced_close(after_open) {
            let inner = &after_open[..close_pos];
            let translated_inner = translate_type_names(inner);
            result.push_str(&translated_inner);
            result.push_str(" | null");
            remaining = &after_open[close_pos + 1..];
        } else {
            result.push_str("Option<");
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Replace `Result<T, E>` with `T (throws E)`.
///
/// The inner content is recursively translated so that `Result<Vec<String>, MyError>`
/// becomes `Array<string> (throws MyError)`.
fn replace_result_type(input: &str) -> String {
    let pattern = &PATTERNS.result_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        let after_open = &remaining[m.end()..];
        if let Some(close_pos) = find_balanced_close(after_open) {
            let inner = &after_open[..close_pos];
            // Split on the first top-level comma to separate T from E.
            if let Some(comma_pos) = find_top_level_comma(inner) {
                let ok_type = translate_type_names(inner[..comma_pos].trim());
                let err_type = translate_type_names(inner[comma_pos + 1..].trim());
                result.push_str(&ok_type);
                result.push_str(" (throws ");
                result.push_str(&err_type);
                result.push(')');
            } else {
                // Single type arg — just show it with throws
                let translated = translate_type_names(inner.trim());
                result.push_str(&translated);
                result.push_str(" (throws)");
            }
            remaining = &after_open[close_pos + 1..];
        } else {
            result.push_str("Result<");
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Replace `impl Fn(T) -> U` or `impl FnMut(T) -> U` or `impl FnOnce(T) -> U`
/// with `(T) => U`.
fn replace_impl_fn_type(input: &str) -> String {
    let pattern = &PATTERNS.impl_fn_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        // After the opening `(`, find the matching `)`.
        let after_paren = &remaining[m.end()..];
        if let Some(close_paren) = find_balanced_paren_close(after_paren) {
            let params = &after_paren[..close_paren];
            let after_close = &after_paren[close_paren + 1..];

            // Check for `-> ReturnType`.
            let trimmed = after_close.trim_start();
            if let Some(rest) = trimmed.strip_prefix("->") {
                let ret = rest.trim_start();
                // Consume the return type (up to a comma, closing bracket, or newline).
                let ret_end = ret.find([',', ')', '\n', ';']).unwrap_or(ret.len());
                let ret_type = ret[..ret_end].trim();
                result.push('(');
                result.push_str(params);
                result.push_str(") => ");
                result.push_str(ret_type);
                remaining = &ret[ret_end..];
            } else {
                // No return type
                result.push('(');
                result.push_str(params);
                result.push_str(") => void");
                remaining = after_close;
            }
        } else {
            // No balanced paren — leave original.
            result.push_str(&remaining[m.start()..m.end()]);
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Replace bare `fn(T) -> U` with `(T) => U`.
fn replace_bare_fn_type(input: &str) -> String {
    let pattern = &PATTERNS.fn_type;
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(m) = pattern.find(remaining) {
        result.push_str(&remaining[..m.start()]);

        let after_paren = &remaining[m.end()..];
        if let Some(close_paren) = find_balanced_paren_close(after_paren) {
            let params = &after_paren[..close_paren];
            let after_close = &after_paren[close_paren + 1..];

            let trimmed = after_close.trim_start();
            if let Some(rest) = trimmed.strip_prefix("->") {
                let ret = rest.trim_start();
                let ret_end = ret.find([',', ')', '\n', ';']).unwrap_or(ret.len());
                let ret_type = ret[..ret_end].trim();
                result.push('(');
                result.push_str(params);
                result.push_str(") => ");
                result.push_str(ret_type);
                remaining = &ret[ret_end..];
            } else {
                result.push('(');
                result.push_str(params);
                result.push_str(") => void");
                remaining = after_close;
            }
        } else {
            result.push_str(&remaining[m.start()..m.end()]);
            remaining = &remaining[m.end()..];
        }
    }

    result.push_str(remaining);
    result
}

/// Find the position of the closing `>` that balances the first `<` we are past.
///
/// Returns the byte offset of `>` relative to the start of the input slice,
/// or `None` if no balanced closing bracket is found.
fn find_balanced_close(input: &str) -> Option<usize> {
    let mut depth = 1;
    for (i, c) in input.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the position of the closing `)` that balances an already-opened `(`.
fn find_balanced_paren_close(input: &str) -> Option<usize> {
    let mut depth = 1;
    for (i, c) in input.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the position of the first comma at depth 0 (not inside nested `<>`).
fn find_top_level_comma(input: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in input.char_indices() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Test 1: String type translation ---
    #[test]
    fn test_translate_string_type_in_error_message() {
        let input = "expected String, found i32";
        let result = translate_type_names(input);
        assert_eq!(result, "expected string, found i32");
    }

    // --- Test 2: Vec<T> translation ---
    #[test]
    fn test_translate_vec_to_array() {
        let input = "expected Vec<String>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Array<string>");
    }

    // --- Test 3: Option<T> translation ---
    #[test]
    fn test_translate_option_to_union_null() {
        let input = "expected Option<i32>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected i32 | null");
    }

    // --- Test 4: Error passthrough with header ---
    #[test]
    fn test_translate_unrecognized_error_shows_raw_header() {
        let input = "error[E9999]: some future error\n --> src/main.rs:3:1\n";
        let result = translate_rustc_errors(input);
        assert!(
            result.starts_with(RAW_HEADER),
            "expected raw header, got:\n{result}"
        );
        assert!(
            result.contains("error[E9999]"),
            "original error should be preserved"
        );
    }

    // --- Test 5: Empty stderr produces empty output ---
    #[test]
    fn test_translate_empty_stderr_returns_empty() {
        let result = translate_rustc_errors("");
        assert!(result.is_empty());
    }

    // --- Test 6: HashMap translation ---
    #[test]
    fn test_translate_hashmap_to_map() {
        let input = "expected HashMap<String, i32>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Map<string, i32>");
    }

    // --- Test 7: HashSet translation ---
    #[test]
    fn test_translate_hashset_to_set() {
        let input = "expected HashSet<String>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Set<string>");
    }

    // --- Test 8: Result<T, E> translation ---
    #[test]
    fn test_translate_result_to_throws() {
        let input = "expected Result<i32, MyError>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected i32 (throws MyError)");
    }

    // --- Test 9: &str translation ---
    #[test]
    fn test_translate_str_ref_to_string_reference() {
        let input = "expected &str, found i32";
        let result = translate_type_names(input);
        assert_eq!(result, "expected string (reference), found i32");
    }

    // --- Test 10: &String translation ---
    #[test]
    fn test_translate_string_ref_to_string_reference() {
        let input = "expected &String";
        let result = translate_type_names(input);
        assert_eq!(result, "expected string (reference)");
    }

    // --- Test 11: impl Fn translation ---
    #[test]
    fn test_translate_impl_fn_to_arrow() {
        let input = "expected impl Fn(i32) -> bool";
        let result = translate_type_names(input);
        assert_eq!(result, "expected (i32) => bool");
    }

    // --- Test 12: impl Trait translation ---
    #[test]
    fn test_translate_impl_trait_to_extends() {
        let input = "impl Display";
        let result = translate_type_names(input);
        assert_eq!(result, "extends Display");
    }

    // --- Test 13: 'static lifetime removal ---
    #[test]
    fn test_translate_static_lifetime_removed() {
        let input = "expected &'static str";
        let result = translate_type_names(input);
        assert_eq!(result, "expected &str");
        // Note: &str itself would be further translated in the full pipeline,
        // but we translate simple types first, then this catches leftovers.
    }

    // --- Test 14: Nested generics ---
    #[test]
    fn test_translate_nested_vec_of_string() {
        let input = "expected Vec<Vec<String>>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Array<Array<string>>");
    }

    // --- Test 15: Full rustc error message translation ---
    #[test]
    fn test_translate_full_rustc_error() {
        let input = r#"error[E0308]: mismatched types
 --> src/main.rs:5:10
  |
5 |     let x: String = 42;
  |                      ^^ expected String, found integer
"#;
        let result = translate_rustc_errors(input);
        assert!(
            result.starts_with(TRANSLATED_HEADER),
            "expected translated header, got:\n{result}"
        );
        assert!(
            result.contains("expected string, found integer"),
            "String should be translated to string in:\n{result}"
        );
    }

    // --- Test 16: Successful build (no error) ---
    #[test]
    fn test_translate_whitespace_only_returns_empty() {
        let result = translate_rustc_errors("   \n  ");
        assert!(result.is_empty());
    }

    // --- Test 17: fn type translation ---
    #[test]
    fn test_translate_fn_type_to_arrow() {
        let input = "expected fn(i32) -> bool";
        let result = translate_type_names(input);
        assert_eq!(result, "expected (i32) => bool");
    }

    // --- Test 18: Complex nested type ---
    #[test]
    fn test_translate_complex_nested_type() {
        let input = "expected HashMap<String, Vec<Option<i32>>>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Map<string, Array<i32 | null>>");
    }

    // --- Test 19: Multiple types in one message ---
    #[test]
    fn test_translate_multiple_types_in_message() {
        let input = "expected Vec<String>, found Option<i32>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected Array<string>, found i32 | null");
    }

    // --- Test 20: impl FnMut translation ---
    #[test]
    fn test_translate_impl_fn_mut_to_arrow() {
        let input = "expected impl FnMut(i32) -> bool";
        let result = translate_type_names(input);
        assert_eq!(result, "expected (i32) => bool");
    }

    // --- Test 21: Result with single type argument ---
    #[test]
    fn test_translate_result_single_arg() {
        let input = "expected Result<i32>";
        let result = translate_type_names(input);
        assert_eq!(result, "expected i32 (throws)");
    }

    // --- Correctness Scenario 1: Type mismatch translation ---
    #[test]
    fn test_correctness_type_mismatch_translation() {
        let input = r#"error[E0308]: mismatched types
 --> src/main.rs:5:10
  |
5 |     let x: String = 42;
  |                      ^^ expected String, found integer
"#;
        let result = translate_rustc_errors(input);
        assert!(
            result.contains("expected string, found integer"),
            "correctness scenario 1 failed: {result}"
        );
    }

    // --- Correctness Scenario 2: Fallback on unknown error ---
    #[test]
    fn test_correctness_fallback_unknown_error() {
        let input = "error[E9999]: some future error\n --> src/main.rs:3:1\n";
        let result = translate_rustc_errors(input);
        assert!(
            result.starts_with(RAW_HEADER),
            "correctness scenario 2 failed: expected raw header, got:\n{result}"
        );
        assert!(
            result.contains("error[E9999]: some future error"),
            "original error should be preserved in:\n{result}"
        );
    }
}

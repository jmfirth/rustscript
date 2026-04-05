//! Phase 2 diagnostic tests — compile invalid `.rts` source and verify error messages.
//!
//! These are fast tests (string comparison only, no cargo invocation).
//!
//! Tests cover Phase 2 error conditions: async syntax errors, invalid crate imports,
//! and error translation from rustc type names to RustScript type names.

mod test_utils;

use rustscript_driver::translate_rustc_errors;
use test_utils::compile_diagnostics;

// ===========================================================================
// 1. Missing await on async call (syntax check)
//
// The compiler should still parse this, but it may produce diagnostics
// or generate code that rustc will reject. We verify no parse crash.
// ===========================================================================

#[test]
fn test_diagnostic_p2_missing_await_no_crash() {
    let source = r#"async function getData(): string {
  return "hello";
}

async function main() {
  const result = getData();
  console.log(result);
}"#;

    // This should NOT crash the compiler. It may or may not produce diagnostics
    // (missing await is a semantic issue often caught by rustc, not our parser).
    let messages = compile_diagnostics(source);
    // If there are diagnostics, they should be meaningful (not panics).
    for msg in &messages {
        assert!(
            !msg.contains("panic"),
            "compiler should not panic, got: {msg}"
        );
    }
}

// ===========================================================================
// 2. Error translation: String → string
//
// Verify that rustc error messages containing `String` get translated to
// `string` in the RustScript-friendly output.
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_string_type() {
    let rustc_error = r#"error[E0308]: mismatched types
 --> src/main.rs:5:10
  |
5 |     let x: String = 42;
  |                      ^^ expected String, found integer
"#;

    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("expected string, found integer"),
        "String should be translated to string, got:\n{translated}"
    );
    assert!(
        translated.contains("RustScript compilation error"),
        "should use translated header"
    );
}

// ===========================================================================
// 3. Error translation: Vec<String> → Array<string>
//
// Verify nested type name translation.
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_vec_to_array() {
    let rustc_error = "error: expected Vec<String>, found i32";
    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("Array<string>"),
        "Vec<String> should become Array<string>, got:\n{translated}"
    );
}

// ===========================================================================
// 4. Error translation: Option<T> → T | null
//
// Verify Option translation.
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_option_to_null() {
    let rustc_error = "error: expected Option<i32>";
    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("i32 | null"),
        "Option<i32> should become i32 | null, got:\n{translated}"
    );
}

// ===========================================================================
// 5. Error translation: Result<T, E> → T (throws E)
//
// Verify Result translation.
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_result_to_throws() {
    let rustc_error = "error: expected Result<String, MyError>";
    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("string (throws MyError)"),
        "Result<String, MyError> should become string (throws MyError), got:\n{translated}"
    );
}

// ===========================================================================
// 6. Error translation: HashMap → Map
//
// Verify HashMap translation.
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_hashmap_to_map() {
    let rustc_error = "error: expected HashMap<String, i32>";
    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("Map<string, i32>"),
        "HashMap<String, i32> should become Map<string, i32>, got:\n{translated}"
    );
}

// ===========================================================================
// 7. Error translation: unknown error falls back to raw header
//
// Verify that unrecognized errors are passed through unchanged.
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_unknown_fallback() {
    let rustc_error = "error[E9999]: something totally unknown";
    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("rustc error (in generated code)"),
        "unknown error should use raw header, got:\n{translated}"
    );
    assert!(
        translated.contains("something totally unknown"),
        "original message should be preserved"
    );
}

// ===========================================================================
// 8. Error translation: complex nested types
//
// Verify deeply nested type translation.
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_complex_nested() {
    let rustc_error = "error: expected HashMap<String, Vec<Option<i32>>>";
    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("Map<string, Array<i32 | null>>"),
        "complex nested type should translate fully, got:\n{translated}"
    );
}

// ===========================================================================
// 9. Compile-time diagnostic: unknown type in RustScript source
//
// Verify that the compiler reports unknown types in the source.
// ===========================================================================

#[test]
fn test_diagnostic_p2_unknown_type_in_async_function() {
    let source = r#"async function process(x: FakeType): FakeType {
  return x;
}"#;

    let messages = compile_diagnostics(source);
    assert!(
        !messages.is_empty(),
        "expected diagnostic for unknown type FakeType"
    );
    let has_unknown = messages.iter().any(|m| m.contains("unknown type"));
    assert!(
        has_unknown,
        "expected 'unknown type' in diagnostics, got: {messages:?}"
    );
    let has_fake = messages.iter().any(|m| m.contains("FakeType"));
    assert!(
        has_fake,
        "expected 'FakeType' in diagnostics, got: {messages:?}"
    );
}

// ===========================================================================
// 10. Error translation: impl Fn → arrow function
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_impl_fn_to_arrow() {
    let rustc_error = "error: expected impl Fn(i32) -> bool";
    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("(i32) => bool"),
        "impl Fn(i32) -> bool should become (i32) => bool, got:\n{translated}"
    );
}

// ===========================================================================
// 11. Error translation: &str → string (reference)
// ===========================================================================

#[test]
fn test_diagnostic_p2_error_translation_str_ref() {
    let rustc_error = "error: expected &str, found i32";
    let translated = translate_rustc_errors(rustc_error, None, None, None);
    assert!(
        translated.contains("string (reference)"),
        "&str should become string (reference), got:\n{translated}"
    );
}

// ===========================================================================
// 12. Syntax error in async context — missing body
// ===========================================================================

#[test]
fn test_diagnostic_p2_async_function_missing_body() {
    let source = r#"async function getData(): string"#;

    let messages = compile_diagnostics(source);
    assert!(
        !messages.is_empty(),
        "expected diagnostic for missing function body"
    );
}

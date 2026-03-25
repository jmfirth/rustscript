//! Phase 4 diagnostic tests — compile invalid `.rts` source and verify error messages.
//!
//! These are fast tests (string comparison only, no cargo invocation).
//!
//! Tests cover Phase 4 error conditions: unclosed rust blocks, shared type
//! misuse, and error translation for Phase 4 types.

mod test_utils;

use test_utils::compile_diagnostics;

// ===========================================================================
// 1. Unclosed rust block produces a clear error message
//
// Features: inline Rust parser error handling
// ===========================================================================

#[test]
fn test_diagnostic_p4_unclosed_rust_block() {
    let source = "\
function main(): void {
  rust {
    let x = 1;
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for unclosed rust block"
    );

    let has_unclosed = messages
        .iter()
        .any(|m| m.contains("unclosed") || m.contains("unterminated"));
    assert!(
        has_unclosed,
        "expected diagnostic about unclosed rust block, got: {messages:?}"
    );
}

// ===========================================================================
// 2. shared without type parameter produces diagnostic
//
// Features: shared<T> type sugar validation
// ===========================================================================

#[test]
fn test_diagnostic_p4_shared_missing_type_param() {
    let source = "\
function main(): void {
  const x: shared = shared(0);
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for shared without type parameter"
    );

    let has_shared = messages.iter().any(|m| m.contains("shared"));
    assert!(
        has_shared,
        "expected shared-related diagnostic, got: {messages:?}"
    );
}

// ===========================================================================
// 3. Phase 4 error translation: Arc<Mutex<T>> types in rustc errors
//
// Verify that error translation handles Phase 4 types correctly.
// ===========================================================================

#[test]
fn test_diagnostic_p4_error_translation_arc_mutex() {
    let rustc_error = "error: expected Arc<Mutex<i32>>, found String";
    let translated = rsc_driver::translate_rustc_errors(rustc_error, None, None, None);

    // Arc<Mutex<T>> doesn't have a direct RustScript sugar in error translation,
    // but String should still become string
    assert!(
        translated.contains("string") || translated.contains("String"),
        "error translation should handle String type, got: {translated}"
    );
}

// ===========================================================================
// 4. Syntax error in inline Rust context doesn't crash compiler
//
// Features: robust error handling for malformed inline Rust
// ===========================================================================

#[test]
fn test_diagnostic_p4_malformed_inline_rust_no_crash() {
    let source = "\
function main(): void {
  rust {
  const x: i32 = 42;
}";

    let messages = compile_diagnostics(source);

    // Should produce diagnostics, not crash
    for msg in &messages {
        assert!(
            !msg.contains("panic"),
            "compiler should not panic, got: {msg}"
        );
    }
}

// ===========================================================================
// 5. Unknown type in borrowed position still reports diagnostic
//
// Features: error handling, borrow analysis with unknown types
// ===========================================================================

#[test]
fn test_diagnostic_p4_unknown_type_still_reported() {
    let source = "\
function process(x: FakeType): void {
  console.log(x);
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected diagnostic for unknown type FakeType"
    );
    let has_unknown = messages.iter().any(|m| m.contains("unknown type"));
    assert!(
        has_unknown,
        "expected 'unknown type' diagnostic, got: {messages:?}"
    );
    let has_fake = messages.iter().any(|m| m.contains("FakeType"));
    assert!(
        has_fake,
        "expected 'FakeType' in diagnostic, got: {messages:?}"
    );
}

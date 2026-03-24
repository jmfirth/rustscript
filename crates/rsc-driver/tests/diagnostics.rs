//! Diagnostic tests — compile invalid `.rts` source and verify error messages.
//!
//! These are fast tests (string comparison only, no cargo invocation).

mod test_utils;

use test_utils::compile_diagnostics;

// ---------------------------------------------------------------------------
// 1. Syntax error — unexpected token
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_syntax_error_reports_expected_expression() {
    let source = "\
function main() {
  const x = ;
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for syntax error"
    );

    // The compiler reports "expected expression, found `;`" — verify the
    // diagnostic contains key terms indicating a syntax error.
    let has_expected = messages
        .iter()
        .any(|m| m.contains("expected") || m.contains("unexpected"));
    assert!(
        has_expected,
        "expected diagnostic to mention 'expected' or 'unexpected', got: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// 2. Unknown type
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_unknown_type_reports_foo() {
    let source = "\
function main() {
  const x: Foo = 42;
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for unknown type"
    );

    let has_unknown_type = messages.iter().any(|m| m.contains("unknown type"));
    assert!(
        has_unknown_type,
        "expected diagnostic to mention 'unknown type', got: {messages:?}"
    );

    let has_foo = messages.iter().any(|m| m.contains("Foo"));
    assert!(
        has_foo,
        "expected diagnostic to mention 'Foo', got: {messages:?}"
    );
}

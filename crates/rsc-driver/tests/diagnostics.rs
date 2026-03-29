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

// ===========================================================================
// Phase 1 Diagnostic Tests (Task 026)
//
// These tests verify error reporting for Phase 1 error conditions.
// The compiler currently detects: syntax errors, unknown type names.
// Many semantic errors are deferred to rustc (by design — the generated
// Rust is passed through to rustc for final validation).
// ===========================================================================

// ---------------------------------------------------------------------------
// P1-3. Unknown type in function parameter
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_p1_unknown_type_in_function_param() {
    let source = "\
function process(x: Widget): Widget {
  return x;
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for unknown type `Widget`"
    );

    let has_unknown = messages.iter().any(|m| m.contains("unknown type"));
    assert!(
        has_unknown,
        "expected diagnostic to mention 'unknown type', got: {messages:?}"
    );

    let has_widget = messages.iter().any(|m| m.contains("Widget"));
    assert!(
        has_widget,
        "expected diagnostic to mention 'Widget', got: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// P1-4. Forward reference to undeclared type
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_p1_forward_reference_type() {
    let source = "\
type Container = { item: Inner }
type Inner = { value: i32 }";

    let messages = compile_diagnostics(source);

    // Forward references are currently reported as unknown types because
    // the compiler processes types in declaration order.
    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for forward type reference"
    );

    let has_inner = messages.iter().any(|m| m.contains("Inner"));
    assert!(
        has_inner,
        "expected diagnostic to mention 'Inner', got: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// P1-5. Syntax error — unterminated template literal
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_p1_unterminated_template_literal() {
    let source = "\
function main() {
  const x = `hello ${name;
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for unterminated template literal"
    );
}

// ---------------------------------------------------------------------------
// P1-6. Syntax error — missing closing brace in class
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_p1_class_missing_brace() {
    let source = "\
class Foo {
  constructor() {
  }
";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for unclosed class definition"
    );
}

// ---------------------------------------------------------------------------
// P1-7. Syntax error — invalid enum variant
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_p1_malformed_enum() {
    let source = "\
type Dir = \"north\" | | \"south\"";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for malformed enum definition"
    );
}

// ===========================================================================
// Do-While Diagnostic Tests (Task 109)
// ===========================================================================

// ---------------------------------------------------------------------------
// Missing `while` after do block → parse error
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_do_while_missing_while_keyword() {
    let source = "\
function main() {
  let x: i32 = 0;
  do {
    x += 1;
  }
}";

    let messages = compile_diagnostics(source);

    assert!(
        !messages.is_empty(),
        "expected at least one diagnostic for missing `while` after `do`"
    );

    let has_while = messages
        .iter()
        .any(|m| m.contains("while") || m.contains("expected"));
    assert!(
        has_while,
        "expected diagnostic to mention 'while' or 'expected', got: {messages:?}"
    );
}

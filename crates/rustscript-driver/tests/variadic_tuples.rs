//! Snapshot, compilation, and diagnostic tests for variadic tuple types.
//!
//! Validates that `[...T, U]` spread syntax in tuple type positions resolves
//! at compile time by flattening known tuple types into concrete Rust tuples.

mod test_utils;

use test_utils::{compile_and_run, compile_diagnostics, compile_to_rust};

// ---------------------------------------------------------------------------
// 1. Spread appends to a tuple type alias
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_variadic_tuple_spread_append() {
    let source = r#"type Pair = [string, i32]
type Extended = [...Pair, bool]"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type Extended = (String, i32, bool);"),
        "expected `type Extended = (String, i32, bool);` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 2. Spread prepends to a tuple type alias
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_variadic_tuple_spread_prepend() {
    let source = r#"type Pair = [string, i32]
type Prepended = [i32, ...Pair]"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type Prepended = (i32, String, i32);"),
        "expected `type Prepended = (i32, String, i32);` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 3. Two spreads combined
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_variadic_tuple_spread_combined() {
    let source = r#"type A = [i32]
type B = [string]
type Combined = [...A, ...B]"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type Combined = (i32, String);"),
        "expected `type Combined = (i32, String);` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 4. Compilation test: variadic tuple resolves to valid Rust
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_compile_variadic_tuple_resolves_to_valid_rust() {
    let source = r#"type Pair = [string, i32]
type Extended = [...Pair, bool]

function main() {
  const val: Extended = ["hello", 42, true];
  console.log(val);
}"#;

    let output = compile_and_run(source);
    assert!(
        output.contains("(\"hello\", 42, true)"),
        "expected tuple output, got:\n{output}"
    );
}

// ---------------------------------------------------------------------------
// 5. Diagnostic test: spread of non-tuple type
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_spread_non_tuple_type() {
    let source = r#"type Bad = [...string, i32]"#;

    let diagnostics = compile_diagnostics(source);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.contains("spread in tuple type must refer to a tuple type")),
        "expected diagnostic about non-tuple spread, got:\n{diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// 6. Spread in middle position
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_variadic_tuple_spread_middle() {
    let source = r#"type Middle = [i32, string]
type Wrapped = [bool, ...Middle, f64]"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type Wrapped = (bool, i32, String, f64);"),
        "expected `type Wrapped = (bool, i32, String, f64);` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7. Spread of empty tuple
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_variadic_tuple_spread_empty() {
    let source = r#"type Empty = []
type WithEmpty = [i32, ...Empty, string]"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type WithEmpty = (i32, String);"),
        "expected `type WithEmpty = (i32, String);` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 8. Spread of single-element tuple
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_variadic_tuple_spread_single() {
    let source = r#"type Single = [bool]
type Extended = [...Single, i32]"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type Extended = (bool, i32);"),
        "expected `type Extended = (bool, i32);` in output, got:\n{actual}"
    );
}

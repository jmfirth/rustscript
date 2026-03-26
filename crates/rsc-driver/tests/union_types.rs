//! Snapshot tests for general union types (`string | i32`, `string | i32 | bool`).
//!
//! Task 065: General Union Types. Validates that union type annotations produce
//! auto-generated enum definitions with `From` impls, and that value construction
//! wraps with `.into()`.

mod test_utils;

use test_utils::{compile_and_run, compile_to_rust};

/// Assert that `actual` matches `expected`, printing a diff on failure.
fn assert_snapshot(name: &str, actual: &str, expected: &str) {
    if actual != expected {
        panic!(
            "snapshot mismatch for `{name}`.\n\n\
             === expected ===\n{expected}\n\
             === actual ===\n{actual}\n\
             === end ===\n"
        );
    }
}

// ---------------------------------------------------------------------------
// 1. Basic two-type union: string | i32
// ---------------------------------------------------------------------------

#[test]
fn test_union_basic_two_types_generates_enum_and_from_impls() {
    let source = "\
function format(value: string | i32): string {
  return \"done\";
}

function main() {
  const x: string | i32 = \"hello\";
}";

    let actual = compile_to_rust(source);

    // The generated code should contain the enum definition
    assert!(
        actual.contains("enum I32OrString"),
        "should contain enum I32OrString, got:\n{actual}"
    );
    // The enum should have tuple variants
    assert!(
        actual.contains("I32(i32)"),
        "should contain I32(i32) variant, got:\n{actual}"
    );
    assert!(
        actual.contains("String(String)"),
        "should contain String(String) variant, got:\n{actual}"
    );
    // From impls should be present
    assert!(
        actual.contains("impl From<i32> for I32OrString"),
        "should contain From<i32> impl, got:\n{actual}"
    );
    assert!(
        actual.contains("impl From<String> for I32OrString"),
        "should contain From<String> impl, got:\n{actual}"
    );
    // The function should use the enum type
    assert!(
        actual.contains("fn format(value: I32OrString)"),
        "should use enum type in param, got:\n{actual}"
    );
    // Value construction should use .into()
    assert!(
        actual.contains(".into()"),
        "should wrap value with .into(), got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 2. Three-type union: string | i32 | bool
// ---------------------------------------------------------------------------

#[test]
fn test_union_three_types_generates_enum() {
    let source = "\
function process(value: string | i32 | bool): string {
  return \"processed\";
}

function main() {
  process(42);
}";

    let actual = compile_to_rust(source);

    // Three-type union should produce BoolOrI32OrString (sorted alphabetically)
    assert!(
        actual.contains("enum BoolOrI32OrString"),
        "should contain enum BoolOrI32OrString, got:\n{actual}"
    );
    assert!(
        actual.contains("Bool(bool)"),
        "should contain Bool(bool), got:\n{actual}"
    );
    assert!(
        actual.contains("I32(i32)"),
        "should contain I32(i32), got:\n{actual}"
    );
    assert!(
        actual.contains("String(String)"),
        "should contain String(String), got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 3. Union in return type
// ---------------------------------------------------------------------------

#[test]
fn test_union_in_return_type() {
    let source = "\
function maybe_number(): string | i32 {
  return 42;
}

function main() {
  const x = maybe_number();
}";

    let actual = compile_to_rust(source);

    // Return type should use the union enum
    assert!(
        actual.contains("-> I32OrString"),
        "should use enum in return type, got:\n{actual}"
    );
    // Return value should be wrapped with .into()
    assert!(
        actual.contains("return 42.into()"),
        "should wrap return with .into(), got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 4. T | null still produces Option<T>
// ---------------------------------------------------------------------------

#[test]
fn test_union_t_or_null_still_produces_option() {
    let source = "\
function find(): string | null {
  return null;
}

function main() {
  const x = find();
}";

    let actual = compile_to_rust(source);

    // T | null should NOT produce a union enum, but Option<T>
    assert!(
        actual.contains("Option<String>"),
        "string | null should produce Option<String>, got:\n{actual}"
    );
    assert!(
        !actual.contains("enum"),
        "string | null should not generate an enum, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 5. Deduplication — same union used in multiple places generates one enum
// ---------------------------------------------------------------------------

#[test]
fn test_union_deduplication_same_union_one_enum() {
    let source = "\
function format(value: string | i32): string {
  return \"formatted\";
}

function process(item: string | i32): string {
  return \"processed\";
}

function main() {
  format(\"hello\");
  process(42);
}";

    let actual = compile_to_rust(source);

    // Count enum definitions — should only be one I32OrString
    let enum_count = actual.matches("enum I32OrString").count();
    assert_eq!(
        enum_count, 1,
        "should have exactly one enum I32OrString definition, found {enum_count} in:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 6. Union types with .into() at call site
// ---------------------------------------------------------------------------

#[test]
fn test_union_into_at_call_site() {
    let source = "\
function process(value: string | i32): string {
  return \"done\";
}

function main() {
  process(\"hello\");
  process(42);
}";

    let actual = compile_to_rust(source);

    // Both calls should wrap args with .into()
    let into_count = actual.matches(".into()").count();
    assert!(
        into_count >= 2,
        "should have at least 2 .into() calls, found {into_count} in:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7. Union with derived traits
// ---------------------------------------------------------------------------

#[test]
fn test_union_enum_has_derive_macros() {
    let source = "\
function format(value: string | i32): string {
  return \"done\";
}

function main() {
  format(\"hello\");
}";

    let actual = compile_to_rust(source);

    // The enum should have #[derive(Debug, Clone, ...)]
    assert!(
        actual.contains("#[derive("),
        "enum should have derive macros, got:\n{actual}"
    );
    assert!(
        actual.contains("Debug"),
        "enum should derive Debug, got:\n{actual}"
    );
    assert!(
        actual.contains("Clone"),
        "enum should derive Clone, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 8. E2E: union type compiles and runs
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_union_e2e_compiles_and_runs() {
    let source = "\
function describe(value: string | i32): string {
  return \"a value\";
}

function main() {
  const result = describe(\"hello\");
  console.log(result);
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "a value");
}

// ---------------------------------------------------------------------------
// 9. E2E: union with multiple argument types
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_union_e2e_different_types_at_call_site() {
    let source = "\
function describe(value: string | i32): string {
  return \"value received\";
}

function main() {
  console.log(describe(\"text\"));
  console.log(describe(42));
}";

    let output = compile_and_run(source);
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "value received");
    assert_eq!(lines[1], "value received");
}

// ---------------------------------------------------------------------------
// 10. Union enum Display impl
// ---------------------------------------------------------------------------

#[test]
fn test_union_enum_has_display_impl() {
    let source = "\
function format(value: string | i32): string {
  return \"done\";
}

function main() {
  format(\"hello\");
}";

    let actual = compile_to_rust(source);

    // The emitter generates Display for all enums
    assert!(
        actual.contains("impl std::fmt::Display for I32OrString"),
        "enum should have Display impl, got:\n{actual}"
    );
}

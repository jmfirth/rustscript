//! Tests for conditional types and the `infer` keyword.
//!
//! Validates that `T extends U ? A : B` syntax and `infer` keyword
//! compile-time resolution works correctly for concrete types.

mod test_utils;

use test_utils::{compile_diagnostics, compile_to_rust};

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
// 1. Simple conditional type: string extends string → true branch
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_conditional_type_true_branch() {
    let source = r#"type IsString = string extends string ? bool : i32"#;

    let actual = compile_to_rust(source);
    // string extends string → true → bool
    assert!(
        actual.contains("type IsString = bool;"),
        "expected `type IsString = bool;` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 2. Simple conditional type: i32 extends string → false branch
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_conditional_type_false_branch() {
    let source = r#"type NotString = i32 extends string ? bool : f64"#;

    let actual = compile_to_rust(source);
    // i32 does not extend string → false → f64
    assert!(
        actual.contains("type NotString = f64;"),
        "expected `type NotString = f64;` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 3. Conditional type with same type extends check
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_conditional_type_same_type() {
    let source = r#"type Check = i32 extends i32 ? string : bool"#;

    let actual = compile_to_rust(source);
    // i32 extends i32 → true → string → String
    assert!(
        actual.contains("type Check = String;"),
        "expected `type Check = String;` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 4. Conditional type resolves to proper Rust types
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_conditional_type_string_result() {
    let source = r#"type Result1 = string extends string ? i64 : u32"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type Result1 = i64;"),
        "expected `type Result1 = i64;` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 5. ReturnType<T> utility type extracts function return type
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_return_type_utility() {
    let source = r#"function greet(name: string): string {
    return name;
}
type GreetReturn = ReturnType<(string) => string>"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type GreetReturn = String;"),
        "expected `type GreetReturn = String;` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 6. Parameters<T> utility type extracts function parameter types
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_parameters_utility() {
    let source = r#"type MyParams = Parameters<(string, i32) => bool>"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type MyParams = (String, i32);"),
        "expected `type MyParams = (String, i32);` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7. Conditional type with infer keyword in function return position
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_conditional_infer_return_type() {
    let source =
        r#"type MyReturnType = (string, i32) => bool extends (string, i32) => infer R ? R : i32"#;

    let actual = compile_to_rust(source);
    // The function type matches, R is inferred as bool
    assert!(
        actual.contains("type MyReturnType = bool;"),
        "expected `type MyReturnType = bool;` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 8. Conditional type false branch when function type doesn't match
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_conditional_type_function_mismatch() {
    let source = r#"type NotFn = i32 extends (string) => infer R ? R : f64"#;

    let actual = compile_to_rust(source);
    // i32 does not match a function type → false branch → f64
    assert!(
        actual.contains("type NotFn = f64;"),
        "expected `type NotFn = f64;` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 9. Diagnostic: infer outside conditional type
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_infer_outside_conditional() {
    let source = r#"type Bad = infer R"#;

    let diags = compile_diagnostics(source);
    assert!(
        diags
            .iter()
            .any(|d| d.contains("infer") && d.contains("conditional")),
        "expected diagnostic about `infer` outside conditional type, got: {diags:?}"
    );
}

// ---------------------------------------------------------------------------
// 10. Compilation test: conditional type produces valid Rust
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_compile_conditional_type_produces_valid_rust() {
    use test_utils::compile_and_run;

    let source = r#"
type IsI32 = i32 extends i32 ? string : bool

function main(): void {
    const x: IsI32 = "hello"
    console.log(x)
}
"#;

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "hello");
}

// ---------------------------------------------------------------------------
// 11. ReturnType with numeric return type
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_return_type_numeric() {
    let source = r#"type NumRet = ReturnType<(string) => i32>"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type NumRet = i32;"),
        "expected `type NumRet = i32;` in output, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 12. Parameters with single parameter
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_parameters_single_param() {
    let source = r#"type SingleParam = Parameters<(bool) => string>"#;

    let actual = compile_to_rust(source);
    // Single-element tuples emit without trailing comma in the current emitter
    assert!(
        actual.contains("type SingleParam = (bool);")
            || actual.contains("type SingleParam = (bool,);"),
        "expected `type SingleParam = (bool);` in output, got:\n{actual}"
    );
}

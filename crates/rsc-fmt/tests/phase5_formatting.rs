//! Phase 5 formatter tests (Task 062).
//!
//! Verifies that the formatter handles all Phase 5 AST nodes correctly
//! and that formatting is idempotent for each new syntax pattern.

use rsc_fmt::format_source;

/// Assert that formatting is idempotent for the given source.
fn assert_idempotent(name: &str, source: &str) {
    let first =
        format_source(source).unwrap_or_else(|e| panic!("{name}: first format failed: {e}"));
    let second =
        format_source(&first).unwrap_or_else(|e| panic!("{name}: second format failed: {e}"));
    assert_eq!(
        first, second,
        "idempotency failed for '{name}'.\nfirst:\n{first}\nsecond:\n{second}"
    );
}

// ---------------------------------------------------------------------------
// Ternary expression
// ---------------------------------------------------------------------------

#[test]
fn test_format_ternary_expression() {
    let input = "function foo(a: bool): i32 { const x=a?1:0; return x; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("a ? 1 : 0"),
        "should space ternary operator: {result}"
    );
}

#[test]
fn test_format_ternary_idempotent() {
    assert_idempotent(
        "ternary",
        "function foo(a: bool): i32 { const x = a ? 1 : 0; return x; }",
    );
}

// ---------------------------------------------------------------------------
// Exponentiation
// ---------------------------------------------------------------------------

#[test]
fn test_format_exponentiation() {
    let input = "function foo(a: i32, b: i32): i32 { return a**b; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("a ** b"),
        "should space exponentiation operator: {result}"
    );
}

#[test]
fn test_format_exponentiation_idempotent() {
    assert_idempotent(
        "exponentiation",
        "function foo(a: i32, b: i32): i32 { return a ** b; }",
    );
}

// ---------------------------------------------------------------------------
// Non-null assertion
// ---------------------------------------------------------------------------

#[test]
fn test_format_non_null_assert() {
    let input = "function foo(a: i32 | null): i32 { return a!; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("a!"),
        "should format non-null assertion: {result}"
    );
}

#[test]
fn test_format_non_null_assert_idempotent() {
    assert_idempotent(
        "non_null_assert",
        "function foo(a: i32 | null): i32 { return a!; }",
    );
}

// ---------------------------------------------------------------------------
// `as` cast
// ---------------------------------------------------------------------------

#[test]
fn test_format_as_cast() {
    let input = "function foo(a: i32): f64 { return a as f64; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("a as f64"),
        "should format as cast: {result}"
    );
}

#[test]
fn test_format_as_cast_idempotent() {
    assert_idempotent("as_cast", "function foo(a: i32): f64 { return a as f64; }");
}

// ---------------------------------------------------------------------------
// `typeof`
// ---------------------------------------------------------------------------

#[test]
fn test_format_typeof() {
    let input = "function foo(a: i32): string { return typeof a; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("typeof a"),
        "should format typeof: {result}"
    );
}

#[test]
fn test_format_typeof_idempotent() {
    assert_idempotent(
        "typeof",
        "function foo(a: i32): string { return typeof a; }",
    );
}

// ---------------------------------------------------------------------------
// Bitwise operators
// ---------------------------------------------------------------------------

#[test]
fn test_format_bitwise_and() {
    let input = "function foo(a: i32, b: i32): i32 { return a&b; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("a & b"),
        "should space bitwise AND: {result}"
    );
}

#[test]
fn test_format_bitwise_operators_idempotent() {
    assert_idempotent(
        "bitwise_ops",
        "function foo(a: i32, b: i32): i32 { return a & b; }",
    );
}

// ---------------------------------------------------------------------------
// Optional parameter
// ---------------------------------------------------------------------------

#[test]
fn test_format_optional_param() {
    let input = "function foo(x?: string) {}";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("x?: string"),
        "should format optional param: {result}"
    );
}

#[test]
fn test_format_optional_param_idempotent() {
    assert_idempotent("optional_param", "function foo(x?: string) {}");
}

// ---------------------------------------------------------------------------
// Default parameter
// ---------------------------------------------------------------------------

#[test]
fn test_format_default_param() {
    let input = "function foo(x: i32 = 5) {}";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("x: i32 = 5"),
        "should format default param: {result}"
    );
}

#[test]
fn test_format_default_param_idempotent() {
    assert_idempotent("default_param", "function foo(x: i32 = 5) {}");
}

// ---------------------------------------------------------------------------
// Rest parameter
// ---------------------------------------------------------------------------

#[test]
fn test_format_rest_param() {
    let input = "function foo(...args: Array<i32>) {}";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("...args: Array<i32>"),
        "should format rest param: {result}"
    );
}

#[test]
fn test_format_rest_param_idempotent() {
    assert_idempotent("rest_param", "function foo(...args: Array<i32>) {}");
}

// ---------------------------------------------------------------------------
// Array spread
// ---------------------------------------------------------------------------

#[test]
fn test_format_array_spread() {
    let input = "function foo() { const a = [...b, 1]; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("...b"),
        "should format array spread: {result}"
    );
}

#[test]
fn test_format_array_spread_idempotent() {
    assert_idempotent("array_spread", "function foo() { const a = [...b, 1]; }");
}

// ---------------------------------------------------------------------------
// Class: field initializer
// ---------------------------------------------------------------------------

#[test]
fn test_format_field_initializer() {
    let input = "class C { x: i32 = 0; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("x: i32 = 0;"),
        "should format field initializer: {result}"
    );
}

#[test]
fn test_format_field_initializer_idempotent() {
    assert_idempotent("field_initializer", "class C {\n  x: i32 = 0;\n}\n");
}

// ---------------------------------------------------------------------------
// Class: constructor parameter property
// ---------------------------------------------------------------------------

#[test]
fn test_format_constructor_param_property() {
    let input = "class C { constructor(public name: string) {} }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("public name: string"),
        "should format constructor param property: {result}"
    );
}

#[test]
fn test_format_constructor_param_property_idempotent() {
    assert_idempotent(
        "constructor_param_property",
        "class C {\n  constructor(public name: string) {}\n}\n",
    );
}

// ---------------------------------------------------------------------------
// Class: static method
// ---------------------------------------------------------------------------

#[test]
fn test_format_static_method() {
    let input = "class C { static foo(): void {} }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("static foo(): void"),
        "should format static method: {result}"
    );
}

#[test]
fn test_format_static_method_idempotent() {
    assert_idempotent("static_method", "class C {\n  static foo(): void {}\n}\n");
}

// ---------------------------------------------------------------------------
// Class: getter
// ---------------------------------------------------------------------------

#[test]
fn test_format_getter() {
    let input = "class C { x: i32 = 0; get value(): i32 { return this.x; } }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("get value(): i32"),
        "should format getter: {result}"
    );
}

#[test]
fn test_format_getter_idempotent() {
    assert_idempotent(
        "getter",
        "class C {\n  x: i32 = 0;\n\n  get value(): i32 {\n    return this.x;\n  }\n}\n",
    );
}

// ---------------------------------------------------------------------------
// Class: setter
// ---------------------------------------------------------------------------

#[test]
fn test_format_setter() {
    let input = "class C { x: i32 = 0; set value(v: i32) { this.x = v; } }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("set value(v: i32)"),
        "should format setter: {result}"
    );
}

#[test]
fn test_format_setter_idempotent() {
    assert_idempotent(
        "setter",
        "class C {\n  x: i32 = 0;\n\n  set value(v: i32) {\n    this.x = v;\n  }\n}\n",
    );
}

// ---------------------------------------------------------------------------
// Class: readonly field
// ---------------------------------------------------------------------------

#[test]
fn test_format_readonly_field() {
    let input = "class C { readonly x: i32 = 0; }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("readonly x: i32"),
        "should format readonly field: {result}"
    );
}

#[test]
fn test_format_readonly_idempotent() {
    assert_idempotent("readonly", "class C {\n  readonly x: i32 = 0;\n}\n");
}

// ---------------------------------------------------------------------------
// Try/catch/finally
// ---------------------------------------------------------------------------

#[test]
fn test_format_try_catch_finally() {
    let input = "function foo() { try { doWork(); } catch (e: Error) { handleError(e); } finally { cleanup(); } }";
    let result = format_source(input).expect("should format");
    assert!(
        result.contains("try {"),
        "should format try block: {result}"
    );
    assert!(
        result.contains("catch (e: Error)"),
        "should format catch: {result}"
    );
    assert!(
        result.contains("finally {"),
        "should format finally block: {result}"
    );
}

#[test]
fn test_format_finally_idempotent() {
    assert_idempotent(
        "finally",
        "function foo() {\n  try {\n    doWork();\n  } catch (e: Error) {\n    handleError(e);\n  } finally {\n    cleanup();\n  }\n}\n",
    );
}

// ---------------------------------------------------------------------------
// JSDoc: source with comments returned unchanged
// ---------------------------------------------------------------------------

#[test]
fn test_format_jsdoc_source_returned_unchanged() {
    let input = "/** Adds two numbers */\nfunction add(a: i32, b: i32): i32 { return a + b; }";
    let result = format_source(input).expect("should format");
    assert_eq!(
        result, input,
        "source with JSDoc should be returned unchanged"
    );
}

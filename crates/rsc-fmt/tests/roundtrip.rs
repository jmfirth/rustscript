//! Formatter roundtrip and idempotency tests (Phase 3 integration).
//!
//! Verifies that `format(format(source)) == format(source)` for all
//! syntax patterns. These are fast tests (string comparison only, no cargo).

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
// Phase 0 syntax: basic functions, variables, arithmetic, control flow
// ---------------------------------------------------------------------------

#[test]
fn test_roundtrip_idempotent_hello_world() {
    assert_idempotent(
        "hello_world",
        "function main() { console.log(\"Hello, World!\"); }",
    );
}

#[test]
fn test_roundtrip_idempotent_arithmetic() {
    assert_idempotent(
        "arithmetic",
        "function main() { const x: i64 = 10; const y: i64 = 3; console.log(x + y); }",
    );
}

#[test]
fn test_roundtrip_idempotent_fibonacci() {
    assert_idempotent(
        "fibonacci",
        "\
function fibonacci(n: i32): i32 {
  if (n <= 1) {
    return n;
  }
  return fibonacci(n - 1) + fibonacci(n - 2);
}

function main() {
  console.log(fibonacci(10));
}",
    );
}

#[test]
fn test_roundtrip_idempotent_if_else() {
    assert_idempotent(
        "if_else",
        "function foo(x: i32): i32 { if (x > 0) { return 1; } else { return 0; } }",
    );
}

#[test]
fn test_roundtrip_idempotent_while_loop() {
    assert_idempotent(
        "while_loop",
        "function foo() { let x = 0; while (x < 10) { x = x + 1; } }",
    );
}

#[test]
fn test_roundtrip_idempotent_for_loop() {
    assert_idempotent(
        "for_loop",
        "function foo() { for (const x of items) { console.log(x); } }",
    );
}

#[test]
fn test_roundtrip_idempotent_multiple_functions() {
    assert_idempotent(
        "multiple_functions",
        "function add(a: i32, b: i32): i32 { return a + b; } function main() { console.log(add(1, 2)); }",
    );
}

// ---------------------------------------------------------------------------
// Phase 1 syntax: types, enums, generics, closures, option/result
// ---------------------------------------------------------------------------

#[test]
fn test_roundtrip_idempotent_type_definition() {
    assert_idempotent(
        "type_definition",
        "type Point = { x: f64, y: f64 }; function main() { const p: Point = { x: 1.0, y: 2.0 }; }",
    );
}

#[test]
fn test_roundtrip_idempotent_enum() {
    assert_idempotent(
        "enum",
        "type Direction = \"north\" | \"south\" | \"east\" | \"west\"",
    );
}

#[test]
fn test_roundtrip_idempotent_generic_function() {
    assert_idempotent(
        "generic_function",
        "function identity<T>(x: T): T { return x; }",
    );
}

#[test]
fn test_roundtrip_idempotent_closure() {
    assert_idempotent(
        "closure",
        "function main() { const add = (a: i32, b: i32): i32 => a + b; console.log(add(1, 2)); }",
    );
}

#[test]
fn test_roundtrip_idempotent_option_null() {
    assert_idempotent(
        "option_null",
        "function find(x: i32): i32 | null { if (x > 0) { return x; } return null; }",
    );
}

#[test]
fn test_roundtrip_idempotent_interface() {
    assert_idempotent(
        "interface",
        "interface Greetable { greet(): string; } type Person = { name: string };",
    );
}

#[test]
fn test_roundtrip_idempotent_imports() {
    assert_idempotent(
        "imports",
        "import { HashMap } from \"std::collections\";\nimport { Arc } from \"std::sync\";\n",
    );
}

// ---------------------------------------------------------------------------
// Phase 2 syntax: async, closures in complex contexts, string methods,
// template literals, iterators
// ---------------------------------------------------------------------------

#[test]
fn test_roundtrip_idempotent_async_function() {
    assert_idempotent(
        "async_function",
        "async function fetchData(): string { return \"data\"; }",
    );
}

#[test]
fn test_roundtrip_idempotent_await_expression() {
    assert_idempotent(
        "await_expression",
        "async function getData(): string { const result = await fetchData(); return result; }",
    );
}

#[test]
fn test_roundtrip_idempotent_template_literal() {
    assert_idempotent(
        "template_literal",
        "function greet(name: string): string { return `Hello, ${name}!`; }",
    );
}

#[test]
fn test_roundtrip_idempotent_string_methods() {
    assert_idempotent(
        "string_methods",
        "function main() { const s = \"hello\"; const upper = s.toUpperCase(); console.log(upper); }",
    );
}

#[test]
fn test_roundtrip_idempotent_complex_closure_with_generics() {
    assert_idempotent(
        "complex_closure_generics",
        "function apply<T>(f: (x: T) => T, val: T): T { return f(val); }",
    );
}

#[test]
fn test_roundtrip_idempotent_async_with_string_methods() {
    assert_idempotent(
        "async_string_methods",
        "\
async function main() {
  const name = \"hello world\";
  const upper = name.toUpperCase();
  console.log(upper);
}",
    );
}

// ---------------------------------------------------------------------------
// Semantic preservation: formatted code compiles to same Rust
// ---------------------------------------------------------------------------

#[test]
fn test_roundtrip_semantic_preservation_simple_function() {
    let source = "function add(a: i32, b: i32): i32 { return a + b; }";
    let formatted = format_source(source).expect("format should succeed");

    // Both original and formatted should compile to the same Rust
    let original_rs = compile_to_rust(source);
    let formatted_rs = compile_to_rust(&formatted);

    assert_eq!(
        original_rs, formatted_rs,
        "formatting should not change compilation output"
    );
}

#[test]
fn test_roundtrip_semantic_preservation_multiple_items() {
    let source = "\
function square(n: i32): i32 { return n * n; }
function main() { console.log(square(5)); }";

    let formatted = format_source(source).expect("format should succeed");

    let original_rs = compile_to_rust(source);
    let formatted_rs = compile_to_rust(&formatted);

    assert_eq!(
        original_rs, formatted_rs,
        "formatting should not change compilation output"
    );
}

#[test]
fn test_roundtrip_semantic_preservation_generic_function() {
    let source = "function identity<T>(x: T): T { return x; }";

    let formatted = format_source(source).expect("format should succeed");

    let original_rs = compile_to_rust(source);
    let formatted_rs = compile_to_rust(&formatted);

    assert_eq!(
        original_rs, formatted_rs,
        "formatting should not change compilation output for generics"
    );
}

// ---------------------------------------------------------------------------
// Helper: compile .rts to .rs via the driver pipeline
// ---------------------------------------------------------------------------

fn compile_to_rust(source: &str) -> String {
    let result = rsc_driver::compile_source(source, "test.rts");
    assert!(
        !result.has_errors,
        "compilation failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    result.rust_source
}

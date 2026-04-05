//! Tests for inline Rust escape hatch (`rust { ... }` blocks).
//!
//! Covers snapshot tests (fast, no cargo), e2e tests (slow, compile+run),
//! and diagnostic tests for the `rust { ... }` syntax.

mod test_utils;

use test_utils::{compile_and_run, compile_diagnostics, compile_to_rust};

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

// ===========================================================================
// Snapshot 1: Function-level rust block emits contents in the function body
// ===========================================================================

#[test]
fn test_snapshot_inline_rust_in_function_body() {
    let source = "\
function main(): void {
  const x: i32 = 42;
  rust {
    println!(\"The answer is {}\", x);
  }
}";

    let expected = "\
fn main() {
    let x: i32 = 42;
    println!(\"The answer is {}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("inline_rust_function_body", &actual, expected);
}

// ===========================================================================
// Snapshot 2: Module-level rust block emits at the top level
// ===========================================================================

#[test]
fn test_snapshot_inline_rust_module_level() {
    let source = "\
rust {
  type Pair = (i32, i32);
}

function main(): void {
  console.log(\"hello\");
}";

    let expected = "\
type Pair = (i32, i32);

fn main() {
    println!(\"{}\", \"hello\");
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("inline_rust_module_level", &actual, expected);
}

// ===========================================================================
// Snapshot 3: Nested braces inside rust block are preserved
// ===========================================================================

#[test]
fn test_snapshot_inline_rust_nested_braces() {
    let source = "\
function main(): void {
  rust {
    if true {
      println!(\"nested\");
    }
  }
}";

    let expected = "\
fn main() {
    if true {
    println!(\"nested\");
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("inline_rust_nested_braces", &actual, expected);
}

// ===========================================================================
// Snapshot 4: Multiple rust blocks in one function
// ===========================================================================

#[test]
fn test_snapshot_inline_rust_multiple_blocks() {
    let source = "\
function main(): void {
  rust {
    let a = 1;
  }
  rust {
    let b = 2;
  }
}";

    let expected = "\
fn main() {
    let a = 1;
    let b = 2;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("inline_rust_multiple_blocks", &actual, expected);
}

// ===========================================================================
// Snapshot 5: rust block mixed with regular RustScript statements
// ===========================================================================

#[test]
fn test_snapshot_inline_rust_mixed_with_rustscript() {
    let source = "\
function process(x: i32): i32 {
  const doubled: i32 = x * 2;
  rust {
    let tripled = doubled * 3;
  }
  return doubled;
}";

    let expected = "\
fn process(x: i32) -> i32 {
    let doubled: i32 = x * 2;
    let tripled = doubled * 3;
    return doubled;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("inline_rust_mixed", &actual, expected);
}

// ===========================================================================
// E2e 6: Inline Rust that defines a helper function, called from RustScript
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_inline_rust_helper_function() {
    let source = "\
rust {
  fn add(a: i32, b: i32) -> i32 {
    a + b
  }
}

function main(): void {
  rust {
    let result = add(3, 4);
    println!(\"{}\", result);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "7");
}

// ===========================================================================
// E2e 7: Inline Rust with unsafe block compiles and runs
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_inline_rust_unsafe_block() {
    let source = "\
function main(): void {
  rust {
    let x: i32 = 42;
    let result = unsafe {
      let ptr = &x as *const i32;
      *ptr
    };
    println!(\"{}\", result);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42");
}

// ===========================================================================
// Diagnostic 8: Unclosed rust block produces a clear error message
// ===========================================================================

#[test]
fn test_diagnostic_unclosed_rust_block() {
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
// Snapshot: Empty rust block produces empty output
// ===========================================================================

#[test]
fn test_snapshot_inline_rust_empty_block() {
    let source = "\
function main(): void {
  rust { }
}";

    let expected = "\
fn main() {
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("inline_rust_empty", &actual, expected);
}

// ===========================================================================
// Snapshot: Module-level rust block with nested function definition
// ===========================================================================

#[test]
fn test_snapshot_inline_rust_module_level_function() {
    let source = "\
rust {
  fn add_pair(p: (i32, i32)) -> i32 {
    p.0 + p.1
  }
}

function main(): void {
  console.log(\"hello\");
}";

    let expected = "\
fn add_pair(p: (i32, i32)) -> i32 {
p.0 + p.1
}

fn main() {
    println!(\"{}\", \"hello\");
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("inline_rust_module_function", &actual, expected);
}

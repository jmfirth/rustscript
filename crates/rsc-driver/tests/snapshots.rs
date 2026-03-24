//! Snapshot tests — compile `.rts` source and compare generated `.rs` against golden output.
//!
//! These are fast tests (no cargo invocation). They validate that the compiler
//! produces the expected Rust output for each Phase 0 test program.

mod test_utils;

use test_utils::compile_to_rust;

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
// 1. Hello World
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_hello_world_generates_println() {
    let source = "\
function main() {
  console.log(\"Hello, World!\");
}";

    let expected = "\
fn main() {
    println!(\"{}\", \"Hello, World!\");
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("hello_world", &actual, expected);
}

// ---------------------------------------------------------------------------
// 2. Arithmetic
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_arithmetic_generates_binary_ops() {
    let source = "\
function main() {
  const x: i64 = 10;
  const y: i64 = 3;
  console.log(x + y);
  console.log(x - y);
  console.log(x * y);
  console.log(x / y);
  console.log(x % y);
}";

    let expected = "\
fn main() {
    let x: i64 = 10;
    let y: i64 = 3;
    println!(\"{}\", x + y);
    println!(\"{}\", x - y);
    println!(\"{}\", x * y);
    println!(\"{}\", x / y);
    println!(\"{}\", x % y);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("arithmetic", &actual, expected);
}

// ---------------------------------------------------------------------------
// 3. Fibonacci
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_fibonacci_generates_recursive_fn() {
    let source = "\
function fibonacci(n: i32): i32 {
  if (n <= 1) {
    return n;
  }
  return fibonacci(n - 1) + fibonacci(n - 2);
}

function main() {
  console.log(fibonacci(10));
}";

    let expected = "\
fn fibonacci(n: i32) -> i32 {
    if n <= 1 {
        return n;
    }
    return fibonacci(n - 1) + fibonacci(n - 2);
}

fn main() {
    println!(\"{}\", fibonacci(10));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("fibonacci", &actual, expected);
}

// ---------------------------------------------------------------------------
// 4. FizzBuzz
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_fizzbuzz_generates_while_with_if_else_chain() {
    let source = "\
function main() {
  let i: i64 = 1;
  while (i <= 15) {
    if (i % 15 == 0) {
      console.log(\"FizzBuzz\");
    } else if (i % 3 == 0) {
      console.log(\"Fizz\");
    } else if (i % 5 == 0) {
      console.log(\"Buzz\");
    } else {
      console.log(i);
    }
    i = i + 1;
  }
}";

    let expected = "\
fn main() {
    let mut i: i64 = 1;
    while i <= 15 {
        if i % 15 == 0 {
            println!(\"{}\", \"FizzBuzz\");
        } else if i % 3 == 0 {
            println!(\"{}\", \"Fizz\");
        } else if i % 5 == 0 {
            println!(\"{}\", \"Buzz\");
        } else {
            println!(\"{}\", i);
        }
        i = i + 1;
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("fizzbuzz", &actual, expected);
}

// ---------------------------------------------------------------------------
// 5. Strings and clone behavior
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_strings_generates_clone_for_reuse() {
    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main() {
  const name: string = \"Alice\";
  greet(name);
  console.log(name);
}";

    let expected = "\
fn greet(name: String) {
    println!(\"{}\", name);
}

fn main() {
    let name: String = \"Alice\";
    greet(name.clone());
    println!(\"{}\", name);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("strings", &actual, expected);
}

// ---------------------------------------------------------------------------
// 6. Boolean logic
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_booleans_generates_comparison_and_negation() {
    let source = "\
function is_even(n: i32): bool {
  return n % 2 == 0;
}

function main() {
  console.log(is_even(4));
  console.log(is_even(7));
  console.log(!is_even(3));
}";

    let expected = "\
fn is_even(n: i32) -> bool {
    return n % 2 == 0;
}

fn main() {
    println!(\"{}\", is_even(4));
    println!(\"{}\", is_even(7));
    println!(\"{}\", !is_even(3));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("booleans", &actual, expected);
}

// ---------------------------------------------------------------------------
// 7. While loop with mutation
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_while_mutation_generates_mut_bindings() {
    let source = "\
function main() {
  let sum: i64 = 0;
  let i: i64 = 1;
  while (i <= 10) {
    sum = sum + i;
    i = i + 1;
  }
  console.log(sum);
}";

    let expected = "\
fn main() {
    let mut sum: i64 = 0;
    let mut i: i64 = 1;
    while i <= 10 {
        sum = sum + i;
        i = i + 1;
    }
    println!(\"{}\", sum);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("while_mutation", &actual, expected);
}

//! End-to-end tests — compile `.rts` source, build with cargo, run, verify stdout.
//!
//! These tests are slow (each invokes `cargo run`) and are marked `#[ignore]`
//! so they only run in the full suite (`just test-all` / `--include-ignored`).

mod test_utils;

use test_utils::compile_and_run;

// ---------------------------------------------------------------------------
// 1. Hello World
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_hello_world_prints_greeting() {
    let source = "\
function main() {
  console.log(\"Hello, World!\");
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Hello, World!");
}

// ---------------------------------------------------------------------------
// 2. Arithmetic
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_arithmetic_prints_five_results() {
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

    let stdout = compile_and_run(source);
    let expected = "13\n7\n30\n3\n1";
    assert_eq!(stdout.trim(), expected);
}

// ---------------------------------------------------------------------------
// 3. Fibonacci
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_fibonacci_prints_55() {
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

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "55");
}

// ---------------------------------------------------------------------------
// 4. FizzBuzz
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_fizzbuzz_prints_standard_sequence() {
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

    let stdout = compile_and_run(source);
    let expected = "\
1
2
Fizz
4
Buzz
Fizz
7
8
Fizz
Buzz
11
Fizz
13
14
FizzBuzz";
    assert_eq!(stdout.trim(), expected);
}

// ---------------------------------------------------------------------------
// 5. Strings and clone behavior
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_strings_clone_behavior() {
    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main() {
  const name: string = \"Alice\";
  greet(name);
  console.log(name);
}";

    let stdout = test_utils::compile_and_run(source);
    assert_eq!(stdout.trim(), "Alice\nAlice");
}

// ---------------------------------------------------------------------------
// 6. Boolean logic
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_booleans_prints_true_false_true() {
    let source = "\
function is_even(n: i32): bool {
  return n % 2 == 0;
}

function main() {
  console.log(is_even(4));
  console.log(is_even(7));
  console.log(!is_even(3));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\nfalse\ntrue");
}

// ---------------------------------------------------------------------------
// 7. While loop with mutation
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_while_mutation_prints_55() {
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

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "55");
}

// ---------------------------------------------------------------------------
// 8. U32 arithmetic (Task 013 correctness scenario 1)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_u32_arithmetic_prints_30() {
    let source = "\
function main() {
  const x: u32 = 10;
  const y: u32 = 20;
  console.log(x + y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "30");
}

// ---------------------------------------------------------------------------
// 9. F32 floating point (Task 013 correctness scenario 2)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_f32_floating_point_prints_4() {
    let source = "\
function main() {
  const x: f32 = 1.5;
  const y: f32 = 2.5;
  console.log(x + y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "4");
}

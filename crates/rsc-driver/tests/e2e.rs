//! End-to-end tests — compile `.rts` source, build with cargo, run, verify stdout.
//!
//! These tests are slow (each invokes `cargo run`) and are marked `#[ignore]`
//! so they only run in the full suite (`just test-all` / `--include-ignored`).

mod test_utils;

use test_utils::{compile_and_run, compile_multi_file_and_run};

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

// ---------------------------------------------------------------------------
// 10. Type def + struct construction (Task 014 correctness scenario 1)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_type_def_struct_construction_prints_fields() {
    let source = "\
type Point = { x: f64, y: f64 }
function main() {
  const p: Point = { x: 1.0, y: 2.0 };
  console.log(p.x);
  console.log(p.y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "1\n2");
}

// ---------------------------------------------------------------------------
// 11. Destructuring (Task 014 correctness scenario 2)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_destructuring_prints_name() {
    let source = "\
type User = { name: string, age: u32 }
function main() {
  const user: User = { name: \"Alice\", age: 30 };
  const { name, age } = user;
  console.log(name);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Alice");
}

// ---------------------------------------------------------------------------
// 12. Nested field access (Task 014 correctness scenario 3)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_nested_field_access_prints_city() {
    let source = "\
type Address = { city: string }
type Person = { name: string, address: Address }
function main() {
  const addr: Address = { city: \"Portland\" };
  const person: Person = { name: \"Bob\", address: addr };
  console.log(person.address.city);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Portland");
}

// ---------------------------------------------------------------------------
// 13. Template Literal — Simple Interpolation (Task 025)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_template_simple_interpolation_prints_hello_alice() {
    let source = "\
function main() {
  const name = \"Alice\";
  const greeting = `Hello, ${name}!`;
  console.log(greeting);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Hello, Alice!");
}

// ---------------------------------------------------------------------------
// 14. Template Literal — Expression Interpolation (Task 025)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_template_expression_interpolation_prints_sum() {
    let source = "\
function main() {
  const a: i32 = 5;
  const b: i32 = 3;
  console.log(`${a} + ${b} = ${a + b}`);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "5 + 3 = 8");
}

// ---------------------------------------------------------------------------
// 15. Template Literal — No Interpolation (Task 025)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_template_no_interpolation_prints_hello_world() {
    let source = "\
function main() {
  const msg = `hello world`;
  console.log(msg);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "hello world");
}

// ---------------------------------------------------------------------------
// 16. Array literal e2e (Task 017 correctness scenario 1)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_array_literal_prints_elements() {
    let source = "\
function main() {
  const numbers: Array<i32> = [1, 2, 3];
  console.log(numbers[0]);
  console.log(numbers[1]);
  console.log(numbers[2]);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "1\n2\n3");
}

// ---------------------------------------------------------------------------
// Task 024: Multi-file module system
// ---------------------------------------------------------------------------

// Correctness Scenario 1: Two-file project e2e
#[test]
#[ignore]
fn test_e2e_two_file_module_import() {
    let stdout = compile_multi_file_and_run(&[
        (
            "index.rts",
            "\
import { greet } from \"./utils\";

function main() {
  greet(\"World\");
}",
        ),
        (
            "utils.rts",
            "\
export function greet(name: string): void {
  console.log(name);
}",
        ),
    ]);
    assert_eq!(stdout.trim(), "World");
}

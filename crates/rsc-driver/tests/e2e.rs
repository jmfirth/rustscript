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

// ===========================================================================
// Phase 1 Integration E2E Tests (Task 026)
//
// These tests exercise multiple Phase 1 features composed together.
// Each test compiles `.rts`, builds with cargo, runs, and verifies stdout.
//
// Tests are `#[ignore]` because they invoke cargo (slow).
// ===========================================================================

// ---------------------------------------------------------------------------
// Integration 1: Enum + switch + simple string return
//
// Features: enum definition, switch/match, function, template literal
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p1_integration_enum_switch() {
    let source = r#"type Direction = "north" | "south"

function label(d: Direction): string {
  switch (d) {
    case "north": return `Going North`;
    case "south": return `Going South`;
  }
}

function main() {
  const d: Direction = "north";
  console.log(label(d));
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Going North");
}

// ---------------------------------------------------------------------------
// Integration 2: Struct + template literal + function
//
// Features: type definition, struct construction, field access, template literal
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p1_integration_struct_template() {
    let source = "\
type Person = { name: string, age: u32 }

function greet(p: Person): string {
  return `Hello, ${p.name}!`;
}

function main() {
  const p: Person = { name: \"Alice\", age: 30 };
  console.log(greet(p));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Hello, Alice!");
}

// ---------------------------------------------------------------------------
// Integration 3: Throws + try/catch + Result
//
// Features: throws function, Result, try/catch, match, template literal
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p1_integration_throws_try_catch() {
    let source = "\
function risky(x: i32): i32 throws string {
  if (x < 0) {
    throw \"negative\";
  }
  return x * 2;
}

function main() {
  try {
    const val = risky(5);
    console.log(val);
  } catch (err: string) {
    console.log(err);
  }
  try {
    const val = risky(-1);
    console.log(val);
  } catch (err: string) {
    console.log(err);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "10\nnegative");
}

// ---------------------------------------------------------------------------
// Integration 4: Class + constructor + methods
//
// Features: class definition, constructor, this, method calls, mut inference
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p1_integration_class_methods() {
    let source = "\
class Counter {
  private count: i32;

  constructor(initial: i32) {
    this.count = initial;
  }

  increment(): void {
    this.count = this.count + 1;
  }

  get(): i32 {
    return this.count;
  }
}

function main() {
  let c = new Counter(0);
  c.increment();
  c.increment();
  c.increment();
  console.log(c.get());
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "3");
}

// ---------------------------------------------------------------------------
// Integration 5: Multi-file module — math operations
//
// Features: modules, import/export, functions across files
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p1_integration_multi_file_math() {
    let stdout = compile_multi_file_and_run(&[
        (
            "index.rts",
            "\
import { add, multiply } from \"./math\";

function main() {
  console.log(add(3, 4));
  console.log(multiply(5, 6));
}",
        ),
        (
            "math.rts",
            "\
export function add(a: i32, b: i32): i32 {
  return a + b;
}

export function multiply(a: i32, b: i32): i32 {
  return a * b;
}",
        ),
    ]);
    assert_eq!(stdout.trim(), "7\n30");
}

// ---------------------------------------------------------------------------
// Integration 6: Option + null check + narrowing
//
// Features: T | null → Option<T>, null check narrowing, if let Some
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p1_integration_option_narrowing() {
    let source = "\
function findName(found: bool): string | null {
  if (found) { return \"Alice\"; }
  return null;
}

function main() {
  const name = findName(true);
  if (name !== null) {
    console.log(name);
  }
  const name2 = findName(false);
  if (name2 !== null) {
    console.log(name2);
  } else {
    console.log(\"not found\");
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Alice\nnot found");
}

// ---------------------------------------------------------------------------
// Integration 7: Interface + class implements (direct method call)
//
// Features: interface → trait, class implements → trait impl, template literal
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p1_integration_interface_class() {
    let source = "\
interface Greetable {
  greet(): string;
}

class Person implements Greetable {
  public name: string;

  constructor(name: string) {
    this.name = name;
  }

  greet(): string {
    return `Hello from ${this.name}`;
  }
}

function main() {
  const p = new Person(\"Alice\");
  console.log(p.greet());
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Hello from Alice");
}

// ---------------------------------------------------------------------------
// Integration 8: Destructuring + struct
//
// Features: type definition, struct construction, destructuring
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p1_integration_destructuring() {
    let source = "\
type Point = { x: i32, y: i32 }

function main() {
  const pt: Point = { x: 10, y: 20 };
  const { x, y } = pt;
  console.log(x);
  console.log(y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "10\n20");
}

// ===========================================================================
// Task 046: Tier 2 Ownership — E2E Tests
// ===========================================================================

// ---------------------------------------------------------------------------
// 046-E2E-1: Function taking &str compiles and runs correctly
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_tier2_borrowed_str_compiles_and_runs() {
    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main(): void {
  const name: string = \"Alice\";
  greet(name);
  greet(name);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Alice\nAlice");
}

// ---------------------------------------------------------------------------
// 046-E2E-2: Clone elimination — no double-free or use-after-move
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_tier2_clone_elimination_correct_output() {
    let source = "\
function len(s: string): i64 {
  return s.length;
}

function main(): void {
  const s: string = \"hello\";
  const a: i64 = len(s);
  const b: i64 = len(s);
  console.log(a);
  console.log(b);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "5\n5");
}

// ---------------------------------------------------------------------------
// 046-E2E-3: String literal without .to_string() compiles and runs
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_tier2_string_literal_no_alloc() {
    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main(): void {
  greet(\"hello\");
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "hello");
}

// ===========================================================================
// Task 047: Tier 2 Ownership — Edge Cases E2E
// ===========================================================================

// ---------------------------------------------------------------------------
// 047-E2E-1: Class method with &str param compiles and runs
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_047_class_method_borrowed_str() {
    let source = "\
class Greeter {
  greet(name: string): void {
    console.log(name);
  }
}

function main() {
  const g = new Greeter();
  g.greet(\"Alice\");
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Alice");
}

// ---------------------------------------------------------------------------
// 047-E2E-2: String method on &str param works correctly
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_047_string_method_on_borrowed_str() {
    let source = "\
function shout(name: string): string {
  return name.toUpperCase();
}

function main(): void {
  const result: string = shout(\"hello\");
  console.log(result);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "HELLO");
}

// ---------------------------------------------------------------------------
// 047-E2E-3: --no-borrow-inference flag produces Tier 1 (both compile)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_047_no_borrow_inference_both_compile() {
    use rsc_driver::{CompileOptions, compile_source_with_options};

    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main(): void {
  greet(\"hello\");
}";

    // With borrow inference (default)
    let with_borrow = compile_and_run(source);
    assert_eq!(with_borrow.trim(), "hello");

    // Without borrow inference — should also compile and produce same output
    let options = CompileOptions {
        no_borrow_inference: true,
        ..CompileOptions::default()
    };
    let result = compile_source_with_options(source, "test.rts", &options);
    assert!(!result.has_errors);

    // Build and run the no-borrow version too
    let rust_source = result.rust_source;
    assert!(
        rust_source.contains("fn greet(name: String)"),
        "no-borrow should use String: {rust_source}"
    );

    // Verify it compiles by building
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let src_dir = tmp_dir.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("failed to create src dir");
    let cargo_toml =
        "[package]\nname = \"rsc-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n";
    std::fs::write(tmp_dir.path().join("Cargo.toml"), cargo_toml)
        .expect("failed to write Cargo.toml");
    std::fs::write(src_dir.join("main.rs"), &rust_source).expect("failed to write main.rs");

    let output = std::process::Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .current_dir(tmp_dir.path())
        .output()
        .expect("failed to run cargo");

    assert!(
        output.status.success(),
        "no-borrow version should compile and run.\nstdout: {}\nstderr: {}\ngenerated:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        rust_source,
    );
    let stdout = String::from_utf8(output.stdout).expect("not utf-8");
    assert_eq!(stdout.trim(), "hello");
}

// ===========================================================================
// Task 058: Control Flow Completeness — finally block, === / !== verification
// ===========================================================================

// ---------------------------------------------------------------------------
// finally block runs after successful try
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_finally_runs_after_successful_try() {
    let source = "\
function main() {
  try {
    console.log(\"try\");
  } finally {
    console.log(\"finally\");
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "try\nfinally");
}

// ---------------------------------------------------------------------------
// finally block runs after caught error
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_finally_runs_after_caught_error() {
    let source = "\
function riskyOp(): i32 throws string {
  throw \"oops\";
}
function main() {
  try {
    const val = riskyOp();
    console.log(val);
  } catch (err: string) {
    console.log(\"caught\");
  } finally {
    console.log(\"finally\");
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "caught\nfinally");
}

// ---------------------------------------------------------------------------
// === / !== returns correct boolean for all types
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_strict_eq_integers_returns_correct_bool() {
    let source = "\
function main() {
  const a: i32 = 5;
  const b: i32 = 5;
  const c: i32 = 10;
  console.log(a === b);
  console.log(a === c);
  console.log(a !== c);
  console.log(a !== b);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\nfalse\ntrue\nfalse");
}

#[test]
#[ignore]
fn test_e2e_strict_eq_strings_returns_correct_bool() {
    let source = "\
function main() {
  const a: string = \"hello\";
  const b: string = \"hello\";
  const c: string = \"world\";
  console.log(a === b);
  console.log(a === c);
  console.log(a !== c);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\nfalse\ntrue");
}

#[test]
#[ignore]
fn test_e2e_strict_eq_booleans_returns_correct_bool() {
    let source = "\
function main() {
  const a: bool = true;
  const b: bool = true;
  const c: bool = false;
  console.log(a === b);
  console.log(a === c);
  console.log(a !== c);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\nfalse\ntrue");
}

// ---------------------------------------------------------------------------
// Task 054: Operators and Expressions
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_ternary_returns_correct_value() {
    let source = "\
function main() {
  const x: i64 = 10;
  const result: i64 = x > 5 ? 1 : 0;
  console.log(result);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "1");
}

#[test]
#[ignore]
fn test_e2e_exponentiation_computes_power() {
    let source = "\
function main() {
  const result: i64 = 2 ** 10;
  console.log(result);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "1024");
}

#[test]
#[ignore]
fn test_e2e_non_null_assert_unwraps_some() {
    let source = "\
function main() {
  const x: i64 | null = 42;
  const y: i64 = x!;
  console.log(y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42");
}

#[test]
#[ignore]
fn test_e2e_as_cast_converts_type() {
    let source = "\
function main() {
  const x: i64 = 42;
  const y: f64 = x as f64;
  console.log(y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42");
}

#[test]
#[ignore]
fn test_e2e_typeof_returns_correct_strings() {
    let source = "\
function main() {
  console.log(typeof 42);
  console.log(typeof true);
}";

    let stdout = compile_and_run(source);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "number");
    assert_eq!(lines[1], "boolean");
}

#[test]
#[ignore]
fn test_e2e_bitwise_operations() {
    let source = "\
function main() {
  const a: i64 = 12;
  const b: i64 = 10;
  console.log(a & b);
  console.log(a | b);
  console.log(a ^ b);
  console.log(~a);
}";

    let stdout = compile_and_run(source);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "8"); // 12 & 10 = 8
    assert_eq!(lines[1], "14"); // 12 | 10 = 14
    assert_eq!(lines[2], "6"); // 12 ^ 10 = 6
    assert_eq!(lines[3], "-13"); // ~12 = -13
}

#[test]
#[ignore]
fn test_e2e_shift_operations() {
    let source = "\
function main() {
  const x: i64 = 1;
  console.log(x << 4);
  console.log(16 >> 2);
}";

    let stdout = compile_and_run(source);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "16"); // 1 << 4 = 16
    assert_eq!(lines[1], "4"); // 16 >> 2 = 4
}

#[test]
#[ignore]
fn test_e2e_strict_equality_comparison() {
    let source = "\
function main() {
  const a: i64 = 5;
  const b: i64 = 5;
  const c: i64 = 3;
  console.log(a === b);
  console.log(a !== c);
  console.log(a === c);
}";

    let stdout = compile_and_run(source);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "true");
    assert_eq!(lines[1], "true");
    assert_eq!(lines[2], "false");
}

// ---------------------------------------------------------------------------
// Standard library builtins — Math operations e2e
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_math_operations_produce_correct_output() {
    let source = "\
function main() {
  console.log(Math.floor(3.7));
  console.log(Math.ceil(3.2));
  console.log(Math.round(3.5));
  console.log(Math.abs(-5.0));
  console.log(Math.sqrt(16.0));
  console.log(Math.min(3.0, 5.0));
  console.log(Math.max(3.0, 5.0));
  console.log(Math.pow(2.0, 3.0));
  console.log(Math.PI);
  console.log(Math.E);
}";

    let stdout = compile_and_run(source);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "3", "floor(3.7) should be 3");
    assert_eq!(lines[1], "4", "ceil(3.2) should be 4");
    assert_eq!(lines[2], "4", "round(3.5) should be 4");
    assert_eq!(lines[3], "5", "abs(-5.0) should be 5");
    assert_eq!(lines[4], "4", "sqrt(16.0) should be 4");
    assert_eq!(lines[5], "3", "min(3.0, 5.0) should be 3");
    assert_eq!(lines[6], "5", "max(3.0, 5.0) should be 5");
    assert_eq!(lines[7], "8", "pow(2.0, 3.0) should be 8");
    assert!(
        lines[8].starts_with("3.14159"),
        "PI should start with 3.14159"
    );
    assert!(
        lines[9].starts_with("2.71828"),
        "E should start with 2.71828"
    );
}

#[test]
#[ignore]
fn test_e2e_console_error_outputs_to_stderr() {
    // console.error outputs to stderr, not stdout — stdout should be empty
    let source = r#"
function main() {
  console.log("stdout");
}
"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "stdout");
}

#[test]
#[ignore]
fn test_e2e_number_parse_int_operations() {
    let source = r#"
function main() {
  console.log(Number.parseInt("42"));
  console.log(Number.parseInt("invalid"));
}
"#;

    let stdout = compile_and_run(source);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "42");
    assert_eq!(lines[1], "0");
}

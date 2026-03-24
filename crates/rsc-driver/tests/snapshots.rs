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
        i += 1;
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
    let name: String = \"Alice\".to_string();
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
        sum += i;
        i += 1;
    }
    println!(\"{}\", sum);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("while_mutation", &actual, expected);
}

// ---------------------------------------------------------------------------
// 8. Compound assignment e2e (Task 012 correctness scenario 1)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_compound_assign_generates_idiomatic_ops() {
    let source = "\
function main() {
  let x = 0;
  x += 5;
  x -= 2;
  console.log(x);
}";

    let expected = "\
fn main() {
    let mut x = 0;
    x += 5;
    x -= 2;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("compound_assign", &actual, expected);
}

// ---------------------------------------------------------------------------
// 9. String in println clean output (Task 012 correctness scenario 2)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_println_string_no_to_string() {
    let source = "\
function main() {
  console.log(\"Hello\");
}";

    let expected = "\
fn main() {
    println!(\"{}\", \"Hello\");
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("println_clean", &actual, expected);
}

// ---------------------------------------------------------------------------
// 10. Omitted type annotation (Task 012 correctness scenario 3)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_omitted_type_annotation_on_literal() {
    let source = "\
function main() {
  const x = 42;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x = 42;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("omitted_type", &actual, expected);
}

// ---------------------------------------------------------------------------
// 11. Explicit type annotation preserved (Task 012 correctness scenario 4)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_explicit_type_annotation_preserved() {
    let source = "\
function main() {
  const x: i32 = 42;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x: i32 = 42;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("explicit_type", &actual, expected);
}

// ---------------------------------------------------------------------------
// 12. Extended primitive: u8 (Task 013)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_u8_const_generates_let_u8() {
    let source = "\
function main() {
  const x: u8 = 255;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x: u8 = 255;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("u8_const", &actual, expected);
}

// ---------------------------------------------------------------------------
// 13. Extended primitive: u16 (Task 013)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_u16_const_generates_let_u16() {
    let source = "\
function main() {
  const x: u16 = 1000;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x: u16 = 1000;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("u16_const", &actual, expected);
}

// ---------------------------------------------------------------------------
// 14. Extended primitive: u32 (Task 013)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_u32_const_generates_let_u32() {
    let source = "\
function main() {
  const x: u32 = 42;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x: u32 = 42;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("u32_const", &actual, expected);
}

// ---------------------------------------------------------------------------
// 15. Extended primitive: u64 (Task 013)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_u64_const_generates_let_u64() {
    let source = "\
function main() {
  const x: u64 = 42;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x: u64 = 42;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("u64_const", &actual, expected);
}

// ---------------------------------------------------------------------------
// 16. Extended primitive: i8 (Task 013)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_i8_const_generates_let_i8() {
    let source = "\
function main() {
  const x: i8 = -1;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x: i8 = -1;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("i8_const", &actual, expected);
}

// ---------------------------------------------------------------------------
// 17. Extended primitive: i16 (Task 013)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_i16_const_generates_let_i16() {
    let source = "\
function main() {
  const x: i16 = 1000;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x: i16 = 1000;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("i16_const", &actual, expected);
}

// ---------------------------------------------------------------------------
// 18. Extended primitive: f32 (Task 013)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_f32_const_generates_let_f32() {
    let source = "\
function main() {
  const x: f32 = 3.14;
  console.log(x);
}";

    let expected = "\
fn main() {
    let x: f32 = 3.14;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("f32_const", &actual, expected);
}

// ---------------------------------------------------------------------------
// 19. Extended primitives: u32 function params and return (Task 013)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_u32_function_params_and_return() {
    let source = "\
function add(a: u32, b: u32): u32 {
  return a + b;
}

function main() {
  console.log(add(10, 20));
}";

    let expected = "\
fn add(a: u32, b: u32) -> u32 {
    return a + b;
}

fn main() {
    println!(\"{}\", add(10, 20));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("u32_function", &actual, expected);
}

// ---------------------------------------------------------------------------
// 20. Cross-type mismatch preserved for rustc (Task 013 correctness scenario 3)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_cross_type_mismatch_preserves_types() {
    let source = "\
function convert(x: i32): i64 {
  return x;
}";

    let expected = "\
fn convert(x: i32) -> i64 {
    return x;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("cross_type_mismatch", &actual, expected);
}

// ---------------------------------------------------------------------------
// 21. Type definition + struct construction (Task 014 correctness scenario 1)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_type_def_and_struct_construction() {
    let source = "\
type Point = { x: f64, y: f64 }
function main() {
  const p: Point = { x: 1.0, y: 2.0 };
  console.log(p.x);
  console.log(p.y);
}";

    let expected = "\
struct Point {
    pub x: f64,
    pub y: f64,
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 };
    println!(\"{}\", p.x);
    println!(\"{}\", p.y);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("type_def_struct_construction", &actual, expected);
}

// ---------------------------------------------------------------------------
// 22. Destructuring (Task 014 correctness scenario 2)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_destructuring() {
    let source = "\
type User = { name: string, age: u32 }
function main() {
  const user: User = { name: \"Alice\", age: 30 };
  const { name, age } = user;
  console.log(name);
}";

    let expected = "\
struct User {
    pub name: String,
    pub age: u32,
}

fn main() {
    let user = User { name: \"Alice\".to_string(), age: 30 };
    let User { name, age, .. } = user;
    println!(\"{}\", name);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("destructuring", &actual, expected);
}

// ---------------------------------------------------------------------------
// 23. Nested field access (Task 014 correctness scenario 3)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_nested_field_access() {
    let source = "\
type Address = { city: string }
type Person = { name: string, address: Address }
function main() {
  const addr: Address = { city: \"Portland\" };
  const person: Person = { name: \"Bob\", address: addr };
  console.log(person.address.city);
}";

    let expected = "\
struct Address {
    pub city: String,
}

struct Person {
    pub name: String,
    pub address: Address,
}

fn main() {
    let addr = Address { city: \"Portland\".to_string() };
    let person = Person { name: \"Bob\".to_string(), address: addr };
    println!(\"{}\", person.address.city);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("nested_field_access", &actual, expected);
}

// ---------------------------------------------------------------------------
// Template Literals (Task 025)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_template_simple_interpolation() {
    let source = "\
function main() {
  const name = \"Alice\";
  const greeting = `Hello, ${name}!`;
  console.log(greeting);
}";

    let expected = "\
fn main() {
    let name = \"Alice\".to_string();
    let greeting = format!(\"Hello, {}!\", name);
    println!(\"{}\", greeting);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("template_simple_interpolation", &actual, expected);
}

#[test]
fn test_snapshot_template_no_interpolation() {
    let source = "\
function main() {
  const msg = `hello world`;
  console.log(msg);
}";

    let expected = "\
fn main() {
    let msg = \"hello world\".to_string();
    println!(\"{}\", msg);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("template_no_interpolation", &actual, expected);
}

#[test]
fn test_snapshot_template_multiple_interpolations() {
    let source = "\
function main() {
  const a: i32 = 5;
  const b: i32 = 3;
  const result = `${a} + ${b} = ${a + b}`;
  console.log(result);
}";

    let expected = "\
fn main() {
    let a: i32 = 5;
    let b: i32 = 3;
    let result = format!(\"{} + {} = {}\", a, b, a + b);
    println!(\"{}\", result);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("template_multiple_interpolations", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 015: Simple Enum Definition + Switch
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_simple_enum_definition() {
    let source = r#"type Direction = "north" | "south" | "east" | "west""#;

    let expected = "\
enum Direction {
    North,
    South,
    East,
    West,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("simple_enum_definition", &actual, expected);
}

#[test]
fn test_snapshot_simple_enum_with_switch() {
    let source = r#"
type Direction = "north" | "south" | "east" | "west"

function opposite(dir: Direction): Direction {
  switch (dir) {
    case "north":
      return "south";
    case "south":
      return "north";
    case "east":
      return "west";
    case "west":
      return "east";
  }
}
"#;

    let expected = "\
enum Direction {
    North,
    South,
    East,
    West,
}

fn opposite(dir: Direction) -> Direction {
    match dir {
        Direction::North => {
            return Direction::South;
        }
        Direction::South => {
            return Direction::North;
        }
        Direction::East => {
            return Direction::West;
        }
        Direction::West => {
            return Direction::East;
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("simple_enum_switch", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 015: Data Enum (Discriminated Union) + Switch
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_data_enum_definition() {
    let source = r#"
type Shape =
  | { kind: "circle", radius: f64 }
  | { kind: "rect", width: f64, height: f64 }
"#;

    let expected = "\
enum Shape {
    Circle {
        pub radius: f64,
    },
    Rect {
        pub width: f64,
        pub height: f64,
    },
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("data_enum_definition", &actual, expected);
}

#[test]
fn test_snapshot_data_enum_with_switch() {
    let source = r#"
type Shape =
  | { kind: "circle", radius: f64 }
  | { kind: "rect", width: f64, height: f64 }

function area(shape: Shape): f64 {
  switch (shape) {
    case "circle":
      return 3.14159 * shape.radius * shape.radius;
    case "rect":
      return shape.width * shape.height;
  }
}
"#;

    let expected = "\
enum Shape {
    Circle {
        pub radius: f64,
    },
    Rect {
        pub width: f64,
        pub height: f64,
    },
}

fn area(shape: Shape) -> f64 {
    match shape {
        Shape::Circle { radius } => {
            return 3.14159 * radius * radius;
        }
        Shape::Rect { width, height } => {
            return width * height;
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("data_enum_switch", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 015: Enum Construction
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_enum_construction() {
    let source = r#"
type Direction = "north" | "south" | "east" | "west"

function main() {
  const dir: Direction = "north";
}
"#;

    let expected = "\
enum Direction {
    North,
    South,
    East,
    West,
}

fn main() {
    let dir = Direction::North;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("enum_construction", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 017: Array literal snapshot (correctness scenario 1)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_array_literal_generates_vec_macro() {
    let source = "\
function main() {
  const numbers: Array<i32> = [1, 2, 3];
  console.log(numbers[0]);
  console.log(numbers[1]);
  console.log(numbers[2]);
}";

    let expected = "\
fn main() {
    let numbers: Vec<i32> = vec![1, 2, 3];
    println!(\"{}\", numbers[0]);
    println!(\"{}\", numbers[1]);
    println!(\"{}\", numbers[2]);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("array_literal", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 017: Map construction snapshot (correctness scenario 2)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_map_construction_generates_hashmap_new() {
    let source = "\
function main() {
  const lookup: Map<string, i32> = new Map();
}";

    let expected = "\
use std::collections::HashMap;

fn main() {
    let lookup: HashMap<String, i32> = HashMap::new();
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("map_construction", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 017: Set construction snapshot (correctness scenario 3)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_set_construction_generates_hashset_new() {
    let source = "\
function main() {
  const unique: Set<string> = new Set();
}";

    let expected = "\
use std::collections::HashSet;

fn main() {
    let unique: HashSet<String> = HashSet::new();
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("set_construction", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 021: throws → Result with try/catch
// ---------------------------------------------------------------------------

// Correctness scenario 1: Simple throws function
#[test]
fn test_snapshot_simple_throws_function_generates_result() {
    let source = "\
function divide(a: f64, b: f64): f64 throws string {
  if (b == 0.0) {
    throw \"division by zero\";
  }
  return a / b;
}";

    let expected = "\
fn divide(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        return Err(\"division by zero\".to_string());
    }
    return Ok(a / b);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("simple_throws", &actual, expected);
}

// Correctness scenario 2: Error propagation with ?
#[test]
fn test_snapshot_error_propagation_inserts_question_mark() {
    let source = "\
function safeDivide(a: f64, b: f64): f64 throws string {
  if (b == 0.0) { throw \"division by zero\"; }
  return a / b;
}
function compute(x: f64): f64 throws string {
  const result = safeDivide(x, 2.0);
  return result;
}";

    let expected = "\
fn safeDivide(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        return Err(\"division by zero\".to_string());
    }
    return Ok(a / b);
}

fn compute(x: f64) -> Result<f64, String> {
    let result = safeDivide(x, 2.0)?;
    return Ok(result);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("error_propagation", &actual, expected);
}

// Correctness scenario 3: Try/catch
#[test]
fn test_snapshot_try_catch_generates_match_on_result() {
    let source = "\
function riskyOp(): i32 throws string {
  throw \"oops\";
}
function main() {
  try {
    const val = riskyOp();
    console.log(val);
  } catch (err: string) {
    console.log(err);
  }
}";

    let expected = "\
fn riskyOp() -> Result<i32, String> {
    return Err(\"oops\".to_string());
}

fn main() {
    match riskyOp() {
        Ok(val) => {
            println!(\"{}\", val);
        }
        Err(err) => {
            println!(\"{}\", err);
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("try_catch", &actual, expected);
}

// Void + throws function
#[test]
fn test_snapshot_void_throws_function_generates_result_unit() {
    let source = "\
function validate(x: i32) throws string {
  if (x < 0) {
    throw \"negative\";
  }
}";

    let expected = "\
fn validate(x: i32) -> Result<(), String> {
    if x < 0 {
        return Err(\"negative\".to_string());
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("void_throws", &actual, expected);
}

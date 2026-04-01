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
fn greet(name: &str) {
    println!(\"{}\", name);
}

fn main() {
    let name: String = \"Alice\".to_string();
    greet(&name);
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
struct Address {
    pub city: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Direction {
    North,
    South,
    East,
    West,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::North => write!(f, \"North\"),
            Direction::South => write!(f, \"South\"),
            Direction::East => write!(f, \"East\"),
            Direction::West => write!(f, \"West\"),
        }
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Direction {
    North,
    South,
    East,
    West,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::North => write!(f, \"North\"),
            Direction::South => write!(f, \"South\"),
            Direction::East => write!(f, \"East\"),
            Direction::West => write!(f, \"West\"),
        }
    }
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
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Circle {
        radius: f64,
    },
    Rect {
        width: f64,
        height: f64,
    },
}

impl std::fmt::Display for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, \"{:?}\", self)
    }
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
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Circle {
        radius: f64,
    },
    Rect {
        width: f64,
        height: f64,
    },
}

impl std::fmt::Display for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, \"{:?}\", self)
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Direction {
    North,
    South,
    East,
    West,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::North => write!(f, \"North\"),
            Direction::South => write!(f, \"South\"),
            Direction::East => write!(f, \"East\"),
            Direction::West => write!(f, \"West\"),
        }
    }
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
    Ok(())
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("void_throws", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 024: Module system snapshots
// ---------------------------------------------------------------------------

// Correctness Scenario 3: Non-exported item visibility
#[test]
fn test_snapshot_exported_vs_non_exported_visibility() {
    let source = "\
function helper(): i32 { return 42; }
export function publicFn(): i32 { return helper(); }";

    let actual = compile_to_rust(source);

    // Non-exported function should NOT have `pub`
    assert!(
        actual.contains("fn helper()"),
        "expected `fn helper()` without pub in output:\n{actual}"
    );
    assert!(
        !actual.contains("pub fn helper()"),
        "helper() should not be pub:\n{actual}"
    );

    // Exported function should have `pub`
    assert!(
        actual.contains("pub fn publicFn()"),
        "expected `pub fn publicFn()` in output:\n{actual}"
    );
}

// Correctness Scenario 2: Export type + import (snapshot portion — single file)
#[test]
fn test_snapshot_exported_type_produces_pub_struct() {
    let source = "\
export type User = { name: string, age: u32 }";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("pub struct User"),
        "expected `pub struct User` in output:\n{actual}"
    );
}

// Import generates use declaration
#[test]
fn test_snapshot_import_generates_use_decl() {
    let source = "\
import { greet } from \"./utils\";

function main() {
  greet(\"World\");
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("use crate::utils::greet;"),
        "expected `use crate::utils::greet;` in output:\n{actual}"
    );
    assert!(
        actual.contains("fn main()"),
        "expected `fn main()` in output:\n{actual}"
    );
}

// Import + mod declarations (via compile_source_with_mods)
#[test]
fn test_snapshot_import_with_mod_decls() {
    use rsc_driver::compile_source_with_mods;
    use rsc_syntax::rust_ir::RustModDecl;

    let source = "\
import { greet } from \"./utils\";

function main() {
  greet(\"World\");
}";

    let result = compile_source_with_mods(
        source,
        "index.rts",
        vec![RustModDecl {
            name: "utils".to_owned(),
            public: false,
            span: None,
        }],
    );

    assert!(
        !result.has_errors,
        "expected no errors, got: {:?}",
        result.diagnostics
    );

    let output = &result.rust_source;
    assert!(
        output.contains("mod utils;"),
        "expected `mod utils;` in output:\n{output}"
    );
    assert!(
        output.contains("use crate::utils::greet;"),
        "expected `use crate::utils::greet;` in output:\n{output}"
    );
}

// ---------------------------------------------------------------------------
// Task 124: Wildcard re-exports — export * from "module"
// ---------------------------------------------------------------------------

// Snapshot: export * from "./utils" emits pub use crate::utils::*;
#[test]
fn test_export_star_snapshot() {
    let source = r#"export * from "./utils";"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("pub use crate::utils::*;"),
        "expected `pub use crate::utils::*;` in output:\n{actual}"
    );
}

// Snapshot: export * from "serde" emits pub use serde::*;
#[test]
fn test_export_star_crate_snapshot() {
    let source = r#"export * from "serde";"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("pub use serde::*;"),
        "expected `pub use serde::*;` in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Task 023: Class sugar — struct + impl + constructor
// ---------------------------------------------------------------------------

// Correctness Scenario 1: Basic class
#[test]
fn test_snapshot_class_counter_generates_struct_and_impl() {
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
  console.log(c.get());
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Counter {
    count: i32,
}

impl Counter {
    fn new(initial: i32) -> Self {
        Self { count: initial }
    }

    fn increment(&mut self) {
        self.count = self.count + 1;
    }

    fn get(&self) -> i32 {
        return self.count;
    }
}

fn main() {
    let mut c = Counter::new(0);
    c.increment();
    c.increment();
    println!(\"{}\", c.get());
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("class_counter", &actual, expected);
}

// Correctness Scenario 2: Class with implements
#[test]
fn test_snapshot_class_implements_generates_trait_impl() {
    let source = "\
interface Describable {
  describe(): string;
}

class User implements Describable {
  public name: string;
  private age: u32;

  constructor(name: string, age: u32) {
    this.name = name;
    this.age = age;
  }

  describe(): string {
    return this.name;
  }
}";

    let actual = compile_to_rust(source);

    // Should contain the trait definition
    assert!(
        actual.contains("trait Describable"),
        "expected trait definition in output:\n{actual}"
    );

    // Should contain the struct definition
    assert!(
        actual.contains("struct User"),
        "expected struct User in output:\n{actual}"
    );

    // Should contain pub name field and non-pub age field
    assert!(
        actual.contains("pub name: String"),
        "expected pub name field in output:\n{actual}"
    );
    assert!(
        actual.contains("age: u32"),
        "expected non-pub age field in output:\n{actual}"
    );
    assert!(
        !actual.contains("pub age: u32"),
        "age should not be pub in output:\n{actual}"
    );

    // Should contain inherent impl with new
    assert!(
        actual.contains("impl User {"),
        "expected inherent impl User in output:\n{actual}"
    );
    assert!(
        actual.contains("fn new("),
        "expected fn new in output:\n{actual}"
    );

    // Should contain trait impl
    assert!(
        actual.contains("impl Describable for User {"),
        "expected trait impl in output:\n{actual}"
    );
    assert!(
        actual.contains("fn describe(&self)"),
        "expected describe method in trait impl:\n{actual}"
    );
}

// Correctness Scenario 3: `new` expression for classes
#[test]
fn test_snapshot_new_class_generates_static_call() {
    let source = "\
class Point {
  public x: f64;
  public y: f64;

  constructor(x: f64, y: f64) {
    this.x = x;
    this.y = y;
  }
}

function main() {
  const p = new Point(1.0, 2.0);
  console.log(p.x);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("Point::new(1.0, 2.0)"),
        "expected Point::new() call in output:\n{actual}"
    );
}

// ===========================================================================
// Phase 1 Integration Snapshots (Task 026)
//
// These snapshot tests cover Phase 1 features that did not yet have
// golden-file style snapshot coverage.
// ===========================================================================

// ---------------------------------------------------------------------------
// P1-1. Generic function snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_generic_function() {
    let source = "\
function identity<T>(x: T): T {
  return x;
}";

    let expected = "\
fn identity<T>(x: T) -> T {
    return x;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_generic_function", &actual, expected);
}

// ---------------------------------------------------------------------------
// P1-2. Generic struct snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_generic_struct() {
    let source = "\
type Pair<T> = { first: T, second: T }

function main() {
  const p: Pair<i32> = { first: 10, second: 20 };
  console.log(p.first);
  console.log(p.second);
}";

    let expected = "\
#[derive(Debug, Clone)]
struct Pair<T> {
    pub first: T,
    pub second: T,
}

fn main() {
    let p: Pair<i32> = Pair { first: 10, second: 20 };
    println!(\"{}\", p.first);
    println!(\"{}\", p.second);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_generic_struct", &actual, expected);
}

// ---------------------------------------------------------------------------
// P1-3. For-of loop snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_for_loop() {
    let source = "\
function main() {
  const numbers: Array<i32> = [1, 2, 3, 4, 5];
  for (const n of numbers) {
    console.log(n);
  }
}";

    let expected = "\
fn main() {
    let numbers: Vec<i32> = vec![1, 2, 3, 4, 5];
    for &n in &numbers {
        println!(\"{}\", n);
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_for_loop", &actual, expected);
}

// ---------------------------------------------------------------------------
// P1-4. For-of with break and continue snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_for_loop_break_continue() {
    let source = "\
function main() {
  const items: Array<i32> = [1, 2, 3, 4, 5];
  for (const n of items) {
    if (n == 2) { continue; }
    if (n == 4) { break; }
    console.log(n);
  }
}";

    let expected = "\
fn main() {
    let items: Vec<i32> = vec![1, 2, 3, 4, 5];
    for &n in &items {
        if n == 2 {
            continue;
        }
        if n == 4 {
            break;
        }
        println!(\"{}\", n);
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_for_loop_break_continue", &actual, expected);
}

// ---------------------------------------------------------------------------
// P1-5. Closure expression body snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_closure_expression() {
    let source = "\
function main() {
  const double = (x: i32): i32 => x * 2;
  console.log(double(21));
}";

    let expected = "\
fn main() {
    let double = |x: i32| -> i32 { x * 2 };
    println!(\"{}\", double(21));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_closure_expression", &actual, expected);
}

// ---------------------------------------------------------------------------
// P1-6. Closure block body snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_closure_block() {
    let source = "\
function main() {
  const add = (a: i32, b: i32): i32 => {
    return a + b;
  };
  console.log(add(3, 4));
}";

    let expected = "\
fn main() {
    let add = |a: i32, b: i32| -> i32 {
        return a + b;
    };
    println!(\"{}\", add(3, 4));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_closure_block", &actual, expected);
}

// ---------------------------------------------------------------------------
// P1-7. Option/null snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_option_null() {
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
}";

    let expected = "\
fn findName(found: bool) -> Option<String> {
    if found {
        return Some(\"Alice\".to_string());
    }
    return None;
}

fn main() {
    let name = findName(true);
    if let Some(name) = name {
        println!(\"{}\", name);
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_option_null", &actual, expected);
}

// ---------------------------------------------------------------------------
// P1-8. Interface definition snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_interface() {
    let source = "\
interface Printable {
  display(): string;
}

interface Sizeable {
  size(): u32;
}";

    let expected = "\
trait Printable {
    fn display(&self) -> String;
}

trait Sizeable {
    fn size(&self) -> u32;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_interface", &actual, expected);
}

// ---------------------------------------------------------------------------
// P1-9. Interface with intersection type parameter snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_interface_intersection() {
    let source = "\
interface Serializable {
  serialize(): string;
}
interface Printable {
  print(): void;
}
function process(input: Serializable & Printable): string {
  input.print();
  return input.serialize();
}";

    let expected = "\
trait Serializable {
    fn serialize(&self) -> String;
}

trait Printable {
    fn print(&self);
}

fn process<T: Serializable + Printable>(input: T) -> String {
    input.print();
    return input.serialize();
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_interface_intersection", &actual, expected);
}

// ===========================================================================
// Phase 1 Multi-Feature Integration Snapshots (Task 026)
//
// These snapshots exercise multiple Phase 1 features composed together.
// ===========================================================================

// ---------------------------------------------------------------------------
// Integration 1: Enum + switch + template literals
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_integration_enum_switch_template() {
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

    let expected = "\
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Direction {
    North,
    South,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::North => write!(f, \"North\"),
            Direction::South => write!(f, \"South\"),
        }
    }
}

fn label(d: Direction) -> String {
    match d {
        Direction::North => {
            return \"Going North\".to_string();
        }
        Direction::South => {
            return \"Going South\".to_string();
        }
    }
}

fn main() {
    let d = Direction::North;
    println!(\"{}\", label(d));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_integration_enum_switch_template", &actual, expected);
}

// ---------------------------------------------------------------------------
// Integration 2: Struct + template literal + function
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_integration_struct_template() {
    let source = "\
type Person = { name: string, age: u32 }

function greet(p: Person): string {
  return `Hello, ${p.name}!`;
}

function main() {
  const p: Person = { name: \"Alice\", age: 30 };
  console.log(greet(p));
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Person {
    pub name: String,
    pub age: u32,
}

fn greet(p: Person) -> String {
    return format!(\"Hello, {}!\", p.name);
}

fn main() {
    let p = Person { name: \"Alice\".to_string(), age: 30 };
    println!(\"{}\", greet(p));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_integration_struct_template", &actual, expected);
}

// ---------------------------------------------------------------------------
// Integration 3: Throws + try/catch + Result
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_integration_throws_try_catch() {
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
}";

    let expected = "\
fn risky(x: i32) -> Result<i32, String> {
    if x < 0 {
        return Err(\"negative\".to_string());
    }
    return Ok(x * 2);
}

fn main() {
    match risky(5) {
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
    assert_snapshot("p1_integration_throws_try_catch", &actual, expected);
}

// ---------------------------------------------------------------------------
// Integration 4: Class + methods
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_integration_class_methods() {
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
  console.log(c.get());
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Counter {
    count: i32,
}

impl Counter {
    fn new(initial: i32) -> Self {
        Self { count: initial }
    }

    fn increment(&mut self) {
        self.count = self.count + 1;
    }

    fn get(&self) -> i32 {
        return self.count;
    }
}

fn main() {
    let mut c = Counter::new(0);
    c.increment();
    c.increment();
    println!(\"{}\", c.get());
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_integration_class_methods", &actual, expected);
}

// ---------------------------------------------------------------------------
// Integration 5: For-of + sum (array + loop + compound assignment)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_integration_for_of_sum() {
    let source = "\
function main() {
  const numbers: Array<i32> = [1, 2, 3, 4, 5];
  let sum: i32 = 0;
  for (const n of numbers) {
    sum += n;
  }
  console.log(sum);
}";

    let expected = "\
fn main() {
    let numbers: Vec<i32> = vec![1, 2, 3, 4, 5];
    let mut sum: i32 = 0;
    for &n in &numbers {
        sum += n;
    }
    println!(\"{}\", sum);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_integration_for_of_sum", &actual, expected);
}

// ---------------------------------------------------------------------------
// Integration 6: Option + null check narrowing
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_integration_option_narrowing() {
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
}";

    let expected = "\
fn findName(found: bool) -> Option<String> {
    if found {
        return Some(\"Alice\".to_string());
    }
    return None;
}

fn main() {
    let name = findName(true);
    if let Some(name) = name {
        println!(\"{}\", name);
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_integration_option_narrowing", &actual, expected);
}

// ---------------------------------------------------------------------------
// Integration 7: Interface + class implements + template literal
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_integration_interface_class() {
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

    let expected = "\
trait Greetable {
    fn greet(&self) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Person {
    pub name: String,
}

impl Person {
    fn new(name: String) -> Self {
        Self { name: name }
    }
}

impl Greetable for Person {
    fn greet(&self) -> String {
        return format!(\"Hello from {}\", self.name);
    }
}

fn main() {
    let p = Person::new(\"Alice\".to_string());
    println!(\"{}\", p.greet());
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_integration_interface_class", &actual, expected);
}

// ---------------------------------------------------------------------------
// Integration 8: Destructuring + struct + field access
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p1_integration_destructuring() {
    let source = "\
type Point = { x: i32, y: i32 }

function main() {
  const pt: Point = { x: 10, y: 20 };
  const { x, y } = pt;
  console.log(x);
  console.log(y);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Point {
    pub x: i32,
    pub y: i32,
}

fn main() {
    let pt = Point { x: 10, y: 20 };
    let Point { x, y, .. } = pt;
    println!(\"{}\", x);
    println!(\"{}\", y);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p1_integration_destructuring", &actual, expected);
}

// ---------------------------------------------------------------------------
// Phase 2: Async/await correctness scenarios (Task 028)
// ---------------------------------------------------------------------------

// Correctness scenario 1: Async function declaration
#[test]
fn test_snapshot_async_function_declaration() {
    let source = r#"async function greet(): string {
  return "hello";
}"#;

    let expected = r#"async fn greet() -> String {
    return "hello".to_string();
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_function_declaration", &actual, expected);
}

// Correctness scenario 2: Await expression
#[test]
fn test_snapshot_await_expression() {
    let source = r#"async function fetchData(): string {
  const result = await getData();
  return result;
}"#;

    let expected = r#"async fn fetchData() -> String {
    let result = getData().await;
    return result;
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("await_expression", &actual, expected);
}

// Correctness scenario 3: Async closure
#[test]
fn test_snapshot_async_closure() {
    let source = r#"function main() {
  const handler = async () => {
    await processRequest();
  };
}"#;

    let expected = r#"fn main() {
    let handler = async || {
        processRequest().await;
    };
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_closure", &actual, expected);
}

// Correctness scenario 4: Non-async function unchanged (regression)
#[test]
fn test_snapshot_non_async_function_unchanged() {
    let source = r#"function add(a: i32, b: i32): i32 {
  return a + b;
}"#;

    let expected = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b;
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("non_async_function_unchanged", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 032: String extension methods — correctness scenarios
// ---------------------------------------------------------------------------

// Correctness scenario 1: string method variety
#[test]
fn test_snapshot_string_method_variety() {
    let source = r#"function main() {
  const name = "Alice";
  const upper = name.toUpperCase();
  const starts = name.startsWith("A");
  const trimmed = "  hello  ".trim();
  const included = name.includes("lic");
  console.log(upper);
}"#;

    let expected = r#"fn main() {
    let name = "Alice".to_string();
    let upper = name.to_uppercase();
    let starts = name.starts_with("A");
    let trimmed = "  hello  ".to_string().trim().to_string();
    let included = name.contains("lic");
    println!("{}", upper);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("string_method_variety", &actual, expected);
}

// Correctness scenario 2: split method
#[test]
fn test_snapshot_string_split_method() {
    let source = r#"function main() {
  const parts = "a,b,c".split(",");
  console.log(parts.length);
}"#;

    let expected = r#"fn main() {
    let parts = "a,b,c".to_string().split(",").map(|s| s.to_string()).collect::<Vec<String>>();
    println!("{}", parts.len() as i64);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("string_split_method", &actual, expected);
}

// Correctness scenario 3: replace method
#[test]
fn test_snapshot_string_replace_method() {
    let source = r#"function main() {
  const result = "hello world".replace("world", "rust");
  console.log(result);
}"#;

    let expected = r#"fn main() {
    let result = "hello world".to_string().replace("world", "rust");
    println!("{}", result);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("string_replace_method", &actual, expected);
}

// Chaining: toUpperCase().startsWith()
#[test]
fn test_snapshot_string_method_chaining() {
    let source = r#"function main() {
  const name = "Alice";
  const result = name.toUpperCase().startsWith("A");
  console.log(result);
}"#;

    let expected = r#"fn main() {
    let name = "Alice".to_string();
    let result = name.to_uppercase().starts_with("A");
    println!("{}", result);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("string_method_chaining", &actual, expected);
}

// toLowerCase and endsWith
#[test]
fn test_snapshot_string_to_lower_case_and_ends_with() {
    let source = r#"function main() {
  const name = "HELLO";
  const lower = name.toLowerCase();
  const ends = name.endsWith("z");
  console.log(lower);
}"#;

    let expected = r#"fn main() {
    let name = "HELLO".to_string();
    let lower = name.to_lowercase();
    let ends = name.ends_with("z");
    println!("{}", lower);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("string_to_lower_case_and_ends_with", &actual, expected);
}

// .length property on strings
#[test]
fn test_snapshot_string_length_property() {
    let source = r#"function main() {
  const name = "Alice";
  const len = name.length;
  console.log(len);
}"#;

    let expected = r#"fn main() {
    let name = "Alice".to_string();
    let len = name.len() as i64;
    println!("{}", len);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("string_length_property", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 030: Promise.all concurrent execution (correctness scenario 1)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_promise_all_concurrent_execution() {
    let source = r#"async function main() {
  const [user, posts] = await Promise.all([
    getUser(),
    getPosts(),
  ]);
  console.log(user);
}

async function getUser(): string {
  return "alice";
}

async function getPosts(): string {
  return "posts";
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    let (user, posts) = tokio::join!(getUser(), getPosts());
    println!("{}", user);
}

async fn getUser() -> String {
    return "alice".to_string();
}

async fn getPosts() -> String {
    return "posts".to_string();
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("promise_all_concurrent", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 030: spawn fire-and-forget (correctness scenario 2)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_spawn_fire_and_forget() {
    let source = r#"async function main() {
  spawn(async () => {
    await doWork();
  });
  console.log("spawned");
}

async function doWork() {
  console.log("working");
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    tokio::spawn(async move {
        doWork().await;
    });
    println!("{}", "spawned");
}

async fn doWork() {
    println!("{}", "working");
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("spawn_fire_and_forget", &actual, expected);
}

// ===========================================================================
// Task 046: Tier 2 Ownership — Callsite Borrow Transform
// ===========================================================================

// ---------------------------------------------------------------------------
// 046-1. BorrowedStr param emits `fn greet(name: &str)`
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tier2_borrowed_str_param_signature() {
    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main(): void {
  const name: string = \"Alice\";
  greet(name);
  greet(name);
}";

    let expected = "\
fn greet(name: &str) {
    println!(\"{}\", name);
}

fn main() {
    let name: String = \"Alice\".to_string();
    greet(&name);
    greet(&name);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tier2_borrowed_str", &actual, expected);
}

// ---------------------------------------------------------------------------
// 046-2. String literal optimization — no .to_string() for BorrowedStr
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tier2_string_literal_no_to_string() {
    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main(): void {
  greet(\"hello\");
}";

    let expected = "\
fn greet(name: &str) {
    println!(\"{}\", name);
}

fn main() {
    greet(\"hello\");
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tier2_string_literal_optimization", &actual, expected);
}

// ---------------------------------------------------------------------------
// 046-3. Mixed borrow and owned params
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tier2_mixed_borrow_and_owned() {
    let source = "\
function readName(name: string): void {
  console.log(name);
}

function takeName(name: string): string {
  return name;
}";

    let expected = "\
fn readName(name: &str) {
    println!(\"{}\", name);
}

fn takeName(name: String) -> String {
    return name;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tier2_mixed_borrow_owned", &actual, expected);
}

// ---------------------------------------------------------------------------
// 046-4. Clone elimination — variable passed to two borrowed-param functions
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tier2_clone_elimination() {
    let source = "\
function len(s: string): i64 {
  return s.length;
}

function main(): void {
  const s: string = \"hello\";
  const a: i64 = len(s);
  const b: i64 = len(s);
}";

    let expected = "\
fn len(s: &str) -> i64 {
    return s.len() as i64;
}

fn main() {
    let s: String = \"hello\".to_string();
    let a: i64 = len(&s);
    let b: i64 = len(&s);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tier2_clone_elimination", &actual, expected);
}

// ---------------------------------------------------------------------------
// 046-5. Variable passed to BorrowedStr emits &var
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tier2_variable_to_borrowed_str() {
    let source = "\
function display(msg: string): void {
  console.log(msg);
}

function main(): void {
  const msg: string = \"world\";
  display(msg);
}";

    let expected = "\
fn display(msg: &str) {
    println!(\"{}\", msg);
}

fn main() {
    let msg: String = \"world\".to_string();
    display(&msg);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tier2_variable_to_borrowed_str", &actual, expected);
}

// ---------------------------------------------------------------------------
// 046-6. Borrowed param for generic collection type (Vec)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tier2_borrowed_vec_param() {
    let source = "\
function countItems(items: Array<i32>): i64 {
  return items.length;
}

function main(): void {
  const nums: Array<i32> = [1, 2, 3];
  const n: i64 = countItems(nums);
  console.log(n);
}";

    let expected = "\
fn countItems(items: &Vec<i32>) -> i64 {
    return items.len() as i64;
}

fn main() {
    let nums: Vec<i32> = vec![1, 2, 3];
    let n: i64 = countItems(&nums);
    println!(\"{}\", n);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tier2_borrowed_vec_param", &actual, expected);
}

// ---------------------------------------------------------------------------
// 046-7. Mixed params — one borrowed, one owned
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tier2_mixed_params_same_function() {
    let source = "\
function process(label: string, count: i64): void {
  console.log(label);
  console.log(count);
}

function main(): void {
  process(\"test\", 42);
}";

    let expected = "\
fn process(label: &str, count: i64) {
    println!(\"{}\", label);
    println!(\"{}\", count);
}

fn main() {
    process(\"test\", 42);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tier2_mixed_params_same_fn", &actual, expected);
}

// ===========================================================================
// Task 047: Tier 2 Ownership — Edge Cases and Optimization
// ===========================================================================

// ---------------------------------------------------------------------------
// 047-1. Class method with borrowed param emits `&str`
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_class_method_borrowed_param() {
    let source = "\
class Greeter {
  greet(name: string): void {
    console.log(name);
  }
}

function main() {
  const g = new Greeter();
  g.greet(\"world\");
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Greeter {
}

impl Greeter {
    fn greet(&self, name: &str) {
        println!(\"{}\", name);
    }
}

fn main() {
    let g = Greeter::new();
    g.greet(\"world\");
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("047_class_method_borrowed_param", &actual, expected);
}

// ---------------------------------------------------------------------------
// 047-2. Class method `self` param unchanged by borrow analysis
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_class_method_self_unchanged() {
    let source = "\
class Counter {
  public count: i32;

  constructor() {
    this.count = 0;
  }

  increment(): void {
    this.count = this.count + 1;
  }

  display(label: string): void {
    console.log(label);
    console.log(this.count);
  }
}";

    let actual = compile_to_rust(source);
    // increment mutates self → &mut self
    assert!(
        actual.contains("fn increment(&mut self)"),
        "increment should have &mut self: {actual}"
    );
    // display only reads self → &self, label is borrowed
    assert!(
        actual.contains("fn display(&self, label: &str)"),
        "display should have &self and label: &str: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 047-3. Trait impl method params stay Owned
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_trait_impl_method_stays_owned() {
    let source = "\
interface Processor {
  process(data: string): void;
}

class MyProcessor implements Processor {
  process(data: string): void {
    console.log(data);
  }
}";

    let actual = compile_to_rust(source);
    // Trait method signature: owned String
    assert!(
        actual.contains("fn process(&self, data: String)"),
        "trait impl method should keep Owned params: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 047-3b. Generic param T used only in println! stays Owned
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_generic_param_stays_owned() {
    let source = "\
function show<T>(val: T): void {
  console.log(val);
}";

    let actual = compile_to_rust(source);
    // Generic type params conservatively stay Owned — we can't know if T is Copy
    assert!(
        actual.contains("fn show<T>(val: T)"),
        "generic param T should stay Owned: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 047-4. Simple enum type param stays Owned (Copy type)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_simple_enum_param_stays_owned() {
    let source = r#"type Color = "red" | "green" | "blue"

function useColor(c: Color, name: string): void {
  console.log(name);
}"#;

    let actual = compile_to_rust(source);
    // Simple enum param: Owned (Copy → no benefit from borrowing)
    // String param: BorrowedStr (&str)
    assert!(
        actual.contains("fn useColor(c: Color, name: &str)"),
        "simple enum param should stay Owned, string should borrow: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 047-5. Option<i32> param stays Owned (Copy type)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_option_copy_param_stays_owned() {
    let source = "\
function check(val: i32 | null): void {
  console.log(val);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("fn check(val: Option<i32>)"),
        "Option<i32> param should stay Owned (Copy): {actual}"
    );
}

// ---------------------------------------------------------------------------
// 047-6. Parameter used in loop body → correct borrow inference
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_param_in_loop_body_borrowed() {
    let source = "\
function repeat(msg: string, n: i32): void {
  let i: i32 = 0;
  while (i < n) {
    console.log(msg);
    i = i + 1;
  }
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("fn repeat(msg: &str, n: i32)"),
        "param read in loop should be borrowed: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 047-6b. String method on &str param (correctness scenario 4)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_string_method_on_borrowed_param() {
    let source = "\
function shout(name: string): string {
  return name.toUpperCase();
}";

    let actual = compile_to_rust(source);
    // name is only used in a method call → ReadOnly → BorrowedStr
    assert!(
        actual.contains("fn shout(name: &str) -> String"),
        "param should be &str with String return: {actual}"
    );
    assert!(
        actual.contains("name.to_uppercase()"),
        "should call .to_uppercase() on &str: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 047-7. --no-borrow-inference flag produces Tier 1 output
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_047_no_borrow_inference_flag() {
    use rsc_driver::{CompileOptions, compile_source_with_options};

    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main(): void {
  greet(\"hello\");
}";

    // With borrow inference (default): name → &str
    let result_with = rsc_driver::compile_source(source, "test.rts");
    assert!(!result_with.has_errors);
    assert!(
        result_with.rust_source.contains("fn greet(name: &str)"),
        "default should use &str: {}",
        result_with.rust_source,
    );

    // Without borrow inference: name → String
    let options = CompileOptions {
        no_borrow_inference: true,
        ..CompileOptions::default()
    };
    let result_without = compile_source_with_options(source, "test.rts", &options);
    assert!(!result_without.has_errors);
    assert!(
        result_without
            .rust_source
            .contains("fn greet(name: String)"),
        "no-borrow should use String: {}",
        result_without.rust_source,
    );
}

// ---------------------------------------------------------------------------
// Task 055: Function Features — Optional, Default, Rest Parameters
// ---------------------------------------------------------------------------

// T055-S1: Function with optional parameters
#[test]
fn test_snapshot_optional_params() {
    let source = "\
function greet(name: string, title?: string): string {
  return name;
}

function main() {
  console.log(greet(\"Alice\"));
  console.log(greet(\"Bob\", \"Dr.\"));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("fn greet(name: String, title: Option<String>) -> String"),
        "optional param should be Option<String>: {actual}"
    );
    assert!(
        actual.contains("greet(\"Alice\".to_string(), None)"),
        "missing optional arg should be None: {actual}"
    );
    assert!(
        actual.contains("greet(\"Bob\".to_string(), \"Dr.\".to_string())"),
        "supplied optional arg should pass through: {actual}"
    );
}

// T055-S2: Function with default parameters
#[test]
fn test_snapshot_default_params() {
    let source = "\
function connect(host: string, port: i64 = 8080): string {
  return host;
}

function main() {
  console.log(connect(\"localhost\"));
  console.log(connect(\"localhost\", 9090));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("fn connect(host: String, port: i64) -> String"),
        "default param should use base type: {actual}"
    );
    assert!(
        actual.contains("connect(\"localhost\".to_string(), 8080)"),
        "missing default arg should be inlined: {actual}"
    );
    assert!(
        actual.contains("connect(\"localhost\".to_string(), 9090)"),
        "supplied default arg should pass through: {actual}"
    );
}

// T055-S3: Function with rest parameters
#[test]
fn test_snapshot_rest_params() {
    let source = "\
function log_all(prefix: string, ...messages: Array<string>): void {
  console.log(prefix);
}

function main() {
  log_all(\"INFO\", \"hello\", \"world\");
  log_all(\"DEBUG\");
}";

    let actual = compile_to_rust(source);
    // Borrow inference may optimize prefix to &str, but messages must be Vec<String>
    assert!(
        actual.contains("messages: Vec<String>"),
        "rest param should be Vec<String>: {actual}"
    );
    assert!(
        actual.contains("vec![\"hello\".to_string(), \"world\".to_string()]"),
        "excess args should be collected into vec![]: {actual}"
    );
    assert!(
        actual.contains("vec![]"),
        "no rest args should produce empty vec: {actual}"
    );
}

// T055-S4: Spread argument in function call
#[test]
fn test_snapshot_spread_arg() {
    let source = "\
function log_all(prefix: string, ...messages: Array<string>): void {
  console.log(prefix);
}

function main() {
  const items: Array<string> = [\"a\", \"b\"];
  log_all(\"INFO\", ...items);
}";

    let actual = compile_to_rust(source);
    // Spread arg should pass the vec directly, not wrap in another vec
    // Borrow inference may optimize prefix to &str
    assert!(
        actual.contains("log_all(\"INFO\""),
        "first arg should be present: {actual}"
    );
    assert!(
        actual.contains(", items)"),
        "spread arg should pass vec directly: {actual}"
    );
}

// T055-S5: Combined optional + default
#[test]
fn test_snapshot_combined_optional_default() {
    let source = "\
function setup(name: string, verbose?: bool, retries: i64 = 3): string {
  return name;
}

function main() {
  console.log(setup(\"test\"));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("fn setup(name: String, verbose: Option<bool>, retries: i64) -> String"),
        "mixed optional and default params: {actual}"
    );
    assert!(
        actual.contains("setup(\"test\".to_string(), None, 3)"),
        "missing args should be filled with None and default: {actual}"
    );
}

// ---------------------------------------------------------------------------
// Task 054: Operators and Expressions
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_ternary_emits_if_else_expr() {
    let source = "\
function main() {
  const x: i64 = 5;
  const result: i64 = x > 3 ? 1 : 0;
  console.log(result);
}";

    let expected = "\
fn main() {
    let x: i64 = 5;
    let result: i64 = if x > 3 { 1 } else { 0 };
    println!(\"{}\", result);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("ternary_if_else", &actual, expected);
}

#[test]
fn test_snapshot_exponentiation_integer_emits_pow() {
    let source = "\
function main() {
  const base: i64 = 2;
  const result: i64 = base ** 10;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".pow(10 as u32)"),
        "expected .pow(b as u32), got: {actual}"
    );
}

#[test]
fn test_snapshot_exponentiation_float_emits_powf() {
    let source = "\
function main() {
  const result: f64 = 2.0 ** 0.5;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".powf(0.5)"),
        "expected .powf(b), got: {actual}"
    );
}

#[test]
fn test_snapshot_triple_equals_emits_double_equals() {
    let source = "\
function main() {
  const a: i64 = 1;
  const b: i64 = 1;
  const eq: bool = a === b;
  const neq: bool = a !== b;
  console.log(eq);
  console.log(neq);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("a == b"),
        "=== should emit ==, got: {actual}"
    );
    assert!(
        actual.contains("a != b"),
        "!== should emit !=, got: {actual}"
    );
}

#[test]
fn test_snapshot_non_null_assert_emits_unwrap() {
    let source = "\
function main() {
  const x: i64 | null = 42;
  const y: i64 = x!;
  console.log(y);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".unwrap()"),
        "x! should emit .unwrap(), got: {actual}"
    );
}

#[test]
fn test_snapshot_as_cast_emits_rust_cast() {
    let source = "\
function main() {
  const x: i64 = 42;
  const y: f64 = x as f64;
  console.log(y);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x as f64"),
        "as cast should emit 'as f64', got: {actual}"
    );
}

#[test]
fn test_snapshot_typeof_number_emits_string_literal() {
    let source = "\
function main() {
  const t: string = typeof 42;
  console.log(t);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("\"number\""),
        "typeof 42 should emit \"number\", got: {actual}"
    );
}

#[test]
fn test_snapshot_typeof_string_emits_string_literal() {
    let source = "\
function main() {
  const t: string = typeof \"hello\";
  console.log(t);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("\"string\""),
        "typeof \"hello\" should emit \"string\", got: {actual}"
    );
}

#[test]
fn test_snapshot_typeof_boolean_emits_string_literal() {
    let source = "\
function main() {
  const t: string = typeof true;
  console.log(t);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("\"boolean\""),
        "typeof true should emit \"boolean\", got: {actual}"
    );
}

#[test]
fn test_snapshot_bitwise_and_emits_ampersand() {
    let source = "\
function main() {
  const x: i64 = 255;
  const y: i64 = 15;
  const result: i64 = x & y;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x & y"),
        "bitwise AND should emit &, got: {actual}"
    );
}

#[test]
fn test_snapshot_bitwise_or_emits_pipe() {
    let source = "\
function main() {
  const x: i64 = 240;
  const y: i64 = 15;
  const result: i64 = x | y;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x | y"),
        "bitwise OR should emit |, got: {actual}"
    );
}

#[test]
fn test_snapshot_bitwise_xor_emits_caret() {
    let source = "\
function main() {
  const x: i64 = 255;
  const y: i64 = 15;
  const result: i64 = x ^ y;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x ^ y"),
        "bitwise XOR should emit ^, got: {actual}"
    );
}

#[test]
fn test_snapshot_bitwise_not_emits_exclamation() {
    let source = "\
function main() {
  const x: i64 = 255;
  const result: i64 = ~x;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("!x"),
        "bitwise NOT (~) should emit ! in Rust, got: {actual}"
    );
}

#[test]
fn test_snapshot_left_shift_emits_shl() {
    let source = "\
function main() {
  const x: i64 = 1;
  const result: i64 = x << 4;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x << 4"),
        "left shift should emit <<, got: {actual}"
    );
}

#[test]
fn test_snapshot_right_shift_emits_shr() {
    let source = "\
function main() {
  const x: i64 = 16;
  const result: i64 = x >> 2;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x >> 2"),
        "right shift should emit >>, got: {actual}"
    );
}

// ---------------------------------------------------------------------------
// Task 057: Class Completeness
// ---------------------------------------------------------------------------

// 057-1: Field initializers
#[test]
fn test_snapshot_057_field_initializer_default_in_new() {
    let source = "\
class Config {
  public host: string = \"localhost\";
  public port: i32 = 8080;
  constructor() {}
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    pub host: String,
    pub port: i32,
}

impl Config {
    fn new() -> Self {
        Self { host: \"localhost\".to_string(), port: 8080 }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("057_field_initializer", &actual, expected);
}

// 057-2: Constructor parameter properties
#[test]
fn test_snapshot_057_constructor_param_properties() {
    let source = "\
class User {
  constructor(public name: string, private age: i32) {}
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    age: i32,
}

impl User {
    fn new(name: String, age: i32) -> Self {
        Self { name: name, age: age }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("057_constructor_param_properties", &actual, expected);
}

// 057-3: Static method
#[test]
fn test_snapshot_057_static_method() {
    let source = "\
class UserService {
  private count: i32;
  constructor() {
    this.count = 0;
  }
  static create(): UserService {
    return new UserService();
  }
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct UserService {
    count: i32,
}

impl UserService {
    fn new() -> Self {
        Self { count: 0 }
    }

    fn create() -> UserService {
        return UserService::new();
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("057_static_method", &actual, expected);
}

// 057-4: Static field (associated constant)
#[test]
fn test_snapshot_057_static_field_assoc_const() {
    let source = "\
class Config {
  static DEFAULT_PORT: i32 = 8080;
  public host: string;
  constructor(host: string) {
    this.host = host;
  }
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    pub host: String,
}

impl Config {
    pub const DEFAULT_PORT: i32 = 8080;

    fn new(host: String) -> Self {
        Self { host: host }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("057_static_field_assoc_const", &actual, expected);
}

// 057-5: Getter and setter
#[test]
fn test_snapshot_057_getter_setter() {
    let source = "\
class User {
  private _name: string;
  constructor(name: string) {
    this._name = name;
  }
  get name(): string {
    return this._name;
  }
  set name(value: string) {
    this._name = value;
  }
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    _name: String,
}

impl User {
    fn new(name: String) -> Self {
        Self { _name: name }
    }

    fn name(&self) -> String {
        return self._name;
    }

    fn set_name(&mut self, value: String) {
        self._name = value;
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("057_getter_setter", &actual, expected);
}

// 057-6: Static method call site transformation
#[test]
fn test_snapshot_057_static_method_call_site() {
    let source = "\
class Factory {
  private value: i32;
  constructor(v: i32) {
    this.value = v;
  }
  static create(v: i32): Factory {
    return new Factory(v);
  }
}

function main() {
  let f = Factory.create(42);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Factory {
    value: i32,
}

impl Factory {
    fn new(v: i32) -> Self {
        Self { value: v }
    }

    fn create(v: i32) -> Factory {
        return Factory::new(v);
    }
}

fn main() {
    let f = Factory::create(42);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("057_static_method_call_site", &actual, expected);
}

// 057-7: Complete class with all features
#[test]
fn test_snapshot_057_complete_class_all_features() {
    let source = "\
class Counter {
  static DEFAULT_START: i32 = 0;
  private _count: i32;

  constructor(public label: string) {
    this._count = 0;
  }

  get count(): i32 {
    return this._count;
  }

  increment(): void {
    this._count = this._count + 1;
  }

  static zero(label: string): Counter {
    return new Counter(label);
  }
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Counter {
    _count: i32,
    pub label: String,
}

impl Counter {
    pub const DEFAULT_START: i32 = 0;

    fn new(label: String) -> Self {
        Self { _count: 0, label: label }
    }

    fn increment(&mut self) {
        self._count = self._count + 1;
    }

    fn zero(label: String) -> Counter {
        return Counter::new(label);
    }

    fn count(&self) -> i32 {
        return self._count;
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("057_complete_class", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 056: Spread Operator
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_array_spread_copy() {
    let source = "\
function main() {
  const arr: Array<i32> = [1, 2, 3];
  const copy: Array<i32> = [...arr];
  console.log(copy);
}";

    let expected = "\
fn main() {
    let arr: Vec<i32> = vec![1, 2, 3];
    let copy: Vec<i32> = arr.clone();
    println!(\"{}\", copy);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("056_array_spread_copy", &actual, expected);
}

#[test]
fn test_snapshot_array_spread_append() {
    let source = "\
function main() {
  const arr: Array<i32> = [1, 2];
  const result: Array<i32> = [...arr, 3, 4];
  console.log(result);
}";

    let expected = "\
fn main() {
    let arr: Vec<i32> = vec![1, 2];
    let result: Vec<i32> = {
        let mut __spread = arr.clone();
        __spread.push(3);
        __spread.push(4);
        __spread
    };
    println!(\"{}\", result);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("056_array_spread_append", &actual, expected);
}

#[test]
fn test_snapshot_array_spread_prepend() {
    let source = "\
function main() {
  const arr: Array<i32> = [3, 4];
  const result: Array<i32> = [1, 2, ...arr];
  console.log(result);
}";

    let expected = "\
fn main() {
    let arr: Vec<i32> = vec![3, 4];
    let result: Vec<i32> = {
        let mut __spread = vec![1, 2];
        __spread.extend(arr.iter().cloned());
        __spread
    };
    println!(\"{}\", result);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("056_array_spread_prepend", &actual, expected);
}

#[test]
fn test_snapshot_array_spread_multiple() {
    let source = "\
function main() {
  const a: Array<i32> = [1, 2];
  const b: Array<i32> = [3, 4];
  const combined: Array<i32> = [...a, 0, ...b];
  console.log(combined);
}";

    let expected = "\
fn main() {
    let a: Vec<i32> = vec![1, 2];
    let b: Vec<i32> = vec![3, 4];
    let combined: Vec<i32> = {
        let mut __spread = a.clone();
        __spread.push(0);
        __spread.extend(b.iter().cloned());
        __spread
    };
    println!(\"{}\", combined);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("056_array_spread_multiple", &actual, expected);
}

#[test]
fn test_snapshot_struct_spread_update() {
    let source = "\
type User = { name: string, age: u32 }

function main() {
  const user: User = { name: \"Alice\", age: 30 };
  const updated: User = { ...user, name: \"Bob\" };
  console.log(updated.name);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
}

fn main() {
    let user = User { name: \"Alice\".to_string(), age: 30 };
    let updated = User { name: \"Bob\".to_string(), ..user.clone() };
    println!(\"{}\", updated.name);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("056_struct_spread_update", &actual, expected);
}

#[test]
fn test_snapshot_struct_spread_pure_copy() {
    let source = "\
type Point = { x: f64, y: f64 }

function main() {
  const p: Point = { x: 1.0, y: 2.0 };
  const copy: Point = { ...p };
  console.log(copy.x);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq)]
struct Point {
    pub x: f64,
    pub y: f64,
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 };
    let copy = p.clone();
    println!(\"{}\", copy.x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("056_struct_spread_pure_copy", &actual, expected);
}

// ---------------------------------------------------------------------------
// 58. JSDoc Comments → Rustdoc
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_jsdoc_function_generates_doc_comments() {
    let source = "\
/**
 * Creates a new user with the given name.
 * @param name - The user's display name
 * @returns The newly created user
 */
function createUser(name: string): string {
  return name;
}";

    let expected = "\
/// Creates a new user with the given name.
///
/// # Arguments
///
/// * `name` - The user's display name
///
/// # Returns
///
/// The newly created user
fn createUser(name: String) -> String {
    return name;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("059_jsdoc_function", &actual, expected);
}

#[test]
fn test_snapshot_jsdoc_type_generates_doc_comments() {
    let source = "\
/** A point in 2D space */
type Point = {
  x: f64,
  y: f64
}";

    let expected = "\
/// A point in 2D space
#[derive(Debug, Clone, PartialEq)]
struct Point {
    pub x: f64,
    pub y: f64,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("059_jsdoc_type", &actual, expected);
}

#[test]
fn test_snapshot_jsdoc_no_comment_generates_no_doc() {
    let source = "\
function add(a: i32, b: i32): i32 {
  return a + b;
}";

    let expected = "\
fn add(a: i32, b: i32) -> i32 {
    return a + b;
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("059_jsdoc_no_comment", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 063: Logical assignment operators
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_nullish_assign_generates_is_none_check() {
    let source = "\
function main() {
  let x: i32 | null = null;
  x ??= 5;
  console.log(x);
}";

    let actual = compile_to_rust(source);
    // x ??= 5 should lower to: if x.is_none() { x = Some(5); }
    assert!(
        actual.contains("is_none()"),
        "??= should generate is_none() check. Got:\n{actual}"
    );
    assert!(
        actual.contains("Some(5)"),
        "??= should wrap value in Some(). Got:\n{actual}"
    );
}

#[test]
fn test_snapshot_or_assign_generates_negation_check() {
    let source = "\
function main() {
  let enabled: bool = false;
  enabled ||= true;
  console.log(enabled);
}";

    let actual = compile_to_rust(source);
    // enabled ||= true should lower to: if !enabled { enabled = true; }
    assert!(
        actual.contains("!enabled"),
        "||= should generate !target check. Got:\n{actual}"
    );
}

#[test]
fn test_snapshot_and_assign_generates_truthy_check() {
    let source = "\
function main() {
  let active: bool = true;
  active &&= false;
  console.log(active);
}";

    let actual = compile_to_rust(source);
    // active &&= false should lower to: if active { active = false; }
    assert!(
        actual.contains("if active"),
        "&&= should generate truthy check. Got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Task 062: Destructuring rename
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_destructure_rename() {
    let source = "\
type Point = { x: i32, y: i32 }
function main() {
  const pt: Point = { x: 10, y: 20 };
  const { x: a, y: b } = pt;
  console.log(a);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Point {
    pub x: i32,
    pub y: i32,
}

fn main() {
    let pt = Point { x: 10, y: 20 };
    let Point { x: a, y: b, .. } = pt;
    println!(\"{}\", a);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("destructure_rename", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 062: Destructuring mixed rename
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_destructure_mixed_rename() {
    let source = "\
type User = { name: string, age: u32 }
function main() {
  const user: User = { name: \"Alice\", age: 30 };
  const { name, age: a } = user;
  console.log(name);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
}

fn main() {
    let user = User { name: \"Alice\".to_string(), age: 30 };
    let User { name, age: a, .. } = user;
    println!(\"{}\", name);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("destructure_mixed_rename", &actual, expected);
}

// ---------------------------------------------------------------------------
// Test Syntax: test() blocks
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_simple_test_block_generates_cfg_test_module() {
    let source = r#"
function add(a: i32, b: i32): i32 {
  return a + b;
}

test("adds two numbers", () => {
  const result: i32 = add(2, 3);
  assert(result === 5);
});
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("#[cfg(test)]"),
        "expected #[cfg(test)] in output:\n{actual}"
    );
    assert!(
        actual.contains("mod tests {"),
        "expected mod tests in output:\n{actual}"
    );
    assert!(
        actual.contains("use super::*;"),
        "expected use super::* in output:\n{actual}"
    );
    assert!(
        actual.contains("#[test]"),
        "expected #[test] in output:\n{actual}"
    );
    assert!(
        actual.contains("fn adds_two_numbers()"),
        "expected fn adds_two_numbers() in output:\n{actual}"
    );
    assert!(
        actual.contains("assert_eq!(result, 5)"),
        "expected assert_eq!(result, 5) in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_multiple_test_blocks_in_single_module() {
    let source = r#"
test("first test", () => {
  assert(true);
});

test("second test", () => {
  assert(1 === 1);
});
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("fn first_test()"),
        "expected fn first_test() in output:\n{actual}"
    );
    assert!(
        actual.contains("fn second_test()"),
        "expected fn second_test() in output:\n{actual}"
    );
    // Should only have one test module
    let cfg_count = actual.matches("#[cfg(test)]").count();
    assert_eq!(
        cfg_count, 1,
        "expected exactly 1 #[cfg(test)], got {cfg_count}:\n{actual}"
    );
}

#[test]
fn test_snapshot_assert_eq_from_triple_equals() {
    let source = r#"
test("equality check", () => {
  const x: i32 = 5;
  assert(x === 5);
});
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("assert_eq!(x, 5)"),
        "expected assert_eq!(x, 5) in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_assert_ne_from_not_equals() {
    let source = r#"
test("inequality check", () => {
  const x: i32 = 5;
  assert(x !== 3);
});
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("assert_ne!(x, 3)"),
        "expected assert_ne!(x, 3) in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_assert_boolean_expr() {
    let source = r#"
test("boolean assert", () => {
  const flag: bool = true;
  assert(flag);
});
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("assert!(flag)"),
        "expected assert!(flag) in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_assert_comparison_passthrough() {
    let source = r#"
test("comparison assert", () => {
  const x: i32 = 10;
  assert(x > 5);
});
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("assert!(x > 5)"),
        "expected assert!(x > 5) in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_describe_it_nested_modules() {
    let source = r#"
describe("Calculator", () => {
  describe("add", () => {
    it("should add positive numbers", () => {
      assert(1 + 2 === 3);
    });

    it("should handle zero", () => {
      assert(0 + 5 === 5);
    });
  });
});
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("mod calculator {"),
        "expected mod calculator in output:\n{actual}"
    );
    assert!(
        actual.contains("mod add {"),
        "expected mod add in output:\n{actual}"
    );
    assert!(
        actual.contains("fn should_add_positive_numbers()"),
        "expected fn should_add_positive_numbers() in output:\n{actual}"
    );
    assert!(
        actual.contains("fn should_handle_zero()"),
        "expected fn should_handle_zero() in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_test_name_sanitization() {
    let source = r#"
test("should handle (edge) cases!", () => {
  assert(true);
});
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("fn should_handle_edge_cases()"),
        "expected sanitized fn name in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Standard library builtins — Math methods
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_math_floor_generates_floor_method() {
    let source = "\
function main() {
  const x: f64 = 3.7;
  console.log(Math.floor(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".floor()"),
        "expected .floor() method call in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_ceil_generates_ceil_method() {
    let source = "\
function main() {
  const x: f64 = 3.2;
  console.log(Math.ceil(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".ceil()"),
        "expected .ceil() method call in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_abs_generates_abs_method() {
    let source = "\
function main() {
  const x: f64 = -5.0;
  console.log(Math.abs(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".abs()"),
        "expected .abs() method call in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_sqrt_generates_sqrt_method() {
    let source = "\
function main() {
  const x: f64 = 16.0;
  console.log(Math.sqrt(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".sqrt()"),
        "expected .sqrt() method call in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_min_max_generates_method_calls() {
    let source = "\
function main() {
  const a: f64 = 3.0;
  const b: f64 = 5.0;
  console.log(Math.min(a, b));
  console.log(Math.max(a, b));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".min("),
        "expected .min() method call in output:\n{actual}"
    );
    assert!(
        actual.contains(".max("),
        "expected .max() method call in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_pow_generates_powf() {
    let source = "\
function main() {
  const x: f64 = 2.0;
  console.log(Math.pow(x, 3.0));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".powf("),
        "expected .powf() method call in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_trig_generates_trig_methods() {
    let source = "\
function main() {
  const x: f64 = 1.0;
  console.log(Math.sin(x));
  console.log(Math.cos(x));
  console.log(Math.tan(x));
  console.log(Math.log(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".sin()"),
        "expected .sin() in output:\n{actual}"
    );
    assert!(
        actual.contains(".cos()"),
        "expected .cos() in output:\n{actual}"
    );
    assert!(
        actual.contains(".tan()"),
        "expected .tan() in output:\n{actual}"
    );
    assert!(
        actual.contains(".ln()"),
        "expected .ln() in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_pi_generates_const() {
    let source = "\
function main() {
  console.log(Math.PI);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("std::f64::consts::PI"),
        "expected std::f64::consts::PI in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_e_generates_const() {
    let source = "\
function main() {
  console.log(Math.E);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("std::f64::consts::E"),
        "expected std::f64::consts::E in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_math_random_generates_rand_call() {
    let source = "\
function main() {
  console.log(Math.random());
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("rand::random::<f64>()"),
        "expected rand::random::<f64>() in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Standard library builtins — console extensions
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_console_error_generates_eprintln() {
    let source = r#"
function main() {
  console.error("error message");
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("eprintln!"),
        "expected eprintln! in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_console_warn_generates_eprintln_with_warning_prefix() {
    let source = r#"
function main() {
  console.warn("caution");
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("eprintln!"),
        "expected eprintln! in output:\n{actual}"
    );
    assert!(
        actual.contains("warning:"),
        "expected warning: prefix in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_console_debug_generates_eprintln_with_debug_prefix() {
    let source = r#"
function main() {
  console.debug("debug info");
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("eprintln!"),
        "expected eprintln! in output:\n{actual}"
    );
    assert!(
        actual.contains("debug:"),
        "expected debug: prefix in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Standard library builtins — Number functions
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_number_parse_int_generates_parse_chain() {
    let source = r#"
function main() {
  const x: i64 = Number.parseInt("42");
  console.log(x);
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("parse::<i64>()"),
        "expected parse::<i64>() in output:\n{actual}"
    );
    assert!(
        actual.contains("unwrap_or(0)"),
        "expected unwrap_or(0) in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_number_parse_float_generates_parse_chain() {
    let source = r#"
function main() {
  const x: f64 = Number.parseFloat("3.14");
  console.log(x);
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("parse::<f64>()"),
        "expected parse::<f64>() in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_number_is_nan_generates_is_nan() {
    let source = "\
function main() {
  const x: f64 = 0.0;
  console.log(Number.isNaN(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".is_nan()"),
        "expected .is_nan() in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_number_is_finite_generates_is_finite() {
    let source = "\
function main() {
  const x: f64 = 0.0;
  console.log(Number.isFinite(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".is_finite()"),
        "expected .is_finite() in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Standard library builtins — JSON methods
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_json_stringify_generates_serde_json() {
    let source = r#"
function main() {
  const data: string = "hello";
  console.log(JSON.stringify(data));
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("serde_json::to_string("),
        "expected serde_json::to_string in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_json_parse_generates_serde_json() {
    let source = r#"
function main() {
  const data: string = "{}";
  const parsed = JSON.parse(data);
  console.log(parsed);
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("serde_json::from_str("),
        "expected serde_json::from_str in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Standard library builtins — needs_serde_json / needs_rand flags
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_json_usage_sets_needs_serde_json_flag() {
    let source = r#"
function main() {
  const data: string = "hello";
  console.log(JSON.stringify(data));
}
"#;

    let result = test_utils::compile_result(source);
    assert!(
        result.needs_serde_json,
        "JSON.stringify should set needs_serde_json"
    );
}

#[test]
fn test_snapshot_math_random_sets_needs_rand_flag() {
    let source = "\
function main() {
  console.log(Math.random());
}";

    let result = test_utils::compile_result(source);
    assert!(result.needs_rand, "Math.random should set needs_rand");
}

#[test]
fn test_snapshot_no_json_does_not_set_serde_json_flag() {
    let source = "\
function main() {
  console.log(Math.floor(3.5));
}";

    let result = test_utils::compile_result(source);
    assert!(
        !result.needs_serde_json,
        "Math.floor should not set needs_serde_json"
    );
}

#[test]
fn test_snapshot_no_random_does_not_set_rand_flag() {
    let source = "\
function main() {
  console.log(Math.floor(3.5));
}";

    let result = test_utils::compile_result(source);
    assert!(!result.needs_rand, "Math.floor should not set needs_rand");
}

// ---------------------------------------------------------------------------
// derives keyword tests
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_type_def_with_derives_serialize_deserialize() {
    let source = "type Foo = { x: i32, name: string } derives Serialize, Deserialize";

    let expected = "\
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Foo {
    pub x: i32,
    pub name: String,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("type_def_derives_serialize", &actual, expected);
}

#[test]
fn test_snapshot_simple_enum_with_derives() {
    let source = r#"type Dir = "north" | "south" derives Serialize"#;

    let expected = "\
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
enum Dir {
    North,
    South,
}

impl std::fmt::Display for Dir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dir::North => write!(f, \"North\"),
            Dir::South => write!(f, \"South\"),
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("simple_enum_derives", &actual, expected);
}

#[test]
fn test_snapshot_derives_sets_needs_serde_flag() {
    let source = "type Foo = { x: i32 } derives Serialize";

    let result = test_utils::compile_result(source);
    assert!(
        result.needs_serde,
        "derives Serialize should set needs_serde"
    );
}

#[test]
fn test_snapshot_no_derives_does_not_set_needs_serde_flag() {
    let source = "type Foo = { x: i32 }";

    let result = test_utils::compile_result(source);
    assert!(!result.needs_serde, "no derives should not set needs_serde");
}

#[test]
fn test_snapshot_derives_deduplicates() {
    let source = "type Foo = { x: i32 } derives Debug, Clone";

    let actual = compile_to_rust(source);
    // Debug and Clone are already auto-inferred — should only appear once each
    let debug_count = actual.matches("Debug").count();
    let clone_count = actual.matches("Clone").count();
    assert_eq!(
        debug_count, 1,
        "Debug should appear only once in derives, got:\n{actual}"
    );
    assert_eq!(
        clone_count, 1,
        "Clone should appear only once in derives, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Index Signatures
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_pure_index_signature_type_alias() {
    let source = "type Config = { [key: string]: string }";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type Config = HashMap<String, String>;"),
        "expected type alias for HashMap, got:\n{actual}"
    );
    assert!(
        actual.contains("use std::collections::HashMap;"),
        "expected HashMap import, got:\n{actual}"
    );
}

#[test]
fn test_snapshot_index_signature_numeric_keys() {
    let source = "type Scores = { [id: i32]: string }";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("type Scores = HashMap<i32, String>;"),
        "expected type alias for HashMap<i32, String>, got:\n{actual}"
    );
}

#[test]
fn test_snapshot_hashmap_init_and_insert() {
    let source = r#"
function main() {
    let config: { [key: string]: string } = {};
    config["debug"] = "true";
    config["verbose"] = "false";
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("HashMap::new()"),
        "expected HashMap::new() init, got:\n{actual}"
    );
    assert!(
        actual.contains(".insert("),
        "expected .insert() call for index assignment, got:\n{actual}"
    );
}

#[test]
fn test_snapshot_hashmap_index_read() {
    let source = r#"
function main() {
    let config: { [key: string]: string } = {};
    const val = config["debug"];
}
"#;

    let actual = compile_to_rust(source);
    // HashMap index access: config["debug".to_string()] is valid Rust
    // because HashMap<String, V> accepts String keys via Index trait
    assert!(
        actual.contains("config["),
        "expected index access on HashMap, got:\n{actual}"
    );
}

#[test]
fn test_snapshot_inline_index_signature_param() {
    let source = r#"
function getConfig(settings: { [key: string]: string }): string {
    return settings["key"];
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("HashMap<String, String>"),
        "expected HashMap<String, String> param type, got:\n{actual}"
    );
}

// ===========================================================================
// Do-While Loop Tests (Task 109)
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. Basic do-while → correct loop + break pattern
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_do_while_basic_generates_loop_break() {
    let source = "\
function main() {
  let x: i32 = 0;
  do {
    x += 1;
  } while (x < 10);
  console.log(x);
}";

    let expected = "\
fn main() {
    let mut x: i32 = 0;
    loop {
        x += 1;
        if !(x < 10) {
            break;
        }
    }
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("do_while_basic", &actual, expected);
}

// ---------------------------------------------------------------------------
// 2. Do-while with compound condition (&&, ||)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_do_while_compound_condition_generates_loop_break() {
    let source = "\
function main() {
  let x: i32 = 0;
  let y: i32 = 100;
  do {
    x += 1;
    y -= 1;
  } while (x < 10 && y > 50);
}";

    let expected = "\
fn main() {
    let mut x: i32 = 0;
    let mut y: i32 = 100;
    loop {
        x += 1;
        y -= 1;
        if !(x < 10 && y > 50) {
            break;
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("do_while_compound", &actual, expected);
}

// ---------------------------------------------------------------------------
// 3. Do-while with break inside body
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_do_while_with_break_generates_loop_break() {
    let source = "\
function main() {
  let x: i32 = 0;
  do {
    x += 1;
    if (x == 5) {
      break;
    }
  } while (x < 10);
}";

    let expected = "\
fn main() {
    let mut x: i32 = 0;
    loop {
        x += 1;
        if x == 5 {
            break;
        }
        if !(x < 10) {
            break;
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("do_while_break", &actual, expected);
}

// ---------------------------------------------------------------------------
// 4. Do-while with continue inside body
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_do_while_with_continue_generates_loop_break() {
    let source = "\
function main() {
  let x: i32 = 0;
  do {
    x += 1;
    if (x == 5) {
      continue;
    }
    console.log(x);
  } while (x < 10);
}";

    let expected = "\
fn main() {
    let mut x: i32 = 0;
    loop {
        x += 1;
        if x == 5 {
            continue;
        }
        println!(\"{}\", x);
        if !(x < 10) {
            break;
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("do_while_continue", &actual, expected);
}

// ---------------------------------------------------------------------------
// 5. Nested do-while loops
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_do_while_nested_generates_nested_loops() {
    let source = "\
function main() {
  let i: i32 = 0;
  do {
    let j: i32 = 0;
    do {
      j += 1;
    } while (j < 3);
    i += 1;
  } while (i < 2);
}";

    let expected = "\
fn main() {
    let mut i: i32 = 0;
    loop {
        let mut j: i32 = 0;
        loop {
            j += 1;
            if !(j < 3) {
                break;
            }
        }
        i += 1;
        if !(i < 2) {
            break;
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("do_while_nested", &actual, expected);
}

// ---------------------------------------------------------------------------
// 6. Do-while with or condition
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_do_while_or_condition_generates_loop_break() {
    let source = "\
function main() {
  let x: i32 = 0;
  do {
    x += 1;
  } while (x < 5 || x == 7);
}";

    let expected = "\
fn main() {
    let mut x: i32 = 0;
    loop {
        x += 1;
        if !(x < 5 || x == 7) {
            break;
        }
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("do_while_or_condition", &actual, expected);
}

// ---------------------------------------------------------------------------
// T110-1. For-in loop on Map iterates keys via .keys()
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_t110_for_in_map_keys() {
    let source = r#"
function main() {
  const map: Map<string, i32> = new Map();
  for (const k in map) {
    console.log(k);
  }
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".keys()"),
        "expected .keys() call for for-in loop, got:\n{actual}"
    );
    assert!(
        actual.contains("for k in map.keys()"),
        "expected `for k in map.keys()`, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// T110-2. For-in vs for-of produce different output for the same collection
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_t110_for_in_vs_for_of_different_output() {
    let source_for_of = r#"
function main() {
  const items: Array<i32> = [1, 2, 3];
  for (const n of items) {
    console.log(n);
  }
}
"#;

    let source_for_in = r#"
function main() {
  const items: Map<string, i32> = new Map();
  for (const k in items) {
    console.log(k);
  }
}
"#;

    let actual_of = compile_to_rust(source_for_of);
    let actual_in = compile_to_rust(source_for_in);

    // for-of should use `for &n in &items` (value iteration)
    assert!(
        actual_of.contains("for &n in &items"),
        "for-of should iterate values, got:\n{actual_of}"
    );

    // for-in should use `.keys()` (key iteration)
    assert!(
        actual_in.contains(".keys()"),
        "for-in should use .keys(), got:\n{actual_in}"
    );
}

// ---------------------------------------------------------------------------
// Task 138: Global isNaN / isFinite + Number.isSafeInteger + constants
// ---------------------------------------------------------------------------

#[test]
fn test_is_nan_snapshot() {
    let source = "\
function main() {
  const x: f64 = 0.0;
  console.log(isNaN(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".is_nan()"),
        "expected .is_nan() in output:\n{actual}"
    );
}

#[test]
fn test_is_finite_snapshot() {
    let source = "\
function main() {
  const x: f64 = 0.0;
  console.log(isFinite(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".is_finite()"),
        "expected .is_finite() in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_number_is_safe_integer() {
    let source = "\
function main() {
  const x: f64 = 42.0;
  console.log(Number.isSafeInteger(x));
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".is_finite()"),
        "expected .is_finite() in isSafeInteger output:\n{actual}"
    );
    assert!(
        actual.contains(".abs()"),
        "expected .abs() in isSafeInteger output:\n{actual}"
    );
}

#[test]
fn test_snapshot_number_max_safe_integer() {
    let source = "\
function main() {
  const x: i64 = Number.MAX_SAFE_INTEGER;
  console.log(x);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("9007199254740991"),
        "expected 9007199254740991 in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_number_min_safe_integer() {
    let source = "\
function main() {
  const x: i64 = Number.MIN_SAFE_INTEGER;
  console.log(x);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("-9007199254740991"),
        "expected -9007199254740991 in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Object static methods
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_object_keys_generates_keys_collect() {
    let source = r#"
function main() {
  const m: Map<string, i32> = new Map();
  const k = Object.keys(m);
  console.log(k);
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".keys().cloned().collect::<Vec<_>>()"),
        "Object.keys should generate .keys().cloned().collect::<Vec<_>>(), got:\n{actual}"
    );
}

#[test]
fn test_snapshot_object_values_generates_values_collect() {
    let source = r#"
function main() {
  const m: Map<string, i32> = new Map();
  const v = Object.values(m);
  console.log(v);
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".values().cloned().collect::<Vec<_>>()"),
        "Object.values should generate .values().cloned().collect::<Vec<_>>(), got:\n{actual}"
    );
}

#[test]
fn test_snapshot_object_entries_generates_iter_collect() {
    let source = r#"
function main() {
  const m: Map<string, i32> = new Map();
  const e = Object.entries(m);
  console.log(e);
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".iter().map("),
        "Object.entries should generate .iter().map(...), got:\n{actual}"
    );
    assert!(
        actual.contains(".collect::<Vec<_>>()"),
        "Object.entries should generate .collect::<Vec<_>>(), got:\n{actual}"
    );
}

#[test]
fn test_snapshot_object_from_entries_generates_into_iter_collect() {
    let source = r#"
function main() {
  const pairs: Array<[string, i32]> = [];
  const m = Object.fromEntries(pairs);
  console.log(m);
}
"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".into_iter().collect::<HashMap<_, _>>()"),
        "Object.fromEntries should generate .into_iter().collect::<HashMap<_, _>>(), got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// using / await using declarations
// ---------------------------------------------------------------------------

#[test]
fn test_using_lowers_to_let() {
    let source = r#"
function main() {
  using file = openFile("data.txt");
  console.log(file);
}
"#;
    let actual = compile_to_rust(source);
    // `using` should lower to a normal `let` binding — Rust RAII handles Drop
    assert!(
        actual.contains("let file = openFile("),
        "using should lower to `let` binding, got:\n{actual}"
    );
}

#[test]
fn test_await_using_lowers_to_let() {
    let source = r#"
async function main() {
  await using conn = getDbConnection();
  console.log(conn);
}
"#;
    let actual = compile_to_rust(source);
    // `await using` should also lower to a normal `let` binding
    assert!(
        actual.contains("let conn = getDbConnection("),
        "await using should lower to `let` binding, got:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// Overload signatures
// ---------------------------------------------------------------------------

#[test]
fn test_overload_only_implementation_emitted() {
    let source = r#"
function greet(name: string): string;
function greet(name: string, greeting: string): string;
function greet(name: string, greeting: Option<string>): string {
  return "Hello " + name;
}

function main() {
  console.log(greet("World", None));
}
"#;

    let actual = compile_to_rust(source);
    // The overload signatures should be erased — only one `fn greet` in output
    let fn_greet_count = actual.matches("fn greet").count();
    assert_eq!(
        fn_greet_count, 1,
        "expected exactly one `fn greet` in output, got {fn_greet_count}:\n{actual}"
    );
    // The implementation body should be present
    assert!(
        actual.contains("fn greet("),
        "expected implementation function in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// declare ambient declarations produce no output (Task 150)
// ---------------------------------------------------------------------------

#[test]
fn test_declare_produces_no_output() {
    let source = "\
declare function fetch(url: string): void;
declare const API_KEY: string;

function main() {
  console.log(\"hello\");
}";

    let actual = compile_to_rust(source);
    assert!(
        !actual.contains("fetch"),
        "declared function should not appear in output, got:\n{actual}"
    );
    assert!(
        !actual.contains("API_KEY"),
        "declared const should not appear in output, got:\n{actual}"
    );
    assert!(
        actual.contains("fn main()"),
        "non-declared function should still appear in output, got:\n{actual}"
    );
}

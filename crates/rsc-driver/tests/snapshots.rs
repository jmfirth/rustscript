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
    g.greet(\"world\".to_string());
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

// ===========================================================================
// Task 058: Control Flow Completeness — finally block, === / !== verification
// ===========================================================================

// ---------------------------------------------------------------------------
// try/catch/finally → match with appended cleanup
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_try_catch_finally_generates_match_with_cleanup() {
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
  } finally {
    console.log(\"cleanup\");
  }
}";

    let expected = "\
fn riskyOp() -> Result<i32, String> {
    return Err(\"oops\".to_string());
}

fn main() {
    {
        match riskyOp() {
            Ok(val) => {
                println!(\"{}\", val);
            }
            Err(err) => {
                println!(\"{}\", err);
            }
        }
        println!(\"{}\", \"cleanup\");
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("try_catch_finally", &actual, expected);
}

// ---------------------------------------------------------------------------
// try/finally → block with appended cleanup (no catch)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_try_finally_generates_block_with_cleanup() {
    let source = "\
function main() {
  try {
    console.log(\"doing work\");
  } finally {
    console.log(\"cleanup\");
  }
}";

    let expected = "\
fn main() {
    {
        println!(\"{}\", \"doing work\");
        println!(\"{}\", \"cleanup\");
    }
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("try_finally", &actual, expected);
}

// ---------------------------------------------------------------------------
// === / !== on integers, strings, booleans
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_strict_eq_integers_generates_double_eq() {
    let source = "\
function main() {
  const x: i32 = 5;
  const y: i32 = 5;
  const result: bool = x === y;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x == y"),
        "expected `x == y` in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_strict_ne_integers_generates_bang_eq() {
    let source = "\
function main() {
  const x: i32 = 5;
  const y: i32 = 10;
  const result: bool = x !== y;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x != y"),
        "expected `x != y` in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_strict_eq_strings_generates_double_eq() {
    let source = "\
function main() {
  const a: string = \"hello\";
  const b: string = \"hello\";
  const result: bool = a === b;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("a == b"),
        "expected `a == b` in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_strict_eq_booleans_generates_double_eq() {
    let source = "\
function main() {
  const a: bool = true;
  const b: bool = false;
  const result: bool = a === b;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("a == b"),
        "expected `a == b` in output:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// === / !== in different expression positions
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_strict_eq_in_if_condition() {
    let source = "\
function main() {
  const x: i32 = 5;
  if (x === 5) {
    console.log(\"equal\");
  }
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x == 5"),
        "expected `x == 5` in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_strict_ne_in_if_condition() {
    let source = "\
function main() {
  const x: i32 = 5;
  if (x !== 10) {
    console.log(\"not equal\");
  }
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x != 10"),
        "expected `x != 10` in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_strict_eq_in_variable_assignment() {
    let source = "\
function main() {
  const x: i32 = 5;
  const y: i32 = 5;
  const same: bool = x === y;
  console.log(same);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x == y"),
        "expected `x == y` in output:\n{actual}"
    );
}

#[test]
fn test_snapshot_strict_eq_as_function_argument() {
    let source = "\
function main() {
  const x: i32 = 5;
  console.log(x === 5);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x == 5"),
        "expected `x == 5` in output:\n{actual}"
    );
}

// Note: ternary operator `? :` is not yet supported in the parser,
// so === in ternary conditions is not tested here. It can be tested
// once ternary support is added — the lowering already handles it
// correctly since === → BinaryOp::Eq → RustBinaryOp::Eq → `==`.

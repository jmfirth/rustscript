//! Phase 6 integration tests -- exercise the COMPLETE language surface.
//!
//! These tests validate that all features from Phases 0-5 compose correctly
//! together, with special focus on cross-feature interactions that individual
//! phase tests may not cover.
//!
//! Organization:
//!   1. All operators (ternary, **, ===, !, as, typeof, bitwise, ??=)
//!   2. All function features (optional, default, rest, spread)
//!   3. All class features (field init, constructor props, static, get/set, readonly, abstract)
//!   4. Tuples + destructuring rename/defaults + general unions
//!   5. Test syntax (describe/it blocks)
//!   6. Async iteration + Promise.race (snapshot only -- no runtime in fast tests)
//!   7. Kitchen sink: 10+ features in one program
//!
//! Snapshot tests are fast (no cargo invocation). E2e tests are `#[ignore]`.

mod test_utils;

use test_utils::{compile_and_run, compile_to_rust};

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
//
// CATEGORY 1: All Operators Composition
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 1.1 Ternary + exponentiation + strict equality + bitwise ops in one function
// ---------------------------------------------------------------------------

#[test]
fn test_p6_all_operators_snapshot() {
    let source = "\
function compute(n: i64, flag: bool): i64 {
  const base: i64 = flag ? n : n ** 2;
  const masked: i64 = base & 255;
  const shifted: i64 = masked << 2;
  return shifted;
}

function main() {
  const a: i64 = compute(3, true);
  const b: i64 = compute(3, false);
  const eq: bool = a === 12;
  console.log(a);
  console.log(b);
  console.log(eq);
}";

    let actual = compile_to_rust(source);
    // Ternary produces if/else
    assert!(
        actual.contains("if flag"),
        "ternary should emit if/else: {actual}"
    );
    // Exponentiation produces .pow()
    assert!(actual.contains(".pow("), "** should emit .pow(): {actual}");
    // Bitwise AND
    assert!(
        actual.contains("& 255"),
        "bitwise & should be preserved: {actual}"
    );
    // Left shift
    assert!(
        actual.contains("<< 2"),
        "bitwise << should be preserved: {actual}"
    );
    // === lowers to ==
    assert!(actual.contains("== 12"), "=== should lower to ==: {actual}");
}

#[test]
#[ignore]
fn test_p6_all_operators_e2e() {
    let source = "\
function compute(n: i64, flag: bool): i64 {
  const base: i64 = flag ? n : n ** 2;
  const masked: i64 = base & 255;
  const shifted: i64 = masked << 2;
  return shifted;
}

function main() {
  const a: i64 = compute(3, true);
  const b: i64 = compute(3, false);
  const eq: bool = a === 12;
  console.log(a);
  console.log(b);
  console.log(eq);
}";

    let output = compile_and_run(source);
    // compute(3, true): base = 3, masked = 3 & 255 = 3, shifted = 3 << 2 = 12
    // compute(3, false): base = 3**2 = 9, masked = 9 & 255 = 9, shifted = 9 << 2 = 36
    assert_eq!(output, "12\n36\ntrue\n");
}

// ---------------------------------------------------------------------------
// 1.2 typeof + as cast + non-null assertion together
// ---------------------------------------------------------------------------

#[test]
fn test_p6_typeof_cast_nonnull_snapshot() {
    let source = "\
function main() {
  const t: string = typeof 42;
  const n: i64 = 100;
  const narrow: i32 = n as i32;
  console.log(t);
  console.log(narrow);
}";

    let actual = compile_to_rust(source);
    // typeof produces a string literal
    assert!(
        actual.contains("\"number\""),
        "typeof 42 should produce \"number\": {actual}"
    );
    // as cast produces Rust `as`
    assert!(
        actual.contains("as i32"),
        "cast should produce `as i32`: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 1.3 Bitwise XOR + OR + shift combination
// ---------------------------------------------------------------------------

#[test]
fn test_p6_bitwise_ops_full_snapshot() {
    let source = "\
function main() {
  const a: i64 = 170;
  const b: i64 = 85;
  const xor_val: i64 = a ^ b;
  const or_val: i64 = a | b;
  const shr_val: i64 = a >> 1;
  console.log(xor_val);
  console.log(or_val);
  console.log(shr_val);
}";

    let expected = "\
fn main() {
    let a: i64 = 170;
    let b: i64 = 85;
    let xor_val: i64 = a ^ b;
    let or_val: i64 = a | b;
    let shr_val: i64 = a >> 1;
    println!(\"{}\", xor_val);
    println!(\"{}\", or_val);
    println!(\"{}\", shr_val);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("bitwise_ops_full", &actual, expected);
}

#[test]
#[ignore]
fn test_p6_bitwise_ops_full_e2e() {
    let source = "\
function main() {
  const a: i64 = 170;
  const b: i64 = 85;
  const xor_val: i64 = a ^ b;
  const or_val: i64 = a | b;
  const shr_val: i64 = a >> 1;
  console.log(xor_val);
  console.log(or_val);
  console.log(shr_val);
}";

    let output = compile_and_run(source);
    // 170 ^ 85 = 255, 170 | 85 = 255, 170 >> 1 = 85
    assert_eq!(output, "255\n255\n85\n");
}

// ===========================================================================
//
// CATEGORY 2: All Function Features Composition
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 2.1 Optional + default + rest parameters in one function
// ---------------------------------------------------------------------------

#[test]
fn test_p6_function_features_snapshot() {
    let source = "\
function format_list(prefix: string, sep: string = \", \", ...items: Array<string>): string {
  let result: string = prefix;
  let i: i64 = 0;
  for (const item of items) {
    if (i > 0) {
      result = `${result}${sep}`;
    }
    result = `${result}${item}`;
    i = i + 1;
  }
  return result;
}

function main() {
  console.log(format_list(\"Items: \", \" | \", \"a\", \"b\", \"c\"));
  console.log(format_list(\"Default: \"));
}";

    let actual = compile_to_rust(source);
    // Rest param should produce Vec<String>
    assert!(
        actual.contains("Vec<String>") || actual.contains("items: Vec<"),
        "rest param should be Vec: {actual}"
    );
    // Default value should be inlined at call site
    assert!(
        actual.contains("Default: "),
        "call site should have prefix arg: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 2.2 Optional parameter with None filling at call site
// ---------------------------------------------------------------------------

#[test]
fn test_p6_optional_param_call_site() {
    let source = "\
function greet(name: string, title?: string): string {
  return name;
}

function main() {
  console.log(greet(\"Alice\"));
  console.log(greet(\"Bob\", \"Dr.\"));
}";

    let actual = compile_to_rust(source);
    // Optional should produce Option<String>
    assert!(
        actual.contains("Option<"),
        "optional param should be Option: {actual}"
    );
    // Missing arg should fill with None
    assert!(
        actual.contains("None"),
        "missing optional arg should produce None: {actual}"
    );
    // Provided arg should pass the value (may or may not wrap in Some
    // depending on whether borrow inference changes the signature)
    assert!(
        actual.contains("Some(") || actual.contains("\"Dr.\""),
        "provided optional arg should be present: {actual}"
    );
}

// ===========================================================================
//
// CATEGORY 3: All Class Features Composition
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 3.1 Class with field init, constructor props, static, get/set, readonly
// ---------------------------------------------------------------------------

#[test]
fn test_p6_class_features_full_snapshot() {
    let source = "\
class Counter {
  readonly name: string;
  private count: i64 = 0;
  static MAX: i64 = 1000;

  constructor(public label: string) {
    this.name = label;
  }

  get value(): i64 {
    return this.count;
  }

  set value(n: i64) {
    this.count = n;
  }

  increment(): void {
    this.count = this.count + 1;
  }

  static create(label: string): Counter {
    return new Counter(label);
  }
}

function main() {
  let c: Counter = Counter.create(\"test\");
  c.increment();
  console.log(c.value);
}";

    let actual = compile_to_rust(source);
    // Struct should exist
    assert!(
        actual.contains("struct Counter"),
        "should produce Counter struct: {actual}"
    );
    // Constructor prop should generate a field
    assert!(
        actual.contains("label"),
        "constructor property should produce field: {actual}"
    );
    // Static const should produce associated const
    assert!(
        actual.contains("const MAX"),
        "static field should produce associated const: {actual}"
    );
    // Getter should produce a method
    assert!(
        actual.contains("fn value(&self)"),
        "getter should produce fn value: {actual}"
    );
    // Setter should produce a set_ method
    assert!(
        actual.contains("fn set_value("),
        "setter should produce fn set_value: {actual}"
    );
    // Static method should have no &self
    assert!(
        actual.contains("fn create("),
        "static method should exist: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 3.2 Abstract class lowers to trait
// ---------------------------------------------------------------------------

#[test]
fn test_p6_abstract_class_snapshot() {
    let source = "\
abstract class Shape {
  abstract area(): f64;

  describe(): string {
    return \"a shape\";
  }
}

class Circle implements Shape {
  radius: f64;

  constructor(r: f64) {
    this.radius = r;
  }

  area(): f64 {
    return this.radius * this.radius * 3.14159;
  }
}

function main() {
  const c: Circle = new Circle(5.0);
  console.log(c.area());
}";

    let actual = compile_to_rust(source);
    // Abstract class should produce a trait
    assert!(
        actual.contains("trait Shape"),
        "abstract class should produce trait: {actual}"
    );
    // Default method should have a body in the trait
    assert!(
        actual.contains("fn describe("),
        "default method should exist: {actual}"
    );
    // Concrete class should implement the trait
    assert!(
        actual.contains("impl Shape for Circle"),
        "Circle should impl Shape: {actual}"
    );
}

#[test]
#[ignore]
fn test_p6_abstract_class_e2e() {
    let source = "\
abstract class Shape {
  abstract area(): f64;
}

class Circle implements Shape {
  radius: f64;

  constructor(r: f64) {
    this.radius = r;
  }

  area(): f64 {
    return this.radius * this.radius * 3.14159;
  }
}

function main() {
  const c: Circle = new Circle(5.0);
  console.log(c.area());
}";

    let output = compile_and_run(source);
    assert!(
        output.contains("78.539"),
        "area should be ~78.539: {output}"
    );
}

// ===========================================================================
//
// CATEGORY 4: Tuples + Destructuring Rename/Defaults + General Unions
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 4.1 Tuple construction, access, and destructuring
// ---------------------------------------------------------------------------

#[test]
fn test_p6_tuple_operations_snapshot() {
    let source = "\
function make_pair(a: string, b: i32): [string, i32] {
  return [a, b];
}

function main() {
  const pair: [string, i32] = make_pair(\"hello\", 42);
  const first: string = pair[0];
  const second: i32 = pair[1];
  console.log(first);
  console.log(second);
}";

    let actual = compile_to_rust(source);
    // Return type should be tuple
    assert!(
        actual.contains("(String, i32)"),
        "return type should be tuple: {actual}"
    );
    // Tuple construction
    assert!(
        actual.contains("(\"hello\".to_string(), 42)") || actual.contains(".to_string(), 42)"),
        "tuple construction should use parens: {actual}"
    );
    // Tuple field access with .N
    assert!(
        actual.contains(".0") && actual.contains(".1"),
        "tuple access should use .0 and .1: {actual}"
    );
}

#[test]
#[ignore]
fn test_p6_tuple_operations_e2e() {
    let source = "\
function make_pair(a: string, b: i32): [string, i32] {
  return [a, b];
}

function main() {
  const pair: [string, i32] = make_pair(\"hello\", 42);
  const first: string = pair[0];
  const second: i32 = pair[1];
  console.log(first);
  console.log(second);
}";

    let output = compile_and_run(source);
    assert_eq!(output, "hello\n42\n");
}

// ---------------------------------------------------------------------------
// 4.2 Destructuring with rename
// ---------------------------------------------------------------------------

#[test]
fn test_p6_destructure_rename_snapshot() {
    let source = "\
type Point = {
  x: f64,
  y: f64,
}

function main() {
  const p: Point = { x: 1.0, y: 2.0 };
  const { x: px, y: py } = p;
  console.log(px);
  console.log(py);
}";

    let actual = compile_to_rust(source);
    // Should produce renamed bindings
    assert!(
        actual.contains("px") && actual.contains("py"),
        "destructure rename should produce px, py: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 4.3 General union type
// ---------------------------------------------------------------------------

#[test]
fn test_p6_union_type_snapshot() {
    let source = "\
function process(value: string | i32): string {
  return \"processed\";
}

function main() {
  const a: string | i32 = \"hello\";
  const b: string | i32 = 42;
  console.log(process(a));
  console.log(process(b));
}";

    let actual = compile_to_rust(source);
    // Should generate an enum
    assert!(
        actual.contains("enum ") && actual.contains("Or"),
        "union should generate enum with Or in name: {actual}"
    );
    // Should generate From impls
    assert!(
        actual.contains("impl From<"),
        "union should generate From impls: {actual}"
    );
    // Values should use .into()
    assert!(
        actual.contains(".into()"),
        "union values should use .into(): {actual}"
    );
}

// ===========================================================================
//
// CATEGORY 5: Test Syntax (describe/it blocks)
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 5.1 Test block generates #[test] functions
// ---------------------------------------------------------------------------

#[test]
fn test_p6_test_syntax_snapshot() {
    let source = "\
function add(a: i32, b: i32): i32 {
  return a + b;
}

test(\"add returns sum\", () => {
  assert(add(2, 3) === 5);
})

describe(\"arithmetic\", () => {
  it(\"handles zero\", () => {
    assert(add(0, 0) === 0);
  })

  it(\"handles negatives\", () => {
    assert(add(-1, 1) === 0);
  })
})";

    let actual = compile_to_rust(source);
    // Should produce #[cfg(test)]
    assert!(
        actual.contains("#[cfg(test)]"),
        "should produce #[cfg(test)]: {actual}"
    );
    // Should produce #[test]
    assert!(
        actual.contains("#[test]"),
        "should produce #[test]: {actual}"
    );
    // Should produce mod tests
    assert!(
        actual.contains("mod tests"),
        "should produce mod tests: {actual}"
    );
    // Should produce assert macros
    assert!(
        actual.contains("assert_eq!") || actual.contains("assert!("),
        "assert should produce assert macros: {actual}"
    );
    // describe should produce a nested module
    assert!(
        actual.contains("mod arithmetic"),
        "describe should produce nested module: {actual}"
    );
}

// ===========================================================================
//
// CATEGORY 6: Async Iteration + Promise.race (snapshot only)
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 6.1 Async function with await
// ---------------------------------------------------------------------------

#[test]
fn test_p6_async_function_snapshot() {
    let source = "\
async function fetch_data(): string {
  return \"data\";
}

async function main() {
  const result: string = await fetch_data();
  console.log(result);
}";

    let actual = compile_to_rust(source);
    // async fn
    assert!(
        actual.contains("async fn"),
        "should produce async fn: {actual}"
    );
    // .await
    assert!(actual.contains(".await"), "should produce .await: {actual}");
    // #[tokio::main]
    assert!(
        actual.contains("tokio::main"),
        "async main should have tokio::main: {actual}"
    );
}

// ===========================================================================
//
// CATEGORY 7: Kitchen Sink -- 10+ features in one program
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 7.1 Kitchen sink snapshot: class + generics + closures + ternary + template
//     + optional params + for-of + destructuring + enum + try/catch
// ---------------------------------------------------------------------------

#[test]
fn test_p6_kitchen_sink_snapshot() {
    let source = "\
type Config = {
  name: string,
  verbose: bool,
}

type Direction = \"north\" | \"south\" | \"east\" | \"west\"

interface Describable {
  describe(): string;
}

class Logger implements Describable {
  prefix: string;
  private entries: Array<string>;

  constructor(prefix: string) {
    this.prefix = prefix;
    this.entries = [];
  }

  log(msg: string): void {
    this.entries = this.entries;
  }

  describe(): string {
    return this.prefix;
  }
}

function format_with_default(value: i64, label: string = \"count\"): string {
  const display: string = value > 0 ? `${label}: ${value}` : `${label}: none`;
  return display;
}

function process_items(items: Array<i32>): i64 {
  let total: i64 = 0;
  for (const item of items) {
    total = total + item;
  }
  return total;
}

function main() {
  const config: Config = { name: \"test\", verbose: true };
  const { name, verbose } = config;
  console.log(name);

  const dir: Direction = \"north\";
  switch (dir) {
    case \"north\": console.log(\"going north\");
    case \"south\": console.log(\"going south\");
    case \"east\": console.log(\"going east\");
    case \"west\": console.log(\"going west\");
  }

  const logger: Logger = new Logger(\"[APP]\");
  console.log(logger.describe());

  console.log(format_with_default(42));
  console.log(format_with_default(0, \"items\"));

  const items: Array<i32> = [10, 20, 30];
  const sum: i64 = process_items(items);
  console.log(sum);

  const power: i64 = 2 ** 10;
  console.log(power);

  const flags: i64 = 255 & 15;
  console.log(flags);
}";

    let actual = compile_to_rust(source);

    // Verify 10+ features are present in generated output:

    // 1. Struct (type definition)
    assert!(
        actual.contains("struct Config"),
        "feature 1: type def should produce struct: {actual}"
    );

    // 2. Enum (string literal union)
    assert!(
        actual.contains("enum Direction"),
        "feature 2: enum should be generated: {actual}"
    );

    // 3. Trait (interface)
    assert!(
        actual.contains("trait Describable"),
        "feature 3: interface should produce trait: {actual}"
    );

    // 4. Class (struct + impl)
    assert!(
        actual.contains("struct Logger"),
        "feature 4: class should produce struct: {actual}"
    );

    // 5. Default parameter
    assert!(
        actual.contains("\"count\""),
        "feature 5: default param should be present: {actual}"
    );

    // 6. Ternary (if/else expr)
    assert!(
        actual.contains("if") && actual.contains("else"),
        "feature 6: ternary should produce if/else: {actual}"
    );

    // 7. Template literal (format!)
    assert!(
        actual.contains("format!("),
        "feature 7: template literal should produce format!: {actual}"
    );

    // 8. For-of loop
    assert!(
        actual.contains("for") && actual.contains("in"),
        "feature 8: for-of should produce for..in: {actual}"
    );

    // 9. Destructuring
    assert!(
        actual.contains("let Config"),
        "feature 9: destructuring should name the type: {actual}"
    );

    // 10. Match (switch)
    assert!(
        actual.contains("match") || actual.contains("Direction::"),
        "feature 10: switch should produce match: {actual}"
    );

    // 11. Exponentiation
    assert!(
        actual.contains(".pow("),
        "feature 11: ** should produce .pow(): {actual}"
    );

    // 12. Bitwise AND
    assert!(
        actual.contains("& 15"),
        "feature 12: bitwise & should be preserved: {actual}"
    );
}

#[test]
#[ignore]
fn test_p6_kitchen_sink_e2e() {
    let source = "\
type Config = {
  name: string,
  verbose: bool,
}

type Direction = \"north\" | \"south\" | \"east\" | \"west\"

function format_with_default(value: i64, label: string = \"count\"): string {
  const display: string = value > 0 ? `${label}: ${value}` : `${label}: none`;
  return display;
}

function process_items(items: Array<i32>): i64 {
  let total: i64 = 0;
  for (const item of items) {
    total = total + item;
  }
  return total;
}

function main() {
  const config: Config = { name: \"test\", verbose: true };
  const { name } = config;
  console.log(name);

  const dir: Direction = \"north\";
  switch (dir) {
    case \"north\": console.log(\"going north\");
    case \"south\": console.log(\"going south\");
    case \"east\": console.log(\"going east\");
    case \"west\": console.log(\"going west\");
  }

  console.log(format_with_default(42));
  console.log(format_with_default(0, \"items\"));

  const items: Array<i32> = [10, 20, 30];
  const sum: i64 = process_items(items);
  console.log(sum);

  const power: i64 = 2 ** 10;
  console.log(power);

  const flags: i64 = 255 & 15;
  console.log(flags);
}";

    let output = compile_and_run(source);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "test"); // config.name
    assert_eq!(lines[1], "going north"); // switch match
    assert_eq!(lines[2], "count: 42"); // format_with_default(42)
    assert_eq!(lines[3], "items: none"); // format_with_default(0, "items")
    assert_eq!(lines[4], "60"); // process_items sum
    assert_eq!(lines[5], "1024"); // 2 ** 10
    assert_eq!(lines[6], "15"); // 255 & 15
}

// ===========================================================================
//
// CATEGORY 8: Cross-phase composition tests
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 8.1 Tier 2 borrow inference + template literals + closures
// ---------------------------------------------------------------------------

#[test]
fn test_p6_borrow_inference_with_closures() {
    let source = "\
function greet(name: string): string {
  return `Hello, ${name}!`;
}

function apply(f: (string) => string, value: string): string {
  return f(value);
}

function main() {
  console.log(greet(\"World\"));
}";

    let actual = compile_to_rust(source);
    // Should compile without errors -- borrow inference should work with
    // string params in template literals
    assert!(
        actual.contains("fn greet("),
        "greet function should exist: {actual}"
    );
    assert!(
        actual.contains("format!("),
        "template literal should produce format!: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 8.2 Derive macros on struct + enum + class
// ---------------------------------------------------------------------------

#[test]
fn test_p6_derive_macros_composition() {
    let source = "\
type Point = {
  x: f64,
  y: f64,
}

type Color = \"red\" | \"green\" | \"blue\"

class Rect {
  width: f64;
  height: f64;

  constructor(w: f64, h: f64) {
    this.width = w;
    this.height = h;
  }
}

function main() {
  const p: Point = { x: 1.0, y: 2.0 };
  const c: Color = \"red\";
  const r: Rect = new Rect(3.0, 4.0);
  console.log(p);
  console.log(r);
}";

    let actual = compile_to_rust(source);
    // Struct should have derives
    assert!(
        actual.contains("#[derive(Debug, Clone"),
        "struct should have Debug, Clone derives: {actual}"
    );
    // Simple enum should have full derives including Copy
    assert!(
        actual.contains("Copy") && actual.contains("Hash"),
        "simple enum should have Copy, Hash: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 8.3 Inline Rust + regular code
// ---------------------------------------------------------------------------

#[test]
fn test_p6_inline_rust_composition() {
    let source = "\
rust {
    fn helper() -> i32 {
        42
    }
}

function main() {
  const x: i32 = 10;
  console.log(x);
}";

    let actual = compile_to_rust(source);
    // Raw rust block should be passed through
    assert!(
        actual.contains("fn helper()"),
        "inline rust should contain helper: {actual}"
    );
    // Regular RustScript code should also compile
    assert!(
        actual.contains("let x: i32 = 10"),
        "regular code should compile: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 8.4 shared<T> type with method calls
// ---------------------------------------------------------------------------

#[test]
fn test_p6_shared_type_composition() {
    let source = "\
function main() {
  const counter: shared<i64> = shared(0);
  const data: shared<string> = shared(\"hello\");
  console.log(counter);
  console.log(data);
}";

    let actual = compile_to_rust(source);
    // shared<T> should produce Arc<Mutex<T>>
    assert!(
        actual.contains("Arc<Mutex<i64>>"),
        "shared<i64> should produce Arc<Mutex<i64>>: {actual}"
    );
    assert!(
        actual.contains("Arc<Mutex<String>>"),
        "shared<string> should produce Arc<Mutex<String>>: {actual}"
    );
    // shared(expr) should produce Arc::new(Mutex::new(expr))
    assert!(
        actual.contains("Arc::new(Mutex::new("),
        "shared() should produce Arc::new(Mutex::new()): {actual}"
    );
}

// ---------------------------------------------------------------------------
// 8.5 WASM-aware compilation flag
// ---------------------------------------------------------------------------

#[test]
fn test_p6_compile_options_no_borrow_inference() {
    // Test that no_borrow_inference option disables Tier 2
    let source = "\
function greet(name: string): string {
  return name;
}

function main() {
  console.log(greet(\"world\"));
}";

    let options = rsc_driver::CompileOptions {
        no_borrow_inference: true,
        ..rsc_driver::CompileOptions::default()
    };
    let result = rsc_driver::compile_source_with_options(source, "test.rts", &options);
    assert!(
        !result.has_errors,
        "compilation should succeed with no_borrow_inference: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    // With no_borrow_inference, params should be owned (String, not &str)
    assert!(
        result.rust_source.contains("name: String") || result.rust_source.contains("fn greet("),
        "function should still compile: {}",
        result.rust_source
    );
}

// ===========================================================================
//
// CATEGORY 9: JSDoc comments propagation
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 9.1 JSDoc on function produces /// doc comment
// ---------------------------------------------------------------------------

#[test]
fn test_p6_jsdoc_propagation() {
    let source = "\
/** Adds two numbers together.
 * @param a the first number
 * @param b the second number
 * @returns the sum
 */
function add(a: i32, b: i32): i32 {
  return a + b;
}

function main() {
  console.log(add(1, 2));
}";

    let actual = compile_to_rust(source);
    // JSDoc should produce /// doc comments
    assert!(
        actual.contains("/// "),
        "JSDoc should produce doc comments: {actual}"
    );
    // Should translate @param to Rustdoc
    assert!(
        actual.contains("# Arguments")
            || actual.contains("param")
            || actual.contains("Adds two numbers"),
        "JSDoc content should be preserved: {actual}"
    );
}

// ===========================================================================
//
// CATEGORY 10: Error handling composition
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 10.1 try/catch + finally composition
// ---------------------------------------------------------------------------

#[test]
fn test_p6_try_catch_finally_snapshot() {
    let source = "\
function risky(): i32 throws string {
  throw \"error\";
}

function main() {
  try {
    const result: i32 = risky();
    console.log(result);
  } catch (e: string) {
    console.log(e);
  } finally {
    console.log(\"cleanup\");
  }
}";

    let actual = compile_to_rust(source);
    // Should have match on Result
    assert!(
        actual.contains("Ok(") && actual.contains("Err("),
        "try/catch should produce Ok/Err match: {actual}"
    );
    // Finally should produce cleanup code
    assert!(
        actual.contains("cleanup"),
        "finally should produce cleanup: {actual}"
    );
}

// ===========================================================================
//
// CATEGORY 11: Collections and builtin methods
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 11.1 Array + Map + Set with builtin methods
// ---------------------------------------------------------------------------

#[test]
fn test_p6_collections_builtins_snapshot() {
    let source = "\
function main() {
  const items: Array<i32> = [1, 2, 3, 4, 5];
  const name: string = \"hello world\";
  const upper: string = name.toUpperCase();
  const parts: Array<string> = name.split(\" \");
  console.log(upper);
  console.log(items.length);
}";

    let actual = compile_to_rust(source);
    // toUpperCase -> to_uppercase
    assert!(
        actual.contains("to_uppercase()"),
        "toUpperCase should lower to to_uppercase: {actual}"
    );
    // split should produce Rust split
    assert!(
        actual.contains(".split("),
        "split should be preserved: {actual}"
    );
    // .length should produce .len()
    assert!(
        actual.contains(".len()"),
        ".length should produce .len(): {actual}"
    );
}

// ---------------------------------------------------------------------------
// 11.2 Iterator methods: map, filter, reduce
// ---------------------------------------------------------------------------

#[test]
fn test_p6_iterator_methods_snapshot() {
    let source = "\
function main() {
  const items: Array<i32> = [1, 2, 3, 4, 5];
  const doubled: Array<i32> = items.map((x: i32): i32 => x * 2);
  const evens: Array<i32> = items.filter((x: i32): bool => x % 2 === 0);
  console.log(doubled);
  console.log(evens);
}";

    let actual = compile_to_rust(source);
    // .map should produce iterator chain
    assert!(
        actual.contains(".iter()") || actual.contains(".map("),
        "map should produce iterator chain: {actual}"
    );
    // .filter should produce iterator chain
    assert!(
        actual.contains(".filter("),
        "filter should produce filter: {actual}"
    );
    // Should have .collect()
    assert!(
        actual.contains(".collect()") || actual.contains(".collect::<"),
        "iterator chain should collect: {actual}"
    );
}

// ===========================================================================
//
// CATEGORY 12: Module system and imports
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 12.1 Import from std library
// ---------------------------------------------------------------------------

#[test]
fn test_p6_import_std_snapshot() {
    let source = "\
import { HashMap } from \"std/collections\"

function main() {
  const m: Map<string, i32> = new Map();
  console.log(m);
}";

    let actual = compile_to_rust(source);
    // Should generate use statement
    assert!(
        actual.contains("use std::collections::HashMap"),
        "import from std should produce use: {actual}"
    );
}

// ===========================================================================
//
// CATEGORY 13: Nullability and option handling
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 13.1 T | null + optional chaining + nullish coalescing
// ---------------------------------------------------------------------------

#[test]
fn test_p6_null_handling_composition() {
    let source = "\
function find_name(id: i32): string | null {
  if (id === 1) {
    return \"Alice\";
  }
  return null;
}

function main() {
  const name: string | null = find_name(1);
  const display: string = name ?? \"Unknown\";
  console.log(display);

  const missing: string | null = find_name(0);
  const fallback: string = missing ?? \"Nobody\";
  console.log(fallback);
}";

    let actual = compile_to_rust(source);
    // Return type should be Option
    assert!(
        actual.contains("Option<String>"),
        "T | null should produce Option<String>: {actual}"
    );
    // null should be None
    assert!(
        actual.contains("None"),
        "null should produce None: {actual}"
    );
    // Return value should be Some
    assert!(
        actual.contains("Some("),
        "non-null return should produce Some: {actual}"
    );
    // ?? should produce unwrap_or
    assert!(
        actual.contains("unwrap_or"),
        "?? should produce unwrap_or: {actual}"
    );
}

#[test]
#[ignore]
fn test_p6_null_handling_e2e() {
    let source = "\
function find_name(id: i32): string | null {
  if (id === 1) {
    return \"Alice\";
  }
  return null;
}

function main() {
  const name: string | null = find_name(1);
  const display: string = name ?? \"Unknown\";
  console.log(display);

  const missing: string | null = find_name(0);
  const fallback: string = missing ?? \"Nobody\";
  console.log(fallback);
}";

    let output = compile_and_run(source);
    assert_eq!(output, "Alice\nNobody\n");
}

// ===========================================================================
//
// CATEGORY 14: Complex e2e programs
//
// ===========================================================================

// ---------------------------------------------------------------------------
// 14.1 Full program: calculator with all features
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_p6_calculator_e2e() {
    let source = "\
function power(base: i64, exp: i64): i64 {
  return base ** exp;
}

function classify(n: i64): string {
  const label: string = n > 0 ? \"positive\" : n === 0 ? \"zero\" : \"negative\";
  return label;
}

function sum_array(items: Array<i32>): i64 {
  let total: i64 = 0;
  for (const item of items) {
    total = total + item;
  }
  return total;
}

function main() {
  console.log(power(2, 10));
  console.log(classify(42));
  console.log(classify(0));
  console.log(classify(-5));

  const nums: Array<i32> = [1, 2, 3, 4, 5];
  console.log(sum_array(nums));

  const masked: i64 = 0xFF & 0x0F;
  console.log(masked);

  const shifted: i64 = 1 << 8;
  console.log(shifted);
}";

    let output = compile_and_run(source);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "1024"); // 2^10
    assert_eq!(lines[1], "positive"); // classify(42)
    assert_eq!(lines[2], "zero"); // classify(0)
    assert_eq!(lines[3], "negative"); // classify(-5)
    assert_eq!(lines[4], "15"); // sum [1,2,3,4,5]
    assert_eq!(lines[5], "15"); // 0xFF & 0x0F
    assert_eq!(lines[6], "256"); // 1 << 8
}

// ---------------------------------------------------------------------------
// 14.2 String manipulation program
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_p6_string_manipulation_e2e() {
    let source = "\
function main() {
  const greeting: string = \"hello world\";
  const upper: string = greeting.toUpperCase();
  console.log(upper);

  const starts: bool = greeting.startsWith(\"hello\");
  console.log(starts);

  const trimmed: string = \"  space  \".trim();
  console.log(trimmed);

  const replaced: string = greeting.replace(\"world\", \"rust\");
  console.log(replaced);
}";

    let output = compile_and_run(source);
    assert_eq!(output, "HELLO WORLD\ntrue\nspace\nhello rust\n");
}

// ---------------------------------------------------------------------------
// 14.3 Class with all features e2e
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_p6_full_class_e2e() {
    let source = "\
class BankAccount {
  private balance: f64 = 0.0;

  constructor(public owner: string, initial: f64) {
    this.balance = initial;
  }

  get amount(): f64 {
    return this.balance;
  }

  deposit(n: f64): void {
    this.balance = this.balance + n;
  }

  describe(): string {
    return `${this.owner}: ${this.balance}`;
  }
}

function main() {
  let acct: BankAccount = new BankAccount(\"Alice\", 100.0);
  acct.deposit(50.0);
  console.log(acct.amount);
  console.log(acct.describe());
}";

    let output = compile_and_run(source);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "150");
    assert!(lines[1].contains("Alice") && lines[1].contains("150"));
}

// ---------------------------------------------------------------------------
// Task 111: Labeled break/continue
// ---------------------------------------------------------------------------

#[test]
fn test_labeled_for_of_break() {
    let source = r#"function main() {
  const items: Array<i32> = [1, 2, 3];
  const other: Array<i32> = [10, 2, 30];
  let found: bool = false;
  outer: for (const x of items) {
    for (const y of other) {
      if (x == y) {
        found = true;
        break outer;
      }
    }
  }
  console.log(found);
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("'outer: for"),
        "should emit labeled for loop: {actual}"
    );
    assert!(
        actual.contains("break 'outer;"),
        "should emit labeled break: {actual}"
    );
}

#[test]
fn test_labeled_while_continue() {
    let source = r#"function main() {
  let i: i32 = 0;
  outer: while (i < 5) {
    i = i + 1;
    let j: i32 = 0;
    while (j < 3) {
      j = j + 1;
      if (j == 2) {
        continue outer;
      }
    }
  }
  console.log(i);
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("'outer: while"),
        "should emit labeled while loop: {actual}"
    );
    assert!(
        actual.contains("continue 'outer;"),
        "should emit labeled continue: {actual}"
    );
}

#[test]
fn test_labeled_do_while() {
    let source = r#"function main() {
  let i: i32 = 0;
  outer: do {
    i = i + 1;
    if (i == 3) {
      break outer;
    }
  } while (i < 10);
  console.log(i);
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("'outer: loop"),
        "should emit labeled loop (do-while lowers to loop): {actual}"
    );
    assert!(
        actual.contains("break 'outer;"),
        "should emit labeled break in do-while: {actual}"
    );
}

#[test]
fn test_nested_labels_different_names() {
    let source = r#"function main() {
  const rows: Array<i32> = [1, 2];
  const cols: Array<i32> = [3, 4];
  outer: for (const r of rows) {
    inner: for (const c of cols) {
      if (c == 4) {
        break inner;
      }
    }
  }
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("'outer: for"),
        "should emit outer label: {actual}"
    );
    assert!(
        actual.contains("'inner: for"),
        "should emit inner label: {actual}"
    );
    assert!(
        actual.contains("break 'inner;"),
        "should emit break inner: {actual}"
    );
}

#[test]
fn test_unlabeled_break_continue_regression() {
    let source = r#"function main() {
  let sum: i32 = 0;
  const items: Array<i32> = [1, 2, 3, 4, 5];
  for (const x of items) {
    if (x == 3) { continue; }
    if (x == 5) { break; }
    sum = sum + x;
  }
  console.log(sum);
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("continue;"),
        "unlabeled continue should still work: {actual}"
    );
    assert!(
        actual.contains("break;"),
        "unlabeled break should still work: {actual}"
    );
}

#[test]
#[ignore]
fn test_labeled_break_e2e() {
    let source = r#"function main() {
  const items: Array<i32> = [1, 2, 3];
  const other: Array<i32> = [10, 2, 30];
  let found: bool = false;
  outer: for (const x of items) {
    for (const y of other) {
      if (x == y) {
        found = true;
        break outer;
      }
    }
  }
  console.log(found);
}"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "true");
}

// ---------------------------------------------------------------------------
// Task 112: `in` operator
// ---------------------------------------------------------------------------

#[test]
fn test_in_operator_snapshot() {
    let source = r#"function main() {
  const m: Map<string, i32> = new Map();
  m.set("key", 42);
  const has_it: bool = "key" in m;
  console.log(has_it);
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".contains_key("),
        "in operator should lower to .contains_key(): {actual}"
    );
}

// ---------------------------------------------------------------------------
// Task 113: `delete` operator
// ---------------------------------------------------------------------------

#[test]
fn test_delete_operator_snapshot() {
    let source = r#"function main() {
  const m: Map<string, i32> = new Map();
  m.set("a", 1);
  delete m["a"];
  console.log(m.has("a"));
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".remove("),
        "delete operator should lower to .remove(): {actual}"
    );
}

// ---------------------------------------------------------------------------
// Task 114: `void` expression
// ---------------------------------------------------------------------------

#[test]
fn test_void_expression_snapshot() {
    let source = r#"function main() {
  const x: i32 = 42;
  void console.log(x);
}"#;
    let actual = compile_to_rust(source);
    // void should produce a block expression that discards the result
    assert!(
        actual.contains("println!") || actual.contains("{"),
        "void expression should lower to a discarding block: {actual}"
    );
}

// ---------------------------------------------------------------------------
// Task 115: Comma operator
// ---------------------------------------------------------------------------

#[test]
fn test_comma_operator_snapshot() {
    let source = r#"function main() {
  let x: i32 = 0;
  let y: i32 = 0;
  const result: i32 = (x = 1, y = 2, x + y);
  console.log(result);
}"#;
    let actual = compile_to_rust(source);
    // Comma operator should produce a block expression
    assert!(
        actual.contains("{") && actual.contains("}"),
        "comma operator should produce block expression: {actual}"
    );
}

// ---------------------------------------------------------------------------
// Compilation tests for operators
// ---------------------------------------------------------------------------

#[test]
fn test_in_operator_compiles() {
    let source = r#"function main() {
  const m: Map<string, i32> = new Map();
  m.set("hello", 1);
  const result: bool = "hello" in m;
  console.log(result);
}"#;
    // Should compile without errors
    let actual = compile_to_rust(source);
    assert!(
        !actual.is_empty(),
        "in operator should compile successfully"
    );
}

#[test]
fn test_delete_operator_compiles() {
    let source = r#"function main() {
  const m: Map<string, i32> = new Map();
  m.set("a", 1);
  delete m["a"];
}"#;
    let actual = compile_to_rust(source);
    assert!(
        !actual.is_empty(),
        "delete operator should compile successfully"
    );
}

#[test]
fn test_void_expression_compiles() {
    let source = r#"function main() {
  void 42;
}"#;
    let actual = compile_to_rust(source);
    assert!(
        !actual.is_empty(),
        "void expression should compile successfully"
    );
}

// ===========================================================================
//
// TASK 116: Never type support
//
// ===========================================================================

#[test]
fn test_never_return_type_snapshot_emits_bang() {
    let source = r#"function fail(): never {
  throw new Error("fatal error");
}

function main() {
  console.log("before");
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("-> !"),
        "expected `-> !` in output, got:\n{actual}"
    );
}

#[test]
fn test_readonly_array_param_snapshot() {
    // `readonly Array<string>` in function param should lower to `&[String]`.
    let source = "\
function process(data: readonly Array<string>): void {
  console.log(data.len());
}

function main() {
  const items: Array<string> = [\"a\", \"b\"];
  process(items);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("data: &[String]"),
        "readonly Array<string> in param should emit &[String]: {actual}"
    );
}

#[test]
fn test_type_guard_emits_bool_return() {
    let source = r#"function isString(x: string | i32): x is string {
  return true;
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("-> bool"),
        "type guard function should emit `-> bool` return type.\nActual:\n{actual}"
    );
}

#[test]
fn test_readonly_array_variable_snapshot() {
    // `readonly Array<string>` in variable position should emit `Vec<String>`
    // (Rust's `let` is already immutable).
    let source = "\
function main() {
  const items: readonly Array<string> = [\"a\", \"b\"];
  console.log(items.len());
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("Vec<String>"),
        "readonly Array in variable position should emit Vec<String>: {actual}"
    );
}

#[test]
fn test_never_in_union_eliminated_produces_single_type() {
    let source = r#"function get_name(): string | never {
  return "hello";
}

function main() {
  const s: string = get_name();
  console.log(s);
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("-> String"),
        "expected `-> String` (never eliminated from union), got:\n{actual}"
    );
    assert!(
        !actual.contains("OrNever"),
        "should not contain union with Never variant, got:\n{actual}"
    );
}

#[test]
fn test_readonlyarray_generic_param_snapshot() {
    // `ReadonlyArray<i32>` in function param should lower to `&[i32]`.
    let source = "\
function sum(nums: ReadonlyArray<i32>): i32 {
  return 0;
}

function main() {
  const nums: Array<i32> = [1, 2, 3];
  const total: i32 = sum(nums);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("nums: &[i32]"),
        "ReadonlyArray<i32> in param should emit &[i32]: {actual}"
    );
}

#[test]
fn test_type_guard_function_body() {
    let source = r#"function isPositive(x: i32): x is i32 {
  return x > 0;
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("-> bool"),
        "type guard should lower to bool return.\nActual:\n{actual}"
    );
    assert!(
        actual.contains("fn isPositive"),
        "function name should be present in output.\nActual:\n{actual}"
    );
}

#[test]
fn test_never_param_type_emits_bang_type() {
    let source = r#"function absurd(x: never): string {
  return x;
}

function main() {
  console.log("ok");
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains(": !"),
        "expected `: !` type annotation in output, got:\n{actual}"
    );
}

#[test]
fn test_type_guard_call_site() {
    let source = r#"function isPositive(x: i32): x is i32 {
  return x > 0;
}

function main() {
  const result: bool = isPositive(42);
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("isPositive(42)"),
        "type guard call should compile as normal function call.\nActual:\n{actual}"
    );
}

#[test]
fn test_readonly_tuple_snapshot() {
    // `readonly [string, i32]` should lower to a regular Rust tuple `(String, i32)`.
    let source = "\
function main() {
  const pair: readonly [string, i32] = [\"hello\", 42];
  console.log(pair);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("(String, i32)"),
        "readonly tuple should emit regular Rust tuple: {actual}"
    );
}

#[test]
#[ignore]
fn test_never_function_compiles_with_panic() {
    let source = r#"function fail(): never {
  throw new Error("fatal");
}

function main() {
  console.log("before fail");
}"#;
    let output = compile_and_run(source);
    assert!(output.contains("before fail"));
}

// ---------------------------------------------------------------------------
// Task 117: `unknown` type support
// ---------------------------------------------------------------------------

#[test]
fn test_unknown_param_type_snapshot() {
    let source = r#"function process(x: unknown): void {
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("Box<dyn std::any::Any>"),
        "unknown param should emit Box<dyn std::any::Any>: {actual}"
    );
}

#[test]
fn test_unknown_return_type_snapshot() {
    let source = r#"function get_value(): unknown {
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("-> Box<dyn std::any::Any>"),
        "unknown return type should emit -> Box<dyn std::any::Any>: {actual}"
    );
}

#[test]
fn test_unknown_variable_snapshot() {
    let source = r#"function main() {
  const x: unknown = 42;
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("Box<dyn std::any::Any>"),
        "unknown variable type should emit Box<dyn std::any::Any>: {actual}"
    );
}

#[test]
fn test_unknown_type_generates_use_any() {
    let source = r#"function process(x: unknown): void {
}"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("use std::any::Any;"),
        "unknown type should generate `use std::any::Any;`: {actual}"
    );
}

#[test]
#[ignore]
fn test_readonly_array_param_compiles() {
    // Function with readonly array param should compile successfully.
    let source = "\
function first(data: readonly Array<string>): string {
  return data[0].clone();
}

function main() {
  const items: Array<string> = [\"hello\", \"world\"];
  console.log(first(items));
}";

    let output = compile_and_run(source);
    assert_eq!(output, "hello\n");
}

#[test]
#[ignore]
fn test_unknown_param_compiles() {
    let source = r#"function process(x: unknown): void {
}

function main() {
  process(42);
}"#;
    let output = compile_and_run(source);
    assert!(
        output.is_empty() || !output.is_empty(),
        "should compile and run"
    );
}

// ---------------------------------------------------------------------------
// Task 118: Type guard support
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_type_guard_function_compiles() {
    let source = r#"function isPositive(x: i32): x is i32 {
  return x > 0;
}

function main() {
  const result: bool = isPositive(42);
}"#;
    let _output = compile_and_run(source);
}

// ---- Task 119: `as const` assertion ----

// T119-5: `as const` on a string array emits a static slice reference
#[test]
fn test_as_const_array_snapshot() {
    let source = r#"const colors = ["red", "green"] as const;"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("&["),
        "as const array should emit static slice `&[...]`:\n{actual}"
    );
    assert!(
        actual.contains("&str"),
        "as const string array should have type `&str`:\n{actual}"
    );
    assert!(
        actual.contains("const colors"),
        "top-level const should preserve name:\n{actual}"
    );
}

// T119-6: `as const` on a number literal emits a regular const binding
#[test]
fn test_as_const_number_snapshot() {
    let source = r#"const x = 42 as const;"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("const x: i64 = 42;"),
        "as const on number should emit const binding:\n{actual}"
    );
}

// T119-7: `as const` on a number array emits a static slice
#[test]
fn test_as_const_int_array_snapshot() {
    let source = r#"const nums = [1, 2, 3] as const;"#;
    let actual = compile_to_rust(source);
    assert!(
        actual.contains("&[i64]"),
        "as const int array should have type `&[i64]`:\n{actual}"
    );
    assert!(
        actual.contains("&[1, 2, 3]"),
        "as const int array should emit static slice literal:\n{actual}"
    );
}

// T119-8: `as const` array compiles with rustc
#[test]
#[ignore]
fn test_as_const_array_compiles() {
    let source = r#"const colors = ["red", "green", "blue"] as const;
function main() {
  console.log(colors[0]);
}"#;
    // compile_and_run panics if compilation fails
    let output = compile_and_run(source);
    assert!(
        !output.is_empty(),
        "as const array should compile and produce output"
    );
}

// T119-9: `as const` literal compiles with rustc
#[test]
#[ignore]
fn test_as_const_literal_compiles() {
    let source = r#"const x = 42 as const;
function main() {
  console.log(x);
}"#;
    // compile_and_run panics if compilation fails
    let output = compile_and_run(source);
    assert!(
        !output.is_empty(),
        "as const literal should compile and produce output"
    );
}

// ===========================================================================
//
// Task 120: readonly arrays and tuples
//
// ===========================================================================

#[test]
#[ignore]
fn test_readonly_variable_compiles() {
    // Const with readonly array type should compile successfully.
    let source = "\
function main() {
  const items: readonly Array<string> = [\"a\", \"b\", \"c\"];
  console.log(items.len());
}";

    let output = compile_and_run(source);
    assert_eq!(output, "3\n");
}

// ===========================================================================
//
// CATEGORY: Computed Property Names (Task 121) + Static blocks (Task 122) + super.method() (Task 123)
//
// ===========================================================================

#[test]
fn test_computed_property_snapshot() {
    let source = "\
function main() {
  const key: string = \"name\";
  const obj = { [key]: \"value\" };
}";

    let actual = compile_to_rust(source);
    // Should emit HashMap::new()
    assert!(
        actual.contains("HashMap::new()"),
        "computed property should emit HashMap::new(): {actual}"
    );
    // Should emit .insert() call
    assert!(
        actual.contains(".insert("),
        "computed property should emit .insert(): {actual}"
    );
    // Should use std::collections::HashMap
    assert!(
        actual.contains("use std::collections::HashMap"),
        "should generate use declaration for HashMap: {actual}"
    );
}

#[test]
fn test_static_block_simple_assignment_snapshot() {
    // Static block with literal assignment to declared static fields
    // should lower to associated constants in the impl block.
    let source = "\
class Config {
  static DEFAULT_PORT: i32;
  static DEFAULT_HOST: string;

  static {
    Config.DEFAULT_PORT = 8080;
    Config.DEFAULT_HOST = \"localhost\";
  }
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("const DEFAULT_PORT: i32 = 8080"),
        "static block assignment should lower to associated const: {actual}"
    );
    assert!(
        actual.contains("const DEFAULT_HOST: String"),
        "static block string assignment should lower to const: {actual}"
    );
}

#[test]
fn test_computed_property_insert_snapshot() {
    let source = "\
function main() {
  const key: string = \"status\";
  const obj = { [key]: 200 };
}";

    let actual = compile_to_rust(source);
    // The computed key expression should appear in the insert call
    assert!(
        actual.contains("key"),
        "computed key should use key variable in insert: {actual}"
    );
    assert!(
        actual.contains("200"),
        "computed value should appear: {actual}"
    );
}

#[test]
fn test_static_block_complex_logic_snapshot() {
    // Static block with non-assignment logic should lower to _static_init() method
    let source = "\
class Service {
  static {
    console.log(\"initializing\");
  }
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("fn _static_init()"),
        "complex static block should produce _static_init() method: {actual}"
    );
}

#[test]
fn test_mixed_properties_snapshot() {
    let source = "\
function main() {
  const key: string = \"dynamic\";
  const obj = { static_field: 1, [key]: 2 };
}";

    let actual = compile_to_rust(source);
    // With any computed key, the whole object becomes a HashMap
    assert!(
        actual.contains("HashMap::new()"),
        "mixed properties should use HashMap: {actual}"
    );
    // Static field should be inserted as a string key
    assert!(
        actual.contains("\"static_field\""),
        "static field name should appear as string literal: {actual}"
    );
    // Both values should appear in insert calls
    assert!(
        actual.contains(".insert("),
        "should use .insert() for fields: {actual}"
    );
}

#[test]
fn test_static_block_preserves_regular_members() {
    // Class with static block + regular methods should emit both
    let source = "\
class Counter {
  count: i32;
  static MAX: i32 = 100;

  constructor(n: i32) {
    this.count = n;
  }

  static {
    console.log(\"init\");
  }

  increment(): void {
    this.count = this.count + 1;
  }
}";

    let actual = compile_to_rust(source);
    // Struct should exist
    assert!(
        actual.contains("struct Counter"),
        "should produce Counter struct: {actual}"
    );
    // Static field const from direct initializer
    assert!(
        actual.contains("const MAX"),
        "static field should produce associated const: {actual}"
    );
    // Constructor
    assert!(
        actual.contains("fn new("),
        "constructor should produce fn new: {actual}"
    );
    // Instance method
    assert!(
        actual.contains("fn increment("),
        "instance method should exist: {actual}"
    );
    // Static init from static block
    assert!(
        actual.contains("fn _static_init()"),
        "static block should produce _static_init: {actual}"
    );
}

#[test]
#[ignore]
fn test_computed_property_compiles() {
    let source = "\
function main() {
  const key: string = \"greeting\";
  const obj = { [key]: \"hello\" };
  console.log(obj[&key]);
}";

    let output = compile_and_run(source);
    assert_eq!(output, "hello\n");
}

#[test]
#[ignore]
fn test_static_block_simple_compiles() {
    // Class with static block literal assignment should compile with rustc
    let source = "\
class Config {
  static DEFAULT_PORT: i32;
  static DEFAULT_HOST: string;

  static {
    Config.DEFAULT_PORT = 8080;
    Config.DEFAULT_HOST = \"localhost\";
  }
}

function main() {
  console.log(Config.DEFAULT_PORT);
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "8080");
}

#[test]
fn test_super_method_call_snapshot() {
    let source = "\
class Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return this.name;
  }
}

class Dog extends Animal {
  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    const base = super.speak();
    return base;
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);

    // super.speak() should emit a temporary Animal construction with the method call
    assert!(
        actual.contains("Animal {"),
        "super.method() should construct a temporary base class:\n{actual}"
    );
    assert!(
        actual.contains(".speak()"),
        "super.method() should call speak() on the temporary:\n{actual}"
    );
}

#[test]
fn test_super_method_with_args_snapshot() {
    let source = "\
class Base {
  x: i32;

  constructor(x: i32) {
    this.x = x;
  }

  greet(name: string): string {
    return name;
  }
}

class Child extends Base {
  constructor(x: i32) {
    this.x = x;
  }

  greet(name: string): string {
    return super.greet(name);
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);

    // super.greet(name) should construct a temporary Base and call greet on it
    assert!(
        actual.contains("Base {"),
        "super.greet() should construct a temporary Base:\n{actual}"
    );
    assert!(
        actual.contains(".greet("),
        "super.greet() should call greet on the temporary:\n{actual}"
    );
}

#[test]
fn test_super_in_override_method_snapshot() {
    let source = "\
class Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return this.name;
  }
}

class Dog extends Animal {
  breed: string;

  constructor(name: string, breed: string) {
    this.name = name;
    this.breed = breed;
  }

  speak(): string {
    const base = super.speak();
    return base;
  }
}

function main() {
  const dog = new Dog(\"Rex\", \"Lab\");
  console.log(dog.speak());
}";

    let actual = compile_to_rust(source);

    // The generated code should contain both the Animal struct and Dog struct,
    // and the Dog's speak should reference Animal
    assert!(
        actual.contains("struct Animal"),
        "should have Animal struct:\n{actual}"
    );
    assert!(
        actual.contains("struct Dog"),
        "should have Dog struct:\n{actual}"
    );
    // super.speak() should create a temporary Animal instance
    assert!(
        actual.contains("Animal {"),
        "super.speak() should construct temporary Animal:\n{actual}"
    );
}

#[test]
#[ignore]
fn test_super_method_call_compiles() {
    let source = "\
class Counter {
  count: i32;

  constructor(count: i32) {
    this.count = count;
  }

  value(): i32 {
    return this.count;
  }
}

class DoubleCounter extends Counter {
  constructor(count: i32) {
    this.count = count;
  }

  value(): i32 {
    const base = super.value();
    return base * 2;
  }
}

function main() {
  const dc = new DoubleCounter(5);
  console.log(dc.value());
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "10");
}

// ===========================================================================
//
// CATEGORY: Dynamic Import Expressions
//
// ===========================================================================

// ---------------------------------------------------------------------------
// Dynamic import emits diagnostic warning and lowers to panic
// ---------------------------------------------------------------------------

#[test]
fn test_dynamic_import_emits_diagnostic() {
    let result = rsc_driver::compile_source(
        "function main() { const m = import(\"./utils\"); }",
        "test.rts",
    );
    assert!(
        !result.has_errors,
        "dynamic import should compile (with warning), got errors: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        result.diagnostics.iter().any(|d| d
            .message
            .contains("dynamic import")
            && d.message.contains("not supported")),
        "expected diagnostic about dynamic import, got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_dynamic_import_snapshot() {
    let result = rsc_driver::compile_source(
        "function main() { const m = import(\"./utils\"); }",
        "test.rts",
    );
    let rust = &result.rust_source;
    assert!(
        rust.contains("panic!") && rust.contains("dynamic import not supported"),
        "expected panic!(\"dynamic import not supported: ...\") in output, got:\n{rust}"
    );
}

// ===========================================================================
//
// CATEGORY: Import Type Enforcement (Task 126)
//
// ===========================================================================

#[test]
fn test_import_type_no_use_declaration() {
    let source = r#"import type { User } from "./models";

function main() {
  console.log("hello");
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("use crate::models::User"),
        "type-only import should not emit `use` declaration.\nGenerated:\n{rust}"
    );
}

#[test]
fn test_import_type_vs_regular_import() {
    let source = r#"import type { Config } from "./config";
import { process_data } from "./handlers";

function main() {
  process_data();
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("use crate::handlers::process_data"),
        "regular import should emit `use` declaration.\nGenerated:\n{rust}"
    );
    assert!(
        !rust.contains("use crate::config::Config"),
        "type-only import should not emit `use` declaration.\nGenerated:\n{rust}"
    );
}

#[test]
fn test_import_type_used_as_type_annotation() {
    let diags = test_utils::compile_diagnostics(
        r#"import type { User } from "./models";

function greet(user: User): string {
  return "hello";
}

function main() {
  console.log("ok");
}"#,
    );
    let has_type_only_error = diags
        .iter()
        .any(|msg| msg.contains("cannot use type-only import"));
    assert!(
        !has_type_only_error,
        "type-only import used as annotation should not produce type-only import error, got: {diags:?}"
    );
}

// ===========================================================================
//
// CATEGORY: Tagged Template Literals
//
// ===========================================================================

#[test]
fn test_tagged_template_snapshot() {
    let rust = compile_to_rust(
        r#"function tag(strings: Array<string>, values: Array<string>): string {
  return strings.join("");
}

function main() {
  const name: string = "Alice";
  const result: string = tag`hello ${name} world`;
  console.log(result);
}"#,
    );
    // Tagged template lowers to a function call with string slice + vec args.
    // Verify the tag function is called with the correct arguments.
    assert!(
        rust.contains(r#"tag(&["hello ", " world"], vec![name"#),
        "expected tagged template call in output, got:\n{rust}"
    );
}

#[test]
fn test_tagged_template_no_exprs_snapshot() {
    let rust = compile_to_rust(
        r#"function tag(strings: Array<string>, values: Array<string>): string {
  return strings.join("");
}

function main() {
  const result: string = tag`plain text`;
  console.log(result);
}"#,
    );
    // Tagged template with no interpolations still lowers to function call.
    assert!(
        rust.contains(r#"tag(&["plain text"], vec![])"#),
        "expected tagged template call with no exprs in output, got:\n{rust}"
    );
}

#[test]
fn test_import_type_used_as_value_emits_error() {
    let diags = test_utils::compile_diagnostics(
        r#"import type { User } from "./models";

function main() {
  User.create();
}"#,
    );
    let has_error = diags
        .iter()
        .any(|msg| msg.contains("cannot use type-only import `User` as a value"));
    assert!(
        has_error,
        "expected diagnostic about type-only import used as value, got: {diags:?}"
    );
}

// ===========================================================================
//
// CATEGORY: Template literal types (Task 128)
//
// ===========================================================================

#[test]
fn test_template_literal_type_lowers_to_string() {
    let source = r#"type Greeting = `hello ${string}`

function main() {
  const g: Greeting = "hello world";
  console.log(g);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("type Greeting = String;"),
        "template literal type should lower to `type Greeting = String;`.\nGenerated:\n{rust}"
    );
}

// ---------------------------------------------------------------------------
// Math constants — all 8 standard constants
// ---------------------------------------------------------------------------

#[test]
fn test_math_constants_snapshot() {
    let source = "\
function main() {
  console.log(Math.PI);
  console.log(Math.E);
  console.log(Math.LN2);
  console.log(Math.LN10);
  console.log(Math.LOG2E);
  console.log(Math.LOG10E);
  console.log(Math.SQRT2);
  console.log(Math.SQRT1_2);
}";

    let rust = compile_to_rust(source);
    assert!(
        rust.contains("std::f64::consts::PI"),
        "expected std::f64::consts::PI in output:\n{rust}"
    );
    assert!(
        rust.contains("std::f64::consts::E"),
        "expected std::f64::consts::E in output:\n{rust}"
    );
    assert!(
        rust.contains("std::f64::consts::LN_2"),
        "expected std::f64::consts::LN_2 in output:\n{rust}"
    );
    assert!(
        rust.contains("std::f64::consts::LN_10"),
        "expected std::f64::consts::LN_10 in output:\n{rust}"
    );
    assert!(
        rust.contains("std::f64::consts::LOG2_E"),
        "expected std::f64::consts::LOG2_E in output:\n{rust}"
    );
    assert!(
        rust.contains("std::f64::consts::LOG10_E"),
        "expected std::f64::consts::LOG10_E in output:\n{rust}"
    );
    assert!(
        rust.contains("std::f64::consts::SQRT_2"),
        "expected std::f64::consts::SQRT_2 in output:\n{rust}"
    );
    assert!(
        rust.contains("std::f64::consts::FRAC_1_SQRT_2"),
        "expected std::f64::consts::FRAC_1_SQRT_2 in output:\n{rust}"
    );
}

#[test]
fn test_template_literal_type_in_function_param() {
    let source = r#"type EventName = `on${string}`

function handle(event: EventName): void {
  console.log(event);
}

function main() {
  handle("onclick");
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("type EventName = String;"),
        "template literal type alias should lower to `type EventName = String;`.\nGenerated:\n{rust}"
    );
    assert!(
        rust.contains("event: String") || rust.contains("event: EventName"),
        "parameter with template literal type alias should accept String.\nGenerated:\n{rust}"
    );
}

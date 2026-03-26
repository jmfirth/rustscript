//! Phase 4 integration snapshot tests — compile `.rts` source and compare
//! generated `.rs` against golden output.
//!
//! These are fast tests (no cargo invocation). They validate that Phase 4
//! features (Tier 2 borrow analysis, inline Rust, derive macros, shared type,
//! `--no-borrow-inference`) compose correctly with each other and with
//! earlier phase features.

mod test_utils;

use rsc_driver::{CompileOptions, compile_source, compile_source_with_options};
use test_utils::{compile_result, compile_to_rust};

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
// 1. Borrowed param passed to closure (borrow + closure composition)
//
// Features: Tier 2 borrow inference, forEach, closure capturing &str param
// ===========================================================================

#[test]
fn test_snapshot_p4_borrow_with_closure_compiles() {
    let source = "\
function process(name: string): void {
  const items: Array<string> = [\"a\", \"b\"];
  items.forEach((item) => {
    console.log(name);
  });
}";

    let rust = compile_to_rust(source);
    // NOTE: Borrow analysis does not currently track usage inside closures,
    // so `name` stays Owned (String) rather than being borrowed (&str).
    // This is a known limitation — documenting rather than fixing per task spec.
    assert!(
        rust.contains("name: String"),
        "name param stays owned when captured in closure: {rust}"
    );
    assert!(
        rust.contains(".iter().for_each("),
        "forEach should lower to iter().for_each(): {rust}"
    );
}

// ===========================================================================
// 2. Borrowed string param with string method chain
//
// Features: Tier 2 borrow inference, toUpperCase, toLowerCase
// ===========================================================================

#[test]
fn test_snapshot_p4_borrow_with_string_methods() {
    let source = "\
function shout(name: string): string {
  return name.toUpperCase();
}

function whisper(name: string): string {
  return name.toLowerCase();
}

function main(): void {
  const name: string = \"Hello\";
  console.log(shout(name));
  console.log(whisper(name));
}";

    let expected = "\
fn shout(name: &str) -> String {
    return name.to_uppercase();
}

fn whisper(name: &str) -> String {
    return name.to_lowercase();
}

fn main() {
    let name: String = \"Hello\".to_string();
    println!(\"{}\", shout(&name));
    println!(\"{}\", whisper(&name));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_borrow_string_methods", &actual, expected);
}

// ===========================================================================
// 3. Borrowed string param in template literal
//
// Features: Tier 2 borrow inference, template literal → format!
// ===========================================================================

#[test]
fn test_snapshot_p4_borrow_with_template() {
    let source = "\
function greetFormal(name: string): string {
  return `Hello, ${name}!`;
}

function main(): void {
  const msg: string = greetFormal(\"World\");
  console.log(msg);
}";

    let expected = "\
fn greetFormal(name: &str) -> String {
    return format!(\"Hello, {}!\", name);
}

fn main() {
    let msg: String = greetFormal(\"World\");
    println!(\"{}\", msg);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_borrow_template", &actual, expected);
}

// ===========================================================================
// 4. Struct with derive + Debug used in println
//
// Features: derive macros (Debug, Clone, PartialEq), struct, field access
// ===========================================================================

#[test]
fn test_snapshot_p4_derive_debug_usage() {
    let source = "\
type Point = { x: f64, y: f64 }

function main(): void {
  const p: Point = { x: 1.0, y: 2.0 };
  console.log(p);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq)]
struct Point {
    pub x: f64,
    pub y: f64,
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 };
    println!(\"{}\", p);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_derive_debug_usage", &actual, expected);
}

// ===========================================================================
// 5. Enum with derive + match (PartialEq, Eq, Copy used in comparison)
//
// Features: derive macros on enum, switch → match
// ===========================================================================

#[test]
fn test_snapshot_p4_enum_derive_with_match() {
    let source = "type Color = \"red\" | \"green\" | \"blue\"\n\nfunction isWarm(c: Color): bool {\n  switch (c) {\n    case \"red\": return true;\n    case \"green\": return false;\n    case \"blue\": return false;\n  }\n}";

    let actual = compile_to_rust(source);
    // Enum should have full derives
    assert!(
        actual.contains("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]"),
        "enum should have all derives: {actual}"
    );
    assert!(
        actual.contains("match c"),
        "switch should become match: {actual}"
    );
    assert!(
        actual.contains("Color::Red"),
        "enum variant should be PascalCase: {actual}"
    );
}

// ===========================================================================
// 6. Inline Rust referencing RustScript variables
//
// Features: inline Rust block, variable scoping across boundary
// ===========================================================================

#[test]
fn test_snapshot_p4_inline_rust_with_vars() {
    let source = "\
function main(): void {
  const x: i32 = 42;
  const y: i32 = 58;
  rust {
    let sum = x + y;
    println!(\"sum = {}\", sum);
  }
}";

    let expected = "\
fn main() {
    let x: i32 = 42;
    let y: i32 = 58;
    let sum = x + y;
    println!(\"sum = {}\", sum);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_inline_rust_vars", &actual, expected);
}

// ===========================================================================
// 7. Shared type with lock operations
//
// Features: shared<T> → Arc<Mutex<T>>, shared() → Arc::new(Mutex::new()),
//           .lock() → .lock().unwrap()
// ===========================================================================

#[test]
fn test_snapshot_p4_shared_with_lock() {
    let source = "\
function main(): void {
  const counter: shared<i32> = shared(0);
  const guard = counter.lock();
  console.log(guard);
}";

    let expected = "\
use std::sync::Arc;
use std::sync::Mutex;

fn main() {
    let counter: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));
    let guard = counter.lock().unwrap();
    println!(\"{}\", guard);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_shared_lock", &actual, expected);
}

// ===========================================================================
// 8. All Phase 4 features combined in one file
//
// Features: struct + derive, borrow inference, inline Rust, shared type
// ===========================================================================

#[test]
fn test_snapshot_p4_all_features_combined() {
    let source = "\
type Config = { name: string, count: i32 }

function describe(config: Config): void {
  console.log(config.name);
}

function main(): void {
  const config: Config = { name: \"test\", count: 42 };
  describe(config);
  describe(config);

  rust {
    let raw_value: i32 = 100;
    println!(\"raw: {}\", raw_value);
  }

  const counter: shared<i32> = shared(0);
  const guard = counter.lock();
  console.log(guard);
}";

    let expected = "\
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    pub name: String,
    pub count: i32,
}

fn describe(config: Config) {
    println!(\"{}\", config.name);
}

fn main() {
    let config = Config { name: \"test\".to_string(), count: 42 };
    describe(config.clone());
    describe(config);
    let raw_value: i32 = 100;
    println!(\"raw: {}\", raw_value);
    let counter: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));
    let guard = counter.lock().unwrap();
    println!(\"{}\", guard);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_all_combined", &actual, expected);
}

// ===========================================================================
// 9. Borrow elimination — len function (Correctness Scenario 2)
//
// Features: Tier 2 borrow inference, clone elimination, multiple calls
// ===========================================================================

#[test]
fn test_snapshot_p4_borrow_elimination_len() {
    let source = "\
function len(s: string): i64 {
  return s.length;
}

function main(): void {
  const s: string = \"hello world\";
  const a: i64 = len(s);
  const b: i64 = len(s);
  const c: i64 = len(s);
  console.log(a);
  console.log(b);
  console.log(c);
}";

    let expected = "\
fn len(s: &str) -> i64 {
    return s.len() as i64;
}

fn main() {
    let s: String = \"hello world\".to_string();
    let a: i64 = len(&s);
    let b: i64 = len(&s);
    let c: i64 = len(&s);
    println!(\"{}\", a);
    println!(\"{}\", b);
    println!(\"{}\", c);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_borrow_elimination_len", &actual, expected);
}

// ===========================================================================
// 10. --no-borrow-inference produces Tier 1 output for len function
//
// The same program compiled with and without borrow inference should both
// compile, but produce different function signatures.
// ===========================================================================

#[test]
fn test_snapshot_p4_no_borrow_inference_len() {
    let source = "\
function len(s: string): i64 {
  return s.length;
}

function main(): void {
  const s: string = \"hello world\";
  const a: i64 = len(s);
  const b: i64 = len(s);
  const c: i64 = len(s);
  console.log(a);
  console.log(b);
  console.log(c);
}";

    // With borrow inference (default): s → &str, no clones at call site
    let result_with = compile_source(source, "test.rts");
    assert!(!result_with.has_errors);
    assert!(
        result_with.rust_source.contains("fn len(s: &str)"),
        "with borrow inference should use &str: {}",
        result_with.rust_source,
    );
    assert!(
        !result_with.rust_source.contains(".clone()"),
        "with borrow inference should have no clones: {}",
        result_with.rust_source,
    );

    // Without borrow inference: s → String, clones at call site
    let options = CompileOptions {
        no_borrow_inference: true,
    };
    let result_without = compile_source_with_options(source, "test.rts", &options);
    assert!(!result_without.has_errors);
    assert!(
        result_without.rust_source.contains("fn len(s: String)"),
        "without borrow inference should use String: {}",
        result_without.rust_source,
    );
    assert!(
        result_without.rust_source.contains(".clone()"),
        "without borrow inference should have clones: {}",
        result_without.rust_source,
    );
}

// ===========================================================================
// 11. --no-borrow-inference regression for all-features-combined
//
// Verify the all-features-combined program compiles in both modes.
// ===========================================================================

#[test]
fn test_snapshot_p4_no_borrow_inference_all_combined() {
    let source = "\
type Config = { name: string, count: i32 }

function describe(config: Config): void {
  console.log(config.name);
}

function main(): void {
  const config: Config = { name: \"test\", count: 42 };
  describe(config);
  describe(config);

  rust {
    let raw_value: i32 = 100;
    println!(\"raw: {}\", raw_value);
  }

  const counter: shared<i32> = shared(0);
  const guard = counter.lock();
  console.log(guard);
}";

    // Both should compile without errors
    let result_with = compile_source(source, "test.rts");
    assert!(
        !result_with.has_errors,
        "with borrow inference should compile"
    );

    let options = CompileOptions {
        no_borrow_inference: true,
    };
    let result_without = compile_source_with_options(source, "test.rts", &options);
    assert!(
        !result_without.has_errors,
        "without borrow inference should compile"
    );

    // Both should have struct derives
    assert!(
        result_with
            .rust_source
            .contains("#[derive(Debug, Clone, PartialEq, Eq)]"),
        "with borrow: struct should have derives"
    );
    assert!(
        result_without
            .rust_source
            .contains("#[derive(Debug, Clone, PartialEq, Eq)]"),
        "no-borrow: struct should have derives"
    );

    // Both should have shared type imports
    assert!(
        result_with.rust_source.contains("use std::sync::Arc;"),
        "with borrow: should have Arc import"
    );
    assert!(
        result_without.rust_source.contains("use std::sync::Arc;"),
        "no-borrow: should have Arc import"
    );

    // Both should have inline Rust pass-through
    assert!(
        result_with
            .rust_source
            .contains("let raw_value: i32 = 100;"),
        "with borrow: inline Rust should pass through"
    );
    assert!(
        result_without
            .rust_source
            .contains("let raw_value: i32 = 100;"),
        "no-borrow: inline Rust should pass through"
    );
}

// ===========================================================================
// 12. Mixed borrowed and owned params in same function
//
// Features: Tier 2 borrow inference, mixed param modes
// ===========================================================================

#[test]
fn test_snapshot_p4_mixed_borrow_owned_params() {
    let source = "\
function display(label: string): void {
  console.log(label);
}

function consume(data: string): string {
  return data;
}

function main(): void {
  const msg: string = \"hello\";
  display(msg);
  const result: string = consume(msg);
  console.log(result);
}";

    let expected = "\
fn display(label: &str) {
    println!(\"{}\", label);
}

fn consume(data: String) -> String {
    return data;
}

fn main() {
    let msg: String = \"hello\".to_string();
    display(&msg);
    let result: String = consume(msg);
    println!(\"{}\", result);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_mixed_borrow_owned", &actual, expected);
}

// ===========================================================================
// 13. Struct with Option field + derives
//
// Features: derive macros, Option<T> type, null handling
// ===========================================================================

#[test]
fn test_snapshot_p4_struct_option_field_derives() {
    let source = "\
type User = { name: string, nickname: string | null }

function main(): void {
  const u: User = { name: \"Alice\", nickname: null };
  console.log(u.name);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub nickname: Option<String>,
}

fn main() {
    let u = User { name: \"Alice\".to_string(), nickname: None };
    println!(\"{}\", u.name);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_struct_option_derives", &actual, expected);
}

// ===========================================================================
// 14. Module-level inline Rust coexisting with RustScript functions
//
// Features: module-level rust block, RustScript function, combined output
// ===========================================================================

#[test]
fn test_snapshot_p4_module_inline_rust_coexist() {
    let source = "\
rust {
  fn helper(x: i32) -> i32 {
    x * 2
  }
}

function main(): void {
  const x: i32 = 21;
  console.log(x);
}";

    let expected = "\
fn helper(x: i32) -> i32 {
x * 2
}

fn main() {
    let x: i32 = 21;
    println!(\"{}\", x);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_module_inline_rust", &actual, expected);
}

// ===========================================================================
// 15. Derived Clone used in Tier 1 clone insertion
//
// Features: derive Clone on struct, Tier 1 clone insertion for struct params
// ===========================================================================

#[test]
fn test_snapshot_p4_derive_clone_used_in_tier1() {
    let source = "\
type Point = { x: f64, y: f64 }

function show(p: Point): void {
  console.log(p.x);
}

function main(): void {
  const p: Point = { x: 1.0, y: 2.0 };
  show(p);
  show(p);
}";

    let expected = "\
#[derive(Debug, Clone, PartialEq)]
struct Point {
    pub x: f64,
    pub y: f64,
}

fn show(p: Point) {
    println!(\"{}\", p.x);
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 };
    show(p.clone());
    show(p);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p4_derive_clone_tier1", &actual, expected);
}

// ===========================================================================
// 16. Phase 0-3 regression: struct + template literal (Phase 1 features)
//
// Verify earlier-phase features are unaffected by Phase 4 changes.
// ===========================================================================

#[test]
fn test_snapshot_p4_phase1_regression_struct_template() {
    let source = "\
type Person = { name: string, age: u32 }

function greet(p: Person): string {
  return `Hello, ${p.name}!`;
}

function main() {
  const p: Person = { name: \"Alice\", age: 30 };
  console.log(greet(p));
}";

    let result = compile_result(source);
    assert!(
        !result.has_errors,
        "Phase 1 struct + template should still compile"
    );
    assert!(
        result.rust_source.contains("struct Person"),
        "struct should be emitted"
    );
    assert!(
        result.rust_source.contains("format!("),
        "template literal should become format!"
    );
    assert!(
        result.rust_source.contains("#[derive("),
        "struct should have derive attribute"
    );
}

// ===========================================================================
// 17. Phase 0-3 regression: async + await (Phase 2 features)
//
// Verify async compilation is unaffected by Phase 4 changes.
// ===========================================================================

#[test]
fn test_snapshot_p4_phase2_regression_async() {
    let source = "\
async function fetchData(): string {
  return \"hello\";
}

async function main() {
  const result = await fetchData();
  console.log(result);
}";

    let result = compile_result(source);
    assert!(!result.has_errors, "Phase 2 async should still compile");
    assert!(
        result.needs_async_runtime,
        "should still need async runtime"
    );
    assert!(
        result.rust_source.contains("async fn fetchData()"),
        "async function should be emitted"
    );
    assert!(
        result.rust_source.contains(".await"),
        "await should be emitted"
    );
    assert!(
        result.rust_source.contains("#[tokio::main]"),
        "async main should have tokio attribute"
    );
}

// ===========================================================================
// 18. --no-borrow-inference for borrowed string methods
//
// Verify same program produces String params without borrow inference.
// ===========================================================================

#[test]
fn test_snapshot_p4_no_borrow_inference_string_methods() {
    let source = "\
function shout(name: string): string {
  return name.toUpperCase();
}

function main(): void {
  const name: string = \"hello\";
  console.log(shout(name));
}";

    // With borrow inference: &str
    let result_with = compile_source(source, "test.rts");
    assert!(!result_with.has_errors);
    assert!(
        result_with.rust_source.contains("fn shout(name: &str)"),
        "with borrow inference should use &str: {}",
        result_with.rust_source,
    );

    // Without borrow inference: String
    let options = CompileOptions {
        no_borrow_inference: true,
    };
    let result_without = compile_source_with_options(source, "test.rts", &options);
    assert!(!result_without.has_errors);
    assert!(
        result_without
            .rust_source
            .contains("fn shout(name: String)"),
        "without borrow inference should use String: {}",
        result_without.rust_source,
    );
}

// ===========================================================================
// Task 047: Borrow-to-owned context crossing fixes
// ===========================================================================

// ---------------------------------------------------------------------------
// Bug 1: Array index with variable needs `as usize` cast
//
// `arr[i]` where `i: i32` must emit `arr[i as usize]`.
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p4_array_index_usize_cast() {
    let source = "\
function getItem(arr: Array<string>, i: i32): string {
  return arr[i];
}";
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("i as usize"),
        "variable index should be cast to usize: {rust}"
    );
    // Literal index should NOT be cast
    let source2 = "\
function getFirst(arr: Array<string>): string {
  return arr[0];
}";
    let rust2 = compile_to_rust(source2);
    assert!(
        !rust2.contains("as usize"),
        "literal index should not be cast: {rust2}"
    );
}

// ---------------------------------------------------------------------------
// Bug 2: for-of loop variable is &T, needs clone on return
//
// `for (const u of users) { return u; }` must emit `u.clone()` in Some().
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p4_for_of_return_clones_reference() {
    let source = "\
class User {
  constructor(public id: i32, public name: string) {}
}

function findUser(users: Array<User>, targetId: i32): User | null {
  for (const u of users) {
    if (u.id === targetId) {
      return u;
    }
  }
  return null;
}";
    let rust = compile_to_rust(source);
    // The return value should be cloned since u is &User in the for loop
    assert!(
        rust.contains("u.clone()"),
        "for-of loop variable should be cloned on return: {rust}"
    );
    assert!(
        rust.contains("Some(u.clone())"),
        "return in Option context should wrap cloned value in Some: {rust}"
    );
}

// ---------------------------------------------------------------------------
// Bug 3: .find() returns Option<T>, should not double-wrap in Some()
//
// `return users.find(u => u.id === id)` should NOT emit `Some(users.iter()...)`
// because .find().cloned() already returns Option<T>.
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p4_find_no_double_wrap() {
    let source = "\
class User {
  constructor(public id: i32, public name: string) {}
}

function findUser(users: Array<User>, targetId: i32): User | null {
  return users.find((u) => u.id === targetId);
}";
    let rust = compile_to_rust(source);
    // Should contain .find().cloned() — already returns Option<T>
    assert!(
        rust.contains(".find(") && rust.contains(".cloned()"),
        "find should emit .find().cloned(): {rust}"
    );
    // Should NOT double-wrap in Some()
    assert!(
        !rust.contains("Some(users.iter()"),
        "find result should not be wrapped in Some: {rust}"
    );
}

// ---------------------------------------------------------------------------
// Bug 4: Iterator closure parameter is &T, function expects T — needs clone
//
// `users.map(u => formatUser(u))` — formatUser takes User (owned),
// but u is &User in the .iter() closure. Should emit formatUser(u.clone()).
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p4_iterator_closure_arg_clone() {
    let source = "\
class User {
  constructor(public id: i32, public name: string) {}
}

function formatUser(u: User): string {
  return u.name;
}

function formatAll(users: Array<User>): Array<string> {
  return users.map((u) => formatUser(u));
}";
    let rust = compile_to_rust(source);
    // The closure param u is &User, but formatUser expects User (owned)
    // so we need u.clone()
    assert!(
        rust.contains("u.clone()"),
        "iterator closure arg should be cloned when passed to owned param: {rust}"
    );
}

// ---------------------------------------------------------------------------
// Regression: for-of with Copy type should NOT insert unnecessary clone
//
// `for (const n of numbers)` where numbers: Array<i32> — n is Copy,
// no clone needed.
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_p4_for_of_copy_type_no_clone() {
    let source = "\
function sumPositive(numbers: Array<i32>): i32 {
  let total: i32 = 0;
  for (const n of numbers) {
    if (n > 0) {
      total = total + n;
    }
  }
  return total;
}";
    let rust = compile_to_rust(source);
    // Copy types use deref pattern (&n), so no clone needed
    assert!(
        !rust.contains("n.clone()"),
        "Copy type for-of variable should not be cloned: {rust}"
    );
}

//! Phase 4 end-to-end tests — compile `.rts`, build with cargo, run, verify stdout.
//!
//! These tests are slow (each invokes `cargo run`) and are marked `#[ignore]`
//! so they only run in the full suite (`just test-all` / `--include-ignored`).
//!
//! Each test exercises multiple Phase 4 features composed together:
//! Tier 2 borrow analysis, inline Rust, derive macros, shared type.

mod test_utils;

use test_utils::compile_and_run;

// ===========================================================================
// 1. Borrow elimination — no unnecessary clones at runtime
//
// Features: Tier 2 borrow inference, clone elimination, string &str
// Correctness Scenario 2: len takes &str, no .clone() on s
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_borrow_no_clone_len() {
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

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "11\n11\n11");
}

// ===========================================================================
// 2. Struct debug-printed correctly via derived Debug
//
// Features: derive macros (Debug), println!("{}", ...) with struct
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_derive_debug_print() {
    let source = "\
type Point = { x: i32, y: i32 }

function main(): void {
  const p: Point = { x: 10, y: 20 };
  console.log(p.x);
  console.log(p.y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "10\n20");
}

// ===========================================================================
// 3. Inline Rust computes a value used by RustScript
//
// Features: inline Rust block, variable scoping, computation
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_inline_rust_computation() {
    let source = "\
function main(): void {
  const x: i32 = 42;
  const y: i32 = 58;
  rust {
    let sum = x + y;
    println!(\"sum = {}\", sum);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "sum = 100");
}

// ===========================================================================
// 4. Shared counter — shared<i32> with lock and print
//
// Features: shared<T> → Arc<Mutex<T>>, .lock() → .lock().unwrap()
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_shared_counter() {
    let source = "\
function main(): void {
  const counter: shared<i32> = shared(42);
  const guard = counter.lock();
  console.log(guard);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42");
}

// ===========================================================================
// 5. String literal passed without allocation to &str param
//
// Features: Tier 2 borrow inference, string literal optimization
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_borrow_string_literal() {
    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main(): void {
  greet(\"hello\");
  greet(\"world\");
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "hello\nworld");
}

// ===========================================================================
// 6. Mixed borrowed and owned params — function with both modes
//
// Features: Tier 2 borrow inference, mixed param modes, clone behavior
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_mixed_borrow_owned() {
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

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "hello\nhello");
}

// ===========================================================================
// 7. Borrow with string method — toUpperCase on &str param
//
// Features: Tier 2 borrow, string method lowering, return type
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_borrow_string_method_shout() {
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

// ===========================================================================
// 8. Borrow with template literal — &str in format!
//
// Features: Tier 2 borrow inference, template literal, string formatting
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_borrow_template_literal() {
    let source = "\
function greetFormal(name: string): string {
  return `Hello, ${name}!`;
}

function main(): void {
  const msg: string = greetFormal(\"World\");
  console.log(msg);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Hello, World!");
}

// ===========================================================================
// 9. Module-level inline Rust helper function called from inline Rust
//
// Features: module-level rust block, function-level rust block, composition
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_module_inline_rust_helper() {
    let source = "\
rust {
  fn double(x: i32) -> i32 {
    x * 2
  }
}

function main(): void {
  rust {
    let result = double(21);
    println!(\"{}\", result);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42");
}

// ===========================================================================
// 10. Enum with derive + match — full enum pipeline
//
// Features: enum definition, switch/match, derive macros, function
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_enum_derive_match() {
    let source = "type Color = \"red\" | \"green\" | \"blue\"\n\nfunction label(c: Color): string {\n  switch (c) {\n    case \"red\": return \"Red\";\n    case \"green\": return \"Green\";\n    case \"blue\": return \"Blue\";\n  }\n}\n\nfunction main(): void {\n  const c: Color = \"red\";\n  console.log(label(c));\n}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Red");
}

// ===========================================================================
// 11. Struct + derives + Tier 1 clone — struct used in multiple calls
//
// Features: derive Clone, Tier 1 clone insertion, struct construction
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_struct_derive_clone_multi_use() {
    let source = "\
type Point = { x: f64, y: f64 }

function showX(p: Point): void {
  console.log(p.x);
}

function showY(p: Point): void {
  console.log(p.y);
}

function main(): void {
  const p: Point = { x: 3.14, y: 2.72 };
  showX(p);
  showY(p);
}";

    let stdout = compile_and_run(source);
    // f64 display: 3.14 and 2.72
    assert_eq!(stdout.trim(), "3.14\n2.72");
}

// ===========================================================================
// 12. --no-borrow-inference with e2e — both modes produce correct output
//
// Features: CompileOptions, Tier 1 vs Tier 2, runtime correctness
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_no_borrow_inference_both_correct() {
    use rustscript_driver::{CompileOptions, compile_source_with_options};

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

    // With borrow inference (default)
    let with_borrow = compile_and_run(source);
    assert_eq!(with_borrow.trim(), "5\n5");

    // Without borrow inference — should also compile and produce same output
    let options = CompileOptions {
        no_borrow_inference: true,
        ..CompileOptions::default()
    };
    let result = compile_source_with_options(source, "test.rts", &options);
    assert!(!result.has_errors);

    // Build and run the no-borrow version
    let rust_source = result.rust_source;
    assert!(
        rust_source.contains("fn len(s: String)"),
        "no-borrow should use String: {rust_source}"
    );

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
    assert_eq!(stdout.trim(), "5\n5");
}

// ===========================================================================
// Task 047: Borrow-to-owned context crossing — full demo program
//
// Exercises all four fixed bugs together:
// - Array index with variable (usize cast)
// - for-of return with clone
// - .find() returning Option without double-wrap
// - Iterator closure parameter clone for function calls
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p4_borrow_edge_cases_demo() {
    let source = r#"
class User {
  constructor(public id: i32, public name: string) {}
}

function formatUser(u: User): string {
  return u.name;
}

function findByIndex(users: Array<User>, i: i32): string {
  return users[i].name;
}

function findByLoop(users: Array<User>, targetId: i32): User | null {
  for (const u of users) {
    if (u.id === targetId) {
      return u;
    }
  }
  return null;
}

function findByFind(users: Array<User>, targetId: i32): User | null {
  return users.find((u) => u.id === targetId);
}

function formatAll(users: Array<User>): Array<string> {
  return users.map((u) => formatUser(u));
}

function main(): void {
  const users: Array<User> = [new User(1, "Alice"), new User(2, "Bob"), new User(3, "Charlie")];

  console.log(findByIndex(users, 1));

  const loopResult: User | null = findByLoop(users, 2);
  if (loopResult !== null) {
    console.log(loopResult.name);
  }

  const findResult: User | null = findByFind(users, 3);
  if (findResult !== null) {
    console.log(findResult.name);
  }

  const names: Array<string> = formatAll(users);
  console.log(names[0]);
}
"#;
    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Bob\nBob\nCharlie\nAlice");
}

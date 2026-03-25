//! Phase 2 composition tests — verify that multiple Phase 2 features work
//! correctly when combined together.
//!
//! These are fast tests (string comparison, no cargo invocation). Each test
//! exercises at least 3 Phase 2 features in combination.

mod test_utils;

use test_utils::{compile_result, compile_to_rust};

// ===========================================================================
// 1. Async + collections — async function returning array, caller awaits + iterates
//
// Features: async, Array<string>, await, .length → .len()
// ===========================================================================

#[test]
fn test_composition_p2_async_array_return_compiles() {
    let source = r#"async function getNames(): Array<string> {
  return ["Alice", "Bob"];
}

async function main() {
  const names = await getNames();
  console.log(names.length);
}"#;

    let result = compile_result(source);
    assert!(result.needs_async_runtime, "should need async runtime");
    assert!(
        result.rust_source.contains("async fn getNames()"),
        "should contain async fn"
    );
    assert!(
        result.rust_source.contains("Vec<String>"),
        "should contain Vec<String>"
    );
    assert!(
        result.rust_source.contains(".await"),
        "should contain .await"
    );
    assert!(
        result.rust_source.contains(".len()"),
        ".length should become .len()"
    );
}

// ===========================================================================
// 2. Async + error handling — async function with throws
//
// Features: async, throws → Result, Ok wrapping
// ===========================================================================

#[test]
fn test_composition_p2_async_throws_generates_result() {
    let source = r#"async function riskyFetch(): string throws string {
  return "success";
}"#;

    let result = compile_result(source);
    assert!(
        result.rust_source.contains("Result<String, String>"),
        "throws should produce Result type"
    );
    assert!(
        result.rust_source.contains("Ok("),
        "return should be wrapped in Ok"
    );
    assert!(result.rust_source.contains("async fn"), "should be async");
}

// ===========================================================================
// 3. String methods + template literal composition
//
// Features: toUpperCase, template literal → format!, console.log
// ===========================================================================

#[test]
fn test_composition_p2_string_method_template_literal() {
    let source = r#"function main() {
  const name = "alice";
  const upper = name.toUpperCase();
  const msg = `Hello, ${upper}!`;
  console.log(msg);
}"#;

    let rust = compile_to_rust(source);
    assert!(
        rust.contains(".to_uppercase()"),
        "toUpperCase should become to_uppercase"
    );
    assert!(
        rust.contains("format!("),
        "template literal should become format!"
    );
    assert!(
        rust.contains("println!("),
        "console.log should become println!"
    );
}

// ===========================================================================
// 4. Async + spawn — main spawns tasks with Promise.all
//
// Features: async main, spawn → tokio::spawn, Promise.all → tokio::join!
// ===========================================================================

#[test]
fn test_composition_p2_async_spawn_promise_all() {
    let source = r#"async function main() {
  const [a, b] = await Promise.all([
    fetchA(),
    fetchB(),
  ]);
  spawn(async () => {
    console.log("background");
  });
  console.log(a);
}

async function fetchA(): string {
  return "alpha";
}

async function fetchB(): string {
  return "beta";
}"#;

    let result = compile_result(source);
    assert!(result.needs_async_runtime, "should need async runtime");
    assert!(
        result.rust_source.contains("tokio::join!"),
        "Promise.all should become tokio::join!"
    );
    assert!(
        result.rust_source.contains("tokio::spawn"),
        "spawn should become tokio::spawn"
    );
    assert!(
        result.rust_source.contains("#[tokio::main]"),
        "async main should have #[tokio::main]"
    );
}

// ===========================================================================
// 5. Crate import + function usage
//
// Features: import statement, use declaration, crate dependency tracking
// ===========================================================================

#[test]
fn test_composition_p2_crate_import_with_usage() {
    let source = r#"import { HashMap } from "std::collections";
function main() {
  const map = new HashMap();
  console.log("ok");
}"#;

    let result = compile_result(source);
    assert!(
        result.rust_source.contains("use std::collections::HashMap"),
        "should contain use statement"
    );
    assert!(
        result.rust_source.contains("HashMap::new()"),
        "new HashMap() should become HashMap::new()"
    );
    assert!(
        !result.crate_dependencies.is_empty(),
        "should track crate dependency"
    );
}

// ===========================================================================
// 6. Full async composition — async + string + collections + await + template
//
// Features: async main, await, toUpperCase, Array, template literal
// ===========================================================================

#[test]
fn test_composition_p2_full_async_string_array() {
    let source = r#"async function processName(name: string): string {
  const upper = name.toUpperCase();
  return upper;
}

async function main() {
  const names: Array<string> = ["alice", "bob"];
  const first = names[0];
  const result = await processName(first);
  console.log(result);
}"#;

    let result = compile_result(source);
    assert!(result.needs_async_runtime, "should need async runtime");
    assert!(
        result.rust_source.contains("async fn processName"),
        "should contain async function"
    );
    assert!(
        result.rust_source.contains("Vec<String>"),
        "Array<string> should become Vec<String>"
    );
    assert!(
        result.rust_source.contains(".to_uppercase()"),
        "toUpperCase should lower"
    );
    assert!(
        result.rust_source.contains(".await"),
        "should contain .await"
    );
}

// ===========================================================================
// 7. Async + struct + await + field access
//
// Features: type definition, async function, struct return, await, field access
// ===========================================================================

#[test]
fn test_composition_p2_async_struct_field_access() {
    let source = r#"type User = { name: string, age: u32 }

async function getUser(): User {
  return { name: "Alice", age: 30 };
}

async function main() {
  const user = await getUser();
  console.log(user.name);
}"#;

    let result = compile_result(source);
    assert!(
        result.rust_source.contains("struct User"),
        "should define User struct"
    );
    assert!(
        result.rust_source.contains("async fn getUser() -> User"),
        "should return User"
    );
    assert!(
        result.rust_source.contains("getUser().await"),
        "should await"
    );
    assert!(
        result.rust_source.contains("user.name"),
        "should access field"
    );
}

// ===========================================================================
// 8. Non-async regression — verify Phase 1 features unaffected
//
// Features: type def, function, struct, template literal (Phase 1 features)
// ===========================================================================

#[test]
fn test_composition_p2_phase1_regression_struct_template() {
    let source = r#"type Point = { x: f64, y: f64 }

function describe(p: Point): string {
  return `(${p.x}, ${p.y})`;
}

function main() {
  const p: Point = { x: 1.0, y: 2.0 };
  console.log(describe(p));
}"#;

    let result = compile_result(source);
    assert!(
        !result.needs_async_runtime,
        "non-async should not need async runtime"
    );
    assert!(
        result.crate_dependencies.is_empty(),
        "should have no crate dependencies"
    );
    assert!(
        result.rust_source.contains("struct Point"),
        "Phase 1 struct should still work"
    );
    assert!(
        result.rust_source.contains("format!("),
        "Phase 1 template literal should still work"
    );
}

// ===========================================================================
// 9. Multiple async functions + sync functions — mixed file
//
// Features: async/non-async coexistence, multiple functions, #[tokio::main]
// ===========================================================================

#[test]
fn test_composition_p2_mixed_async_sync_functions() {
    let source = r#"function add(a: i32, b: i32): i32 {
  return a + b;
}

async function fetchValue(): i32 {
  return 42;
}

async function main() {
  const val = await fetchValue();
  const sum = add(val, 8);
  console.log(sum);
}"#;

    let result = compile_result(source);
    assert!(result.needs_async_runtime, "should need async runtime");
    assert!(
        result.rust_source.contains("fn add("),
        "sync function should not be async"
    );
    assert!(
        !result.rust_source.contains("async fn add("),
        "sync function should NOT be marked async"
    );
    assert!(
        result.rust_source.contains("async fn fetchValue"),
        "async function should be async"
    );
    assert!(
        result.rust_source.contains("#[tokio::main]"),
        "main should have tokio attribute"
    );
}

// ===========================================================================
// 10. String method variety — all 9 string methods in one program
//
// Features: toUpperCase, toLowerCase, startsWith, endsWith, includes,
//           trim, split, replace, length
// ===========================================================================

#[test]
fn test_composition_p2_all_string_methods() {
    let source = r#"function main() {
  const name = "  Hello World  ";
  const trimmed = name.trim();
  const upper = trimmed.toUpperCase();
  const lower = trimmed.toLowerCase();
  const starts = trimmed.startsWith("Hello");
  const ends = trimmed.endsWith("World");
  const has = trimmed.includes("lo");
  const replaced = trimmed.replace("World", "Rust");
  const parts = trimmed.split(" ");
  const len = trimmed.length;
  console.log(upper);
}"#;

    let rust = compile_to_rust(source);
    assert!(rust.contains(".trim()"), "trim should lower");
    assert!(rust.contains(".to_uppercase()"), "toUpperCase should lower");
    assert!(rust.contains(".to_lowercase()"), "toLowerCase should lower");
    assert!(rust.contains(".starts_with("), "startsWith should lower");
    assert!(rust.contains(".ends_with("), "endsWith should lower");
    assert!(rust.contains(".contains("), "includes should lower");
    assert!(rust.contains(".replace("), "replace should lower");
    assert!(rust.contains(".split("), "split should lower");
    assert!(rust.contains(".len()"), "length should become len()");
}

// ===========================================================================
// 11. Async + template literal + string interpolation
//
// Features: async, template literal with expressions, string concat
// ===========================================================================

#[test]
fn test_composition_p2_async_template_literal_expressions() {
    let source = r#"async function main() {
  const a: i32 = 5;
  const b: i32 = 3;
  console.log(`${a} + ${b} = ${a + b}`);
}"#;

    let result = compile_result(source);
    assert!(result.needs_async_runtime, "should need async runtime");
    assert!(
        result.rust_source.contains("format!("),
        "template literal should become format!"
    );
    assert!(
        result.rust_source.contains("#[tokio::main]"),
        "async main needs tokio"
    );
}

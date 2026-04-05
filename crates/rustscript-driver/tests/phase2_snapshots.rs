//! Phase 2 integration snapshot tests — compile `.rts` source and compare
//! generated `.rs` against golden output.
//!
//! These are fast tests (no cargo invocation). They validate that the compiler
//! produces the expected Rust output for Phase 2 feature combinations.
//!
//! Each test exercises at least 3 Phase 2 features together.

mod test_utils;

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
// 1. Async basic — async function with string return
//
// Features: async function, string type, return expression
// ===========================================================================

#[test]
fn test_snapshot_p2_async_basic_string_return() {
    let source = r#"async function greet(): string {
  return "hello";
}"#;

    let expected = r#"async fn greet() -> String {
    return "hello".to_string();
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_basic", &actual, expected);
}

// ===========================================================================
// 2. Await expression — async function calling another async function
//
// Features: async function, await, function call
// ===========================================================================

#[test]
fn test_snapshot_p2_await_expression_generates_dot_await() {
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

// ===========================================================================
// 3. Promise.all — concurrent execution with tuple destructuring
//
// Features: async, await, Promise.all → tokio::join!, tuple destructuring, multiple async fns
// ===========================================================================

#[test]
fn test_snapshot_p2_promise_all_generates_tokio_join() {
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
    assert_snapshot("promise_all", &actual, expected);
}

// ===========================================================================
// 4. Spawn task — tokio::spawn with async closure
//
// Features: async, spawn → tokio::spawn, async closure, console.log
// ===========================================================================

#[test]
fn test_snapshot_p2_spawn_task_generates_tokio_spawn() {
    let source = r#"async function main() {
  spawn(async () => {
    console.log("in task");
  });
  console.log("spawned");
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    tokio::spawn(async move {
        println!("{}", "in task");
    });
    println!("{}", "spawned");
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("spawn_task", &actual, expected);
}

// ===========================================================================
// 5. String methods — variety of TS string methods to Rust
//
// Features: toUpperCase, toLowerCase, startsWith, includes, trim
// ===========================================================================

#[test]
fn test_snapshot_p2_string_methods_variety() {
    let source = r#"function main() {
  const name = "Alice";
  const upper = name.toUpperCase();
  const lower = name.toLowerCase();
  const starts = name.startsWith("A");
  console.log(upper);
}"#;

    let expected = r#"fn main() {
    let name = "Alice".to_string();
    let upper = name.to_uppercase();
    let lower = name.to_lowercase();
    let starts = name.starts_with("A");
    println!("{}", upper);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("string_methods", &actual, expected);
}

// ===========================================================================
// 6. Crate import — external crate use statement
//
// Features: import statement, use declaration, crate dependency tracking
// ===========================================================================

#[test]
fn test_snapshot_p2_crate_import_generates_use_statement() {
    let source = r#"import { HashMap } from "std::collections";
function main() {
  console.log("imported");
}"#;

    let expected = r#"use std::collections::HashMap;

fn main() {
    println!("{}", "imported");
}
"#;

    let result = compile_result(source);
    assert_snapshot("crate_import", &result.rust_source, expected);
    assert!(
        !result.crate_dependencies.is_empty(),
        "expected crate dependency for std::collections"
    );
}

// ===========================================================================
// 7. Async throws — async function returning Result
//
// Features: async, throws → Result, Ok wrapping
// ===========================================================================

#[test]
fn test_snapshot_p2_async_throws_generates_result_return() {
    let source = r#"async function riskyFetch(): string throws string {
  return "success";
}"#;

    let expected = r#"async fn riskyFetch() -> Result<String, String> {
    return Ok("success".to_string());
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_throws", &actual, expected);
}

// ===========================================================================
// 8. Async + string methods + template literal composition
//
// Features: async main, string method (toUpperCase), template literal (format!), console.log
// ===========================================================================

#[test]
fn test_snapshot_p2_async_string_template_composition() {
    let source = r#"async function main() {
  const name = "hello world";
  const upper = name.toUpperCase();
  const msg = `Result: ${upper}`;
  console.log(msg);
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    let name = "hello world".to_string();
    let upper = name.to_uppercase();
    let msg = format!("Result: {}", upper);
    println!("{}", msg);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_string_template", &actual, expected);
}

// ===========================================================================
// 9. Async + collections — async function returning array
//
// Features: async, Array<string> → Vec<String>, await, .length → .len()
// ===========================================================================

#[test]
fn test_snapshot_p2_async_collections_array_return() {
    let source = r#"async function getNames(): Array<string> {
  return ["Alice", "Bob"];
}

async function main() {
  const names = await getNames();
  console.log(names.length);
}"#;

    let expected = r#"async fn getNames() -> Vec<String> {
    return vec!["Alice".to_string(), "Bob".to_string()];
}

#[tokio::main]
async fn main() {
    let names = getNames().await;
    println!("{}", names.len() as i64);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_collections", &actual, expected);
}

// ===========================================================================
// 10. Spawn + Promise.all composition
//
// Features: async, Promise.all → tokio::join!, spawn → tokio::spawn,
//           multiple async functions, tuple destructuring
// ===========================================================================

#[test]
fn test_snapshot_p2_spawn_promise_all_composition() {
    let source = r#"async function main() {
  const [a, b] = await Promise.all([
    fetchA(),
    fetchB(),
  ]);
  spawn(async () => {
    console.log("background");
  });
  console.log(a);
  console.log(b);
}

async function fetchA(): string {
  return "alpha";
}

async function fetchB(): string {
  return "beta";
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    let (a, b) = tokio::join!(fetchA(), fetchB());
    tokio::spawn(async move {
        println!("{}", "background");
    });
    println!("{}", a);
    println!("{}", b);
}

async fn fetchA() -> String {
    return "alpha".to_string();
}

async fn fetchB() -> String {
    return "beta".to_string();
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("spawn_promise_all", &actual, expected);
}

// ===========================================================================
// 11. Full async composition — async + string methods + array + await
//
// Features: async main, async function, string method (toUpperCase),
//           Array<string>, array indexing, await
// ===========================================================================

#[test]
fn test_snapshot_p2_full_async_composition() {
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

    let expected = r#"async fn processName(name: &str) -> String {
    let upper = name.to_uppercase();
    return upper;
}

#[tokio::main]
async fn main() {
    let names: Vec<String> = vec!["alice".to_string(), "bob".to_string()];
    let first = names[0];
    let result = processName(&first).await;
    println!("{}", result);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("full_async_composition", &actual, expected);
}

// ===========================================================================
// 12. needs_async_runtime flag — verify flag is set for async, unset for sync
//
// Features: CompileResult metadata, async detection
// ===========================================================================

#[test]
fn test_snapshot_p2_needs_async_runtime_flag_set() {
    let async_source = r#"async function main() {
  console.log("async");
}"#;

    let sync_source = r#"function main() {
  console.log("sync");
}"#;

    let async_result = compile_result(async_source);
    assert!(
        async_result.needs_async_runtime,
        "async source should set needs_async_runtime"
    );

    let sync_result = compile_result(sync_source);
    assert!(
        !sync_result.needs_async_runtime,
        "sync source should not set needs_async_runtime"
    );
}

// ===========================================================================
// 13. String method chaining — toUpperCase().startsWith()
//
// Features: string method chaining, method composition
// ===========================================================================

#[test]
fn test_snapshot_p2_string_method_chaining() {
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

// ===========================================================================
// 14. String split method — produces iterator chain
//
// Features: split → split().map().collect(), string method
// ===========================================================================

#[test]
fn test_snapshot_p2_string_split_method() {
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
    assert_snapshot("string_split", &actual, expected);
}

// ===========================================================================
// 15. String replace method
//
// Features: replace method, string manipulation
// ===========================================================================

#[test]
fn test_snapshot_p2_string_replace_method() {
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
    assert_snapshot("string_replace", &actual, expected);
}

// ===========================================================================
// 16. Async function does not change non-async functions (regression)
//
// Features: async/non-async coexistence, regression validation
// ===========================================================================

#[test]
fn test_snapshot_p2_non_async_function_unchanged() {
    let source = r#"function add(a: i32, b: i32): i32 {
  return a + b;
}

async function fetchValue(): i32 {
  return 42;
}"#;

    let expected = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b;
}

async fn fetchValue() -> i32 {
    return 42;
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_non_async_coexistence", &actual, expected);
}

// ===========================================================================
// 17. Crate import with function usage
//
// Features: import statement, use declaration, crate dependencies in metadata
// ===========================================================================

#[test]
fn test_snapshot_p2_crate_import_tracks_dependencies() {
    let source = r#"import { HashMap } from "std::collections";
function main() {
  console.log("imported");
}"#;

    let result = compile_result(source);
    assert!(
        result
            .crate_dependencies
            .iter()
            .any(|d| d.name == "std::collections"),
        "expected std::collections in crate dependencies, got: {:?}",
        result
            .crate_dependencies
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
}

// ===========================================================================
// 18. Async closure — async arrow function expression
//
// Features: async closure, await inside closure
// ===========================================================================

#[test]
fn test_snapshot_p2_async_closure_generates_async_block() {
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

// ===========================================================================
// 19. Async main generates #[tokio::main] attribute
//
// Features: async main, #[tokio::main] attribute injection
// ===========================================================================

#[test]
fn test_snapshot_p2_async_main_generates_tokio_main_attribute() {
    let source = r#"async function main() {
  console.log("hello async");
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    println!("{}", "hello async");
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_main_tokio", &actual, expected);
}

// ===========================================================================
// 20. String length property
//
// Features: .length → .len(), property-to-method lowering
// ===========================================================================

#[test]
fn test_snapshot_p2_string_length_property() {
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
    assert_snapshot("string_length", &actual, expected);
}

// ===========================================================================
// 21. Async + multiple string methods
//
// Features: async, toUpperCase, trim, template literal, string composition
// ===========================================================================

#[test]
fn test_snapshot_p2_async_multiple_string_methods() {
    let source = r#"async function formatName(raw: string): string {
  const trimmed = raw.trim();
  const upper = trimmed.toUpperCase();
  return upper;
}

async function main() {
  const result = await formatName("  alice  ");
  console.log(result);
}"#;

    let expected = r#"async fn formatName(raw: &str) -> String {
    let trimmed = raw.trim().to_string();
    let upper = trimmed.to_uppercase();
    return upper;
}

#[tokio::main]
async fn main() {
    let result = formatName("  alice  ").await;
    println!("{}", result);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_multiple_string_methods", &actual, expected);
}

// ===========================================================================
// 22. Async + struct + await
//
// Features: async, type definition, struct construction, await, field access
// ===========================================================================

#[test]
fn test_snapshot_p2_async_struct_await() {
    let source = r#"type User = { name: string, age: u32 }

async function getUser(): User {
  return { name: "Alice", age: 30 };
}

async function main() {
  const user = await getUser();
  console.log(user.name);
}"#;

    let expected = r#"#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
}

async fn getUser() -> User {
    return User { name: "Alice".to_string(), age: 30 };
}

#[tokio::main]
async fn main() {
    let user = getUser().await;
    println!("{}", user.name);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_struct_await", &actual, expected);
}

// ===========================================================================
// 23. String endsWith and toLowerCase
//
// Features: endsWith → ends_with, toLowerCase → to_lowercase
// ===========================================================================

#[test]
fn test_snapshot_p2_string_ends_with_and_to_lower_case() {
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
    assert_snapshot("string_ends_with_to_lower", &actual, expected);
}

// ===========================================================================
// 24. Async + throws + function composition
//
// Features: async, throws → Result, Ok wrapping, multiple async functions
// ===========================================================================

#[test]
fn test_snapshot_p2_async_throws_composition() {
    let source = r#"async function fetchUser(): string throws string {
  return "Alice";
}

async function fetchAge(): i32 throws string {
  return 30;
}"#;

    let expected = r#"async fn fetchUser() -> Result<String, String> {
    return Ok("Alice".to_string());
}

async fn fetchAge() -> Result<i32, String> {
    return Ok(30);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("async_throws_composition", &actual, expected);
}

// ---------------------------------------------------------------------------
// Task 066: Async iteration and Promise methods
// ---------------------------------------------------------------------------

// T066-1: for await → while let Some(item) = stream.next().await
#[test]
fn test_snapshot_t066_for_await_generates_while_let_next_await() {
    let source = r#"async function main() {
  for await (const msg of channel) {
    console.log(msg);
  }
}"#;

    let expected = r#"use futures::StreamExt;

#[tokio::main]
async fn main() {
    while let Some(msg) = channel.next().await {
        println!("{}", msg);
    }
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("for_await", &actual, expected);
}

// T066-2: Promise.race → tokio::select!
#[test]
fn test_snapshot_t066_promise_race_generates_tokio_select() {
    let source = r#"async function main() {
  const first = await Promise.race([fetch1(), fetch2()]);
  console.log(first);
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    let first = tokio::select! { result = fetch1() => result, result = fetch2() => result };
    println!("{}", first);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("promise_race", &actual, expected);
}

// T066-3: Promise.any → futures::future::select_ok
#[test]
fn test_snapshot_t066_promise_any_generates_futures_select_ok() {
    let source = r#"async function main() {
  const first = await Promise.any([tryA(), tryB()]);
  console.log(first);
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    let first = futures::future::select_ok(vec![tryA(), tryB()]).await.expect("all promises rejected").0;
    println!("{}", first);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("promise_any", &actual, expected);
}

// T066-4: for await with let binding
#[test]
fn test_snapshot_t066_for_await_let_binding() {
    let source = r#"async function process() {
  for await (let item of stream) {
    console.log(item);
  }
}"#;

    let expected = r#"use futures::StreamExt;

async fn process() {
    while let Some(item) = stream.next().await {
        println!("{}", item);
    }
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("for_await_let", &actual, expected);
}

// T066-5: Promise.race with three branches
#[test]
fn test_snapshot_t066_promise_race_three_branches() {
    let source = r#"async function main() {
  const result = await Promise.race([a(), b(), c()]);
}"#;

    let expected = r#"#[tokio::main]
async fn main() {
    let result = tokio::select! { result = a() => result, result = b() => result, result = c() => result };
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("promise_race_three", &actual, expected);
}

// T066-6: needs_futures_crate flag is set for for-await
#[test]
fn test_t066_for_await_sets_needs_futures_crate() {
    let source = r#"async function main() {
  for await (const msg of channel) {
    console.log(msg);
  }
}"#;

    let result = test_utils::compile_result(source);
    assert!(
        result.needs_futures_crate,
        "for await should set needs_futures_crate"
    );
}

// T066-7: needs_futures_crate flag is set for Promise.any
#[test]
fn test_t066_promise_any_sets_needs_futures_crate() {
    let source = r#"async function main() {
  const first = await Promise.any([tryA(), tryB()]);
}"#;

    let result = test_utils::compile_result(source);
    assert!(
        result.needs_futures_crate,
        "Promise.any should set needs_futures_crate"
    );
}

// T066-8: needs_async_runtime is set for Promise.race
#[test]
fn test_t066_promise_race_sets_needs_async_runtime() {
    let source = r#"async function main() {
  const first = await Promise.race([a(), b()]);
}"#;

    let result = test_utils::compile_result(source);
    assert!(
        result.needs_async_runtime,
        "Promise.race should set needs_async_runtime"
    );
}

// ===========================================================================
// Promise.all + throws — auto-unwrap Results from tokio::join!
//
// When Promise.all elements call throwing functions, the tokio::join! results
// are Result values. In a throws context, each result must be unwrapped
// with `?` after the join.
// ===========================================================================

// Test: Promise.all with throwing functions in throws context generates unwrap
#[test]
fn test_snapshot_promise_all_throws_generates_unwrap() {
    let source = r#"async function fetchData(url: string): string throws string {
  return "data";
}

async function fetchAll(): void throws string {
  const [a, b] = await Promise.all([fetchData("/users"), fetchData("/posts")]);
  console.log(a);
}"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("let (a, b) = tokio::join!(fetchData("),
        "should contain tokio::join! destructure, got: {actual}"
    );
    assert!(
        actual.contains("let a = a?;"),
        "should unwrap a with ?, got: {actual}"
    );
    assert!(
        actual.contains("let b = b?;"),
        "should unwrap b with ?, got: {actual}"
    );
}

// Test: Promise.all with non-throwing functions — no unwrap
#[test]
fn test_snapshot_promise_all_non_throws_no_unwrap() {
    let source = r#"async function getUser(): string {
  return "alice";
}

async function getPosts(): string {
  return "posts";
}

async function main() {
  const [user, posts] = await Promise.all([getUser(), getPosts()]);
  console.log(user);
}"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("let (user, posts) = tokio::join!(getUser(), getPosts())"),
        "should contain tokio::join! destructure, got: {actual}"
    );
    assert!(
        !actual.contains("let user = user?;"),
        "should NOT unwrap non-throwing results, got: {actual}"
    );
    assert!(
        !actual.contains("let posts = posts?;"),
        "should NOT unwrap non-throwing results, got: {actual}"
    );
}

// Test: Promise.all with mixed throwing/non-throwing — only unwrap throwing
#[test]
fn test_snapshot_promise_all_mixed_throws_selective_unwrap() {
    let source = r#"async function safeFetch(): string {
  return "safe";
}

async function riskyFetch(url: string): string throws string {
  return "risky";
}

async function doWork(): void throws string {
  const [a, b] = await Promise.all([safeFetch(), riskyFetch("/api")]);
  console.log(a);
}"#;

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("let (a, b) = tokio::join!("),
        "should contain tokio::join! destructure, got: {actual}"
    );
    assert!(
        !actual.contains("let a = a?;"),
        "should NOT unwrap non-throwing element a, got: {actual}"
    );
    assert!(
        actual.contains("let b = b?;"),
        "should unwrap throwing element b, got: {actual}"
    );
}

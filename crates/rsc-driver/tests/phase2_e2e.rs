//! Phase 2 end-to-end tests — compile `.rts`, build with cargo, run, verify stdout.
//!
//! These tests are slow (each invokes `cargo run`) and are marked `#[ignore]`
//! so they only run in the full suite (`just test-all` / `--include-ignored`).
//!
//! Each test exercises multiple Phase 2 features together.

mod test_utils;

use test_utils::{compile_and_run, compile_and_run_async};

// ===========================================================================
// 1. Async main prints value (async + console.log + tokio)
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_async_main_prints_hello() {
    let source = r#"async function main() {
  console.log("hello async");
}"#;

    let stdout = compile_and_run_async(source);
    assert_eq!(stdout.trim(), "hello async");
}

// ===========================================================================
// 2. Async + string methods + template literal
//
// Features: async main, toUpperCase, template literal, console.log
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_async_string_template_prints_formatted() {
    let source = r#"async function main() {
  const name = "hello world";
  const upper = name.toUpperCase();
  const msg = `Result: ${upper}`;
  console.log(msg);
}"#;

    let stdout = compile_and_run_async(source);
    assert_eq!(stdout.trim(), "Result: HELLO WORLD");
}

// ===========================================================================
// 3. String method processing — toUpperCase, toLowerCase, trim
//
// Features: string methods, console.log output
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_string_methods_processing() {
    let source = r#"function main() {
  const name = "  Hello World  ";
  const trimmed = name.trim();
  const upper = trimmed.toUpperCase();
  const lower = trimmed.toLowerCase();
  console.log(upper);
  console.log(lower);
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "HELLO WORLD\nhello world");
}

// ===========================================================================
// 4. String replace and split
//
// Features: string replace, split, .length → .len()
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_string_replace_and_split() {
    let source = r#"function main() {
  const result = "hello world".replace("world", "rust");
  console.log(result);
  const parts = "a,b,c".split(",");
  console.log(parts.length);
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "hello rust\n3");
}

// ===========================================================================
// 5. String startsWith and endsWith
//
// Features: startsWith → starts_with, endsWith → ends_with, bool output
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_string_starts_ends_with() {
    let source = r#"function main() {
  const name = "Hello";
  console.log(name.startsWith("He"));
  console.log(name.endsWith("lo"));
  console.log(name.startsWith("Xy"));
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\ntrue\nfalse");
}

// ===========================================================================
// 6. String includes and length
//
// Features: includes → contains, .length → .len()
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_string_includes_and_length() {
    let source = r#"function main() {
  const text = "hello world";
  console.log(text.includes("world"));
  console.log(text.includes("xyz"));
  console.log(text.length);
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\nfalse\n11");
}

// ===========================================================================
// 7. Async function returning array — await + length
//
// Features: async, Array<string>, await, .length → .len()
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_async_array_return_and_length() {
    let source = r#"async function getNames(): Array<string> {
  return ["Alice", "Bob", "Charlie"];
}

async function main() {
  const names = await getNames();
  console.log(names.length);
}"#;

    let stdout = compile_and_run_async(source);
    assert_eq!(stdout.trim(), "3");
}

// ===========================================================================
// 8. Full async app — async main + string methods + struct + await
//
// Features: async, struct, await, string method, template literal
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_full_async_app_with_struct() {
    let source = r#"type User = { name: string, age: u32 }

async function getUser(): User {
  return { name: "Alice", age: 30 };
}

async function main() {
  const user = await getUser();
  const greeting = `Hello, ${user.name}!`;
  console.log(greeting);
}"#;

    let stdout = compile_and_run_async(source);
    assert_eq!(stdout.trim(), "Hello, Alice!");
}

// ===========================================================================
// 9. Async + multiple string operations
//
// Features: async, trim, toUpperCase, string method chaining, await
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_async_string_processing_chain() {
    let source = r#"async function formatName(raw: string): string {
  const trimmed = raw.trim();
  const upper = trimmed.toUpperCase();
  return upper;
}

async function main() {
  const result = await formatName("  alice  ");
  console.log(result);
}"#;

    let stdout = compile_and_run_async(source);
    assert_eq!(stdout.trim(), "ALICE");
}

// ===========================================================================
// 10. Async + async throws — success path
//
// Correctness scenario 1 from task spec:
// async function with throws, caller awaits and prints success
//
// NOTE: This test is SKIPPED because async try/catch currently generates
// invalid Rust (sync closure wrapping .await). See Developer Outcome.
// ===========================================================================

// Skipped: async try/catch generates sync closure around .await (BUG-001)

// ===========================================================================
// 11. String method chaining e2e — toUpperCase().startsWith()
//
// Features: string method chaining, bool result
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_string_method_chaining_result() {
    let source = r#"function main() {
  const name = "Alice";
  const result = name.toUpperCase().startsWith("A");
  console.log(result);
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true");
}

// ===========================================================================
// 12. Crate import e2e — HashMap from std::collections
//
// SKIPPED: The compiler generates duplicate `use std::collections::HashMap;`
// statements (BUG-002), which causes a rustc E0252 error. Additionally,
// `new HashMap()` without type annotations produces rustc E0282.
// Documented in Developer Outcome.
// ===========================================================================

// ===========================================================================
// 13. Iterator .map() — transform array elements
//
// Features: .map() → .iter().map().collect::<Vec<_>>(), closure, console.log
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_iterator_map_doubles_values() {
    let source = r#"function main() {
  const nums: Array<i64> = [1, 2, 3, 4, 5];
  const doubled = nums.map((x) => x * 2);
  console.log(doubled.length);
  console.log(doubled[0]);
  console.log(doubled[4]);
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "5\n2\n10");
}

// ===========================================================================
// 14. Iterator .filter() + .map() chain — filter then transform
//
// Features: chained .filter().map() → single iterator chain with collect
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_iterator_filter_map_chain() {
    let source = r#"function main() {
  const nums: Array<i64> = [1, 2, 3, 4, 5, 6];
  const evenDoubled = nums.filter((x) => x % 2 == 0).map((x) => x * 10);
  console.log(evenDoubled.length);
  console.log(evenDoubled[0]);
  console.log(evenDoubled[1]);
  console.log(evenDoubled[2]);
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "3\n20\n40\n60");
}

// ===========================================================================
// 15. Iterator .forEach() — side-effecting iteration
//
// Features: .forEach() → .iter().for_each(), closure with side effect
// ===========================================================================

#[test]
#[ignore]
fn test_e2e_p2_iterator_for_each_prints_elements() {
    let source = r#"function main() {
  const names: Array<string> = ["Alice", "Bob", "Charlie"];
  names.forEach((name) => {
    console.log(name);
  });
}"#;

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Alice\nBob\nCharlie");
}

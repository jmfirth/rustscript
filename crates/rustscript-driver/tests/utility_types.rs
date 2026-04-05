//! Snapshot tests for TypeScript utility/mapped types.
//!
//! Validates that `Partial<T>`, `Required<T>`, `Readonly<T>`, `Record<K, V>`,
//! `Pick<T, K>`, and `Omit<T, K>` compile to the expected Rust output.

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
// 1. Partial<T> — all fields become Option
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_partial_wraps_fields_in_option() {
    let source = r#"type User = { name: string, age: u32 }
type PartialUser = Partial<User>"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PartialUser {
    pub name: Option<String>,
    pub age: Option<u32>,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("partial", &actual, expected);
}

// ---------------------------------------------------------------------------
// 2. Required<T> — strip Option from fields
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_required_unwraps_option_fields() {
    let source = r#"type Config = { name: string | null, debug: bool | null }
type FullConfig = Required<Config>"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    pub name: Option<String>,
    pub debug: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FullConfig {
    pub name: String,
    pub debug: bool,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("required", &actual, expected);
}

// ---------------------------------------------------------------------------
// 3. Readonly<T> — no-op (Rust fields are immutable by default)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_readonly_is_noop() {
    let source = r#"type Point = { x: f64, y: f64 }
type ReadonlyPoint = Readonly<Point>"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq)]
struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct ReadonlyPoint {
    pub x: f64,
    pub y: f64,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("readonly", &actual, expected);
}

// ---------------------------------------------------------------------------
// 4. Record<K, V> — type alias to HashMap
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_record_produces_hashmap_alias() {
    let source = r#"type StringMap = Record<string, i32>"#;

    let expected = "\
use std::collections::HashMap;

type StringMap = HashMap<String, i32>;
";

    let actual = compile_to_rust(source);
    assert_snapshot("record", &actual, expected);
}

// ---------------------------------------------------------------------------
// 5. Pick<T, K> — select subset of fields
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_pick_selects_fields() {
    let source = r#"type User = { name: string, age: u32, email: string }
type NameAndAge = Pick<User, "name" | "age">"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NameAndAge {
    pub name: String,
    pub age: u32,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("pick", &actual, expected);
}

// ---------------------------------------------------------------------------
// 6. Omit<T, K> — remove specified fields
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_omit_removes_fields() {
    let source = r#"type User = { name: string, age: u32, email: string }
type NoEmail = Omit<User, "email">"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NoEmail {
    pub name: String,
    pub age: u32,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("omit", &actual, expected);
}

// ---------------------------------------------------------------------------
// 7. Pick with single field
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_pick_single_field() {
    let source = r#"type User = { name: string, age: u32, email: string }
type NameOnly = Pick<User, "name">"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NameOnly {
    pub name: String,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("pick_single", &actual, expected);
}

// ---------------------------------------------------------------------------
// 8. Omit with multiple fields
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_omit_multiple_fields() {
    let source = r#"type User = { name: string, age: u32, email: string }
type NameOnly = Omit<User, "age" | "email">"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NameOnly {
    pub name: String,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("omit_multiple", &actual, expected);
}

// ---------------------------------------------------------------------------
// 9. Required on non-optional fields is identity
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_utility_required_on_non_optional_is_identity() {
    let source = r#"type Point = { x: f64, y: f64 }
type RequiredPoint = Required<Point>"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq)]
struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct RequiredPoint {
    pub x: f64,
    pub y: f64,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("required_identity", &actual, expected);
}

// ---------------------------------------------------------------------------
// 10. Compilation tests (ignored — require cargo)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_compile_utility_partial_struct_has_option_fields() {
    let source = r#"type User = { name: string, age: u32 }
type PartialUser = Partial<User>

function main() {
    const p: PartialUser = { name: null, age: null };
    console.log("ok");
}"#;

    let output = test_utils::compile_and_run(source);
    assert_eq!(output.trim(), "ok");
}

#[test]
#[ignore]
fn test_compile_utility_record_works_with_hashmap_ops() {
    let source = r#"type Scores = Record<string, i32>

function main() {
    let scores: Scores = new Map();
    scores.set("alice", 100);
    console.log(scores.get("alice"));
}"#;

    let output = test_utils::compile_and_run(source);
    assert!(output.contains("100") || output.contains("Some(100)"));
}

#[test]
#[ignore]
fn test_compile_utility_pick_produces_valid_struct() {
    let source = r#"type User = { name: string, age: u32, email: string }
type NameAndAge = Pick<User, "name" | "age">

function main() {
    const na: NameAndAge = { name: "Alice", age: 30 };
    console.log(na.name);
}"#;

    let output = test_utils::compile_and_run(source);
    assert_eq!(output.trim(), "Alice");
}

// ---------------------------------------------------------------------------
// 11. Diagnostic tests
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_utility_partial_unknown_type() {
    let diags = test_utils::compile_diagnostics(r#"type Foo = Partial<NonExistent>"#);
    assert!(
        diags
            .iter()
            .any(|d| d.contains("unknown type") || d.contains("NonExistent")),
        "expected diagnostic about unknown type, got: {diags:?}"
    );
}

#[test]
fn test_diagnostic_utility_pick_unknown_field() {
    let diags = test_utils::compile_diagnostics(
        r#"type User = { name: string, age: u32 }
type Bad = Pick<User, "nonexistent">"#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("unknown field") && d.contains("nonexistent")),
        "expected diagnostic about unknown field, got: {diags:?}"
    );
}

#[test]
fn test_diagnostic_utility_omit_unknown_field() {
    let diags = test_utils::compile_diagnostics(
        r#"type User = { name: string, age: u32 }
type Bad = Omit<User, "nonexistent">"#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("unknown field") && d.contains("nonexistent")),
        "expected diagnostic about unknown field, got: {diags:?}"
    );
}

// ---------------------------------------------------------------------------
// Task 155: Mapped types
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_mapped_type_nullable() {
    let source = r#"type User = { name: string, age: i32 }
type NullableUser = { [K in keyof User]: User[K] | null }"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NullableUser {
    pub name: Option<String>,
    pub age: Option<i32>,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("mapped_nullable", &actual, expected);
}

#[test]
fn test_snapshot_mapped_type_identity() {
    let source = r#"type Point = { x: f64, y: f64 }
type SamePoint = { [K in keyof Point]: Point[K] }"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq)]
struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct SamePoint {
    pub x: f64,
    pub y: f64,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("mapped_identity", &actual, expected);
}

#[test]
fn test_snapshot_mapped_type_optional_modifier() {
    // { [K in keyof T]?: V } makes all fields optional
    let source = r#"type User = { name: string, age: i32 }
type PartialUser = { [K in keyof User]?: User[K] }"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PartialUser {
    pub name: Option<String>,
    pub age: Option<i32>,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("mapped_optional", &actual, expected);
}

#[test]
fn test_snapshot_mapped_type_remove_optional() {
    // { [K in keyof T]-?: V } strips Option from fields
    let source = r#"type Config = { name: string | null, debug: bool | null }
type RequiredConfig = { [K in keyof Config]-?: Config[K] }"#;

    let expected = "\
#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    pub name: Option<String>,
    pub debug: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequiredConfig {
    pub name: String,
    pub debug: bool,
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("mapped_remove_optional", &actual, expected);
}

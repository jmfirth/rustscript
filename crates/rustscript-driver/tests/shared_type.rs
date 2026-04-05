//! Snapshot and diagnostic tests for the `shared<T>` type sugar (Task 051).
//!
//! Tests that `shared<T>` correctly desugars to `Arc<Mutex<T>>` and that
//! `shared(expr)` desugars to `Arc::new(Mutex::new(expr))`.

mod test_utils;

use test_utils::{compile_diagnostics, compile_to_rust};

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
// 1. shared<i32> type emits Arc<Mutex<i32>>
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_shared_i32_emits_arc_mutex_i32() {
    let source = "\
function main(): void {
  const counter: shared<i32> = shared(0);
}";

    let expected = "\
use std::sync::Arc;
use std::sync::Mutex;

fn main() {
    let counter: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("shared_i32", &actual, expected);
}

// ---------------------------------------------------------------------------
// 2. shared(0) constructor emits Arc::new(Mutex::new(0))
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_shared_constructor_emits_arc_new_mutex_new() {
    let source = "\
function main(): void {
  const data: shared<string> = shared(\"hello\");
}";

    let expected = "\
use std::sync::Arc;
use std::sync::Mutex;

fn main() {
    let data: Arc<Mutex<String>> = Arc::new(Mutex::new(\"hello\".to_string()));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("shared_constructor", &actual, expected);
}

// ---------------------------------------------------------------------------
// 3. .lock() method emits .lock().unwrap()
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_shared_lock_emits_lock_unwrap() {
    let source = "\
function main(): void {
  const counter: shared<i32> = shared(0);
  const guard = counter.lock();
}";

    let expected = "\
use std::sync::Arc;
use std::sync::Mutex;

fn main() {
    let counter: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));
    let guard = counter.lock().unwrap();
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("shared_lock", &actual, expected);
}

// ---------------------------------------------------------------------------
// 4. use std::sync::{Arc, Mutex} generated when shared types are used
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_shared_generates_use_declarations() {
    let source = "\
function main(): void {
  const x: shared<bool> = shared(true);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("use std::sync::Arc;"),
        "expected Arc use declaration in:\n{actual}"
    );
    assert!(
        actual.contains("use std::sync::Mutex;"),
        "expected Mutex use declaration in:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 5. shared<Array<string>> emits Arc<Mutex<Vec<String>>>
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_shared_generic_inner_type() {
    let source = "\
function main(): void {
  const items: shared<Array<string>> = shared([]);
}";

    let expected = "\
use std::sync::Arc;
use std::sync::Mutex;

fn main() {
    let items: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("shared_generic_inner", &actual, expected);
}

// ---------------------------------------------------------------------------
// 6. shared without type parameter produces diagnostic
// ---------------------------------------------------------------------------

#[test]
fn test_diagnostic_shared_without_type_param() {
    let source = "\
function main(): void {
  const x: shared = shared(0);
}";

    let diagnostics = compile_diagnostics(source);
    assert!(
        !diagnostics.is_empty(),
        "expected diagnostic for shared without type parameter"
    );
    assert!(
        diagnostics.iter().any(|d| d.contains("shared")),
        "expected shared-related diagnostic, got: {diagnostics:?}"
    );
}

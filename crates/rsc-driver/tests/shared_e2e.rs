//! End-to-end tests for the `shared<T>` type sugar (Task 051).
//!
//! These tests compile RustScript source all the way to a Rust binary,
//! run it, and verify the output. They are slow (invoke cargo) and are
//! marked `#[ignore]` by convention.

mod test_utils;

use test_utils::compile_and_run;

// ---------------------------------------------------------------------------
// 1. Shared counter incremented in main thread compiles and runs
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_shared_counter_basic() {
    let source = "\
function main(): void {
  const counter: shared<i32> = shared(42);
  const guard = counter.lock();
  console.log(guard);
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "42");
}

// ---------------------------------------------------------------------------
// 2. Shared value with .lock() produces correct output
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_shared_string_lock() {
    let source = "\
function main(): void {
  const data: shared<string> = shared(\"hello shared\");
  const guard = data.lock();
  console.log(guard);
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "hello shared");
}

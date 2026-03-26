//! End-to-end tests for tuple types (Task 064).
//!
//! These tests compile `.rts` to `.rs`, build with cargo, run, and check stdout.

mod test_utils;

use test_utils::compile_and_run;

// ---------------------------------------------------------------------------
// 1. Tuple construction and access
// ---------------------------------------------------------------------------

#[test]
#[ignore] // slow — invokes cargo
fn test_e2e_tuple_construction_and_access() {
    let source = "\
function main() {
  const pair: [string, i32] = [\"hello\", 42];
  console.log(pair[0]);
  console.log(pair[1]);
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "hello\n42");
}

// ---------------------------------------------------------------------------
// 2. Tuple swap function
// ---------------------------------------------------------------------------

#[test]
#[ignore] // slow — invokes cargo
fn test_e2e_tuple_swap_function() {
    let source = "\
function swap(pair: [i32, i32]): [i32, i32] {
  return [pair[1], pair[0]];
}

function main() {
  const result: [i32, i32] = swap([1, 2]);
  console.log(result[0]);
  console.log(result[1]);
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "2\n1");
}

// ---------------------------------------------------------------------------
// 3. Tuple destructuring
// ---------------------------------------------------------------------------

#[test]
#[ignore] // slow — invokes cargo
fn test_e2e_tuple_destructuring() {
    let source = "\
function main() {
  const pair: [string, i32] = [\"world\", 99];
  const [name, age] = pair;
  console.log(name);
  console.log(age);
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "world\n99");
}

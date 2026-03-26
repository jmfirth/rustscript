//! Snapshot tests for tuple types (Task 064).
//!
//! Tests tuple type annotations, tuple construction, tuple field access,
//! tuple destructuring, and tuples in function signatures.

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
// 1. Basic tuple construction with type annotation
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tuple_construction_basic() {
    let source = "\
function main() {
  const pair: [string, i32] = [\"hello\", 42];
  console.log(pair);
}";

    let expected = "\
fn main() {
    let pair: (String, i32) = (\"hello\".to_string(), 42);
    println!(\"{}\", pair);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tuple_construction_basic", &actual, expected);
}

// ---------------------------------------------------------------------------
// 2. Tuple field access
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tuple_field_access() {
    let source = "\
function main() {
  const pair: [string, i32] = [\"hello\", 42];
  console.log(pair[0]);
  console.log(pair[1]);
}";

    let expected = "\
fn main() {
    let pair: (String, i32) = (\"hello\".to_string(), 42);
    println!(\"{}\", pair.0);
    println!(\"{}\", pair.1);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tuple_field_access", &actual, expected);
}

// ---------------------------------------------------------------------------
// 3. Tuple as function parameter and return type
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tuple_function_param_return() {
    let source = "\
function swap(pair: [i32, i32]): [i32, i32] {
  return [pair[1], pair[0]];
}

function main() {
  const result: [i32, i32] = swap([1, 2]);
  console.log(result[0]);
  console.log(result[1]);
}";

    let expected = "\
fn swap(pair: (i32, i32)) -> (i32, i32) {
    return (pair.1, pair.0);
}

fn main() {
    let result: (i32, i32) = swap((1, 2));
    println!(\"{}\", result.0);
    println!(\"{}\", result.1);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tuple_function_param_return", &actual, expected);
}

// ---------------------------------------------------------------------------
// 4. Tuple destructuring
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tuple_destructuring() {
    let source = "\
function main() {
  const pair: [string, i32] = [\"hello\", 42];
  const [a, b] = pair;
  console.log(a);
  console.log(b);
}";

    let expected = "\
fn main() {
    let pair: (String, i32) = (\"hello\".to_string(), 42);
    let (a, b) = pair;
    println!(\"{}\", a);
    println!(\"{}\", b);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tuple_destructuring", &actual, expected);
}

// ---------------------------------------------------------------------------
// 5. Three-element tuple
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_tuple_three_elements() {
    let source = "\
function main() {
  const triple: [string, i32, bool] = [\"hello\", 42, true];
  console.log(triple[0]);
  console.log(triple[1]);
  console.log(triple[2]);
}";

    let expected = "\
fn main() {
    let triple: (String, i32, bool) = (\"hello\".to_string(), 42, true);
    println!(\"{}\", triple.0);
    println!(\"{}\", triple.1);
    println!(\"{}\", triple.2);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("tuple_three_elements", &actual, expected);
}

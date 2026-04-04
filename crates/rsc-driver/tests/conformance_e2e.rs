//! Conformance end-to-end tests — compile `.rts`, build with cargo, run, verify stdout.
//!
//! These are slow (each invokes `cargo run`) and are marked `#[ignore]`
//! so they only run in the full suite (`just test-all` / `--include-ignored`).
//!
//! Each test exercises one specific TypeScript idiom and verifies the
//! runtime output is correct. This is the behavioral verification layer.

mod test_utils;

use test_utils::compile_and_run;

// ===========================================================================
// ===========================================================================
//
// Common TypeScript idioms — runtime verification
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. map with transform
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_map_transform() {
    let source = "\
function main() {
  const nums: Array<i32> = [1, 2, 3, 4, 5];
  const doubled = nums.map((n: i32): i32 => n * 2);
  for (const d of doubled) {
    console.log(d);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "2\n4\n6\n8\n10");
}

// ---------------------------------------------------------------------------
// 2. filter with predicate
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_filter_predicate() {
    let source = "\
function main() {
  const nums: Array<i32> = [1, 2, 3, 4, 5, 6];
  const evens = nums.filter((n: i32): bool => n % 2 == 0);
  for (const e of evens) {
    console.log(e);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "2\n4\n6");
}

// ---------------------------------------------------------------------------
// 3. reduce to sum
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_reduce_sum() {
    let source = "\
function main() {
  const prices: Array<i32> = [10, 20, 30, 40];
  const total = prices.reduce((sum: i32, p: i32): i32 => sum + p, 0);
  console.log(total);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "100");
}

// ---------------------------------------------------------------------------
// 4. for-of iteration
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_for_of_iteration() {
    let source = "\
function main() {
  const items: Array<string> = [\"alpha\", \"beta\", \"gamma\"];
  for (const item of items) {
    console.log(item);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "alpha\nbeta\ngamma");
}

// ---------------------------------------------------------------------------
// 5. template literal
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_template_literal() {
    let source = "\
function main() {
  const name = \"World\";
  const greeting = `Hello, ${name}!`;
  console.log(greeting);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Hello, World!");
}

// ---------------------------------------------------------------------------
// 6. struct literal + field access
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_struct_literal_field_access() {
    let source = "\
type Config = { host: string, port: i32 }

function main() {
  const config: Config = { host: \"localhost\", port: 8080 };
  console.log(config.host);
  console.log(config.port);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "localhost\n8080");
}

// ---------------------------------------------------------------------------
// 7. destructuring assignment
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_destructuring() {
    let source = "\
type User = { name: string, age: i32 }

function main() {
  const user: User = { name: \"Alice\", age: 30 };
  const { name, age } = user;
  console.log(name);
  console.log(age);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Alice\n30");
}

// ---------------------------------------------------------------------------
// 8. switch/case (enum)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_switch_case() {
    let source = "\
type Status = \"ok\" | \"error\" | \"pending\"

function describe(s: Status): string {
  switch (s) {
    case \"ok\": return \"all good\";
    case \"error\": return \"failed\";
    case \"pending\": return \"waiting\";
  }
}

function main() {
  console.log(describe(\"ok\"));
  console.log(describe(\"error\"));
  console.log(describe(\"pending\"));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "all good\nfailed\nwaiting");
}

// ---------------------------------------------------------------------------
// 9. try/catch
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_try_catch() {
    let source = "\
function riskyOp(): i32 throws string {
  return 42;
}

function main() {
  try {
    const val = riskyOp();
    console.log(val);
  } catch (e) {
    console.log(\"error\");
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42");
}

// ---------------------------------------------------------------------------
// 10. string toUpperCase
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_string_to_upper_case() {
    let source = "\
function main() {
  const name = \"hello world\";
  const upper = name.toUpperCase();
  console.log(upper);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "HELLO WORLD");
}

// ---------------------------------------------------------------------------
// 11. string split
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_string_split() {
    let source = "\
function main() {
  const csv = \"a,b,c\";
  const parts = csv.split(\",\");
  console.log(parts.length);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "3");
}

// ---------------------------------------------------------------------------
// 12. export function
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_export_function() {
    let source = "\
export function helper(): i32 {
  return 42;
}

function main() {
  console.log(helper());
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42");
}

// ---------------------------------------------------------------------------
// 13. forEach side effect
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_foreach_side_effect() {
    let source = "\
function main() {
  const items: Array<string> = [\"x\", \"y\", \"z\"];
  items.forEach((item: string): void => {
    console.log(item);
  });
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "x\ny\nz");
}

// ---------------------------------------------------------------------------
// 14. class with methods
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_class_methods() {
    let source = "\
class Calculator {
  value: i32;
  constructor(initial: i32) {
    this.value = initial;
  }
  add(n: i32): void {
    this.value = this.value + n;
  }
  getResult(): i32 {
    return this.value;
  }
}

function main() {
  let calc = new Calculator(0);
  calc.add(10);
  calc.add(32);
  console.log(calc.getResult());
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42");
}

// ---------------------------------------------------------------------------
// 15. interface + class implements
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_interface_implementation() {
    let source = "\
interface Describable {
  describe(): string;
}

class Item implements Describable {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  describe(): string {
    return this.name;
  }
}

function main() {
  const item = new Item(\"widget\");
  console.log(item.describe());
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "widget");
}

// ---------------------------------------------------------------------------
// 16. multiple function calls / composition
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_nested_function_calls() {
    let source = "\
function double(x: i32): i32 {
  return x * 2;
}

function add(a: i32, b: i32): i32 {
  return a + b;
}

function main() {
  console.log(add(double(3), double(4)));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "14");
}

// ---------------------------------------------------------------------------
// 17. multiple return paths
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_multiple_return_paths() {
    let source = "\
function classify(n: i32): string {
  if (n > 0) {
    return \"positive\";
  } else if (n < 0) {
    return \"negative\";
  }
  return \"zero\";
}

function main() {
  console.log(classify(5));
  console.log(classify(-3));
  console.log(classify(0));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "positive\nnegative\nzero");
}

// ---------------------------------------------------------------------------
// 18. while loop with break
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_while_break() {
    let source = "\
function main() {
  let i: i32 = 0;
  while (true) {
    if (i >= 5) {
      break;
    }
    i = i + 1;
  }
  console.log(i);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "5");
}

// ---------------------------------------------------------------------------
// 19. boolean logic
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_boolean_logic() {
    let source = "\
function is_even(n: i32): bool {
  return n % 2 == 0;
}

function main() {
  console.log(is_even(4));
  console.log(is_even(7));
  console.log(!is_even(3));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\nfalse\ntrue");
}

// ---------------------------------------------------------------------------
// 20. fibonacci (recursion)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_fibonacci_recursion() {
    let source = "\
function fibonacci(n: i32): i32 {
  if (n <= 1) {
    return n;
  }
  return fibonacci(n - 1) + fibonacci(n - 2);
}

function main() {
  console.log(fibonacci(10));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "55");
}

// ===========================================================================
// Do-While Loop E2E Tests (Task 109)
// ===========================================================================

// ---------------------------------------------------------------------------
// Do-while loop that iterates 5 times → correct output
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_do_while_iterates_five_times() {
    let source = "\
function main() {
  let x: i32 = 0;
  do {
    x += 1;
  } while (x < 5);
  console.log(x);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "5");
}

// ---------------------------------------------------------------------------
// Do-while executes body at least once (condition false on first check)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_do_while_executes_at_least_once() {
    let source = "\
function main() {
  let x: i32 = 100;
  do {
    console.log(x);
    x += 1;
  } while (x < 5);
  console.log(x);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "100\n101");
}

// ---------------------------------------------------------------------------
// Destructuring with indexed array access
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_conformance_e2e_destructure_indexed_array() {
    let source = "\
type Pair = { a: i32, b: i32 }

function main() {
  const pairs: Array<Pair> = [{ a: 1, b: 2 }, { a: 3, b: 4 }];
  let i: i32 = 0;
  while (i < pairs.length) {
    const { a, b } = pairs[i];
    console.log(a + b);
    i = i + 1;
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "3\n7");
}

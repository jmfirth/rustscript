//! Showcase end-to-end tests — compile `.rts`, build with cargo, run, verify stdout.
//!
//! These tests prove that the showcase programs actually work: the generated
//! Rust compiles with rustc and produces correct output.
//!
//! Only programs that need no external crates and no async runtime are tested
//! here (programs 2, 3, 5). Programs 1 and 4 need axum/tokio, program 6 needs
//! a concurrent context to be interesting.
//!
//! All tests are `#[ignore]` because they invoke `cargo run`.

mod test_utils;

use test_utils::compile_and_run;

// ===========================================================================
// 2. "Data Pipeline" — filter/map/reduce with structs
//
// Verifies:
//   - Struct construction and field access work at runtime
//   - Iterator chain (filter → map → collect) produces correct values
//   - Iterator chain (filter → map → fold) computes correct sum
//   - for-of iteration prints in correct order
//   - Template literals produce correct formatted strings
// ===========================================================================

#[test]
#[ignore]
fn test_showcase_e2e_data_pipeline_prints_available_products_and_total() {
    let source = r#"type Product = { name: string, price: f64, inStock: bool }

function main() {
  const a: Product = { name: "Widget", price: 29.99, inStock: true };
  const b: Product = { name: "Gadget", price: 49.99, inStock: false };
  const c: Product = { name: "Doohickey", price: 9.99, inStock: true };
  const products: Array<Product> = [a, b, c];

  const available = products
    .filter(p => p.inStock)
    .map(p => `${p.name}: $${p.price}`);

  for (const item of available) {
    console.log(item);
  }

  const total = products
    .filter(p => p.inStock)
    .map(p => p.price)
    .reduce((sum: f64, p: f64): f64 => sum + p, 0.0);

  console.log(`Total: $${total}`);
}"#;

    let stdout = compile_and_run(source);
    let expected = "\
Widget: $29.99
Doohickey: $9.99
Total: $39.98";

    assert_eq!(stdout.trim(), expected);
}

// ===========================================================================
// 3. "Safe Errors" — Option + Result + pattern matching
//
// Verifies:
//   - Option<T> from nullable return works correctly
//   - null check compiles to `let Some(x) = ... else { return Err(...) }`
//   - Result<T, E> from throws propagates through try/catch
//   - Successful path prints user info
//   - Error path prints the error message
//   - Template literals in error messages format correctly
// ===========================================================================

#[test]
#[ignore]
fn test_showcase_e2e_safe_errors_finds_alice_and_fails_on_charlie() {
    let source = r#"type User = { name: string, age: u32 }

function findUser(name: string): User | null {
  if (name == "Alice") {
    return { name: "Alice", age: 30 };
  }
  return null;
}

function getUser(name: string): User throws string {
  const user = findUser(name);
  if (user === null) {
    throw `user not found: ${name}`;
  }
  return user;
}

function main() {
  try {
    const user = getUser("Alice");
    console.log(`Found: ${user.name}, age ${user.age}`);
  } catch (err: string) {
    console.log(err);
  }

  try {
    const user = getUser("Charlie");
    console.log(`Found: ${user.name}`);
  } catch (err: string) {
    console.log(err);
  }
}"#;

    let stdout = compile_and_run(source);
    let expected = "\
Found: Alice, age 30
user not found: Charlie";

    assert_eq!(stdout.trim(), expected);
}

// ===========================================================================
// 5. "State Machine" — Enum + exhaustive match
//
// Verifies:
//   - String union type compiles to a working Rust enum
//   - switch/case compiles to exhaustive match
//   - Enum values cycle correctly through state transitions
//   - let mut works correctly for reassignment in a while loop
//   - Template literal with function call formats correctly
// ===========================================================================

#[test]
#[ignore]
fn test_showcase_e2e_state_machine_cycles_through_six_states() {
    let source = r#"type TrafficLight = "red" | "yellow" | "green"

function next(light: TrafficLight): TrafficLight {
  switch (light) {
    case "red": return "green";
    case "green": return "yellow";
    case "yellow": return "red";
  }
}

function display(light: TrafficLight): string {
  switch (light) {
    case "red": return "STOP";
    case "green": return "GO";
    case "yellow": return "CAUTION";
  }
}

function main() {
  let light: TrafficLight = "red";
  let i = 0;
  while (i < 6) {
    console.log(`${display(light)}`);
    light = next(light);
    i += 1;
  }
}"#;

    let stdout = compile_and_run(source);
    let expected = "\
STOP
GO
CAUTION
STOP
GO
CAUTION";

    assert_eq!(stdout.trim(), expected);
}

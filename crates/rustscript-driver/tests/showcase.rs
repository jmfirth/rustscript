//! Showcase snapshot tests — six programs that demonstrate RustScript to
//! TypeScript and Rust developers.
//!
//! Each test compiles a self-contained `.rts` program and verifies that the
//! generated Rust is clean, idiomatic, and correct. These are marketing
//! materials as much as they are tests — the quality of the generated Rust
//! matters above all.

mod test_utils;

use rustscript_driver::compile_source;
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

// ===========================================================================
// 1. "Hello HTTP" — Express-like web server (~25 lines)
//
// THE FLAGSHIP EXAMPLE. Shows a TypeScript developer what RustScript feels
// like. Demonstrates:
//   - External crate imports (axum)
//   - Struct type definitions → Rust structs with derives
//   - Array operations with reduce → iter().fold()
//   - Template literals → format!()
//   - Async main → #[tokio::main]
//
// This is snapshot-only because it references the axum crate, which is not
// available in the test environment.
//
// RustScript source:
// ```
// import { Router, serve } from "axum";
//
// type User = {
//   id: u32,
//   name: string,
//   email: string,
// }
//
// function getUsers(): string {
//   const users: Array<string> = ["Alice <alice@example.com>", "Bob <bob@example.com>"];
//   return users.reduce((acc: string, s: string): string => `${acc}\n${s}`, "");
// }
//
// async function main() {
//   console.log("Starting server...");
//   console.log(getUsers());
// }
// ```
// ===========================================================================

#[test]
fn test_showcase_hello_http_snapshot() {
    let source = r#"import { Router, serve } from "axum";

type User = {
  id: u32,
  name: string,
  email: string,
}

function getUsers(): string {
  const users: Array<string> = ["Alice <alice@example.com>", "Bob <bob@example.com>"];
  return users.reduce((acc: string, s: string): string => `${acc}\n${s}`, "");
}

async function main() {
  console.log("Starting server...");
  console.log(getUsers());
}"#;

    let result = compile_source(source, "test.rts");
    assert!(
        !result.has_errors,
        "compilation failed with errors: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let expected = r#"use axum::Router;
use axum::serve;

#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub id: u32,
    pub name: String,
    pub email: String,
}

fn getUsers() -> String {
    let users: Vec<String> = vec!["Alice <alice@example.com>".to_string(), "Bob <bob@example.com>".to_string()];
    return users.iter().fold("".to_string(), |acc, s| format!("{}\n{}", acc, s));
}

#[tokio::main]
async fn main() {
    println!("{}", "Starting server...");
    println!("{}", getUsers());
}
"#;

    assert_snapshot("showcase_hello_http", &result.rust_source, expected);

    // Verify metadata: needs async runtime and has axum dependency
    assert!(
        result.needs_async_runtime,
        "async main should set needs_async_runtime"
    );
    assert!(
        result.crate_dependencies.iter().any(|d| d.name == "axum"),
        "should track axum as a crate dependency"
    );
}

// ===========================================================================
// 2. "Data Pipeline" — Collection processing (~20 lines)
//
// Pure computation, no external dependencies. Demonstrates the full
// iterator pipeline: filter → map → reduce, all with closures.
//   - Struct type with mixed field types (string, f64, bool)
//   - .filter() → .iter().filter().cloned()
//   - .map() → .map()
//   - .reduce() → .fold()
//   - for-of loop → for ... in &
//   - Template literals → format!()
//
// RustScript source:
// ```
// type Product = { name: string, price: f64, inStock: bool }
//
// function main() {
//   const a: Product = { name: "Widget", price: 29.99, inStock: true };
//   const b: Product = { name: "Gadget", price: 49.99, inStock: false };
//   const c: Product = { name: "Doohickey", price: 9.99, inStock: true };
//   const products: Array<Product> = [a, b, c];
//
//   const available = products
//     .filter(p => p.inStock)
//     .map(p => `${p.name}: $${p.price}`);
//
//   for (const item of available) {
//     console.log(item);
//   }
//
//   const total = products
//     .filter(p => p.inStock)
//     .map(p => p.price)
//     .reduce((sum: f64, p: f64): f64 => sum + p, 0.0);
//
//   console.log(`Total: $${total}`);
// }
// ```
// ===========================================================================

#[test]
fn test_showcase_data_pipeline_snapshot() {
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

    let expected = r#"#[derive(Debug, Clone, PartialEq)]
struct Product {
    pub name: String,
    pub price: f64,
    pub inStock: bool,
}

fn main() {
    let a = Product { name: "Widget".to_string(), price: 29.99, inStock: true };
    let b = Product { name: "Gadget".to_string(), price: 49.99, inStock: false };
    let c = Product { name: "Doohickey".to_string(), price: 9.99, inStock: true };
    let products: Vec<Product> = vec![a, b, c];
    let available = products.iter().filter(|p| p.inStock).cloned().map(|p| format!("{}: ${}", p.name, p.price)).collect::<Vec<_>>();
    for item in &available {
        println!("{}", item);
    }
    let total = products.iter().filter(|p| p.inStock).cloned().map(|p| p.price).fold(0.0, |sum, p| sum + p);
    println!("{}", format!("Total: ${}", total));
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("showcase_data_pipeline", &actual, expected);
}

// ===========================================================================
// 3. "Safe Errors" — Type-safe error handling (~30 lines)
//
// Shows RustScript's error handling being BETTER than JavaScript. TypeScript
// devs see familiar try/catch syntax, Rust devs see idiomatic Result/Option.
//   - Nullable return → Option<T>
//   - null check → if let Some(x) = ... else { ... }
//   - throws → Result<T, E>
//   - throw → return Err(...)
//   - try/catch → match on Result
//   - Template literals in error messages → format!()
//
// RustScript source:
// ```
// type User = { name: string, age: u32 }
//
// function findUser(name: string): User | null {
//   if (name == "Alice") {
//     return { name: "Alice", age: 30 };
//   }
//   return null;
// }
//
// function getUser(name: string): User throws string {
//   const user = findUser(name);
//   if (user === null) {
//     throw `user not found: ${name}`;
//   }
//   return user;
// }
//
// function main() {
//   try {
//     const user = getUser("Alice");
//     console.log(`Found: ${user.name}, age ${user.age}`);
//   } catch (err: string) {
//     console.log(err);
//   }
//
//   try {
//     const user = getUser("Charlie");
//     console.log(`Found: ${user.name}`);
//   } catch (err: string) {
//     console.log(err);
//   }
// }
// ```
// ===========================================================================

#[test]
fn test_showcase_safe_errors_snapshot() {
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

    let expected = r#"#[derive(Debug, Clone, PartialEq, Eq)]
struct User {
    pub name: String,
    pub age: u32,
}

fn findUser(name: &str) -> Option<User> {
    if name == "Alice".to_string() {
        return Some(User { name: "Alice".to_string(), age: 30 });
    }
    return None;
}

fn getUser(name: String) -> Result<User, String> {
    let user = findUser(&name);
    let Some(user) = user else {
        return Err(format!("user not found: {}", name));
    };
    return Ok(user);
}

fn main() {
    match getUser("Alice".to_string()) {
        Ok(user) => {
            println!("{}", format!("Found: {}, age {}", user.name, user.age));
        }
        Err(err) => {
            println!("{}", err);
        }
    }
    match getUser("Charlie".to_string()) {
        Ok(user) => {
            println!("{}", format!("Found: {}", user.name));
        }
        Err(err) => {
            println!("{}", err);
        }
    }
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("showcase_safe_errors", &actual, expected);
}

// ===========================================================================
// 4. "Concurrent" — Promise.all pattern (~20 lines)
//
// Shows async/await mapping to Rust's async model. TypeScript devs see
// familiar Promise.all; Rust devs see tokio::join!.
//   - async function → async fn
//   - await → .await
//   - Promise.all([...]) → tokio::join!(...)
//   - Tuple destructuring for concurrent results
//   - Template literals → format!()
//
// Snapshot-only because it needs tokio runtime.
//
// RustScript source:
// ```
// async function fetchUser(id: u32): string {
//   return `User-${id}`;
// }
//
// async function fetchPosts(id: u32): string {
//   return `Posts-for-${id}`;
// }
//
// async function main() {
//   const [user, posts] = await Promise.all([
//     fetchUser(1),
//     fetchPosts(1),
//   ]);
//   console.log(`${user}: ${posts}`);
// }
// ```
// ===========================================================================

#[test]
fn test_showcase_concurrent_snapshot() {
    let source = r#"async function fetchUser(id: u32): string {
  return `User-${id}`;
}

async function fetchPosts(id: u32): string {
  return `Posts-for-${id}`;
}

async function main() {
  const [user, posts] = await Promise.all([
    fetchUser(1),
    fetchPosts(1),
  ]);
  console.log(`${user}: ${posts}`);
}"#;

    let expected = r#"async fn fetchUser(id: u32) -> String {
    return format!("User-{}", id);
}

async fn fetchPosts(id: u32) -> String {
    return format!("Posts-for-{}", id);
}

#[tokio::main]
async fn main() {
    let (user, posts) = tokio::join!(fetchUser(1), fetchPosts(1));
    println!("{}", format!("{}: {}", user, posts));
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("showcase_concurrent", &actual, expected);
}

// ===========================================================================
// 5. "State Machine" — Enums + pattern matching (~35 lines)
//
// Shows TypeScript string union types compiling to Rust enums with
// exhaustive pattern matching. This is where RustScript shines — the
// TypeScript is familiar but the Rust is type-safe.
//   - String union type → #[derive(...)] enum
//   - switch/case → match with enum variants
//   - String literal enum values → PascalCase variants
//   - let + mutation → let mut
//   - while loop
//   - Template literal in loop → format!()
//
// RustScript source:
// ```
// type TrafficLight = "red" | "yellow" | "green"
//
// function next(light: TrafficLight): TrafficLight {
//   switch (light) {
//     case "red": return "green";
//     case "green": return "yellow";
//     case "yellow": return "red";
//   }
// }
//
// function display(light: TrafficLight): string {
//   switch (light) {
//     case "red": return "STOP";
//     case "green": return "GO";
//     case "yellow": return "CAUTION";
//   }
// }
//
// function main() {
//   let light: TrafficLight = "red";
//   let i = 0;
//   while (i < 6) {
//     console.log(`${display(light)}`);
//     light = next(light);
//     i += 1;
//   }
// }
// ```
// ===========================================================================

#[test]
fn test_showcase_state_machine_snapshot() {
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

    let expected = r#"#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TrafficLight {
    Red,
    Yellow,
    Green,
}

impl std::fmt::Display for TrafficLight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrafficLight::Red => write!(f, "Red"),
            TrafficLight::Yellow => write!(f, "Yellow"),
            TrafficLight::Green => write!(f, "Green"),
        }
    }
}

fn next(light: TrafficLight) -> TrafficLight {
    match light {
        TrafficLight::Red => {
            return TrafficLight::Green;
        }
        TrafficLight::Green => {
            return TrafficLight::Yellow;
        }
        TrafficLight::Yellow => {
            return TrafficLight::Red;
        }
    }
}

fn display(light: TrafficLight) -> String {
    match light {
        TrafficLight::Red => {
            return "STOP".to_string();
        }
        TrafficLight::Green => {
            return "GO".to_string();
        }
        TrafficLight::Yellow => {
            return "CAUTION".to_string();
        }
    }
}

fn main() {
    let mut light = TrafficLight::Red;
    let mut i = 0;
    while i < 6 {
        println!("{}", format!("{}", display(light)));
        light = next(light);
        i += 1;
    }
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("showcase_state_machine", &actual, expected);
}

// ===========================================================================
// 6. "Shared State" — Concurrency sugar (~20 lines)
//
// Shows RustScript's shared<T> sugar for Arc<Mutex<T>>. TypeScript devs
// see simple shared state; Rust devs see zero-cost concurrency primitives.
//   - shared<T> type → Arc<Mutex<T>>
//   - shared(expr) → Arc::new(Mutex::new(expr))
//   - .lock() → .lock().unwrap()
//   - Auto-generated use declarations for std::sync
//
// Snapshot-only because shared state patterns are most interesting in
// concurrent contexts.
//
// RustScript source:
// ```
// function main() {
//   const counter: shared<u32> = shared(0);
//   console.log("Initial: 0");
//
//   const guard = counter.lock();
//   console.log("Locked and accessed");
// }
// ```
// ===========================================================================

#[test]
fn test_showcase_shared_state_snapshot() {
    let source = r#"function main() {
  const counter: shared<u32> = shared(0);
  console.log("Initial: 0");

  const guard = counter.lock();
  console.log("Locked and accessed");
}"#;

    let expected = r#"use std::sync::Arc;
use std::sync::Mutex;

fn main() {
    let counter: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    println!("{}", "Initial: 0");
    let guard = counter.lock().unwrap();
    println!("{}", "Locked and accessed");
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("showcase_shared_state", &actual, expected);
}

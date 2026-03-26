//! Ecosystem showcase snapshot tests — six programs targeting Rust developers.
//!
//! These demonstrate common Rust crate patterns written in RustScript syntax:
//! "What if your Rust code looked like TypeScript but compiled to the same thing?"
//!
//! Each test compiles a self-contained `.rts` program and verifies that the
//! generated Rust is clean, idiomatic, and recognizable to a Rust developer.
//! The generated Rust is the sales pitch — it must look like something a Rust
//! developer would write by hand.

mod test_utils;

use rsc_driver::compile_source;
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
// 1. "REST API" — axum + serde + tokio
//
// A Rust dev's bread and butter. Shows route handlers that look like Express.
// Demonstrates:
//   - External crate imports (axum, serde)
//   - Struct type definitions with #[derive] annotations
//   - Array operations: filter → map → reduce (iterator chains)
//   - Template literals → format!()
//   - Ownership inference: &Vec<T> params with .iter()
//
// Snapshot-only because it references axum/serde crates.
//
// RustScript source:
// ```
// import { Router, serve } from "axum";
// import { Serialize, Deserialize } from "serde";
//
// type Todo = {
//   id: u32,
//   title: string,
//   completed: bool,
// }
//
// function listTodos(todos: Array<Todo>): string {
//   return todos
//     .filter(t => !t.completed)
//     .map(t => `[ ] ${t.title}`)
//     .reduce((acc: string, s: string): string => `${acc}\n${s}`, "");
// }
//
// function main() {
//   const a: Todo = { id: 1, title: "Learn RustScript", completed: true };
//   const b: Todo = { id: 2, title: "Build an API", completed: false };
//   const todos: Array<Todo> = [a, b];
//   console.log("=== Pending Todos ===");
//   console.log(listTodos(todos));
// }
// ```
// ===========================================================================

#[test]
fn test_ecosystem_rest_api_snapshot() {
    let source = r#"import { Router, serve } from "axum";
import { Serialize, Deserialize } from "serde";

type Todo = {
  id: u32,
  title: string,
  completed: bool,
}

function listTodos(todos: Array<Todo>): string {
  return todos
    .filter(t => !t.completed)
    .map(t => `[ ] ${t.title}`)
    .reduce((acc: string, s: string): string => `${acc}\n${s}`, "");
}

function main() {
  const a: Todo = { id: 1, title: "Learn RustScript", completed: true };
  const b: Todo = { id: 2, title: "Build an API", completed: false };
  const todos: Array<Todo> = [a, b];
  console.log("=== Pending Todos ===");
  console.log(listTodos(todos));
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
use serde::Serialize;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Todo {
    pub id: u32,
    pub title: String,
    pub completed: bool,
}

fn listTodos(todos: &Vec<Todo>) -> String {
    return todos.iter().filter(|t| !t.completed).cloned().map(|t| format!("[ ] {}", t.title)).fold("".to_string(), |acc, s| format!("{}\n{}", acc, s));
}

fn main() {
    let a = Todo { id: 1, title: "Learn RustScript".to_string(), completed: true };
    let b = Todo { id: 2, title: "Build an API".to_string(), completed: false };
    let todos: Vec<Todo> = vec![a, b];
    println!("{}", "=== Pending Todos ===");
    println!("{}", listTodos(&todos));
}
"#;

    assert_snapshot("ecosystem_rest_api", &result.rust_source, expected);

    // Verify metadata
    assert!(
        result.needs_async_runtime == false,
        "sync main should not set needs_async_runtime"
    );
    assert!(
        result.crate_dependencies.iter().any(|d| d.name == "axum"),
        "should track axum as a crate dependency"
    );
    assert!(
        result.crate_dependencies.iter().any(|d| d.name == "serde"),
        "should track serde as a crate dependency"
    );
}

// ===========================================================================
// 2. "CLI Tool" — clap-style argument handling
//
// Shows a command-line tool that parses args and does work. Demonstrates:
//   - Struct type with mixed primitive fields
//   - void return type → no return type
//   - while loop with mutation → let mut + while
//   - if/else control flow
//   - Template literals with expressions → format!()
//   - Struct field access in conditions and interpolation
//
// RustScript source:
// ```
// type Config = {
//   verbose: bool,
//   count: u32,
//   name: string,
// }
//
// function greet(config: Config): void {
//   let i: u32 = 0;
//   while (i < config.count) {
//     if (config.verbose) {
//       console.log(`[${i + 1}/${config.count}] Hello, ${config.name}!`);
//     } else {
//       console.log(`Hello, ${config.name}!`);
//     }
//     i += 1;
//   }
// }
//
// function main() {
//   const config: Config = { verbose: true, count: 3, name: "Rustacean" };
//   greet(config);
// }
// ```
// ===========================================================================

#[test]
fn test_ecosystem_cli_tool_snapshot() {
    let source = r#"type Config = {
  verbose: bool,
  count: u32,
  name: string,
}

function greet(config: Config): void {
  let i: u32 = 0;
  while (i < config.count) {
    if (config.verbose) {
      console.log(`[${i + 1}/${config.count}] Hello, ${config.name}!`);
    } else {
      console.log(`Hello, ${config.name}!`);
    }
    i += 1;
  }
}

function main() {
  const config: Config = { verbose: true, count: 3, name: "Rustacean" };
  greet(config);
}"#;

    let expected = r#"#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    pub verbose: bool,
    pub count: u32,
    pub name: String,
}

fn greet(config: Config) {
    let mut i: u32 = 0;
    while i < config.count {
        if config.verbose {
            println!("{}", format!("[{}/{}] Hello, {}!", i + 1, config.count, config.name));
        } else {
            println!("{}", format!("Hello, {}!", config.name));
        }
        i += 1;
    }
}

fn main() {
    let config = Config { verbose: true, count: 3, name: "Rustacean".to_string() };
    greet(config);
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("ecosystem_cli_tool", &actual, expected);
}

// ===========================================================================
// 3. "JSON Processing" — serde-style data transformation
//
// Parse, transform, filter — the ETL pipeline every backend dev writes.
// Demonstrates:
//   - Struct type with mixed field types (string, u64)
//   - .filter() with string comparison → .iter().filter().cloned()
//   - .length in template literals → .len() in format!()
//   - for-of loop with struct field access → for ... in &
//   - Template literals with field interpolation → format!()
//   - Ownership inference: &Vec<T> for read-only params
//
// RustScript source:
// ```
// type Event = {
//   kind: string,
//   timestamp: u64,
//   payload: string,
// }
//
// function countErrors(events: Array<Event>): string {
//   const errors = events.filter(e => e.kind == "error");
//   const infos = events.filter(e => e.kind == "info");
//   return `Errors: ${errors.length}, Info: ${infos.length}`;
// }
//
// function main() {
//   const a: Event = { kind: "info", timestamp: 1000, payload: "started" };
//   const b: Event = { kind: "error", timestamp: 1001, payload: "connection failed" };
//   const c: Event = { kind: "info", timestamp: 1002, payload: "retry succeeded" };
//   const d: Event = { kind: "error", timestamp: 1003, payload: "timeout" };
//   const events: Array<Event> = [a, b, c, d];
//
//   for (const event of events) {
//     console.log(`[${event.kind}] ${event.payload}`);
//   }
//
//   console.log(countErrors(events));
// }
// ```
// ===========================================================================

#[test]
fn test_ecosystem_json_processing_snapshot() {
    let source = r#"type Event = {
  kind: string,
  timestamp: u64,
  payload: string,
}

function countErrors(events: Array<Event>): string {
  const errors = events.filter(e => e.kind == "error");
  const infos = events.filter(e => e.kind == "info");
  return `Errors: ${errors.length}, Info: ${infos.length}`;
}

function main() {
  const a: Event = { kind: "info", timestamp: 1000, payload: "started" };
  const b: Event = { kind: "error", timestamp: 1001, payload: "connection failed" };
  const c: Event = { kind: "info", timestamp: 1002, payload: "retry succeeded" };
  const d: Event = { kind: "error", timestamp: 1003, payload: "timeout" };
  const events: Array<Event> = [a, b, c, d];

  for (const event of events) {
    console.log(`[${event.kind}] ${event.payload}`);
  }

  console.log(countErrors(events));
}"#;

    let expected = r#"#[derive(Debug, Clone, PartialEq, Eq)]
struct Event {
    pub kind: String,
    pub timestamp: u64,
    pub payload: String,
}

fn countErrors(events: &Vec<Event>) -> String {
    let errors = events.iter().filter(|e| e.kind == "error".to_string()).cloned().collect::<Vec<_>>();
    let infos = events.iter().filter(|e| e.kind == "info".to_string()).cloned().collect::<Vec<_>>();
    return format!("Errors: {}, Info: {}", errors.len() as i64, infos.len() as i64);
}

fn main() {
    let a = Event { kind: "info".to_string(), timestamp: 1000, payload: "started".to_string() };
    let b = Event { kind: "error".to_string(), timestamp: 1001, payload: "connection failed".to_string() };
    let c = Event { kind: "info".to_string(), timestamp: 1002, payload: "retry succeeded".to_string() };
    let d = Event { kind: "error".to_string(), timestamp: 1003, payload: "timeout".to_string() };
    let events: Vec<Event> = vec![a, b, c, d];
    for event in &events {
        println!("{}", format!("[{}] {}", event.kind, event.payload));
    }
    println!("{}", countErrors(&events));
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("ecosystem_json_processing", &actual, expected);
}

// ===========================================================================
// 4. "Async Pipeline" — tokio concurrent processing
//
// Shows async/await + Promise.all looking clean. Demonstrates:
//   - async function → async fn
//   - async main → #[tokio::main] async fn main
//   - Template literals in async context → format!()
//   - Promise.all([...]) → tokio::join!(...)
//   - Tuple destructuring for concurrent results
//   - for-of loop over &Vec
//   - &str params from ownership inference
//
// Snapshot-only because it needs tokio runtime.
//
// RustScript source:
// ```
// async function fetchMetrics(service: string): string {
//   return `${service}: 200ms avg, 99.9% uptime`;
// }
//
// async function healthCheck(service: string): bool {
//   return true;
// }
//
// async function main() {
//   const services: Array<string> = ["api", "database", "cache"];
//
//   for (const svc of services) {
//     const [metrics, healthy] = await Promise.all([
//       fetchMetrics(svc),
//       healthCheck(svc),
//     ]);
//
//     if (healthy) {
//       console.log(`${svc}: OK - ${metrics}`);
//     } else {
//       console.log(`${svc}: DOWN`);
//     }
//   }
// }
// ```
// ===========================================================================

#[test]
fn test_ecosystem_async_pipeline_snapshot() {
    let source = r#"async function fetchMetrics(service: string): string {
  return `${service}: 200ms avg, 99.9% uptime`;
}

async function healthCheck(service: string): bool {
  return true;
}

async function main() {
  const services: Array<string> = ["api", "database", "cache"];

  for (const svc of services) {
    const [metrics, healthy] = await Promise.all([
      fetchMetrics(svc),
      healthCheck(svc),
    ]);

    if (healthy) {
      console.log(`${svc}: OK - ${metrics}`);
    } else {
      console.log(`${svc}: DOWN`);
    }
  }
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

    let expected = r#"async fn fetchMetrics(service: &str) -> String {
    return format!("{}: 200ms avg, 99.9% uptime", service);
}

async fn healthCheck(service: &str) -> bool {
    return true;
}

#[tokio::main]
async fn main() {
    let services: Vec<String> = vec!["api".to_string(), "database".to_string(), "cache".to_string()];
    for svc in &services {
        let (metrics, healthy) = tokio::join!(fetchMetrics(&svc), healthCheck(&svc));
        if healthy {
            println!("{}", format!("{}: OK - {}", svc, metrics));
        } else {
            println!("{}", format!("{}: DOWN", svc));
        }
    }
}
"#;

    assert_snapshot("ecosystem_async_pipeline", &result.rust_source, expected);

    // Verify metadata
    assert!(
        result.needs_async_runtime,
        "async main should set needs_async_runtime"
    );
}

// ===========================================================================
// 5. "Type-Safe Config" — enums + validation + error handling
//
// Shows discriminated unions and error handling doing real work. Demonstrates:
//   - String union type → Rust enum with Copy/Hash derives
//   - Struct type for config with #[derive(...)]
//   - Nullable return (string | null) → Option<String>
//   - null check → if let Some(x) = ...
//   - throws → Result<T, E>
//   - throw → return Err(...)
//   - try/catch → match on Result
//   - Template literals → format!()
//   - .length on string → .len()
//   - clone insertion for ownership
//
// RustScript source:
// ```
// type LogLevel = "debug" | "info" | "warn" | "error"
//
// type DatabaseConfig = {
//   host: string,
//   port: u32,
//   name: string,
// }
//
// function validate(config: DatabaseConfig): string | null {
//   if (config.port == 0) {
//     return "port must be non-zero";
//   }
//   if (config.host.length == 0) {
//     return "host cannot be empty";
//   }
//   return null;
// }
//
// function connectionString(config: DatabaseConfig): string throws string {
//   const err = validate(config);
//   if (err !== null) {
//     throw err;
//   }
//   return `postgres://${config.host}:${config.port}/${config.name}`;
// }
//
// function main() {
//   const config: DatabaseConfig = { host: "localhost", port: 5432, name: "myapp" };
//
//   try {
//     const connStr = connectionString(config);
//     console.log(connStr);
//   } catch (err: string) {
//     console.log(`Config error: ${err}`);
//   }
// }
// ```
// ===========================================================================

#[test]
fn test_ecosystem_type_safe_config_snapshot() {
    let source = r#"type LogLevel = "debug" | "info" | "warn" | "error"

type DatabaseConfig = {
  host: string,
  port: u32,
  name: string,
}

function validate(config: DatabaseConfig): string | null {
  if (config.port == 0) {
    return "port must be non-zero";
  }
  if (config.host.length == 0) {
    return "host cannot be empty";
  }
  return null;
}

function connectionString(config: DatabaseConfig): string throws string {
  const err = validate(config);
  if (err !== null) {
    throw err;
  }
  return `postgres://${config.host}:${config.port}/${config.name}`;
}

function main() {
  const config: DatabaseConfig = { host: "localhost", port: 5432, name: "myapp" };

  try {
    const connStr = connectionString(config);
    console.log(connStr);
  } catch (err: string) {
    console.log(`Config error: ${err}`);
  }
}"#;

    let expected = r#"#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatabaseConfig {
    pub host: String,
    pub port: u32,
    pub name: String,
}

fn validate(config: DatabaseConfig) -> Option<String> {
    if config.port == 0 {
        return Some("port must be non-zero".to_string());
    }
    if config.host.len() as i64 == 0 {
        return Some("host cannot be empty".to_string());
    }
    return None;
}

fn connectionString(config: DatabaseConfig) -> Result<String, String> {
    let err = validate(config.clone());
    if let Some(err) = err {
        return Err(err);
    }
    return Ok(format!("postgres://{}:{}/{}", config.host, config.port, config.name));
}

fn main() {
    let config = DatabaseConfig { host: "localhost".to_string(), port: 5432, name: "myapp".to_string() };
    match connectionString(config) {
        Ok(connStr) => {
            println!("{}", connStr);
        }
        Err(err) => {
            println!("{}", format!("Config error: {}", err));
        }
    }
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("ecosystem_type_safe_config", &actual, expected);
}

// ===========================================================================
// 6. "Concurrent Workers" — spawn + process + collect
//
// Shows clean batch processing with function calls and iteration.
// Demonstrates:
//   - Simple function with template literal → format!()
//   - Array literal with function call elements → vec![...]
//   - for-of loop → for ... in &
//   - .length → .len()
//   - Template literal with .length → format!() with .len()
//
// RustScript source:
// ```
// function processItem(id: u32): string {
//   return `processed-${id}`;
// }
//
// function main() {
//   const results: Array<string> = [
//     processItem(1),
//     processItem(2),
//     processItem(3),
//   ];
//
//   for (const result of results) {
//     console.log(result);
//   }
//
//   console.log(`Total: ${results.length} items`);
// }
// ```
// ===========================================================================

#[test]
fn test_ecosystem_concurrent_workers_snapshot() {
    let source = r#"function processItem(id: u32): string {
  return `processed-${id}`;
}

function main() {
  const results: Array<string> = [
    processItem(1),
    processItem(2),
    processItem(3),
  ];

  for (const result of results) {
    console.log(result);
  }

  console.log(`Total: ${results.length} items`);
}"#;

    let expected = r#"fn processItem(id: u32) -> String {
    return format!("processed-{}", id);
}

fn main() {
    let results: Vec<String> = vec![processItem(1), processItem(2), processItem(3)];
    for result in &results {
        println!("{}", result);
    }
    println!("{}", format!("Total: {} items", results.len() as i64));
}
"#;

    let actual = compile_to_rust(source);
    assert_snapshot("ecosystem_concurrent_workers", &actual, expected);
}

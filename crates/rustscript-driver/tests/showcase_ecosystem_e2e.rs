//! Ecosystem showcase end-to-end tests — compile `.rts`, build with cargo, run, verify stdout.
//!
//! These tests prove that the ecosystem showcase programs actually work: the
//! generated Rust compiles with rustc and produces correct output.
//!
//! Programs 2, 3, 5, 6 are tested here (no external crates, no async runtime).
//! Program 1 needs axum/serde; Program 4 needs tokio.
//!
//! All tests are `#[ignore]` because they invoke `cargo run`.

mod test_utils;

use test_utils::compile_and_run;

// ===========================================================================
// 2. "CLI Tool" — struct config + while loop + if/else
//
// Verifies:
//   - Struct construction with mixed field types
//   - void return type compiles correctly
//   - while loop with let mut works at runtime
//   - if/else branches produce correct output
//   - Template literals with arithmetic expressions format correctly
//   - Struct field access in loop conditions and interpolation
// ===========================================================================

#[test]
#[ignore]
fn test_ecosystem_e2e_cli_tool_prints_verbose_greetings() {
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

    let stdout = compile_and_run(source);
    let expected = "\
[1/3] Hello, Rustacean!
[2/3] Hello, Rustacean!
[3/3] Hello, Rustacean!";

    assert_eq!(stdout.trim(), expected);
}

// ===========================================================================
// 3. "JSON Processing" — filter + for-of + template literals on structs
//
// Verifies:
//   - Struct construction with mixed field types
//   - .filter() with string comparison produces correct results
//   - .length in template literals produces correct count
//   - for-of loop with struct field access iterates correctly
//   - Template literal with struct field interpolation formats correctly
//   - Ownership inference: &Vec<T> param allows borrow after for-of loop
// ===========================================================================

#[test]
#[ignore]
fn test_ecosystem_e2e_json_processing_formats_and_counts_events() {
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

    let stdout = compile_and_run(source);
    let expected = "\
[info] started
[error] connection failed
[info] retry succeeded
[error] timeout
Errors: 2, Info: 2";

    assert_eq!(stdout.trim(), expected);
}

// ===========================================================================
// 5. "Type-Safe Config" — enum + Option + Result + try/catch
//
// Verifies:
//   - String union type compiles to a working Rust enum
//   - Struct construction with string fields
//   - Nullable return (string | null) → Option<String> works
//   - null check compiles to if let Some(x) = ...
//   - throws → Result<T, E> propagates correctly
//   - throw → Err(...) is caught by try/catch → match
//   - Template literal with multiple struct field interpolation
//   - .length on string → .len() works at runtime
//   - clone insertion allows config reuse across validate + connectionString
// ===========================================================================

#[test]
#[ignore]
fn test_ecosystem_e2e_type_safe_config_prints_connection_string() {
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

    let stdout = compile_and_run(source);
    let expected = "postgres://localhost:5432/myapp";

    assert_eq!(stdout.trim(), expected);
}

// ===========================================================================
// 6. "Concurrent Workers" — function calls + collect + iterate
//
// Verifies:
//   - Function call results collected into Vec
//   - for-of loop iterates in correct order
//   - Template literal with u32 formats correctly
//   - .length → .len() on Vec<String>
//   - Template literal with .length produces correct count
// ===========================================================================

#[test]
#[ignore]
fn test_ecosystem_e2e_concurrent_workers_processes_items() {
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

    let stdout = compile_and_run(source);
    let expected = "\
processed-1
processed-2
processed-3
Total: 3 items";

    assert_eq!(stdout.trim(), expected);
}

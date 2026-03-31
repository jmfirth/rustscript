//! Phase 5 syntax integration tests — exercise multiple Phase 5 features together.
//!
//! These tests validate that Phase 5 features (ternary, **, ===, !, as, typeof,
//! bitwise ops, optional/default/rest params, spread, field init, constructor
//! properties, static, get/set, readonly, finally, JSDoc, ~60 builtin methods)
//! compose correctly with each other and with earlier phase features.
//!
//! Snapshot tests are fast (no cargo invocation). E2e tests are `#[ignore]`.

mod test_utils;

use test_utils::{compile_and_run, compile_to_rust};

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
// ===========================================================================
//
// CATEGORY 1: Composition Tests — combine multiple Phase 5 features
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 1.1 Ternary + optional params: optional param used in ternary condition
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_ternary_with_optional_param() {
    let source = "\
function greet(name: string, formal?: bool): string {
  const prefix: string = formal === true ? \"Dear\" : \"Hi\";
  return prefix;
}

function main() {
  console.log(greet(\"Alice\", true));
  console.log(greet(\"Bob\"));
}";

    let actual = compile_to_rust(source);
    // Must have Option<bool> for formal
    assert!(
        actual.contains("Option<bool>"),
        "optional bool should be Option<bool>: {actual}"
    );
    // Must have ternary as if/else expression
    assert!(
        actual.contains("if") && actual.contains("else"),
        "ternary should emit if/else: {actual}"
    );
    // Should pass None when formal is omitted
    assert!(
        actual.contains("None"),
        "missing optional arg should be None: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 1.2 Ternary + exponentiation: ternary selects exponentiation base
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_ternary_exponentiation() {
    let source = "\
function main() {
  const use_big: bool = true;
  const base: i64 = use_big ? 10 : 2;
  const result: i64 = base ** 3;
  console.log(result);
}";

    let expected = "\
fn main() {
    let use_big: bool = true;
    let base: i64 = if use_big { 10 } else { 2 };
    let result: i64 = base.pow(3 as u32);
    println!(\"{}\", result);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p5_ternary_exponentiation", &actual, expected);
}

// ---------------------------------------------------------------------------
// 1.3 Default params + template literal: default value in formatted output
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_default_params_template_literal() {
    let source = "\
function format_item(name: string, count: i64 = 1): string {
  return `${name}: ${count}`;
}

function main() {
  console.log(format_item(\"apples\"));
  console.log(format_item(\"oranges\", 5));
}";

    let actual = compile_to_rust(source);
    // Default should be inlined at call site (borrow inference may optimize name to &str)
    assert!(
        actual.contains("format_item(\"apples\"") && actual.contains(", 1)"),
        "default arg should be inlined: {actual}"
    );
    // Template literal should produce format!
    assert!(
        actual.contains("format!("),
        "template literal should produce format!: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 1.4 Spread + ternary: ternary selects which array to spread
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_spread_ternary() {
    let source = "\
function main() {
  const a: Array<i32> = [1, 2, 3];
  const b: Array<i32> = [4, 5, 6];
  const use_a: bool = true;
  const selected: Array<i32> = use_a ? a : b;
  const result: Array<i32> = [0, ...selected];
  console.log(result);
}";

    let actual = compile_to_rust(source);
    // Ternary should produce if/else
    assert!(
        actual.contains("if use_a"),
        "ternary should emit if/else: {actual}"
    );
    // Spread should produce extend-based pattern
    assert!(
        actual.contains("__spread") || actual.contains(".extend(") || actual.contains(".clone()"),
        "spread should produce clone/extend pattern: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 1.5 Bitwise ops + strict equality: bit masking with === check
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_bitwise_strict_eq() {
    let source = "\
function main() {
  const flags: i64 = 255;
  const mask: i64 = 15;
  const masked: i64 = flags & mask;
  const is_max: bool = masked === 15;
  console.log(masked);
  console.log(is_max);
}";

    let expected = "\
fn main() {
    let flags: i64 = 255;
    let mask: i64 = 15;
    let masked: i64 = flags & mask;
    let is_max: bool = masked == 15;
    println!(\"{}\", masked);
    println!(\"{}\", is_max);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p5_bitwise_strict_eq", &actual, expected);
}

// ---------------------------------------------------------------------------
// 1.6 typeof + ternary: typeof result used in ternary
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_typeof_ternary() {
    let source = "\
function main() {
  const t: string = typeof 42;
  const label: string = t === \"number\" ? \"numeric\" : \"other\";
  console.log(label);
}";

    let actual = compile_to_rust(source);
    // typeof should produce a string literal
    assert!(
        actual.contains("\"number\""),
        "typeof should produce string literal: {actual}"
    );
    // ternary should produce if/else
    assert!(
        actual.contains("if") && actual.contains("else"),
        "ternary should produce if/else: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 1.7 as cast + exponentiation: cast then exponent
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_cast_exponentiation() {
    let source = "\
function main() {
  const x: i64 = 3;
  const y: f64 = x as f64;
  const result: f64 = y ** 2.0;
  console.log(result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("x as f64"),
        "as cast should be present: {actual}"
    );
    assert!(
        actual.contains(".powf("),
        "f64 variable ** should use .powf(): {actual}"
    );
}

// ---------------------------------------------------------------------------
// 1.8 Rest params + spread: collect rest then spread out
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_rest_spread_roundtrip() {
    let source = "\
function collect_and_prefix(prefix: i32, ...items: Array<i32>): Array<i32> {
  return [prefix, ...items];
}

function main() {
  const result = collect_and_prefix(0, 1, 2, 3);
  console.log(result);
}";

    let actual = compile_to_rust(source);
    // Rest param should be Vec<i32>
    assert!(
        actual.contains("items: Vec<i32>"),
        "rest param should be Vec<i32>: {actual}"
    );
    // Spread should produce extend-based code
    assert!(
        actual.contains("__spread") || actual.contains(".extend("),
        "spread in return should produce extend: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 1.9 Non-null assert + optional param composition
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_non_null_with_as_cast() {
    let source = "\
function main() {
  const x: i64 | null = 42;
  const y: i64 = x!;
  const z: f64 = y as f64;
  console.log(z);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".unwrap()"),
        "non-null assert should emit .unwrap(): {actual}"
    );
    assert!(
        actual.contains("as f64"),
        "as cast should be present: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 1.10 Nested ternary: chained ternary for multi-way branch
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_nested_ternary() {
    let source = "\
function classify(n: i64): string {
  return n > 0 ? \"positive\" : n === 0 ? \"zero\" : \"negative\";
}

function main() {
  console.log(classify(5));
  console.log(classify(0));
  console.log(classify(-3));
}";

    let actual = compile_to_rust(source);
    // Should have nested if/else
    assert!(
        actual.contains("if n > 0"),
        "outer ternary should check n > 0: {actual}"
    );
    // Inner ternary should check n == 0 (=== lowers to ==)
    assert!(
        actual.contains("n == 0"),
        "inner ternary should check n == 0: {actual}"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 2: Class Feature Composition
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 2.1 Class with field init + constructor props + static + getter/setter + JSDoc
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_class_all_features() {
    let source = "\
/**
 * A configurable server.
 * @param host - The hostname to bind
 */
class Server {
  static DEFAULT_PORT: i32 = 8080;
  private _host: string;
  public port: i32 = 8080;

  constructor(host: string) {
    this._host = host;
  }

  get host(): string {
    return this._host;
  }

  set host(value: string) {
    this._host = value;
  }

  static create(host: string): Server {
    return new Server(host);
  }
}";

    let actual = compile_to_rust(source);
    // JSDoc should produce doc comments
    assert!(
        actual.contains("/// A configurable server."),
        "JSDoc should produce rustdoc: {actual}"
    );
    // Static field should produce associated const
    assert!(
        actual.contains("pub const DEFAULT_PORT: i32 = 8080"),
        "static field should be associated const: {actual}"
    );
    // Getter should produce fn host(&self)
    assert!(
        actual.contains("fn host(&self)"),
        "getter should produce fn with &self: {actual}"
    );
    // Setter should produce fn set_host(&mut self, ...)
    assert!(
        actual.contains("fn set_host(&mut self"),
        "setter should produce fn with &mut self: {actual}"
    );
    // Static method should not have &self
    assert!(
        actual.contains("fn create(host: String) -> Server"),
        "static method should not have self param: {actual}"
    );
    // Field init default should appear in new()
    assert!(
        actual.contains("port: 8080"),
        "field init default should be in new(): {actual}"
    );
}

// ---------------------------------------------------------------------------
// 2.2 Class with constructor param properties + methods
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_constructor_props_methods() {
    let source = "\
class Rectangle {
  constructor(public width: f64, public height: f64) {}

  area(): f64 {
    return this.width * this.height;
  }
}

function main() {
  const r = new Rectangle(10.0, 5.0);
  console.log(r.area());
}";

    let actual = compile_to_rust(source);
    // Struct should have pub width and pub height
    assert!(
        actual.contains("pub width: f64"),
        "constructor prop should produce pub field: {actual}"
    );
    assert!(
        actual.contains("pub height: f64"),
        "constructor prop should produce pub field: {actual}"
    );
    // Method should have &self
    assert!(
        actual.contains("fn area(&self) -> f64"),
        "method should have &self: {actual}"
    );
    // new() should forward parameters
    assert!(
        actual.contains("fn new(width: f64, height: f64) -> Self"),
        "constructor should forward params: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 2.3 Static method call site + ternary
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_static_method_ternary() {
    let source = "\
class Config {
  private value: i32;
  constructor(v: i32) {
    this.value = v;
  }
  static default_val(): i32 {
    return 42;
  }
}

function main() {
  const use_default: bool = true;
  const v: i32 = use_default ? Config.default_val() : 0;
  console.log(v);
}";

    let actual = compile_to_rust(source);
    // Static method call should use :: syntax
    assert!(
        actual.contains("Config::default_val()"),
        "static call should use :: syntax: {actual}"
    );
    // Ternary should wrap the call
    assert!(
        actual.contains("if use_default"),
        "ternary should produce if/else: {actual}"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 3: Builtin Method Coverage
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 3.1 String methods: comprehensive chaining
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_string_methods_chained() {
    let source = "\
function main() {
  const name: string = \"  Hello World  \";
  const trimmed: string = name.trim();
  const upper: string = trimmed.toUpperCase();
  const starts: bool = upper.startsWith(\"HELLO\");
  const ends: bool = upper.endsWith(\"WORLD\");
  console.log(trimmed);
  console.log(upper);
  console.log(starts);
  console.log(ends);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".trim()"),
        "trim should be present: {actual}"
    );
    assert!(
        actual.contains(".to_uppercase()"),
        "toUpperCase should lower to to_uppercase: {actual}"
    );
    assert!(
        actual.contains(".starts_with("),
        "startsWith should lower to starts_with: {actual}"
    );
    assert!(
        actual.contains(".ends_with("),
        "endsWith should lower to ends_with: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 3.2 String methods: charAt, indexOf, slice, pad, repeat
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_string_methods_extended() {
    let source = "\
function main() {
  const s: string = \"hello world\";
  const ch: string = s.charAt(0);
  const idx: i64 = s.indexOf(\"world\");
  const sliced: string = s.slice(0, 5);
  const padded: string = s.padStart(15, \"*\");
  const repeated: string = \"ab\".repeat(3);
  console.log(ch);
  console.log(idx);
  console.log(sliced);
  console.log(padded);
  console.log(repeated);
}";

    let actual = compile_to_rust(source);
    // charAt should produce .chars().nth(N)
    assert!(
        actual.contains(".chars().nth("),
        "charAt should produce chars().nth(): {actual}"
    );
    // indexOf should produce .find() or similar
    assert!(
        actual.contains(".find(") || actual.contains("index"),
        "indexOf should produce find/index pattern: {actual}"
    );
    // slice should produce string slicing
    assert!(
        actual.contains("[") || actual.contains("get(") || actual.contains("chars()"),
        "slice should produce some form of string slicing: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 3.3 String methods: split, replace, includes, trimStart, trimEnd
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_string_methods_transform() {
    let source = "\
function main() {
  const csv: string = \"a,b,c,d\";
  const parts: Array<string> = csv.split(\",\");
  const replaced: string = csv.replace(\",\", \";\");
  const has_b: bool = csv.includes(\"b\");
  const s: string = \"  hello  \";
  const left: string = s.trimStart();
  const right: string = s.trimEnd();
  console.log(parts);
  console.log(replaced);
  console.log(has_b);
  console.log(left);
  console.log(right);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".split("),
        "split should be present: {actual}"
    );
    assert!(
        actual.contains(".replace(") || actual.contains(".replacen("),
        "replace should be present: {actual}"
    );
    assert!(
        actual.contains(".contains("),
        "includes should lower to contains: {actual}"
    );
    assert!(
        actual.contains(".trim_start()"),
        "trimStart should lower to trim_start: {actual}"
    );
    assert!(
        actual.contains(".trim_end()"),
        "trimEnd should lower to trim_end: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 3.4 Array methods: push, pop, sort, reverse, join
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_array_methods_mutating() {
    let source = "\
function main() {
  let items: Array<i32> = [3, 1, 4, 1, 5];
  items.push(9);
  items.sort();
  items.reverse();
  const joined: string = items.join(\", \");
  console.log(joined);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".push("),
        "push should be present: {actual}"
    );
    assert!(
        actual.contains(".sort()"),
        "sort should be present: {actual}"
    );
    assert!(
        actual.contains(".reverse()"),
        "reverse should be present: {actual}"
    );
    assert!(
        actual.contains(".join("),
        "join should be present: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 3.5 Array methods: map/filter/find chained with builtins
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_array_iterator_chain() {
    let source = "\
function main() {
  const numbers: Array<i32> = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
  const evens: Array<i32> = numbers.filter((n) => n % 2 === 0);
  const doubled: Array<i32> = evens.map((n) => n * 2);
  console.log(doubled);
}";

    let actual = compile_to_rust(source);
    // Should produce iterator chain with filter
    assert!(
        actual.contains(".filter("),
        "filter should produce .filter(): {actual}"
    );
    // Should produce iterator chain with map
    assert!(
        actual.contains(".map("),
        "map should produce .map(): {actual}"
    );
    // Should collect into Vec (compiler emits .collect::<Vec<_>>())
    assert!(
        actual.contains(".collect::<Vec<_>>()") || actual.contains(".collect()"),
        "should collect into Vec: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 3.6 Array methods: some, every, find, findIndex
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_array_search_methods() {
    let source = "\
function main() {
  const nums: Array<i32> = [10, 20, 30, 40, 50];
  const has_big: bool = nums.some((n) => n > 25);
  const all_pos: bool = nums.every((n) => n > 0);
  console.log(has_big);
  console.log(all_pos);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".any("),
        "some should lower to .any(): {actual}"
    );
    assert!(
        actual.contains(".all("),
        "every should lower to .all(): {actual}"
    );
}

// ---------------------------------------------------------------------------
// 3.7 Map methods: get, set, has, delete, keys, values
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_map_operations() {
    let source = "\
function main() {
  const m: Map<string, i32> = new Map();
  m.set(\"a\", 1);
  m.set(\"b\", 2);
  const has_a: bool = m.has(\"a\");
  m.delete(\"b\");
  console.log(has_a);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("HashMap"),
        "Map should lower to HashMap: {actual}"
    );
    assert!(
        actual.contains(".insert("),
        "map.set should lower to .insert(): {actual}"
    );
    assert!(
        actual.contains(".contains_key("),
        "map.has should lower to .contains_key(): {actual}"
    );
    assert!(
        actual.contains(".remove("),
        "map.delete should lower to .remove(): {actual}"
    );
}

// ---------------------------------------------------------------------------
// 3.8 Set methods: add, has, delete
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_set_operations() {
    let source = "\
function main() {
  const s: Set<string> = new Set();
  s.add(\"hello\");
  s.add(\"world\");
  const has_hello: bool = s.has(\"hello\");
  s.delete(\"world\");
  console.log(has_hello);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("HashSet"),
        "Set should lower to HashSet: {actual}"
    );
    assert!(
        actual.contains(".insert("),
        "set.add should lower to .insert(): {actual}"
    );
    assert!(
        actual.contains(".contains(") && !actual.contains(".contains_key("),
        "set.has should lower to .contains(), not .contains_key(): {actual}"
    );
    assert!(
        actual.contains(".remove("),
        "set.delete should lower to .remove(): {actual}"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 4: Error Handling — finally + throws + try/catch + ternary
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 4.1 try/catch/finally snapshot: verify finally block placement
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_try_catch_finally_snapshot() {
    let source = "\
function riskyOp(): i32 throws string {
  throw \"oops\";
}

function main() {
  try {
    const val = riskyOp();
    console.log(val);
  } catch (err: string) {
    console.log(err);
  } finally {
    console.log(\"done\");
  }
}";

    let actual = compile_to_rust(source);
    // Should have match on Result
    assert!(
        actual.contains("match riskyOp()"),
        "try/catch should produce match: {actual}"
    );
    // Should have Ok arm
    assert!(
        actual.contains("Ok(val)"),
        "should have Ok binding: {actual}"
    );
    // Should have Err arm
    assert!(
        actual.contains("Err(err)"),
        "should have Err binding: {actual}"
    );
    // Finally println should appear after match
    assert!(
        actual.contains("\"done\""),
        "finally body should be present: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 4.2 try/finally without catch
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_try_finally_no_catch() {
    let source = "\
function main() {
  try {
    console.log(\"work\");
  } finally {
    console.log(\"cleanup\");
  }
}";

    let actual = compile_to_rust(source);
    // Both println calls should be present
    assert!(
        actual.contains("\"work\""),
        "try body should be present: {actual}"
    );
    assert!(
        actual.contains("\"cleanup\""),
        "finally body should be present: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 4.3 throws function + ternary: error handling with conditional
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_throws_with_ternary() {
    let source = "\
function maybe_fail(fail: bool): i32 throws string {
  if (fail) {
    throw \"error\";
  }
  return 42;
}

function main() {
  const should_fail: bool = false;
  try {
    const v = maybe_fail(should_fail);
    const label: string = v > 40 ? \"big\" : \"small\";
    console.log(label);
  } catch (err: string) {
    console.log(err);
  }
}";

    let actual = compile_to_rust(source);
    // throws should produce Result return type
    assert!(
        actual.contains("Result<i32, String>"),
        "throws should produce Result: {actual}"
    );
    // throw should produce Err
    assert!(
        actual.contains("Err("),
        "throw should produce Err: {actual}"
    );
    // Ternary inside try block
    assert!(
        actual.contains("if") && actual.contains("\"big\""),
        "ternary inside catch block: {actual}"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 5: Full Program — realistic multi-feature programs
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 5.1 Full program: uses ternary, optional params, default params, spread,
//     template literal, strict equality, class features, builtins
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_full_program_task_manager() {
    let source = "\
class TaskItem {
  static NEXT_ID: i32 = 1;
  public id: i32;
  public title: string;
  public done: bool = false;

  constructor(title: string) {
    this.id = 1;
    this.title = title;
  }

  toggle(): void {
    this.done = !this.done;
  }
}

function create_task(title: string, done: bool = false): TaskItem {
  const t = new TaskItem(title);
  return t;
}

function main() {
  const t1 = create_task(\"Write tests\");
  const t2 = create_task(\"Review code\", true);
  const status: string = t1.done === true ? \"done\" : \"pending\";
  console.log(status);
  console.log(t2.title);
}";

    let actual = compile_to_rust(source);
    // Static field should be associated constant
    assert!(
        actual.contains("pub const NEXT_ID: i32 = 1"),
        "static field: {actual}"
    );
    // Field init for done
    assert!(
        actual.contains("done: false"),
        "field init default: {actual}"
    );
    // Default param should be inlined
    assert!(
        actual.contains("create_task(\"Write tests\".to_string(), false)"),
        "default param should be inlined: {actual}"
    );
    // Strict equality
    assert!(
        actual.contains("t1.done =="),
        "=== should lower to ==: {actual}"
    );
    // Ternary
    assert!(
        actual.contains("if") && actual.contains("\"done\"") && actual.contains("\"pending\""),
        "ternary should be present: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 5.2 Full program: string processing pipeline
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_full_program_string_pipeline() {
    let source = "\
function process(input: string): string {
  const trimmed: string = input.trim();
  const upper: string = trimmed.toUpperCase();
  const replaced: string = upper.replace(\"HELLO\", \"HI\");
  return replaced;
}

function main() {
  const result: string = process(\"  hello world  \");
  const starts_hi: bool = result.startsWith(\"HI\");
  const has_world: bool = result.includes(\"WORLD\");
  console.log(result);
  console.log(starts_hi);
  console.log(has_world);
}";

    let actual = compile_to_rust(source);
    // Chain of string method calls
    assert!(actual.contains(".trim()"), "trim: {actual}");
    assert!(actual.contains(".to_uppercase()"), "toUpperCase: {actual}");
    assert!(
        actual.contains(".replace(") || actual.contains(".replacen("),
        "replace: {actual}"
    );
    assert!(actual.contains(".starts_with("), "startsWith: {actual}");
    assert!(actual.contains(".contains("), "includes: {actual}");
}

// ---------------------------------------------------------------------------
// 5.3 Full program: data processing with array methods + ternary + spread
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_full_program_data_processing() {
    let source = "\
function main() {
  const scores: Array<i32> = [85, 92, 78, 95, 67, 88];
  const high: Array<i32> = scores.filter((s) => s >= 80);
  const base: Array<i32> = [100];
  const combined: Array<i32> = [...base, ...high];
  const all_pass: bool = scores.every((s) => s >= 60);
  const has_perfect: bool = scores.some((s) => s === 100);
  console.log(all_pass);
  console.log(has_perfect);
}";

    let actual = compile_to_rust(source);
    // Filter
    assert!(actual.contains(".filter("), "filter: {actual}");
    // Every -> all
    assert!(actual.contains(".all("), "every -> all: {actual}");
    // Some -> any
    assert!(actual.contains(".any("), "some -> any: {actual}");
    // Spread
    assert!(
        actual.contains("__spread") || actual.contains(".extend(") || actual.contains(".clone()"),
        "spread: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 5.4 Full program: JSDoc on multiple declaration types
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_jsdoc_all_declarations() {
    let source = "\
/** A 2D point. */
type Point = {
  x: f64,
  y: f64
}

/**
 * Calculate distance from origin.
 * @param p - The point to measure
 * @returns The distance
 */
function distance(p: Point): f64 {
  return p.x;
}

/** A shape interface. */
interface Shape {
  area(): f64;
}";

    let actual = compile_to_rust(source);
    // Type JSDoc
    assert!(
        actual.contains("/// A 2D point."),
        "type JSDoc should produce rustdoc: {actual}"
    );
    // Function JSDoc
    assert!(
        actual.contains("/// Calculate distance from origin."),
        "function JSDoc should produce rustdoc: {actual}"
    );
    // @param should produce # Arguments
    assert!(
        actual.contains("# Arguments"),
        "@param should produce # Arguments section: {actual}"
    );
    // @returns should produce # Returns
    assert!(
        actual.contains("# Returns"),
        "@returns should produce # Returns section: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 5.5 Full program: Map + Set operations with control flow
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_full_program_map_set_control_flow() {
    let source = "\
function main() {
  const counts: Map<string, i32> = new Map();
  counts.set(\"a\", 1);
  counts.set(\"b\", 2);
  counts.set(\"c\", 3);

  const seen: Set<string> = new Set();
  seen.add(\"a\");
  seen.add(\"b\");

  const has_a: bool = counts.has(\"a\");
  const in_seen: bool = seen.has(\"a\");
  console.log(has_a);
  console.log(in_seen);
}";

    let actual = compile_to_rust(source);
    assert!(actual.contains("HashMap"), "Map -> HashMap: {actual}");
    assert!(actual.contains("HashSet"), "Set -> HashSet: {actual}");
    assert!(actual.contains(".insert("), "set/add -> insert: {actual}");
    assert!(
        actual.contains(".contains_key(") || actual.contains(".contains("),
        "has -> contains/contains_key: {actual}"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 6: E2E Behavioral Tests (#[ignore])
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 6.1 Ternary behavioral: correct branch taken
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_ternary_correct_branch() {
    let source = "\
function main() {
  const x: i64 = 7;
  const result: string = x > 5 ? \"big\" : \"small\";
  console.log(result);
  const result2: string = x > 10 ? \"huge\" : \"medium\";
  console.log(result2);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "big\nmedium");
}

// ---------------------------------------------------------------------------
// 6.2 Exponentiation behavioral: 2**10 == 1024
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_exponentiation_power_of_two() {
    let source = "\
function main() {
  const result: i64 = 2 ** 10;
  console.log(result);
  const result2: i64 = 3 ** 4;
  console.log(result2);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "1024\n81");
}

// ---------------------------------------------------------------------------
// 6.3 Optional params behavioral: None/Some behavior
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_optional_params_none_some() {
    let source = "\
function greet(name: string, title?: string): string {
  return name;
}

function main() {
  console.log(greet(\"Alice\"));
  console.log(greet(\"Bob\", \"Dr.\"));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "Alice\nBob");
}

// ---------------------------------------------------------------------------
// 6.4 Default params behavioral: defaults used when omitted
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_default_params_inlined() {
    let source = "\
function connect(host: string, port: i64 = 8080): i64 {
  return port;
}

function main() {
  console.log(connect(\"localhost\"));
  console.log(connect(\"localhost\", 9090));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "8080\n9090");
}

// ---------------------------------------------------------------------------
// 6.5 Rest params behavioral: variable args collected
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_rest_params_collected() {
    let source = "\
function sum(first: i64, ...rest: Array<i64>): i64 {
  let total: i64 = first;
  for (const n of rest) {
    total = total + n;
  }
  return total;
}

function main() {
  console.log(sum(1, 2, 3, 4));
  console.log(sum(10));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "10\n10");
}

// ---------------------------------------------------------------------------
// 6.6 Spread array behavioral: combined array correct
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_spread_array_combined() {
    let source = "\
function main() {
  const a: Array<i32> = [1, 2];
  const b: Array<i32> = [3, 4];
  const c: Array<i32> = [...a, 0, ...b];
  for (const n of c) {
    console.log(n);
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "1\n2\n0\n3\n4");
}

// ---------------------------------------------------------------------------
// 6.7 Struct spread behavioral: field override correct
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_struct_spread_override() {
    let source = "\
type Config = { host: string, port: i32 }

function main() {
  const base: Config = { host: \"localhost\", port: 8080 };
  const custom: Config = { ...base, port: 9090 };
  console.log(custom.host);
  console.log(custom.port);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "localhost\n9090");
}

// ---------------------------------------------------------------------------
// 6.8 Class features behavioral: field init, static, getter/setter
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_class_features_behavior() {
    let source = "\
class Counter {
  private _count: i32 = 0;

  constructor() {}

  get count(): i32 {
    return this._count;
  }

  increment(): void {
    this._count = this._count + 1;
  }

  static zero(): Counter {
    return new Counter();
  }
}

function main() {
  let c = Counter.zero();
  console.log(c.count());
  c.increment();
  c.increment();
  console.log(c.count());
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "0\n2");
}

// ---------------------------------------------------------------------------
// 6.9 finally behavioral: runs in success and error paths
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_finally_both_paths() {
    let source = "\
function success_op(): i32 throws string {
  return 42;
}

function fail_op(): i32 throws string {
  throw \"error\";
}

function main() {
  try {
    const v = success_op();
    console.log(v);
  } catch (err: string) {
    console.log(err);
  } finally {
    console.log(\"cleanup1\");
  }

  try {
    const v = fail_op();
    console.log(v);
  } catch (err: string) {
    console.log(err);
  } finally {
    console.log(\"cleanup2\");
  }
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "42\ncleanup1\nerror\ncleanup2");
}

// ---------------------------------------------------------------------------
// 6.10 Bitwise ops behavioral: verify computed values
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_bitwise_computations() {
    // Using decimal literals because the parser does not support 0b/0x prefix.
    let source = "\
function main() {
  const flags: i64 = 12;
  const mask: i64 = 10;
  console.log(flags & mask);
  console.log(flags | mask);
  console.log(flags ^ mask);
  console.log(flags << 2);
  console.log(flags >> 1);
}";

    let stdout = compile_and_run(source);
    // 12 & 10 = 8, 12 | 10 = 14, 12 ^ 10 = 6, 12 << 2 = 48, 12 >> 1 = 6
    assert_eq!(stdout.trim(), "8\n14\n6\n48\n6");
}

// ---------------------------------------------------------------------------
// 6.11 Nested ternary behavioral: multi-way classification
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_nested_ternary_classify() {
    let source = "\
function classify(n: i64): string {
  return n > 0 ? \"positive\" : n === 0 ? \"zero\" : \"negative\";
}

function main() {
  console.log(classify(5));
  console.log(classify(0));
  console.log(classify(-3));
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "positive\nzero\nnegative");
}

// ---------------------------------------------------------------------------
// 6.12 Full integration: string processing end-to-end
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_string_processing_pipeline() {
    let source = "\
function main() {
  const raw: string = \"  Hello, World!  \";
  const trimmed: string = raw.trim();
  const upper: string = trimmed.toUpperCase();
  const starts: bool = upper.startsWith(\"HELLO\");
  const ends: bool = upper.endsWith(\"WORLD!\");
  const replaced: string = upper.replace(\"HELLO\", \"HI\");
  console.log(trimmed);
  console.log(starts);
  console.log(ends);
  console.log(replaced);
}";

    let stdout = compile_and_run(source);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "Hello, World!");
    assert_eq!(lines[1], "true");
    assert_eq!(lines[2], "true");
    assert_eq!(lines[3], "HI, WORLD!");
}

// ---------------------------------------------------------------------------
// 6.13 Full integration: Map operations end-to-end
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_map_operations() {
    let source = "\
function main() {
  const m: Map<string, i32> = new Map();
  m.set(\"x\", 10);
  m.set(\"y\", 20);
  const has_x: bool = m.has(\"x\");
  const has_z: bool = m.has(\"z\");
  console.log(has_x);
  console.log(has_z);
  m.delete(\"y\");
  const has_y: bool = m.has(\"y\");
  console.log(has_y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\nfalse\nfalse");
}

// ---------------------------------------------------------------------------
// 6.14 Full integration: Set operations end-to-end
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_set_operations() {
    let source = "\
function main() {
  const s: Set<i32> = new Set();
  s.add(1);
  s.add(2);
  s.add(3);
  s.add(2);
  const has_2: bool = s.has(2);
  const has_5: bool = s.has(5);
  s.delete(3);
  const has_3: bool = s.has(3);
  console.log(has_2);
  console.log(has_5);
  console.log(has_3);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "true\nfalse\nfalse");
}

// ---------------------------------------------------------------------------
// 6.15 Full integration: realistic 30-line program
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_full_realistic_program() {
    let source = "\
class Config {
  static DEFAULT_PORT: i32 = 8080;
  public host: string;
  public port: i32 = 8080;
  public debug: bool = false;

  constructor(host: string) {
    this.host = host;
  }

  static create(host: string, port: i32 = 8080): Config {
    const c = new Config(host);
    return c;
  }
}

function format_addr(config: Config): string {
  return `${config.host}:${config.port}`;
}

function main() {
  const dev = Config.create(\"localhost\");
  const prod = Config.create(\"prod.example.com\", 443);
  const addr1: string = format_addr(dev);
  const addr2: string = format_addr(prod);
  const is_default: bool = dev.port === 8080;
  const label: string = is_default ? \"default\" : \"custom\";
  console.log(addr1);
  console.log(addr2);
  console.log(label);
}";

    let stdout = compile_and_run(source);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "localhost:8080");
    assert_eq!(lines[1], "prod.example.com:443");
    assert_eq!(lines[2], "default");
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 7: Edge Cases and Robustness
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 7.1 Empty rest params: no excess args
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_empty_rest_params() {
    let source = "\
function log_all(prefix: string, ...messages: Array<string>): void {
  console.log(prefix);
}

function main() {
  log_all(\"INFO\");
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("vec![]"),
        "no rest args should produce empty vec: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.2 Spread of empty array
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_spread_empty_array() {
    let source = "\
function main() {
  const empty: Array<i32> = [];
  const result: Array<i32> = [1, ...empty, 2];
  console.log(result);
}";

    let actual = compile_to_rust(source);
    // Should still produce valid spread code
    assert!(
        actual.contains("__spread") || actual.contains(".extend(") || actual.contains("vec!"),
        "spread of empty array should compile: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.3 Multiple ternaries in sequence
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_multiple_ternaries() {
    let source = "\
function main() {
  const a: i64 = 1;
  const b: i64 = 2;
  const c: i64 = 3;
  const x: i64 = a > 0 ? 10 : 0;
  const y: i64 = b > 0 ? 20 : 0;
  const z: i64 = c > 0 ? 30 : 0;
  console.log(x);
  console.log(y);
  console.log(z);
}";

    let expected = "\
fn main() {
    let a: i64 = 1;
    let b: i64 = 2;
    let c: i64 = 3;
    let x: i64 = if a > 0 { 10 } else { 0 };
    let y: i64 = if b > 0 { 20 } else { 0 };
    let z: i64 = if c > 0 { 30 } else { 0 };
    println!(\"{}\", x);
    println!(\"{}\", y);
    println!(\"{}\", z);
}
";

    let actual = compile_to_rust(source);
    assert_snapshot("p5_multiple_ternaries", &actual, expected);
}

// ---------------------------------------------------------------------------
// 7.4 Exponentiation with different types
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_exponentiation_types() {
    let source = "\
function main() {
  const int_result: i64 = 3 ** 4;
  const float_result: f64 = 2.0 ** 3.0;
  console.log(int_result);
  console.log(float_result);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains(".pow("),
        "integer ** should use pow: {actual}"
    );
    assert!(
        actual.contains(".powf("),
        "float ** should use powf: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.5 === and !== with all primitive types
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_strict_eq_all_types() {
    let source = "\
function main() {
  const a: i64 = 1;
  const b: i64 = 1;
  const c: bool = true;
  const d: bool = true;
  const e: string = \"hello\";
  const f: string = \"hello\";
  console.log(a === b);
  console.log(c === d);
  console.log(e === f);
  console.log(a !== b);
}";

    let actual = compile_to_rust(source);
    // All === should become ==
    assert!(
        actual.contains("a == b"),
        "i64 === should become ==: {actual}"
    );
    assert!(
        actual.contains("c == d"),
        "bool === should become ==: {actual}"
    );
    assert!(
        actual.contains("e == f"),
        "string === should become ==: {actual}"
    );
    assert!(actual.contains("a != b"), "!== should become !=: {actual}");
}

// ---------------------------------------------------------------------------
// 7.6 All bitwise operators in one program
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_all_bitwise_ops() {
    let source = "\
function main() {
  const a: i64 = 0xFF;
  const b: i64 = 0x0F;
  const and_result: i64 = a & b;
  const or_result: i64 = a | b;
  const xor_result: i64 = a ^ b;
  const not_result: i64 = ~a;
  const shl_result: i64 = b << 4;
  const shr_result: i64 = a >> 4;
  console.log(and_result);
  console.log(or_result);
}";

    let actual = compile_to_rust(source);
    assert!(actual.contains("a & b"), "AND: {actual}");
    assert!(actual.contains("a | b"), "OR: {actual}");
    assert!(actual.contains("a ^ b"), "XOR: {actual}");
    assert!(actual.contains("!a"), "NOT: {actual}");
    assert!(actual.contains("b << 4"), "SHL: {actual}");
    assert!(actual.contains("a >> 4"), "SHR: {actual}");
}

// ---------------------------------------------------------------------------
// 7.7 typeof on different expression types
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_typeof_all_types() {
    let source = "\
function main() {
  const t1: string = typeof 42;
  const t2: string = typeof 3.14;
  const t3: string = typeof true;
  const t4: string = typeof \"hello\";
  console.log(t1);
  console.log(t2);
  console.log(t3);
  console.log(t4);
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("\"number\""),
        "typeof integer should be number: {actual}"
    );
    assert!(
        actual.contains("\"boolean\""),
        "typeof bool should be boolean: {actual}"
    );
    assert!(
        actual.contains("\"string\""),
        "typeof string should be string: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.8 Combined optional + default + rest params
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_all_param_types() {
    let source = "\
function mixed(required: string, optional?: bool, defaulted: i64 = 42, ...rest: Array<i64>): string {
  return required;
}

function main() {
  console.log(mixed(\"hello\"));
  console.log(mixed(\"world\", true, 10, 1, 2, 3));
}";

    let actual = compile_to_rust(source);
    // Optional should be Option
    assert!(
        actual.contains("Option<bool>"),
        "optional should be Option: {actual}"
    );
    // Default should use base type
    assert!(
        actual.contains("defaulted: i64"),
        "default param should use base type: {actual}"
    );
    // Rest should be Vec
    assert!(
        actual.contains("rest: Vec<i64>"),
        "rest should be Vec: {actual}"
    );
    // First call should fill defaults
    assert!(
        actual.contains("None") && actual.contains(", 42"),
        "missing args should be filled: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.9 JSDoc with @param on function with optional/default params
// ---------------------------------------------------------------------------

#[test]
fn test_p5_integration_jsdoc_with_fancy_params() {
    let source = "\
/**
 * Connect to a database.
 * @param host - The database host
 * @param port - The port number (default: 5432)
 * @returns Connection string
 */
function connect(host: string, port: i64 = 5432): string {
  return host;
}";

    let actual = compile_to_rust(source);
    // JSDoc comments should be present
    assert!(
        actual.contains("/// Connect to a database."),
        "JSDoc should produce rustdoc: {actual}"
    );
    assert!(
        actual.contains("`host`"),
        "@param host should be preserved: {actual}"
    );
    assert!(
        actual.contains("`port`"),
        "@param port should be preserved: {actual}"
    );
    assert!(
        actual.contains("# Returns"),
        "@returns should produce Returns section: {actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.10 E2E: Constructor parameter properties with method calls
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_e2e_p5_constructor_props_with_methods() {
    let source = "\
class Point {
  constructor(public x: f64, public y: f64) {}

  distance_from_origin(): f64 {
    return (this.x ** 2.0 + this.y ** 2.0) ** 0.5;
  }
}

function main() {
  const p = new Point(3.0, 4.0);
  console.log(p.x);
  console.log(p.y);
}";

    let stdout = compile_and_run(source);
    assert_eq!(stdout.trim(), "3\n4");
}

// ===========================================================================
// ===========================================================================
//
// TASK 067: Minor Syntax Completions
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. import type — type-only imports do NOT generate use declarations (Task 126)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_import_type_generates_same_as_import() {
    let source = "\
import type { User } from \"./models\";

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);
    assert!(
        !actual.contains("use crate::models::User;"),
        "import type should NOT generate use declaration (type-only erasure):\n{actual}"
    );
}

#[test]
fn test_snapshot_import_type_mixed_with_regular() {
    let source = "\
import { Post } from \"./models\";
import type { User } from \"./models\";

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("use crate::models::Post;"),
        "regular import should work:\n{actual}"
    );
    assert!(
        !actual.contains("use crate::models::User;"),
        "import type should NOT generate use (type-only erasure):\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 2. satisfies operator — compile-time only, stripped in output
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_satisfies_is_stripped() {
    let source = "\
function main() {
  const x: i64 = 42 satisfies i64;
  console.log(x);
}";

    let actual = compile_to_rust(source);
    assert!(
        !actual.contains("satisfies"),
        "satisfies should be stripped from output:\n{actual}"
    );
    assert!(
        actual.contains("let x: i64 = 42;"),
        "expression should pass through:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 3. abstract classes — lower to traits
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_abstract_class_generates_trait() {
    let source = "\
abstract class Shape {
  abstract area(): f64;
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("trait Shape"),
        "abstract class should generate trait:\n{actual}"
    );
    assert!(
        actual.contains("fn area(&self) -> f64;"),
        "abstract method should be a trait method:\n{actual}"
    );
}

#[test]
fn test_snapshot_abstract_class_with_default_method() {
    let source = "\
abstract class Shape {
  abstract area(): f64;

  describe(): string {
    return \"a shape\";
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("trait Shape"),
        "abstract class should generate trait:\n{actual}"
    );
    assert!(
        actual.contains("fn area(&self) -> f64;"),
        "abstract method should be signature-only:\n{actual}"
    );
    assert!(
        actual.contains("fn describe(&self)"),
        "concrete method should have a body:\n{actual}"
    );
}

#[test]
fn test_snapshot_class_extends_abstract() {
    let source = "\
abstract class Shape {
  abstract area(): f64;
}

class Circle extends Shape {
  radius: f64;

  constructor(radius: f64) {
    this.radius = radius;
  }

  area(): f64 {
    return 3.14 * this.radius;
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("trait Shape"),
        "abstract class should generate trait:\n{actual}"
    );
    assert!(
        actual.contains("struct Circle"),
        "concrete class should generate struct:\n{actual}"
    );
    assert!(
        actual.contains("impl Shape for Circle"),
        "concrete class should implement trait:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 4. override keyword — documentation only, stripped in output
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_override_keyword_stripped() {
    let source = "\
abstract class Base {
  abstract greet(): string;
}

class Impl extends Base {
  override greet(): string {
    return \"hello\";
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);
    assert!(
        !actual.contains("override"),
        "override keyword should not appear in Rust output:\n{actual}"
    );
    assert!(
        actual.contains("fn greet(&self) -> String"),
        "method should still be generated:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 5. #private fields — truly private (no pub)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_hash_private_field_no_pub() {
    let source = "\
class User {
  name: string;
  #password: string;

  constructor(name: string, password: string) {
    this.name = name;
    this.#password = password;
  }

  checkPassword(input: string): bool {
    return input === this.#password;
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);
    assert!(
        actual.contains("pub name: String"),
        "regular field should be pub:\n{actual}"
    );
    // The password field should not have `pub`
    assert!(
        actual.contains("password: String") && !actual.contains("pub password"),
        "hash-private field should not have pub:\n{actual}"
    );
    // self.password access (not self.#password) in method
    assert!(
        actual.contains("self.password"),
        "hash-private access should strip #:\n{actual}"
    );
}

// ===========================================================================
// CATEGORY 7: Concrete class inheritance via `extends`
// ===========================================================================

// ---------------------------------------------------------------------------
// 7.1 Base class generates {Name}Trait when extended
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_concrete_extends_generates_trait() {
    let source = "\
class Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return this.name;
  }
}

class Dog extends Animal {
  breed: string;

  constructor(name: string, breed: string) {
    this.name = name;
    this.breed = breed;
  }

  speak(): string {
    return this.name;
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);

    // Base class generates AnimalTrait
    assert!(
        actual.contains("trait AnimalTrait"),
        "extended base class should generate {{Name}}Trait:\n{actual}"
    );

    // Base class struct still generated
    assert!(
        actual.contains("struct Animal"),
        "base class struct should still exist:\n{actual}"
    );

    // Derived class struct generated
    assert!(
        actual.contains("struct Dog"),
        "derived class struct should exist:\n{actual}"
    );

    // Derived class has inherited fields
    assert!(
        actual.contains("pub name: String") && actual.contains("pub breed: String"),
        "derived struct should have inherited + own fields:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.2 Non-extended class does NOT generate a trait (regression)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_non_extended_class_no_trait() {
    let source = "\
class User {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  greet(): string {
    return this.name;
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);

    // No trait should be generated for a class that is never extended
    assert!(
        !actual.contains("trait UserTrait"),
        "non-extended class should not generate a trait:\n{actual}"
    );
    assert!(
        !actual.contains("trait User "),
        "non-extended class should not generate a trait:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.3 Derived class gets impl {Base}Trait for {Derived}
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_concrete_extends_trait_impl() {
    let source = "\
class Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return this.name;
  }
}

class Dog extends Animal {
  breed: string;

  constructor(name: string, breed: string) {
    this.name = name;
    this.breed = breed;
  }

  speak(): string {
    return this.name;
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);

    // impl AnimalTrait for Animal
    assert!(
        actual.contains("impl AnimalTrait for Animal"),
        "base class should implement its own trait:\n{actual}"
    );

    // impl AnimalTrait for Dog
    assert!(
        actual.contains("impl AnimalTrait for Dog"),
        "derived class should implement base trait:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.4 Polymorphic function parameter rewriting
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_polymorphic_param_rewrite() {
    let source = "\
class Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return this.name;
  }
}

class Dog extends Animal {
  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return this.name;
  }
}

function makeSpeak(animal: Animal): string {
  return animal.speak();
}

function main() {
  const dog = new Dog(\"Rex\");
  console.log(makeSpeak(dog));
}";

    let actual = compile_to_rust(source);

    // Parameter type rewritten to &dyn AnimalTrait
    assert!(
        actual.contains("&dyn AnimalTrait"),
        "base class param should be rewritten to &dyn Trait:\n{actual}"
    );

    // Call site should add & before the argument
    assert!(
        actual.contains("makeSpeak(&dog)"),
        "call site should borrow the argument:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.5 Field accessor methods in trait
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_field_accessors_in_trait() {
    let source = "\
class Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return this.name;
  }
}

class Dog extends Animal {
  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return this.name;
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);

    // Trait should have field accessor: fn name(&self) -> &str
    assert!(
        actual.contains("fn name(&self) -> &str"),
        "trait should have field accessor method:\n{actual}"
    );

    // AnimalTrait impl for Animal should have accessor body
    assert!(
        actual.contains("&self.name"),
        "trait impl should have accessor body:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.6 Extends with extra fields: derived adds fields beyond inherited ones
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_extends_with_extra_fields() {
    let source = "\
class Vehicle {
  speed: f64;

  constructor(speed: f64) {
    this.speed = speed;
  }

  describe(): string {
    return \"vehicle\";
  }
}

class Car extends Vehicle {
  brand: string;

  constructor(speed: f64, brand: string) {
    this.speed = speed;
    this.brand = brand;
  }

  describe(): string {
    return this.brand;
  }
}

function main() {
  console.log(\"ok\");
}";

    let actual = compile_to_rust(source);

    // Car struct should have both inherited and own fields
    // Check that Car struct appears and has speed and brand
    let car_section = actual.split("struct Car").nth(1);
    assert!(car_section.is_some(), "struct Car should exist:\n{actual}");
    let car_section = car_section.unwrap();
    assert!(
        car_section.contains("speed: f64") && car_section.contains("brand: String"),
        "Car should have inherited speed and own brand fields:\n{actual}"
    );
}

// ---------------------------------------------------------------------------
// 7.7 E2e: polymorphic dispatch runs correctly
// ---------------------------------------------------------------------------

#[test]
#[ignore] // e2e test — requires cargo
fn test_e2e_concrete_extends_polymorphic_dispatch() {
    let source = "\
class Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return \"generic sound\";
  }
}

class Dog extends Animal {
  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return \"woof\";
  }
}

function doSpeak(a: Animal): string {
  return a.speak();
}

function main() {
  const dog = new Dog(\"Rex\");
  console.log(doSpeak(dog));
}";

    let output = compile_and_run(source);
    assert_eq!(output.trim(), "woof");
}

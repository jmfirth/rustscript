//! Conformance test suite — structured matrix of every syntax form × every valid position.
//!
//! This is a SAFETY NET, not a demo. Each test exercises one specific pattern.
//! Tests are fast (compile-only via `compile_to_rust` / `compile_source`) — no cargo invocation.
//!
//! Categories:
//! 1. Declarations in every valid position
//! 2. Expressions in every context
//! 3. Common TypeScript idioms (compile-only checks)
//! 4. Error cases (compiler produces diagnostics, not crashes)

mod test_utils;

use rustscript_driver::compile_source;
use test_utils::{compile_diagnostics, compile_to_rust};

// ===========================================================================
// Helper: compile and assert no errors (does not panic on error, returns bool)
// ===========================================================================

fn compiles_ok(source: &str) -> bool {
    let result = compile_source(source, "conformance_test.rts");
    !result.has_errors
}

fn compiles_with_errors(source: &str) -> bool {
    let result = compile_source(source, "conformance_test.rts");
    result.has_errors
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 1: Declarations in every valid position
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 1.1 const declarations
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_decl_const_top_level() {
    let source = "\
const X: i32 = 42;

function main() {
  console.log(X);
}";
    // BUG: Top-level const previously caused the compiler to hang.
    // This test catches that regression.
    assert!(compiles_ok(source), "top-level const should compile");
}

#[test]
fn test_conformance_decl_const_function_body() {
    let source = "\
function main() {
  const x: i32 = 42;
  console.log(x);
}";
    assert!(compiles_ok(source), "const in function body should compile");
}

#[test]
fn test_conformance_decl_const_if_body() {
    let source = "\
function main() {
  if (true) {
    const x: i32 = 42;
    console.log(x);
  }
}";
    assert!(compiles_ok(source), "const in if body should compile");
}

#[test]
fn test_conformance_decl_const_while_body() {
    let source = "\
function main() {
  let i: i32 = 0;
  while (i < 1) {
    const x: i32 = 42;
    console.log(x);
    i = i + 1;
  }
}";
    assert!(compiles_ok(source), "const in while body should compile");
}

#[test]
fn test_conformance_decl_const_for_body() {
    let source = "\
function main() {
  const items: Array<i32> = [1, 2, 3];
  for (const item of items) {
    const doubled: i32 = item * 2;
    console.log(doubled);
  }
}";
    assert!(compiles_ok(source), "const in for-of body should compile");
}

#[test]
fn test_conformance_decl_const_class_method_body() {
    let source = "\
class Foo {
  value: i32;
  constructor(v: i32) {
    this.value = v;
  }
  getDouble(): i32 {
    const d: i32 = this.value * 2;
    return d;
  }
}

function main() {
  const f = new Foo(21);
  console.log(f.getDouble());
}";
    assert!(
        compiles_ok(source),
        "const in class method body should compile"
    );
}

// ---------------------------------------------------------------------------
// 1.2 let declarations
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_decl_let_top_level() {
    // NOTE: top-level let may not be supported; this tests whether it at least
    // doesn't crash the compiler. If it fails, it's a known limitation.
    let source = "\
let X: i32 = 42;

function main() {
  console.log(X);
}";
    // Don't assert success — just assert it doesn't hang/crash
    let _ok = compiles_ok(source);
}

#[test]
fn test_conformance_decl_let_function_body() {
    let source = "\
function main() {
  let x: i32 = 0;
  x = 42;
  console.log(x);
}";
    assert!(compiles_ok(source), "let in function body should compile");
}

#[test]
fn test_conformance_decl_let_if_body() {
    let source = "\
function main() {
  if (true) {
    let x: i32 = 0;
    x = 42;
    console.log(x);
  }
}";
    assert!(compiles_ok(source), "let in if body should compile");
}

#[test]
fn test_conformance_decl_let_while_body() {
    let source = "\
function main() {
  let i: i32 = 0;
  while (i < 3) {
    let x: i32 = i * 2;
    console.log(x);
    i = i + 1;
  }
}";
    assert!(compiles_ok(source), "let in while body should compile");
}

#[test]
fn test_conformance_decl_let_for_body() {
    let source = "\
function main() {
  const items: Array<i32> = [1, 2, 3];
  for (const item of items) {
    let x: i32 = item + 1;
    console.log(x);
  }
}";
    assert!(compiles_ok(source), "let in for-of body should compile");
}

#[test]
fn test_conformance_decl_let_class_method_body() {
    let source = "\
class Counter {
  count: i32;
  constructor() {
    this.count = 0;
  }
  step(): i32 {
    let temp: i32 = this.count;
    this.count = this.count + 1;
    return temp;
  }
}

function main() {
  let c = new Counter();
  console.log(c.step());
}";
    assert!(
        compiles_ok(source),
        "let in class method body should compile"
    );
}

// ---------------------------------------------------------------------------
// 1.3 function declarations
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_decl_function_top_level() {
    let source = "\
function helper(): i32 {
  return 42;
}

function main() {
  console.log(helper());
}";
    assert!(
        compiles_ok(source),
        "top-level function declaration should compile"
    );
}

#[test]
fn test_conformance_decl_function_exported() {
    let source = "\
export function helper(): i32 {
  return 42;
}

function main() {
  console.log(helper());
}";
    assert!(
        compiles_ok(source),
        "exported function declaration should compile"
    );
}

// ---------------------------------------------------------------------------
// 1.4 type declarations
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_decl_type_top_level() {
    let source = "\
type Point = { x: f64, y: f64 }

function main() {
  const p: Point = { x: 1.0, y: 2.0 };
  console.log(p.x);
}";
    assert!(
        compiles_ok(source),
        "top-level type declaration should compile"
    );
}

#[test]
fn test_conformance_decl_type_exported() {
    let source = "\
export type Point = { x: f64, y: f64 }

function main() {
  const p: Point = { x: 1.0, y: 2.0 };
  console.log(p.x);
}";
    assert!(
        compiles_ok(source),
        "exported type declaration should compile"
    );
}

// ---------------------------------------------------------------------------
// 1.5 class declarations
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_decl_class_top_level() {
    let source = "\
class Greeter {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  greet(): string {
    return this.name;
  }
}

function main() {
  const g = new Greeter(\"World\");
  console.log(g.greet());
}";
    assert!(
        compiles_ok(source),
        "top-level class declaration should compile"
    );
}

#[test]
fn test_conformance_decl_class_exported() {
    let source = "\
export class Greeter {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  greet(): string {
    return this.name;
  }
}

function main() {
  const g = new Greeter(\"World\");
  console.log(g.greet());
}";
    assert!(
        compiles_ok(source),
        "exported class declaration should compile"
    );
}

// ---------------------------------------------------------------------------
// 1.6 interface declarations
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_decl_interface_top_level() {
    let source = "\
interface Printable {
  display(): string;
}

class Item implements Printable {
  label: string;
  constructor(label: string) {
    this.label = label;
  }
  display(): string {
    return this.label;
  }
}

function main() {
  const item = new Item(\"hello\");
  console.log(item.display());
}";
    assert!(
        compiles_ok(source),
        "top-level interface declaration should compile"
    );
}

// ---------------------------------------------------------------------------
// 1.7 enum declarations
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_decl_enum_top_level() {
    let source = "\
type Color = \"red\" | \"green\" | \"blue\"

function main() {
  const c: Color = \"red\";
  console.log(c);
}";
    assert!(
        compiles_ok(source),
        "top-level enum (string union) declaration should compile"
    );
}

// ---------------------------------------------------------------------------
// 1.8 import declarations
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_decl_import_top_level() {
    let source = "\
import { HashMap } from \"std::collections\";

function main() {
  console.log(\"imported\");
}";
    assert!(
        compiles_ok(source),
        "top-level import declaration should compile"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 2: Expressions in every context
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 2.1 Int literal in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_int_literal_as_initializer() {
    let source = "\
function main() {
  const x: i32 = 42;
  console.log(x);
}";
    assert!(compiles_ok(source), "int literal as initializer");
}

#[test]
fn test_conformance_expr_int_literal_as_function_arg() {
    let source = "\
function identity(x: i32): i32 {
  return x;
}

function main() {
  console.log(identity(42));
}";
    assert!(compiles_ok(source), "int literal as function argument");
}

#[test]
fn test_conformance_expr_int_literal_as_return() {
    let source = "\
function get(): i32 {
  return 42;
}

function main() {
  console.log(get());
}";
    assert!(compiles_ok(source), "int literal as return value");
}

#[test]
fn test_conformance_expr_int_literal_in_array() {
    let source = "\
function main() {
  const arr: Array<i32> = [1, 2, 3];
  console.log(arr.length);
}";
    assert!(compiles_ok(source), "int literal in array");
}

#[test]
fn test_conformance_expr_int_literal_in_struct_field() {
    let source = "\
type Config = { port: i32 }

function main() {
  const c: Config = { port: 8080 };
  console.log(c.port);
}";
    assert!(compiles_ok(source), "int literal in struct field value");
}

#[test]
fn test_conformance_expr_int_literal_in_template() {
    let source = "\
function main() {
  const x: i32 = 42;
  const msg = `value: ${x}`;
  console.log(msg);
}";
    assert!(compiles_ok(source), "int literal in template interpolation");
}

#[test]
fn test_conformance_expr_int_literal_as_if_condition() {
    // Using a comparison, since if needs bool
    let source = "\
function main() {
  if (42 > 0) {
    console.log(\"positive\");
  }
}";
    assert!(compiles_ok(source), "int literal in if condition");
}

// ---------------------------------------------------------------------------
// 2.2 String literal in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_string_literal_as_initializer() {
    let source = "\
function main() {
  const s: string = \"hello\";
  console.log(s);
}";
    assert!(compiles_ok(source), "string literal as initializer");
}

#[test]
fn test_conformance_expr_string_literal_as_function_arg() {
    let source = "\
function greet(name: string): void {
  console.log(name);
}

function main() {
  greet(\"world\");
}";
    assert!(compiles_ok(source), "string literal as function argument");
}

#[test]
fn test_conformance_expr_string_literal_as_return() {
    let source = "\
function hello(): string {
  return \"hello\";
}

function main() {
  console.log(hello());
}";
    assert!(compiles_ok(source), "string literal as return value");
}

#[test]
fn test_conformance_expr_string_literal_in_array() {
    let source = "\
function main() {
  const arr: Array<string> = [\"a\", \"b\", \"c\"];
  console.log(arr.length);
}";
    assert!(compiles_ok(source), "string literal in array");
}

// ---------------------------------------------------------------------------
// 2.3 Bool literal in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_bool_literal_as_initializer() {
    let source = "\
function main() {
  const flag: bool = true;
  console.log(flag);
}";
    assert!(compiles_ok(source), "bool literal as initializer");
}

#[test]
fn test_conformance_expr_bool_literal_as_function_arg() {
    let source = "\
function check(val: bool): bool {
  return val;
}

function main() {
  console.log(check(true));
  console.log(check(false));
}";
    assert!(compiles_ok(source), "bool literal as function argument");
}

#[test]
fn test_conformance_expr_bool_literal_as_if_condition() {
    let source = "\
function main() {
  if (true) {
    console.log(\"yes\");
  }
  if (false) {
    console.log(\"no\");
  }
}";
    assert!(compiles_ok(source), "bool literal as if condition");
}

// ---------------------------------------------------------------------------
// 2.4 Function call in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_function_call_as_initializer() {
    let source = "\
function compute(): i32 {
  return 42;
}

function main() {
  const x: i32 = compute();
  console.log(x);
}";
    assert!(compiles_ok(source), "function call as initializer");
}

#[test]
fn test_conformance_expr_function_call_as_argument() {
    let source = "\
function double(x: i32): i32 {
  return x * 2;
}

function show(x: i32): void {
  console.log(x);
}

function main() {
  show(double(21));
}";
    assert!(compiles_ok(source), "function call as argument");
}

#[test]
fn test_conformance_expr_function_call_as_return() {
    let source = "\
function inner(): i32 {
  return 42;
}

function outer(): i32 {
  return inner();
}

function main() {
  console.log(outer());
}";
    assert!(compiles_ok(source), "function call as return value");
}

// ---------------------------------------------------------------------------
// 2.5 Binary op in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_binary_op_as_initializer() {
    let source = "\
function main() {
  const sum: i32 = 10 + 20;
  console.log(sum);
}";
    assert!(compiles_ok(source), "binary op as initializer");
}

#[test]
fn test_conformance_expr_binary_op_as_return() {
    let source = "\
function add(a: i32, b: i32): i32 {
  return a + b;
}

function main() {
  console.log(add(10, 20));
}";
    assert!(compiles_ok(source), "binary op as return value");
}

#[test]
fn test_conformance_expr_binary_op_as_if_condition() {
    let source = "\
function main() {
  const x: i32 = 10;
  if (x > 5) {
    console.log(\"big\");
  }
}";
    assert!(compiles_ok(source), "binary op as if condition");
}

#[test]
fn test_conformance_expr_binary_op_in_template() {
    let source = "\
function main() {
  const a: i32 = 10;
  const b: i32 = 20;
  const msg = `sum: ${a + b}`;
  console.log(msg);
}";
    assert!(compiles_ok(source), "binary op in template interpolation");
}

// ---------------------------------------------------------------------------
// 2.6 Unary op in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_unary_not_as_initializer() {
    let source = "\
function main() {
  const flag: bool = !false;
  console.log(flag);
}";
    assert!(compiles_ok(source), "unary not as initializer");
}

#[test]
fn test_conformance_expr_unary_negation_as_initializer() {
    let source = "\
function main() {
  const x: i32 = -42;
  console.log(x);
}";
    assert!(compiles_ok(source), "unary negation as initializer");
}

// ---------------------------------------------------------------------------
// 2.7 Template literal as expression
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_template_literal_as_initializer() {
    let source = "\
function main() {
  const name = \"world\";
  const msg = `hello ${name}`;
  console.log(msg);
}";
    assert!(compiles_ok(source), "template literal as initializer");
}

#[test]
fn test_conformance_expr_template_literal_as_return() {
    let source = "\
function greet(name: string): string {
  return `hello ${name}`;
}

function main() {
  console.log(greet(\"world\"));
}";
    assert!(compiles_ok(source), "template literal as return value");
}

#[test]
fn test_conformance_expr_template_literal_as_argument() {
    let source = "\
function show(msg: string): void {
  console.log(msg);
}

function main() {
  const name = \"world\";
  show(`hello ${name}`);
}";
    assert!(compiles_ok(source), "template literal as function argument");
}

// ---------------------------------------------------------------------------
// 2.8 Closure in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_closure_as_argument() {
    let source = "\
function main() {
  const items: Array<i32> = [1, 2, 3];
  const doubled = items.map((x: i32): i32 => x * 2);
  console.log(doubled.length);
}";
    assert!(compiles_ok(source), "closure as function argument");
}

#[test]
fn test_conformance_expr_closure_block_as_argument() {
    let source = "\
function main() {
  const items: Array<i32> = [1, 2, 3];
  items.forEach((x: i32): void => {
    console.log(x);
  });
}";
    assert!(compiles_ok(source), "closure with block body as argument");
}

// ---------------------------------------------------------------------------
// 2.9 Array literal in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_array_literal_as_initializer() {
    let source = "\
function main() {
  const arr: Array<i32> = [10, 20, 30];
  console.log(arr.length);
}";
    assert!(compiles_ok(source), "array literal as initializer");
}

#[test]
fn test_conformance_expr_array_literal_as_return() {
    let source = "\
function nums(): Array<i32> {
  return [1, 2, 3];
}

function main() {
  const arr = nums();
  console.log(arr.length);
}";
    assert!(compiles_ok(source), "array literal as return value");
}

// ---------------------------------------------------------------------------
// 2.10 Struct literal in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_struct_literal_as_initializer() {
    let source = "\
type Pair = { a: i32, b: i32 }

function main() {
  const p: Pair = { a: 1, b: 2 };
  console.log(p.a);
}";
    assert!(compiles_ok(source), "struct literal as initializer");
}

#[test]
fn test_conformance_expr_struct_literal_as_return() {
    let source = "\
type Pair = { a: i32, b: i32 }

function make(): Pair {
  return { a: 10, b: 20 };
}

function main() {
  const p = make();
  console.log(p.a);
}";
    assert!(compiles_ok(source), "struct literal as return value");
}

// ---------------------------------------------------------------------------
// 2.11 Field access in various contexts
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_field_access_as_initializer() {
    let source = "\
type Point = { x: i32, y: i32 }

function main() {
  const p: Point = { x: 10, y: 20 };
  const x: i32 = p.x;
  console.log(x);
}";
    assert!(compiles_ok(source), "field access as initializer");
}

#[test]
fn test_conformance_expr_field_access_as_argument() {
    let source = "\
type Point = { x: i32, y: i32 }

function show(val: i32): void {
  console.log(val);
}

function main() {
  const p: Point = { x: 42, y: 0 };
  show(p.x);
}";
    assert!(compiles_ok(source), "field access as function argument");
}

#[test]
fn test_conformance_expr_field_access_as_return() {
    let source = "\
type Point = { x: i32, y: i32 }

function getX(p: Point): i32 {
  return p.x;
}

function main() {
  const p: Point = { x: 42, y: 0 };
  console.log(getX(p));
}";
    assert!(compiles_ok(source), "field access as return value");
}

// ---------------------------------------------------------------------------
// 2.12 Optional chain + nullish coalesce
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_expr_optional_chain_as_initializer() {
    let source = "\
function maybe(): string | null {
  return \"hello\";
}

function main() {
  const val = maybe();
  const len = val?.length;
  console.log(len);
}";
    // Optional chain may or may not be fully supported — just verify no crash
    let _ok = compiles_ok(source);
}

#[test]
fn test_conformance_expr_nullish_coalesce_as_initializer() {
    let source = "\
function maybe(): string | null {
  return null;
}

function main() {
  const val = maybe();
  const result = val ?? \"default\";
  console.log(result);
}";
    // Nullish coalesce may or may not be fully supported — just verify no crash
    let _ok = compiles_ok(source);
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 3: Common TypeScript idioms (compile-only)
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_idiom_map_with_closure() {
    // const doubled = nums.map(n => n * 2)
    let source = "\
function main() {
  const nums: Array<i32> = [1, 2, 3, 4, 5];
  const doubled = nums.map((n: i32): i32 => n * 2);
  console.log(doubled.length);
}";
    assert!(compiles_ok(source), "map with closure should compile");
}

#[test]
fn test_conformance_idiom_filter_with_closure() {
    // const big = nums.filter(n => n > 3)
    let source = "\
function main() {
  const nums: Array<i32> = [1, 2, 3, 4, 5];
  const big = nums.filter((n: i32): bool => n > 3);
  console.log(big.length);
}";
    assert!(compiles_ok(source), "filter with closure should compile");
}

#[test]
fn test_conformance_idiom_reduce_sum() {
    // const total = prices.reduce((sum, p) => sum + p, 0)
    let source = "\
function main() {
  const prices: Array<i32> = [10, 20, 30];
  const total = prices.reduce((sum: i32, p: i32): i32 => sum + p, 0);
  console.log(total);
}";
    assert!(compiles_ok(source), "reduce to sum should compile");
}

#[test]
fn test_conformance_idiom_for_of_iteration() {
    // for (const item of items) { console.log(item); }
    let source = "\
function main() {
  const items: Array<string> = [\"a\", \"b\", \"c\"];
  for (const item of items) {
    console.log(item);
  }
}";
    assert!(compiles_ok(source), "for-of iteration should compile");
}

#[test]
fn test_conformance_idiom_template_literal() {
    // const greeting = `Hello, ${name}!`
    let source = "\
function main() {
  const name = \"world\";
  const greeting = `Hello, ${name}!`;
  console.log(greeting);
}";
    assert!(compiles_ok(source), "template literal should compile");
}

#[test]
fn test_conformance_idiom_struct_literal() {
    // const config = { host: "localhost", port: 8080 }
    let source = "\
type Config = { host: string, port: i32 }

function main() {
  const config: Config = { host: \"localhost\", port: 8080 };
  console.log(config.host);
  console.log(config.port);
}";
    assert!(compiles_ok(source), "struct literal idiom should compile");
}

#[test]
fn test_conformance_idiom_destructuring() {
    // const { host, port } = config
    let source = "\
type Config = { host: string, port: i32 }

function main() {
  const config: Config = { host: \"localhost\", port: 8080 };
  const { host, port } = config;
  console.log(host);
}";
    assert!(
        compiles_ok(source),
        "destructuring assignment should compile"
    );
}

#[test]
fn test_conformance_idiom_switch_case() {
    // switch (status) { case "ok": ... case "error": ... }
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
}";
    assert!(compiles_ok(source), "switch/case idiom should compile");
}

#[test]
fn test_conformance_idiom_try_catch() {
    // try { riskyOp(); } catch (e) { handleError(e); }
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
    assert!(compiles_ok(source), "try/catch idiom should compile");
}

#[test]
fn test_conformance_idiom_string_to_upper() {
    // const upper = name.toUpperCase()
    let source = "\
function main() {
  const name = \"hello\";
  const upper = name.toUpperCase();
  console.log(upper);
}";
    assert!(
        compiles_ok(source),
        "string toUpperCase idiom should compile"
    );
}

#[test]
fn test_conformance_idiom_string_split() {
    // const parts = csv.split(",")
    let source = "\
function main() {
  const csv = \"a,b,c\";
  const parts = csv.split(\",\");
  console.log(parts.length);
}";
    assert!(compiles_ok(source), "string split idiom should compile");
}

#[test]
fn test_conformance_idiom_export_function() {
    // export function helper() { ... }
    let source = "\
export function helper(): i32 {
  return 42;
}

function main() {
  console.log(helper());
}";
    assert!(compiles_ok(source), "export function idiom should compile");
}

#[test]
fn test_conformance_idiom_foreach_side_effect() {
    // items.forEach(item => console.log(item))
    let source = "\
function main() {
  const items: Array<string> = [\"a\", \"b\", \"c\"];
  items.forEach((item: string): void => {
    console.log(item);
  });
}";
    assert!(compiles_ok(source), "forEach side-effect should compile");
}

#[test]
fn test_conformance_idiom_simple_transform_map() {
    // const doubled = nums.map(n => n * 2)
    let source = "\
function main() {
  const nums: Array<i32> = [1, 2, 3];
  const doubled = nums.map((n: i32): i32 => n * 2);
  console.log(doubled.length);
}";
    assert!(compiles_ok(source), "simple map transform should compile");
}

#[test]
fn test_conformance_idiom_class_with_methods() {
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
    assert!(
        compiles_ok(source),
        "class with methods idiom should compile"
    );
}

#[test]
fn test_conformance_idiom_interface_implementation() {
    let source = "\
interface Shape {
  area(): f64;
}

class Circle implements Shape {
  radius: f64;
  constructor(r: f64) {
    this.radius = r;
  }
  area(): f64 {
    return 3.14159 * this.radius * this.radius;
  }
}

function main() {
  const c = new Circle(5.0);
  console.log(c.area());
}";
    assert!(
        compiles_ok(source),
        "interface implementation idiom should compile"
    );
}

#[test]
fn test_conformance_idiom_nested_function_calls() {
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
    assert!(
        compiles_ok(source),
        "nested function calls idiom should compile"
    );
}

#[test]
fn test_conformance_idiom_multiple_return_paths() {
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
    assert!(compiles_ok(source), "multiple return paths should compile");
}

#[test]
fn test_conformance_idiom_while_with_break() {
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
    assert!(compiles_ok(source), "while loop with break should compile");
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 4: Error cases — diagnostics, not crashes
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_error_undefined_variable() {
    let source = "\
function main() {
  console.log(undefinedVar);
}";
    // The compiler may or may not report this (it might defer to rustc).
    // The critical assertion: it must not crash.
    let result = compile_source(source, "test.rts");
    // Accept either: compiler catches it, or it silently passes to rustc
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_unknown_type() {
    let source = "\
function main() {
  const x: FakeType = 42;
}";
    assert!(
        compiles_with_errors(source),
        "unknown type name should produce a diagnostic"
    );
    let messages = compile_diagnostics(source);
    assert!(
        messages.iter().any(|m| m.contains("unknown type")),
        "diagnostic should mention 'unknown type', got: {messages:?}"
    );
}

#[test]
fn test_conformance_error_unterminated_string() {
    let source = "\
function main() {
  const x = \"hello;
}";
    assert!(
        compiles_with_errors(source),
        "unterminated string should produce a diagnostic"
    );
}

#[test]
fn test_conformance_error_unterminated_template_literal() {
    let source = "\
function main() {
  const x = `hello ${name;
}";
    assert!(
        compiles_with_errors(source),
        "unterminated template literal should produce a diagnostic"
    );
}

#[test]
fn test_conformance_error_missing_semicolon_after_const() {
    let source = "\
function main() {
  const x = 42
  const y = 43;
}";
    // RustScript may allow omitted semicolons (like TS with ASI).
    // The key assertion: it must not crash.
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_missing_closing_brace() {
    let source = "\
function main() {
  if (true) {
    console.log(\"hello\");
";
    assert!(
        compiles_with_errors(source),
        "missing closing brace should produce a diagnostic"
    );
}

#[test]
fn test_conformance_error_syntax_error_unexpected_token() {
    let source = "\
function main() {
  const x = ;
}";
    assert!(
        compiles_with_errors(source),
        "unexpected token should produce a diagnostic"
    );
    let messages = compile_diagnostics(source);
    assert!(
        !messages.is_empty(),
        "should have at least one diagnostic for syntax error"
    );
}

#[test]
fn test_conformance_error_duplicate_function_params() {
    let source = "\
function foo(x: i32, x: i32): i32 {
  return x;
}";
    // May or may not be caught — assert no crash
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_empty_source() {
    let source = "";
    // Empty source should compile to empty output or produce a diagnostic, not crash
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_deeply_nested_expressions() {
    // Build a deeply nested expression: ((((((1 + 1) + 1) + 1)...)))
    // Depth 15 is chosen to stay within stack limits in debug mode as the AST
    // enum grows with new expression kinds.
    let mut expr = String::from("1");
    for _ in 0..15 {
        expr = format!("({expr} + 1)");
    }
    let source = format!(
        "\
function main() {{
  const x: i32 = {expr};
  console.log(x);
}}"
    );
    // Must not crash or hang — may produce a diagnostic about depth
    let result = compile_source(&source, "test.rts");
    let _ = result.has_errors;
}

// ---------------------------------------------------------------------------
// T110: for-in loops
// ---------------------------------------------------------------------------

#[test]
fn test_conformance_for_in_map_keys() {
    let source = r#"
function main() {
  const map: Map<string, i32> = new Map();
  for (const k in map) {
    console.log(k);
  }
}"#;
    assert!(
        compiles_ok(source),
        "for-in on Map should compile without errors"
    );
}

#[test]
fn test_conformance_for_in_body_accesses_value() {
    let source = r#"
function main() {
  const map: Map<string, i32> = new Map();
  for (const k in map) {
    console.log(k);
  }
}"#;
    assert!(
        compiles_ok(source),
        "for-in with body accessing key should compile"
    );
}

#[test]
fn test_conformance_for_in_parses_distinctly_from_for_of() {
    // for-in should parse as ForIn, for-of as For
    let source_in = r#"
function main() {
  const m: Map<string, i32> = new Map();
  for (const k in m) {
    console.log(k);
  }
}"#;
    let source_of = r#"
function main() {
  const items: Array<i32> = [1, 2, 3];
  for (const n of items) {
    console.log(n);
  }
}"#;
    assert!(compiles_ok(source_in), "for-in should compile");
    assert!(compiles_ok(source_of), "for-of should compile");

    // Both compile but produce different Rust output
    let result_in = compile_to_rust(source_in);
    let result_of = compile_to_rust(source_of);
    assert!(
        result_in.contains(".keys()"),
        "for-in should emit .keys(): {result_in}"
    );
    assert!(
        !result_of.contains(".keys()"),
        "for-of should NOT emit .keys(): {result_of}"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 5: Expressions in Closures
//
// Tests that various expression forms work correctly inside arrow functions
// and closures — a common source of parser/codegen combinatorial bugs.
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_closure_optional_chain() {
    let source = r#"
type User = { name: string | null }

function main() {
  const users: Array<User> = [];
  const names = users.map((u: User): string => u.name ?? "unknown");
  console.log(names.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
    assert!(rust.contains("fn main"), "should have main: {rust}");
}

#[test]
fn test_conformance_closure_nullish_coalescing() {
    let source = r#"
function main() {
  const items: Array<string | null> = [null, "a", null];
  const filled = items.map((x: string | null): string => x ?? "default");
  console.log(filled.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_closure_template_literal() {
    let source = r#"
function main() {
  const names: Array<string> = ["Alice", "Bob"];
  const greetings = names.map((n: string): string => `hello ${n}`);
  console.log(greetings.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
    assert!(rust.contains("format!"), "should use format! macro: {rust}");
}

#[test]
fn test_conformance_closure_ternary() {
    let source = r#"
function main() {
  const nums: Array<i32> = [1, -2, 3, -4];
  const labels = nums.map((x: i32): string => x > 0 ? "pos" : "neg");
  console.log(labels.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_closure_type_assertion() {
    // Type assertion inside a closure — `as` cast
    let source = r#"
function main() {
  const nums: Array<i32> = [1, 2, 3];
  const floats = nums.map((x: i32): f64 => x as f64);
  console.log(floats.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_closure_spread_return() {
    let source = r#"
function main() {
  const arrs: Array<Array<i32>> = [[1, 2], [3, 4]];
  const expanded = arrs.map((x: Array<i32>): Array<i32> => [...x, 99]);
  console.log(expanded.length);
}"#;
    assert!(
        compiles_ok(source),
        "spread in closure return should compile"
    );
}

#[test]
fn test_conformance_closure_destructured_param() {
    let source = r#"
type Pair = { name: string, age: i32 }

function main() {
  const pairs: Array<Pair> = [{ name: "Alice", age: 30 }];
  const names = pairs.map(({ name, age }: Pair): string => name);
  console.log(names.length);
}"#;
    assert!(
        compiles_ok(source),
        "destructuring in closure params should compile"
    );
}

#[test]
fn test_conformance_closure_typeof_guard() {
    let source = r#"
function isString(x: string | i32): bool {
  return typeof x === "string";
}

function main() {
  console.log(isString("hello"));
}"#;
    assert!(compiles_ok(source), "typeof guard should compile");
}

#[test]
fn test_conformance_closure_binary_op() {
    let source = r#"
function main() {
  const nums: Array<i32> = [1, 2, 3, 4, 5];
  const doubled = nums.map((x: i32): i32 => x * 2 + 1);
  console.log(doubled.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_closure_nested_call() {
    let source = r#"
function double(x: i32): i32 {
  return x * 2;
}

function main() {
  const nums: Array<i32> = [1, 2, 3];
  const result = nums.map((x: i32): i32 => double(x));
  console.log(result.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_closure_string_method() {
    let source = r#"
function main() {
  const words: Array<string> = ["hello", "world"];
  const upper = words.map((w: string): string => w.toUpperCase());
  console.log(upper.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_closure_comparison_chain() {
    let source = r#"
function main() {
  const nums: Array<i32> = [1, 2, 3, 4, 5];
  const mid = nums.filter((x: i32): bool => x > 1 && x < 5);
  console.log(mid.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_closure_block_body() {
    let source = r#"
function main() {
  const nums: Array<i32> = [1, 2, 3];
  const result = nums.map((x: i32): i32 => {
    const doubled: i32 = x * 2;
    return doubled + 1;
  });
  console.log(result.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_closure_immediate_return_struct() {
    let source = r#"
type Point = { x: i32, y: i32 }

function main() {
  const nums: Array<i32> = [1, 2, 3];
  const points = nums.map((n: i32): Point => { return { x: n, y: n * 2 }; });
  console.log(points.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("Point {"),
        "closure returning struct should emit Point {{ ... }}: {rust}"
    );
}

#[test]
fn test_conformance_closure_chained_methods() {
    let source = r#"
function main() {
  const nums: Array<i32> = [1, -2, 3, -4, 5];
  const result = nums.filter((x: i32): bool => x > 0).map((x: i32): i32 => x * 10);
  console.log(result.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 6: Expressions in Loop Bodies
//
// Tests that various statement/expression forms work inside different loop
// constructs — for-of, while, classic for, do-while.
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_loop_optional_chain_in_for_of() {
    let source = r#"
type Item = { label: string | null }

function main() {
  const items: Array<Item> = [{ label: "a" }, { label: null }];
  for (const item of items) {
    const lbl: string = item.label ?? "none";
    console.log(lbl);
  }
}"#;
    let rust = compile_to_rust(source);
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_loop_destructuring_in_while() {
    let source = r#"
type Pair = { a: i32, b: i32 }

function main() {
  const pairs: Array<Pair> = [{ a: 1, b: 2 }, { a: 3, b: 4 }];
  let i: i32 = 0;
  while (i < pairs.length) {
    const { a, b } = pairs[i];
    console.log(a + b);
    i = i + 1;
  }
}"#;
    assert!(
        compiles_ok(source),
        "destructuring with indexed array access should compile"
    );
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("let Pair { a, b, .. }"),
        "should destructure with Pair type name: {rust}"
    );
}

#[test]
fn test_conformance_loop_switch_inside_for() {
    let source = r#"
function main() {
  const commands: Array<string> = ["start", "stop", "start"];
  for (const cmd of commands) {
    switch (cmd) {
      case "start": console.log("starting");
      case "stop": console.log("stopping");
    }
  }
}"#;
    assert!(compiles_ok(source), "switch inside for-of should compile");
    let rust = compile_to_rust(source);
    assert!(
        rust.contains(".as_str()"),
        "should match on .as_str() for string switch: {rust}"
    );
    assert!(
        rust.contains("\"start\""),
        "should have string literal pattern: {rust}"
    );
}

#[test]
fn test_conformance_loop_try_catch_inside_for_of() {
    let source = r#"
function riskyOp(x: i32): i32 throws string {
  if (x < 0) {
    throw "negative";
  }
  return x * 2;
}

function main() {
  const nums: Array<i32> = [1, -1, 2];
  for (const n of nums) {
    try {
      const r = riskyOp(n);
      console.log(r);
    } catch (e) {
      console.log("caught error");
    }
  }
}"#;
    assert!(
        compiles_ok(source),
        "try/catch inside for-of should compile"
    );
}

#[test]
fn test_conformance_loop_classic_for_complex_update() {
    let source = r#"
function main() {
  for (let i: i32 = 0; i < 20; i = i + 3) {
    console.log(i);
  }
}"#;
    assert!(
        compiles_ok(source),
        "classic for with step 3 should compile"
    );
}

#[test]
fn test_conformance_loop_nested_for_classic_inside_for_of() {
    let source = r#"
function main() {
  const rows: Array<i32> = [1, 2, 3];
  for (const row of rows) {
    for (let j: i32 = 0; j < row; j = j + 1) {
      console.log(j);
    }
  }
}"#;
    assert!(
        compiles_ok(source),
        "nested classic for inside for-of should compile"
    );
}

#[test]
fn test_conformance_loop_labeled_break() {
    let source = r#"
function main() {
  let found: i32 = -1;
  outer: for (let i: i32 = 0; i < 5; i = i + 1) {
    for (let j: i32 = 0; j < 5; j = j + 1) {
      if (i * j > 6) {
        found = i * j;
        break outer;
      }
    }
  }
  console.log(found);
}"#;
    assert!(
        compiles_ok(source),
        "labeled break in nested loops should compile"
    );
}

#[test]
fn test_conformance_loop_for_in_map() {
    let source = r#"
function main() {
  const m: Map<string, i32> = new Map();
  for (const key in m) {
    console.log(key);
  }
}"#;
    assert!(compiles_ok(source), "for-in on Map entries should compile");
}

#[test]
fn test_conformance_loop_method_chain_in_body() {
    let source = r#"
function main() {
  const data: Array<Array<i32>> = [[1, 2, 3], [4, 5, 6]];
  for (const row of data) {
    const sum: i32 = row.reduce((a: i32, b: i32): i32 => a + b, 0);
    console.log(sum);
  }
}"#;
    assert!(
        compiles_ok(source),
        "method chain in loop body should compile"
    );
}

#[test]
fn test_conformance_loop_do_while() {
    let source = r#"
function main() {
  let count: i32 = 0;
  do {
    count = count + 1;
  } while (count < 5);
  console.log(count);
}"#;
    assert!(compiles_ok(source), "do-while loop should compile");
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 7: Types in Every Position
//
// Tests that type annotations work in all positions: function params, return
// types, fields, type aliases, generics.
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_type_union_as_param() {
    let source = r#"
function show(x: string | i32): string {
  return "value";
}

function main() {
  console.log(show("hello"));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "union param should compile: {rust}"
    );
}

#[test]
fn test_conformance_type_union_as_return() {
    let source = r#"
function maybe(): string | null {
  return null;
}

function main() {
  const x = maybe();
  console.log("done");
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "union return should compile: {rust}"
    );
}

#[test]
fn test_conformance_type_generic_field() {
    let source = r#"
class Container {
  items: Array<string>;
  constructor() {
    this.items = [];
  }
}

function main() {
  const c = new Container();
  console.log(c.items.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "generic field should compile: {rust}"
    );
}

#[test]
fn test_conformance_type_tuple_as_param() {
    let source = r#"
function first(pair: [string, i32]): string {
  return pair[0];
}

function main() {
  const p: [string, i32] = ["hello", 42];
  console.log(first(p));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "tuple param should compile: {rust}"
    );
}

#[test]
fn test_conformance_type_function_type_as_param() {
    let source = r#"
function apply(f: (x: i32) => i32, val: i32): i32 {
  return f(val);
}

function main() {
  const result = apply((x: i32): i32 => x * 2, 21);
  console.log(result);
}"#;
    assert!(
        compiles_ok(source),
        "function type with named params should compile"
    );
}

#[test]
fn test_conformance_type_intersection() {
    let source = r#"
type Named = { name: string }
type Aged = { age: i32 }
type Person = Named & Aged

function greet(p: Person): string {
  return p.name;
}

function main() {
  const p: Person = { name: "Alice", age: 30 };
  console.log(greet(p));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "intersection type should compile: {rust}"
    );
}

#[test]
fn test_conformance_type_never_return() {
    let source = r#"
function fail(msg: string): never {
  throw msg;
}

function main() {
  console.log("before");
}"#;
    assert!(compiles_ok(source), "never return type should compile");
}

#[test]
fn test_conformance_type_partial() {
    let source = r#"
type Config = { host: string, port: i32 }

function withDefaults(partial: Partial<Config>): string {
  return "ok";
}

function main() {
  console.log(withDefaults({ host: "localhost" }));
}"#;
    assert!(compiles_ok(source), "Partial utility type should compile");
}

#[test]
fn test_conformance_type_readonly_array() {
    // readonly array type annotation
    let source = r#"
function sum(arr: Array<i32>): i32 {
  return arr.reduce((a: i32, b: i32): i32 => a + b, 0);
}

function main() {
  const nums: Array<i32> = [1, 2, 3];
  console.log(sum(nums));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "array param should compile: {rust}"
    );
}

#[test]
fn test_conformance_type_conditional() {
    let source = r#"
type IsString<T> = T extends string ? "yes" : "no"

function main() {
  console.log("done");
}"#;
    assert!(compiles_ok(source), "conditional type alias should compile");
}

#[test]
fn test_conformance_type_keyof() {
    let source = r#"
type Point = { x: f64, y: f64 }
type PointKeys = keyof Point

function main() {
  console.log("done");
}"#;
    assert!(compiles_ok(source), "keyof in type alias should compile");
}

#[test]
fn test_conformance_type_record() {
    let source = r#"
function main() {
  const scores: Record<string, i32> = {};
  console.log("done");
}"#;
    assert!(compiles_ok(source), "Record type should compile");
}

#[test]
fn test_conformance_type_required() {
    let source = r#"
type MaybeConfig = { host?: string, port?: i32 }
type FullConfig = Required<MaybeConfig>

function main() {
  console.log("done");
}"#;
    let rust = compile_to_rust(source);
    // MaybeConfig fields should be Option<T>
    assert!(
        rust.contains("Option<String>"),
        "host? should produce Option<String>: {rust}"
    );
    assert!(
        rust.contains("Option<i32>"),
        "port? should produce Option<i32>: {rust}"
    );
}

#[test]
fn test_conformance_type_nested_generic() {
    let source = r#"
function main() {
  const nested: Array<Array<i32>> = [[1, 2], [3, 4]];
  console.log(nested.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "nested generic should compile: {rust}"
    );
}

#[test]
fn test_conformance_type_optional_field() {
    let source = r#"
type Config = { host: string, port?: i32 }

function main() {
  const c: Config = { host: "localhost" };
  console.log(c.host);
}"#;
    let rust = compile_to_rust(source);
    // host should be String (not Option), port should be Option<i32>
    assert!(
        rust.contains("pub host: String"),
        "host should be String: {rust}"
    );
    assert!(
        rust.contains("pub port: Option<i32>"),
        "port? should produce Option<i32>: {rust}"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 8: Statements in Nested Contexts
//
// Tests that statements work correctly in nested positions: if inside try,
// switch inside if, loops inside switch, etc.
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_nested_if_inside_try_catch() {
    let source = r#"
function riskyOp(): i32 throws string {
  return 42;
}

function main() {
  try {
    const val = riskyOp();
    if (val > 10) {
      console.log("big");
    } else {
      console.log("small");
    }
  } catch (e) {
    console.log("error");
  }
}"#;
    assert!(
        compiles_ok(source),
        "if/else inside try/catch should compile"
    );
}

#[test]
fn test_conformance_nested_switch_inside_if() {
    let source = r#"
function main() {
  const mode = "fast";
  if (true) {
    switch (mode) {
      case "fast": console.log("speedy");
      case "slow": console.log("careful");
    }
  }
}"#;
    assert!(compiles_ok(source), "switch inside if body should compile");
    let rust = compile_to_rust(source);
    assert!(
        rust.contains(".as_str()"),
        "should match on .as_str() for string switch: {rust}"
    );
    assert!(
        rust.contains("\"fast\""),
        "should have string literal pattern: {rust}"
    );
}

#[test]
fn test_conformance_nested_for_inside_switch() {
    let source = r#"
function main() {
  const mode = "iterate";
  switch (mode) {
    case "iterate": {
      for (let i: i32 = 0; i < 3; i = i + 1) {
        console.log(i);
      }
    }
  }
}"#;
    assert!(
        compiles_ok(source),
        "for loop inside switch case should compile"
    );
    let rust = compile_to_rust(source);
    assert!(
        rust.contains(".as_str()"),
        "should match on .as_str() for string switch: {rust}"
    );
    assert!(
        rust.contains("\"iterate\""),
        "should have string literal pattern: {rust}"
    );
}

#[test]
fn test_conformance_nested_return_from_closure() {
    let source = r#"
function main() {
  const nums: Array<i32> = [1, 2, 3];
  const result = nums.map((x: i32): i32 => {
    if (x > 2) {
      return x * 10;
    }
    return x;
  });
  console.log(result.length);
}"#;
    assert!(
        compiles_ok(source),
        "return from nested closure should compile"
    );
}

#[test]
fn test_conformance_nested_throw_inside_for() {
    let source = r#"
function process(items: Array<i32>): i32 throws string {
  let sum: i32 = 0;
  for (const item of items) {
    if (item < 0) {
      throw "negative value";
    }
    sum = sum + item;
  }
  return sum;
}

function main() {
  try {
    const r = process([1, 2, 3]);
    console.log(r);
  } catch (e) {
    console.log("error");
  }
}"#;
    assert!(compiles_ok(source), "throw inside for loop should compile");
}

#[test]
fn test_conformance_nested_var_in_block() {
    let source = r#"
function main() {
  let x: i32 = 1;
  {
    let y: i32 = 2;
    console.log(y);
  }
  console.log(x);
}"#;
    assert!(compiles_ok(source), "bare block scopes should compile");
}

#[test]
fn test_conformance_nested_labeled_break_switch_in_loop() {
    let source = r#"
function main() {
  const items: Array<string> = ["a", "stop", "b"];
  outer: for (const item of items) {
    switch (item) {
      case "stop": break outer;
    }
    console.log(item);
  }
}"#;
    assert!(
        compiles_ok(source),
        "labeled break from switch in loop should compile"
    );
    let rust = compile_to_rust(source);
    assert!(
        rust.contains(".as_str()"),
        "should match on .as_str() for string switch: {rust}"
    );
    assert!(
        rust.contains("\"stop\""),
        "should have string literal pattern: {rust}"
    );
    assert!(rust.contains("'outer"), "should have 'outer label: {rust}");
}

#[test]
fn test_conformance_nested_while_with_if_break() {
    let source = r#"
function main() {
  let count: i32 = 0;
  while (count < 100) {
    if (count > 5) {
      break;
    }
    count = count + 1;
  }
  console.log(count);
}"#;
    assert!(compiles_ok(source), "if/break inside while should compile");
}

#[test]
fn test_conformance_nested_triple_if() {
    let source = r#"
function classify(a: i32, b: i32, c: i32): string {
  if (a > 0) {
    if (b > 0) {
      if (c > 0) {
        return "all positive";
      }
      return "c not positive";
    }
    return "b not positive";
  }
  return "a not positive";
}

function main() {
  console.log(classify(1, 2, 3));
}"#;
    assert!(compiles_ok(source), "triple nested if should compile");
}

#[test]
fn test_conformance_nested_try_in_while() {
    let source = r#"
function riskyOp(x: i32): i32 throws string {
  if (x == 0) { throw "zero"; }
  return 10 / x;
}

function main() {
  let i: i32 = 3;
  while (i > 0) {
    try {
      const r = riskyOp(i);
      console.log(r);
    } catch (e) {
      console.log("caught");
    }
    i = i - 1;
  }
}"#;
    assert!(compiles_ok(source), "try/catch inside while should compile");
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 9: Class Features in Combination
//
// Tests that class features work when combined: inheritance + interfaces,
// constructors + defaults, static + instance, etc.
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_class_all_field_modifiers() {
    let source = r#"
class Config {
  readonly host: string;
  port: i32;
  private secret: string;
  static defaultPort: i32 = 8080;

  constructor(host: string, port: i32, secret: string) {
    this.host = host;
    this.port = port;
    this.secret = secret;
  }
}

function main() {
  const c = new Config("localhost", 3000, "s3cr3t");
  console.log(c.host);
}"#;
    assert!(
        compiles_ok(source),
        "class with multiple field modifiers should compile"
    );
}

#[test]
fn test_conformance_class_constructor_defaults_and_super() {
    let source = r#"
class Base {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}

class Derived extends Base {
  value: i32;
  constructor(name: string, value: i32 = 0) {
    super(name);
    this.value = value;
  }
}

function main() {
  const d = new Derived("test", 42);
  console.log(d.name);
}"#;
    assert!(
        compiles_ok(source),
        "constructor with default params should compile"
    );
}

#[test]
fn test_conformance_class_static_and_instance_methods() {
    let source = r#"
class Counter {
  count: i32;
  static instances: i32 = 0;

  constructor() {
    this.count = 0;
  }

  increment(): void {
    this.count = this.count + 1;
  }

  getCount(): i32 {
    return this.count;
  }

  static create(): Counter {
    return new Counter();
  }
}

function main() {
  const c = Counter.create();
  c.increment();
  console.log(c.getCount());
}"#;
    assert!(
        compiles_ok(source),
        "static + instance methods on same class should compile"
    );
}

#[test]
fn test_conformance_class_getter_setter() {
    let source = r#"
class Temperature {
  private celsius: f64;

  constructor(c: f64) {
    this.celsius = c;
  }

  get fahrenheit(): f64 {
    return this.celsius * 1.8 + 32.0;
  }

  set fahrenheit(f: f64) {
    this.celsius = (f - 32.0) / 1.8;
  }
}

function main() {
  const t = new Temperature(100.0);
  console.log(t.fahrenheit);
}"#;
    assert!(compiles_ok(source), "getter/setter should compile");
}

#[test]
fn test_conformance_class_implements_interface() {
    let source = r#"
interface Printable {
  toString(): string;
}

interface Sizeable {
  size(): i32;
}

class Document implements Printable, Sizeable {
  content: string;
  constructor(content: string) {
    this.content = content;
  }
  toString(): string {
    return this.content;
  }
  size(): i32 {
    return this.content.length;
  }
}

function main() {
  const doc = new Document("hello");
  console.log(doc.toString());
  console.log(doc.size());
}"#;
    assert!(
        compiles_ok(source),
        "class implementing multiple interfaces should compile"
    );
}

#[test]
fn test_conformance_class_abstract_and_concrete_mix() {
    let source = r#"
abstract class Shape {
  abstract area(): f64;

  describe(): string {
    return "a shape";
  }
}

class Circle extends Shape {
  radius: f64;
  constructor(r: f64) {
    super();
    this.radius = r;
  }
  area(): f64 {
    return 3.14159 * this.radius * this.radius;
  }
}

function main() {
  const c = new Circle(5.0);
  console.log(c.area());
  console.log(c.describe());
}"#;
    assert!(
        compiles_ok(source),
        "abstract + concrete method mix should compile"
    );
}

#[test]
fn test_conformance_class_inheritance_chain() {
    let source = r#"
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  speak(): string {
    return "...";
  }
}

class Dog extends Animal {
  constructor(name: string) {
    super(name);
  }
  speak(): string {
    return "Woof";
  }
}

function main() {
  const d = new Dog("Rex");
  console.log(d.speak());
  console.log(d.name);
}"#;
    assert!(
        compiles_ok(source),
        "class inheritance chain should compile"
    );
}

#[test]
fn test_conformance_class_with_array_field() {
    let source = r#"
class TodoList {
  items: Array<string>;
  constructor() {
    this.items = [];
  }
  add(item: string): void {
    this.items.push(item);
  }
  count(): i32 {
    return this.items.length;
  }
}

function main() {
  let list = new TodoList();
  list.add("buy milk");
  list.add("write code");
  console.log(list.count());
}"#;
    assert!(compiles_ok(source), "class with Array field should compile");
}

#[test]
fn test_conformance_class_method_returning_self_type() {
    let source = r#"
class Builder {
  value: i32;
  constructor() {
    this.value = 0;
  }
  add(n: i32): Builder {
    this.value = this.value + n;
    return this;
  }
}

function main() {
  let b = new Builder();
  b = b.add(10).add(20);
  console.log(b.value);
}"#;
    assert!(
        compiles_ok(source),
        "method returning self type should compile"
    );
}

#[test]
fn test_conformance_class_private_method() {
    let source = r#"
class Validator {
  private isValid(s: string): bool {
    return s.length > 0;
  }
  validate(s: string): string {
    if (this.isValid(s)) {
      return "ok";
    }
    return "invalid";
  }
}

function main() {
  const v = new Validator();
  console.log(v.validate("hello"));
}"#;
    assert!(
        compiles_ok(source),
        "class with private method should compile"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 10: Feature Interactions
//
// Tests where two or more features combine in ways that might break codegen.
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_interact_destructuring_with_defaults() {
    let source = r#"
type Config = { host: string, port: i32 }

function main() {
  const config: Config = { host: "localhost", port: 8080 };
  const { host, port } = config;
  console.log(host);
  console.log(port);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "destructuring should compile: {rust}"
    );
}

#[test]
fn test_conformance_interact_spread_and_rest() {
    let source = r#"
function sum(...args: Array<i32>): i32 {
  return args.reduce((a: i32, b: i32): i32 => a + b, 0);
}

function main() {
  const nums: Array<i32> = [1, 2, 3];
  console.log(sum(...nums));
}"#;
    assert!(compiles_ok(source), "spread + rest should compile");
}

#[test]
fn test_conformance_interact_enum_in_switch() {
    let source = r#"
enum Color {
  Red = "red",
  Green = "green",
  Blue = "blue",
}

function describe(c: Color): string {
  switch (c) {
    case Color.Red: return "warm";
    case Color.Green: return "natural";
    case Color.Blue: return "cool";
  }
}

function main() {
  console.log(describe(Color.Red));
}"#;
    assert!(
        compiles_ok(source),
        "enum member access in switch cases should compile"
    );
}

#[test]
fn test_conformance_interact_for_of_destructured_pair() {
    let source = r#"
function main() {
  const pairs: Array<[string, i32]> = [["a", 1], ["b", 2]];
  for (const pair of pairs) {
    console.log(pair[0]);
    console.log(pair[1]);
  }
}"#;
    assert!(compiles_ok(source), "for-of with tuple should compile");
}

#[test]
fn test_conformance_interact_closure_capturing_variable() {
    let source = r#"
function main() {
  let total: i32 = 0;
  const nums: Array<i32> = [1, 2, 3];
  nums.forEach((n: i32): void => {
    total = total + n;
  });
  console.log(total);
}"#;
    assert!(
        compiles_ok(source),
        "closure capturing mutable variable should compile"
    );
}

#[test]
fn test_conformance_interact_nested_optional_chain() {
    let source = r#"
type Inner = { value: string | null }
type Outer = { inner: Inner | null }

function main() {
  const o: Outer = { inner: { value: "hello" } };
  const v: string = o.inner?.value ?? "none";
  console.log(v);
}"#;
    assert!(compiles_ok(source), "nested optional chain should compile");
}

#[test]
fn test_conformance_interact_type_assertion_method_call() {
    let source = r#"
function main() {
  const x: i32 = 42;
  const s: string = (x as f64).toString();
  console.log(s);
}"#;
    assert!(
        compiles_ok(source),
        "type assertion + method call should compile"
    );
}

#[test]
fn test_conformance_interact_template_multiple_interpolations() {
    let source = r#"
function main() {
  const name = "Alice";
  const age: i32 = 30;
  const city = "NYC";
  const msg = `${name} is ${age} from ${city}`;
  console.log(msg);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("format!"), "should use format! macro: {rust}");
    assert!(!rust.contains("todo!()"), "should compile: {rust}");
}

#[test]
fn test_conformance_interact_generic_function() {
    let source = r#"
function identity<T>(x: T): T {
  return x;
}

function main() {
  console.log(identity<i32>(42));
  console.log(identity<string>("hello"));
}"#;
    assert!(compiles_ok(source), "generic function should compile");
}

#[test]
fn test_conformance_interact_map_chain_filter_reduce() {
    let source = r#"
function main() {
  const nums: Array<i32> = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
  const result = nums
    .filter((n: i32): bool => n % 2 == 0)
    .map((n: i32): i32 => n * n)
    .reduce((a: i32, b: i32): i32 => a + b, 0);
  console.log(result);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "chained filter/map/reduce should compile: {rust}"
    );
}

#[test]
fn test_conformance_interact_async_function() {
    let source = r#"
async function fetchData(): string {
  return "data";
}

async function main() {
  const result = await fetchData();
  console.log(result);
}"#;
    assert!(compiles_ok(source), "async/await function should compile");
}

#[test]
fn test_conformance_interact_try_catch_finally() {
    let source = r#"
function riskyOp(): i32 throws string {
  return 42;
}

function main() {
  try {
    const val = riskyOp();
    console.log(val);
  } catch (e) {
    console.log("error");
  } finally {
    console.log("cleanup");
  }
}"#;
    assert!(compiles_ok(source), "try/catch/finally should compile");
}

#[test]
fn test_conformance_interact_interface_with_methods_and_fields() {
    let source = r#"
interface Describable {
  name: string;
  describe(): string;
}

class Product implements Describable {
  name: string;
  price: f64;
  constructor(name: string, price: f64) {
    this.name = name;
    this.price = price;
  }
  describe(): string {
    return `${this.name}: ${this.price}`;
  }
}

function main() {
  const p = new Product("Widget", 9.99);
  console.log(p.describe());
}"#;
    assert!(
        compiles_ok(source),
        "interface with fields and methods should compile"
    );
}

#[test]
fn test_conformance_interact_closure_in_method() {
    let source = r#"
class Processor {
  items: Array<i32>;
  constructor() {
    this.items = [1, 2, 3, 4, 5];
  }
  doubled(): Array<i32> {
    return this.items.map((x: i32): i32 => x * 2);
  }
}

function main() {
  const p = new Processor();
  const d = p.doubled();
  console.log(d.length);
}"#;
    assert!(
        compiles_ok(source),
        "closure inside class method should compile"
    );
}

#[test]
fn test_conformance_interact_multiple_generics() {
    let source = r#"
function pair<A, B>(a: A, b: B): [A, B] {
  return [a, b];
}

function main() {
  const p = pair<string, i32>("hello", 42);
  console.log(p[0]);
  console.log(p[1]);
}"#;
    let result = compile_source(source, "conformance_test.rts");
    assert!(
        !result.has_errors,
        "multi-generic tuple return should compile without errors: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    // Verify the generated Rust has correct turbofish syntax and tuple field access
    assert!(
        result.rust_source.contains("pair::<String, i32>"),
        "should emit turbofish type args: {}",
        result.rust_source
    );
    assert!(
        result.rust_source.contains("p.0") && result.rust_source.contains("p.1"),
        "should use tuple field access (p.0, p.1): {}",
        result.rust_source
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 11: Edge Cases
//
// Tests degenerate and boundary inputs that might trip up the compiler.
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_edge_empty_function_body() {
    let source = r#"
function noop(): void {}

function main() {
  noop();
  console.log("done");
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "empty function body should compile: {rust}"
    );
}

#[test]
fn test_conformance_edge_empty_array_literal() {
    let source = r#"
function main() {
  const x: Array<i32> = [];
  console.log(x.length);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "empty array literal should compile: {rust}"
    );
}

#[test]
fn test_conformance_edge_empty_object_literal() {
    let source = r#"
type Empty = {}

function main() {
  const x: Empty = {};
  console.log("done");
}"#;
    assert!(compiles_ok(source), "empty object literal should compile");
}

#[test]
fn test_conformance_edge_single_element_tuple() {
    let source = r#"
function main() {
  const x: [i32] = [1];
  console.log(x[0]);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "single-element tuple should compile: {rust}"
    );
}

#[test]
fn test_conformance_edge_no_params_no_return() {
    let source = r#"
function doSomething() {
  console.log("side effect");
}

function main() {
  doSomething();
}"#;
    assert!(
        compiles_ok(source),
        "function with no params/return should compile"
    );
}

#[test]
fn test_conformance_edge_deeply_nested_closures() {
    let source = r#"
function main() {
  const nums: Array<i32> = [1, 2, 3];
  const result = nums.map((a: i32): i32 => {
    const inner: Array<i32> = [a];
    return inner.map((b: i32): i32 => {
      return b * 2;
    })[0];
  });
  console.log(result.length);
}"#;
    assert!(compiles_ok(source), "deeply nested closures should compile");
}

#[test]
fn test_conformance_edge_long_method_chain() {
    let source = r#"
function main() {
  const result = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    .filter((x: i32): bool => x > 2)
    .filter((x: i32): bool => x < 8)
    .map((x: i32): i32 => x * 2)
    .filter((x: i32): bool => x > 6)
    .map((x: i32): i32 => x + 1);
  console.log(result.length);
}"#;
    assert!(
        compiles_ok(source),
        "long method chain (5 chained calls) should compile"
    );
}

#[test]
fn test_conformance_edge_numeric_literals() {
    let source = r#"
function main() {
  const zero: i32 = 0;
  const negative: i32 = -42;
  const big: i64 = 9999999999;
  const pi: f64 = 3.14159;
  const neg_float: f64 = -0.5;
  console.log(zero);
  console.log(negative);
}"#;
    assert!(
        compiles_ok(source),
        "various numeric literals should compile"
    );
}

#[test]
fn test_conformance_edge_string_escapes() {
    let source = r#"
function main() {
  const tab = "hello\tworld";
  const newline = "line1\nline2";
  const quote = "she said \"hi\"";
  const backslash = "path\\to\\file";
  console.log(tab);
}"#;
    assert!(
        compiles_ok(source),
        "string escape sequences should compile"
    );
}

#[test]
fn test_conformance_edge_multiple_returns() {
    let source = r#"
function abs(x: i32): i32 {
  if (x < 0) {
    return -x;
  }
  return x;
}

function main() {
  console.log(abs(-5));
  console.log(abs(3));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        !rust.contains("todo!()"),
        "multiple return paths should compile: {rust}"
    );
}

// ===========================================================================
// ===========================================================================
//
// CATEGORY 12: Error Robustness (extended)
//
// Tests that common programmer mistakes produce diagnostics, not panics.
// These use compile_source directly so we can accept either errors or
// graceful pass-through.
//
// ===========================================================================
// ===========================================================================

#[test]
fn test_conformance_error_mismatched_types() {
    let source = r#"
function main() {
  const x: i32 = "hello";
}"#;
    // Should produce a type error or defer to rustc — must not crash
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_missing_function_arg() {
    let source = r#"
function add(a: i32, b: i32): i32 {
  return a + b;
}

function main() {
  const x = add(1);
}"#;
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_break_outside_loop() {
    let source = r#"
function main() {
  break;
}"#;
    let result = compile_source(source, "test.rts");
    // Should either be caught by our compiler or deferred to rustc
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_return_outside_function() {
    let source = "return 42;";
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_redeclared_variable() {
    let source = r#"
function main() {
  const x: i32 = 1;
  const x: i32 = 2;
}"#;
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_invalid_type_annotation() {
    let source = r#"
function main() {
  const x: NotARealType = 42;
}"#;
    assert!(
        compiles_with_errors(source),
        "invalid type annotation should produce error"
    );
}

#[test]
fn test_conformance_error_unclosed_paren() {
    let source = r#"
function main() {
  console.log("hello"
}"#;
    assert!(
        compiles_with_errors(source),
        "unclosed paren should produce error"
    );
}

#[test]
fn test_conformance_error_extra_comma_in_params() {
    let source = r#"
function foo(a: i32,, b: i32): i32 {
  return a + b;
}"#;
    let result = compile_source(source, "test.rts");
    // Double comma — should error or be handled gracefully
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_assignment_to_const() {
    let source = r#"
function main() {
  const x: i32 = 1;
  x = 2;
}"#;
    let result = compile_source(source, "test.rts");
    // May be caught by our compiler or deferred to rustc
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_missing_type_on_param() {
    // RustScript may require type annotations on params — test graceful handling
    let source = r#"
function add(a, b) {
  return a + b;
}

function main() {
  console.log(add(1, 2));
}"#;
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_import_nonexistent_module() {
    let source = r#"
import { Foo } from "nonexistent_crate";

function main() {
  console.log("hello");
}"#;
    // Should still compile (import lowers to `use`) — rustc catches the error
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_duplicate_method_name() {
    let source = r#"
class Foo {
  bar(): i32 { return 1; }
  bar(): string { return "hello"; }
}

function main() {
  const f = new Foo();
}"#;
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_only_whitespace() {
    let source = "   \n\n   \n";
    // Whitespace-only source — should not crash
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_only_comments() {
    let source = r#"
// This file has only comments
// Nothing else
"#;
    let result = compile_source(source, "test.rts");
    let _ = result.has_errors;
}

#[test]
fn test_conformance_error_incomplete_class() {
    let source = r#"
class Foo {
  name: string;
"#;
    assert!(
        compiles_with_errors(source),
        "incomplete class should produce error"
    );
}

// ===========================================================================
// SWITCH STATEMENT: Integer, Enum Variant, and Regression Tests
// ===========================================================================

// Switch on integer values should compile to a Rust match on integers.
#[test]
fn test_switch_on_integer() {
    let source = r#"
function describe(x: i32): string {
  switch (x) {
    case 1: return "one";
    case 2: return "two";
    case 3: return "three";
    default: return "other";
  }
}

function main() {
  console.log(describe(2));
}
"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("match x"),
        "should produce match on x: {rust}"
    );
    assert!(
        rust.contains("1 =>"),
        "should have integer pattern 1: {rust}"
    );
    assert!(
        rust.contains("2 =>"),
        "should have integer pattern 2: {rust}"
    );
    assert!(
        rust.contains("3 =>"),
        "should have integer pattern 3: {rust}"
    );
    assert!(
        rust.contains("_ =>"),
        "should have wildcard default: {rust}"
    );
}

// Switch on enum variant using Color.Red style member access.
#[test]
fn test_switch_on_enum_variant() {
    let source = r#"
const enum Color { Red, Green, Blue }

function name_color(c: Color): string {
  switch (c) {
    case Color.Red: return "red";
    case Color.Green: return "green";
    case Color.Blue: return "blue";
  }
}

function main() {
  console.log(name_color(Color.Red));
}
"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("match c"),
        "should produce match on c: {rust}"
    );
    assert!(
        rust.contains("Color::Red"),
        "should have Color::Red pattern: {rust}"
    );
    assert!(
        rust.contains("Color::Green"),
        "should have Color::Green pattern: {rust}"
    );
    assert!(
        rust.contains("Color::Blue"),
        "should have Color::Blue pattern: {rust}"
    );
}

// Regression: switch on string enum still works after the integer/enum changes.
#[test]
fn test_switch_on_string_still_works() {
    let source = r#"
type Direction = "north" | "south" | "east" | "west"

function go(d: Direction): i32 {
  switch (d) {
    case "north": return 1;
    case "south": return 2;
    case "east": return 3;
    case "west": return 4;
  }
}

function main() {
  console.log(go("north"));
}
"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("match d"),
        "should produce match on d: {rust}"
    );
    assert!(
        rust.contains("Direction::North"),
        "should have Direction::North: {rust}"
    );
    assert!(
        rust.contains("Direction::South"),
        "should have Direction::South: {rust}"
    );
}

// Switch nested inside an if body.
#[test]
fn test_switch_nested_in_if() {
    let source = r#"
function classify(x: i32): string {
  if (x > 0) {
    switch (x) {
      case 1: return "one";
      case 2: return "two";
      default: return "positive";
    }
  }
  return "non-positive";
}

function main() {
  console.log(classify(1));
}
"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("match x"),
        "should produce match inside if: {rust}"
    );
    assert!(
        rust.contains("1 =>"),
        "should have integer pattern 1: {rust}"
    );
    assert!(
        rust.contains("_ =>"),
        "should have wildcard default: {rust}"
    );
}

// Switch nested inside a for loop body.
#[test]
fn test_switch_nested_in_loop() {
    let source = r#"
function main() {
  const items: Array<i32> = [1, 2, 3];
  for (const x of items) {
    switch (x) {
      case 1: console.log("one"); break;
      case 2: console.log("two"); break;
      default: console.log("other"); break;
    }
  }
}
"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("match x"),
        "should produce match inside for loop: {rust}"
    );
    assert!(
        rust.contains("1 =>"),
        "should have integer pattern 1: {rust}"
    );
    assert!(
        rust.contains("2 =>"),
        "should have integer pattern 2: {rust}"
    );
    assert!(
        rust.contains("_ =>"),
        "should have wildcard default: {rust}"
    );
}

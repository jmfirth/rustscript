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

use rsc_driver::compile_source;
use test_utils::compile_diagnostics;

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
    let mut expr = String::from("1");
    for _ in 0..50 {
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

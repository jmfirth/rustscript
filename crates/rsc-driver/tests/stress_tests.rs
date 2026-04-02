//! Compilation stress tests — 60 real-world TypeScript patterns exercising
//! multiple features simultaneously through the full RustScript pipeline.
//!
//! Each test compiles a complete, self-contained program via `compile_to_rust`
//! and asserts the output contains expected patterns. These are integration
//! tests that catch bugs in how features interact, not individual feature tests.
//!
//! Groups:
//!   1. Data Processing (10 tests)
//!   2. Class Patterns (10 tests)
//!   3. Type System (10 tests)
//!   4. Control Flow (10 tests)
//!   5. Module & Import Patterns (5 tests)
//!   6. Advanced Patterns (10 tests)
//!   7. Real-world Composite (5 tests)

mod test_utils;

use test_utils::{compile_and_run, compile_to_rust};

// ===========================================================================
// ===========================================================================
//
// GROUP 1: Data Processing (10 tests)
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. Array chain: filter + map + reduce
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_01_array_chain() {
    let source = r#"function main() {
  const nums: Array<i32> = [1, -2, 3, -4, 5];
  const result: i32 = nums.filter((x: i32): bool => x > 0).map((x: i32): i32 => x * 2).reduce((a: i32, b: i32): i32 => a + b, 0);
  console.log(result);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(rust.contains("filter"), "should have filter: {rust}");
    assert!(rust.contains("map"), "should have map: {rust}");
    assert!(
        rust.contains("fold") || rust.contains("reduce"),
        "should have fold/reduce: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 2. Object destructuring with rename
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_02_object_destructuring() {
    let source = r#"type User = {
  name: string,
  age: i32,
  email: string,
}

function main() {
  const user: User = { name: "Alice", age: 30, email: "alice@example.com" };
  const { name, age } = user;
  console.log(name);
  console.log(age);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("struct User"), "should have User struct: {rust}");
    assert!(rust.contains("name"), "should have name binding: {rust}");
    assert!(rust.contains("age"), "should have age binding: {rust}");
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 3. Map/filter with string methods
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_03_filter_map_string_methods() {
    let source = r#"function main() {
  const words: Array<string> = ["hello", "world", "foo"];
  const upper: Array<string> = words.map((w: string): string => w.toUpperCase());
  console.log(upper);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains("to_uppercase"),
        "should have to_uppercase: {rust}"
    );
    assert!(
        rust.contains(".map("),
        "should have .map() call: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 4. Nested data transformation: array of structs, map fields, filter
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_04_nested_data_transformation() {
    let source = r#"type Item = {
  name: string,
  price: f64,
  inStock: bool,
}

function get_expensive_names(items: Array<Item>): Array<string> {
  return items.filter((i: Item): bool => i.price > 10.0).map((i: Item): string => i.name);
}

function main() {
  const items: Array<Item> = [
    { name: "apple", price: 1.5, inStock: true },
    { name: "laptop", price: 999.99, inStock: true },
    { name: "book", price: 15.0, inStock: false }
  ];
  const names: Array<string> = get_expensive_names(items);
  console.log(names);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("struct Item"), "should have Item struct: {rust}");
    assert!(rust.contains("filter"), "should have filter: {rust}");
    assert!(rust.contains("map"), "should have map: {rust}");
    assert!(
        rust.contains("fn get_expensive_names"),
        "should have get_expensive_names fn: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 5. Reduce to accumulate a sum
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_05_reduce_accumulation() {
    let source = r#"function main() {
  const prices: Array<f64> = [9.99, 24.50, 3.75, 100.00];
  const total: f64 = prices.reduce((acc: f64, p: f64): f64 => acc + p, 0.0);
  console.log(total);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains("fold") || rust.contains("reduce"),
        "should have fold/reduce: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 6. Tuple creation and field access
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_06_tuple_access() {
    let source = r#"function make_pair(a: string, b: i32): [string, i32] {
  return [a, b];
}

function main() {
  const pair: [string, i32] = make_pair("hello", 42);
  const key: string = pair[0];
  const val: i32 = pair[1];
  console.log(key);
  console.log(val);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains("(String, i32)"),
        "should have tuple type: {rust}"
    );
    assert!(rust.contains(".0") && rust.contains(".1"), "should access tuple fields: {rust}");
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 7. Generic identity function
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_07_generic_identity() {
    let source = r#"function identity<T>(x: T): T {
  return x;
}

function main() {
  const a: i32 = identity(42);
  const b: string = identity("hello");
  console.log(a);
  console.log(b);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn identity"),
        "should have identity fn: {rust}"
    );
    assert!(
        rust.contains("<T>") || rust.contains("T>") || rust.contains("impl"),
        "should have generic type param: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 8. Async data fetch pattern
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_08_async_function() {
    let source = r#"async function fetch_data(): string {
  return "fetched data";
}

async function main() {
  const result: string = await fetch_data();
  console.log(result);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("async fn"), "should have async fn: {rust}");
    assert!(rust.contains(".await"), "should have .await: {rust}");
    assert!(
        rust.contains("tokio::main"),
        "should have tokio::main for async main: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 9. Error handling: try/catch with throw
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_09_try_catch_error_handling() {
    let source = r#"function parse_number(s: string): i32 throws string {
  throw "parse error";
}

function main() {
  try {
    const n: i32 = parse_number("abc");
    console.log(n);
  } catch (e: string) {
    console.log(e);
  }
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains("Result<") || rust.contains("Err("),
        "should have Result type or Err: {rust}"
    );
    assert!(
        rust.contains("Ok(") || rust.contains("Err("),
        "should have Ok/Err: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 10. Template literal interpolation with multiple expressions
// ---------------------------------------------------------------------------

#[test]
fn test_stress_data_10_template_literal() {
    let source = r#"function main() {
  const name: string = "World";
  const count: i32 = 42;
  const greeting: string = `Hello ${name}, you have ${count} items`;
  console.log(greeting);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains("format!("),
        "template literal should produce format!: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ===========================================================================
// ===========================================================================
//
// GROUP 2: Class Patterns (10 tests)
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 11. Class with all features: constructor, methods, static, private, readonly
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_11_full_featured() {
    let source = r#"class Counter {
  readonly name: string;
  private count: i64 = 0;
  static MAX: i64 = 1000;

  constructor(public label: string) {
    this.name = label;
  }

  increment(): void {
    this.count = this.count + 1;
  }

  get_count(): i64 {
    return this.count;
  }

  static create(label: string): Counter {
    return new Counter(label);
  }
}

function main() {
  let c: Counter = Counter.create("test");
  c.increment();
  console.log(c.get_count());
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Counter"),
        "should have Counter struct: {rust}"
    );
    assert!(
        rust.contains("fn increment"),
        "should have increment method: {rust}"
    );
    assert!(
        rust.contains("const MAX"),
        "should have static MAX: {rust}"
    );
    assert!(
        rust.contains("fn create("),
        "should have static create: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 12. Class inheritance: abstract class + implements
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_12_inheritance() {
    let source = r#"abstract class Animal {
  abstract sound(): string;

  describe(): string {
    return "an animal";
  }
}

class Dog implements Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  sound(): string {
    return "woof";
  }
}

function main() {
  const d: Dog = new Dog("Rex");
  console.log(d.sound());
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("trait Animal"),
        "abstract class should produce trait: {rust}"
    );
    assert!(
        rust.contains("struct Dog"),
        "should have Dog struct: {rust}"
    );
    assert!(
        rust.contains("impl Animal for Dog"),
        "should implement trait: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 13. Abstract class with default method
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_13_abstract_with_default() {
    let source = r#"abstract class Shape {
  abstract area(): f64;

  describe(): string {
    return "a shape";
  }
}

class Square implements Shape {
  side: f64;

  constructor(s: f64) {
    this.side = s;
  }

  area(): f64 {
    return this.side * this.side;
  }
}

function main() {
  const sq: Square = new Square(5.0);
  console.log(sq.area());
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("trait Shape"),
        "abstract class should produce trait: {rust}"
    );
    assert!(
        rust.contains("fn describe("),
        "should have default method: {rust}"
    );
    assert!(
        rust.contains("impl Shape for Square"),
        "should implement trait: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 14. Class with methods and field types
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_14_methods_and_fields() {
    let source = r#"class Container {
  value: i32;

  constructor(v: i32) {
    this.value = v;
  }

  get(): i32 {
    return this.value;
  }

  set(v: i32): void {
    this.value = v;
  }
}

function main() {
  let c: Container = new Container(10);
  c.set(20);
  console.log(c.get());
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Container"),
        "should have Container struct: {rust}"
    );
    assert!(rust.contains("value: i32"), "should have value field: {rust}");
    assert!(
        rust.contains("fn get(") || rust.contains("fn get "),
        "should have get method: {rust}"
    );
    assert!(
        rust.contains("fn set("),
        "should have set method: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 15. Class implementing interface
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_15_implements_interface() {
    let source = r#"interface Printable {
  to_string(): string;
}

class Person implements Printable {
  name: string;
  age: i32;

  constructor(name: string, age: i32) {
    this.name = name;
    this.age = age;
  }

  to_string(): string {
    return this.name;
  }
}

function main() {
  const p: Person = new Person("Alice", 30);
  console.log(p.to_string());
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("trait Printable"),
        "interface should produce trait: {rust}"
    );
    assert!(
        rust.contains("struct Person"),
        "should have Person struct: {rust}"
    );
    assert!(
        rust.contains("impl Printable for Person"),
        "should implement trait: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 16. Class with getter/setter
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_16_getter_setter() {
    let source = r#"class Temperature {
  private celsius: f64;

  constructor(c: f64) {
    this.celsius = c;
  }

  get value(): f64 {
    return this.celsius;
  }

  set value(c: f64) {
    this.celsius = c;
  }
}

function main() {
  let t: Temperature = new Temperature(100.0);
  console.log(t.value);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Temperature"),
        "should have Temperature struct: {rust}"
    );
    assert!(
        rust.contains("fn value(&self)"),
        "should have getter: {rust}"
    );
    assert!(
        rust.contains("fn set_value("),
        "should have setter: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 17. Static factory method pattern
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_17_static_factory() {
    let source = r#"class Config {
  debug: bool;
  name: string;

  constructor(name: string, debug: bool) {
    this.name = name;
    this.debug = debug;
  }

  static default(): Config {
    return new Config("app", false);
  }

  static development(): Config {
    return new Config("dev", true);
  }
}

function main() {
  const cfg: Config = Config.default();
  console.log(cfg.name);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Config"),
        "should have Config struct: {rust}"
    );
    assert!(
        rust.contains("fn default("),
        "should have static default: {rust}"
    );
    assert!(
        rust.contains("fn development("),
        "should have static development: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 18. Class with async methods
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_18_async_methods() {
    let source = r#"class DataService {
  url: string;

  constructor(url: string) {
    this.url = url;
  }

  async fetch(): string {
    return "data from server";
  }
}

async function main() {
  const svc: DataService = new DataService("http://example.com");
  const data: string = await svc.fetch();
  console.log(data);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct DataService"),
        "should have DataService struct: {rust}"
    );
    assert!(rust.contains("async fn"), "should have async fn: {rust}");
    assert!(rust.contains(".await"), "should have .await: {rust}");
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 19. Class with optional parameters and defaults
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_19_optional_defaults() {
    let source = r#"class Greeter {
  prefix: string;

  constructor(prefix: string) {
    this.prefix = prefix;
  }

  greet(name: string, excited: bool = false): string {
    const msg: string = `${this.prefix} ${name}`;
    return msg;
  }
}

function main() {
  const g: Greeter = new Greeter("Hello");
  console.log(g.greet("Alice"));
  console.log(g.greet("Bob", true));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Greeter"),
        "should have Greeter struct: {rust}"
    );
    assert!(
        rust.contains("fn greet("),
        "should have greet method: {rust}"
    );
    assert!(
        rust.contains("format!("),
        "should have format! from template literal: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 20. Simple class with method chaining
// ---------------------------------------------------------------------------

#[test]
fn test_stress_class_20_simple_class_usage() {
    let source = r#"class Stack {
  items: Array<i32>;

  constructor() {
    this.items = [];
  }

  push(val: i32): void {
    this.items.push(val);
  }

  size(): i32 {
    return this.items.length;
  }
}

function main() {
  let s: Stack = new Stack();
  s.push(1);
  s.push(2);
  s.push(3);
  console.log(s.size());
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Stack"),
        "should have Stack struct: {rust}"
    );
    assert!(
        rust.contains("fn push("),
        "should have push method: {rust}"
    );
    assert!(
        rust.contains("fn size("),
        "should have size method: {rust}"
    );
    assert!(
        rust.contains(".len()"),
        ".length should lower to .len(): {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ===========================================================================
// ===========================================================================
//
// GROUP 3: Type System (10 tests)
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 21. Union type with value
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_21_union_type() {
    let source = r#"function process(value: string | i32): string {
  return "processed";
}

function main() {
  const a: string | i32 = "hello";
  const b: string | i32 = 42;
  console.log(process(a));
  console.log(process(b));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("enum") && rust.contains("Or"),
        "union should generate enum: {rust}"
    );
    assert!(
        rust.contains("impl From<"),
        "should generate From impls: {rust}"
    );
    assert!(
        rust.contains(".into()"),
        "values should use .into(): {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 22. Type alias and struct
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_22_type_alias_struct() {
    let source = r#"type Point = {
  x: f64,
  y: f64,
}

function distance(a: Point, b: Point): f64 {
  const dx: f64 = b.x - a.x;
  const dy: f64 = b.y - a.y;
  return dx * dx + dy * dy;
}

function main() {
  const p1: Point = { x: 0.0, y: 0.0 };
  const p2: Point = { x: 3.0, y: 4.0 };
  console.log(distance(p1, p2));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Point"),
        "should have Point struct: {rust}"
    );
    assert!(
        rust.contains("fn distance("),
        "should have distance fn: {rust}"
    );
    assert!(
        rust.contains("x: f64") && rust.contains("y: f64"),
        "should have f64 fields: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 23. Utility type Partial<T>
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_23_partial_type() {
    let source = r#"type User = {
  name: string,
  age: u32,
}

type PartialUser = Partial<User>"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct User"),
        "should have User struct: {rust}"
    );
    assert!(
        rust.contains("struct PartialUser"),
        "should have PartialUser struct: {rust}"
    );
    assert!(
        rust.contains("Option<"),
        "Partial fields should be Option: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 24. Conditional type
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_24_conditional_type() {
    let source = r#"type IsString = string extends string ? bool : i32
type NotString = i32 extends string ? bool : f64"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("type IsString = bool;"),
        "string extends string should resolve to bool: {rust}"
    );
    assert!(
        rust.contains("type NotString = f64;"),
        "i32 extends string should resolve to f64: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 25. Intersection types via type alias
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_25_intersection_types() {
    // Intersection types merge fields from all constituent struct types.
    let source = r#"type Named = { name: string }
type Aged = { age: i32 }
type Person = Named & Aged

function main() {
  const n: Named = { name: "Alice" };
  console.log(n.name);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Named"),
        "should have Named struct: {rust}"
    );
    assert!(
        rust.contains("struct Aged"),
        "should have Aged struct: {rust}"
    );
    assert!(
        rust.contains("struct Person"),
        "should have Person struct (merged intersection): {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

#[test]
fn test_intersection_merges_fields() {
    // Two inline object types intersected produce a struct with all fields.
    let source = r#"type A = { x: i32 }
type B = { y: string }
type C = A & B

function main() {
  const c: C = { x: 42, y: "hello" };
  console.log(c.x);
  console.log(c.y);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct C"),
        "intersection should produce struct C: {rust}"
    );
    // Verify the struct contains both fields
    assert!(
        rust.contains("x:") || rust.contains("x :"),
        "struct C should have field x: {rust}"
    );
    assert!(
        rust.contains("y:") || rust.contains("y :"),
        "struct C should have field y: {rust}"
    );
}

#[test]
fn test_intersection_three_types() {
    // Three types intersected should merge all fields.
    let source = r#"type HasName = { name: string }
type HasAge = { age: i32 }
type HasEmail = { email: string }
type Person = HasName & HasAge & HasEmail

function main() {
  const p: Person = { name: "Alice", age: 30, email: "alice@example.com" };
  console.log(p.name);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Person"),
        "three-way intersection should produce struct Person: {rust}"
    );
    // Verify the struct contains all three fields
    assert!(
        rust.contains("name"),
        "Person should have name field: {rust}"
    );
    assert!(
        rust.contains("age"),
        "Person should have age field: {rust}"
    );
    assert!(
        rust.contains("email"),
        "Person should have email field: {rust}"
    );
}

#[test]
fn test_intersection_with_named_types() {
    // Intersection of named types: type C = A & B
    let source = r#"type A = { x: i32 }
type B = { y: string }
type C = A & B

function main() {
  const c: C = { x: 1, y: "test" };
  console.log(c.x);
  console.log(c.y);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct C"),
        "intersection of named types should produce struct C: {rust}"
    );
    assert!(
        rust.contains("struct A"),
        "should still have struct A: {rust}"
    );
    assert!(
        rust.contains("struct B"),
        "should still have struct B: {rust}"
    );
}

// ---------------------------------------------------------------------------
// 26. String literal union as enum + switch
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_26_string_enum_switch() {
    let source = r#"type Direction = "north" | "south" | "east" | "west"

function describe(dir: Direction): string {
  switch (dir) {
    case "north": return "going north";
    case "south": return "going south";
    case "east": return "going east";
    case "west": return "going west";
  }
}

function main() {
  const d: Direction = "north";
  console.log(describe(d));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("enum Direction"),
        "string union should produce enum: {rust}"
    );
    assert!(
        rust.contains("match"),
        "switch should produce match: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 27. Type alias chain
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_27_type_alias_chain() {
    let source = r#"type ID = string

function get_user(id: ID): string {
  return `user_${id}`;
}

function main() {
  const uid: ID = "abc123";
  console.log(get_user(uid));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn get_user("),
        "should have get_user fn: {rust}"
    );
    assert!(
        rust.contains("format!("),
        "template literal should produce format!: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 28. Readonly array parameter (Array used as immutable)
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_28_array_param() {
    let source = r#"function sum(nums: Array<i32>): i64 {
  let total: i64 = 0;
  for (const n of nums) {
    total = total + n;
  }
  return total;
}

function main() {
  const data: Array<i32> = [1, 2, 3, 4, 5];
  console.log(sum(data));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn sum("),
        "should have sum fn: {rust}"
    );
    assert!(
        rust.contains("for") && rust.contains("in"),
        "for-of should produce for..in: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 29. Optional chaining on nullable type
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_29_optional_chaining() {
    let source = r#"function find_name(id: i32): string | null {
  if (id === 1) {
    return "Alice";
  }
  return null;
}

function main() {
  const name: string | null = find_name(1);
  const missing: string | null = find_name(0);
  console.log(name);
  console.log(missing);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("Option<String>"),
        "T | null should produce Option: {rust}"
    );
    assert!(
        rust.contains("None"),
        "null should produce None: {rust}"
    );
    assert!(
        rust.contains("Some("),
        "non-null return should produce Some: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 30. Nullish coalescing with fallback
// ---------------------------------------------------------------------------

#[test]
fn test_stress_types_30_nullish_coalescing() {
    let source = r#"function find_name(id: i32): string | null {
  if (id === 1) {
    return "Alice";
  }
  return null;
}

function main() {
  const name: string | null = find_name(1);
  const display: string = name ?? "anonymous";
  console.log(display);

  const missing: string | null = find_name(0);
  const fallback: string = missing ?? "nobody";
  console.log(fallback);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("Option<String>"),
        "should have Option type: {rust}"
    );
    assert!(
        rust.contains("unwrap_or"),
        "?? should produce unwrap_or: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ===========================================================================
// ===========================================================================
//
// GROUP 4: Control Flow (10 tests)
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 31. Nested loops with labeled break
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_31_labeled_break() {
    let source = r#"function main() {
  const items: Array<i32> = [1, 2, 3];
  const targets: Array<i32> = [10, 2, 30];
  let found: bool = false;
  outer: for (const x of items) {
    for (const y of targets) {
      if (x == y) {
        found = true;
        break outer;
      }
    }
  }
  console.log(found);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("'outer: for"),
        "should have labeled loop: {rust}"
    );
    assert!(
        rust.contains("break 'outer"),
        "should have labeled break: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 32. Classic for loop with index access
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_32_classic_for_loop() {
    let source = r#"function main() {
  for (let i = 0; i < 5; i++) {
    console.log(i);
  }
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains("for i in 0..5") || (rust.contains("let mut i") && rust.contains("while")),
        "classic for should produce range or while: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 33. For-of with array iteration
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_33_for_of_iteration() {
    let source = r#"function main() {
  const items: Array<string> = ["alpha", "beta", "gamma"];
  for (const item of items) {
    console.log(item);
  }
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains("for"),
        "should have for loop: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 34. Switch with multiple cases + default
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_34_switch_complex() {
    let source = r#"type Color = "red" | "green" | "blue"

function color_code(c: Color): i32 {
  switch (c) {
    case "red": return 1;
    case "green": return 2;
    case "blue": return 3;
  }
}

function main() {
  console.log(color_code("red"));
  console.log(color_code("blue"));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("enum Color"),
        "string union should produce enum: {rust}"
    );
    assert!(
        rust.contains("match"),
        "switch should produce match: {rust}"
    );
    assert!(
        rust.contains("fn color_code"),
        "should have color_code fn: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 35. Try/catch/finally with return values
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_35_try_catch_finally() {
    let source = r#"function risky(): i32 throws string {
  throw "oops";
}

function main() {
  try {
    const val: i32 = risky();
    console.log(val);
  } catch (e: string) {
    console.log(e);
  } finally {
    console.log("done");
  }
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("Ok(") && rust.contains("Err("),
        "try/catch should produce Ok/Err: {rust}"
    );
    assert!(
        rust.contains("done"),
        "finally should produce cleanup: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 36. Do-while with condition
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_36_do_while() {
    let source = r#"function main() {
  let i: i32 = 0;
  do {
    i = i + 1;
  } while (i < 5);
  console.log(i);
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains("loop"),
        "do-while should produce loop: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 37. For-in over Map keys
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_37_for_in_map() {
    let source = r#"function main() {
  const map: Map<string, i32> = new Map();
  for (const k in map) {
    console.log(k);
  }
}"#;
    let rust = compile_to_rust(source);
    assert!(rust.contains("fn main()"), "should have main: {rust}");
    assert!(
        rust.contains(".keys()"),
        "for-in should use .keys(): {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 38. While loop with complex condition and early return
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_38_while_early_return() {
    let source = r#"function find_first_even(items: Array<i32>): i32 {
  let i: i32 = 0;
  while (i < items.length) {
    if (items[i] % 2 == 0) {
      return items[i];
    }
    i = i + 1;
  }
  return -1;
}

function main() {
  const nums: Array<i32> = [1, 3, 4, 7, 8];
  console.log(find_first_even(nums));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn find_first_even"),
        "should have find_first_even fn: {rust}"
    );
    assert!(
        rust.contains("while"),
        "should have while loop: {rust}"
    );
    assert!(
        rust.contains("return"),
        "should have early return: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 39. Nested if/else chains
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_39_nested_if_else() {
    let source = r#"function classify(n: i64): string {
  if (n > 100) {
    return "large";
  } else if (n > 10) {
    return "medium";
  } else if (n > 0) {
    return "small";
  } else if (n == 0) {
    return "zero";
  } else {
    return "negative";
  }
}

function main() {
  console.log(classify(200));
  console.log(classify(50));
  console.log(classify(5));
  console.log(classify(0));
  console.log(classify(-10));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn classify"),
        "should have classify fn: {rust}"
    );
    assert!(
        rust.contains("if") && rust.contains("else"),
        "should have if/else chain: {rust}"
    );
    // Multiple return statements
    let return_count = rust.matches("return").count();
    assert!(
        return_count >= 5,
        "should have 5+ return statements, found {return_count}: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 40. While loop with complex condition
// ---------------------------------------------------------------------------

#[test]
fn test_stress_flow_40_while_complex_condition() {
    let source = r#"function gcd(a: i32, b: i32): i32 {
  let x: i32 = a;
  let y: i32 = b;
  while (y != 0) {
    const temp: i32 = y;
    y = x % y;
    x = temp;
  }
  return x;
}

function main() {
  console.log(gcd(48, 18));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn gcd("),
        "should have gcd fn: {rust}"
    );
    assert!(
        rust.contains("while"),
        "should have while loop: {rust}"
    );
    assert!(
        rust.contains("let mut"),
        "should have mutable bindings: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ===========================================================================
// ===========================================================================
//
// GROUP 5: Module & Import Patterns (5 tests)
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 41. Import from external crate
// ---------------------------------------------------------------------------

#[test]
fn test_stress_module_41_import_use() {
    let source = r#"import { HashMap } from "std/collections"

function main() {
  console.log("hello");
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("use std::collections::HashMap"),
        "import should produce use statement: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 42. Export function + export type
// ---------------------------------------------------------------------------

#[test]
fn test_stress_module_42_export_function_type() {
    let source = r#"export type Config = {
  name: string,
  debug: bool,
}

export function create_config(name: string): Config {
  return { name: name, debug: false };
}

function main() {
  const cfg: Config = create_config("test");
  console.log(cfg.name);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("pub struct Config") || rust.contains("pub fn create_config"),
        "exports should produce pub: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 43. Import type (type-only import)
// ---------------------------------------------------------------------------

#[test]
fn test_stress_module_43_import_type() {
    let source = r#"import { Serialize } from "serde"

type Data = {
  id: u32,
  name: string,
}

function main() {
  const d: Data = { id: 1, name: "test" };
  console.log(d.name);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("use serde::Serialize"),
        "import should produce use: {rust}"
    );
    assert!(
        rust.contains("struct Data"),
        "should have Data struct: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 44. Re-export: export * from module
// ---------------------------------------------------------------------------

#[test]
fn test_stress_module_44_reexport() {
    let source = r#"export * from "./utils";"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("pub use") && rust.contains("*"),
        "export * should produce pub use wildcard: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 45. Multiple named imports
// ---------------------------------------------------------------------------

#[test]
fn test_stress_module_45_multiple_imports() {
    let source = r#"import { Router, serve } from "axum"
import { Serialize, Deserialize } from "serde"

function main() {
  console.log("hello");
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("use axum::Router"),
        "should import Router: {rust}"
    );
    assert!(
        rust.contains("use axum::serve"),
        "should import serve: {rust}"
    );
    assert!(
        rust.contains("use serde::Serialize"),
        "should import Serialize: {rust}"
    );
    assert!(
        rust.contains("use serde::Deserialize"),
        "should import Deserialize: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ===========================================================================
// ===========================================================================
//
// GROUP 6: Advanced Patterns (10 tests)
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 46. Async function with await
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_46_async_await_pair() {
    let source = r#"async function fetch_a(): string {
  return "data-a";
}

async function fetch_b(): string {
  return "data-b";
}

async function main() {
  const a: string = await fetch_a();
  const b: string = await fetch_b();
  console.log(a);
  console.log(b);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("async fn fetch_a"),
        "should have async fetch_a: {rust}"
    );
    assert!(
        rust.contains("async fn fetch_b"),
        "should have async fetch_b: {rust}"
    );
    assert!(
        rust.matches(".await").count() >= 2,
        "should have 2+ .await calls: {rust}"
    );
    assert!(
        rust.contains("tokio::main"),
        "should have tokio::main: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 47. Closure capturing variables
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_47_closure_capture() {
    let source = r#"function make_adder(base: i32): (i32) => i32 {
  return (x: i32): i32 => base + x;
}

function main() {
  const add5 = make_adder(5);
  console.log(add5(10));
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn make_adder"),
        "should have make_adder fn: {rust}"
    );
    assert!(
        rust.contains("move") || rust.contains("Fn(") || rust.contains("impl") || rust.contains("Box"),
        "should have closure type or move: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 48. Higher-order function: takes and returns functions
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_48_higher_order() {
    let source = r#"function apply(f: (i32) => i32, x: i32): i32 {
  return f(x);
}

function double(x: i32): i32 {
  return x * 2;
}

function main() {
  const result: i32 = apply(double, 21);
  console.log(result);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn apply("),
        "should have apply fn: {rust}"
    );
    assert!(
        rust.contains("fn double("),
        "should have double fn: {rust}"
    );
    assert!(
        rust.contains("Fn(") || rust.contains("fn(") || rust.contains("impl"),
        "should have function type: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 49. Method call chaining on class
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_49_method_chaining() {
    let source = r#"class Logger {
  entries: Array<string>;

  constructor() {
    this.entries = [];
  }

  log(msg: string): void {
    this.entries.push(msg);
  }

  count(): i32 {
    return this.entries.length;
  }
}

function main() {
  let logger: Logger = new Logger();
  logger.log("first");
  logger.log("second");
  logger.log("third");
  console.log(logger.count());
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Logger"),
        "should have Logger struct: {rust}"
    );
    assert!(
        rust.contains("fn log("),
        "should have log method: {rust}"
    );
    assert!(
        rust.contains("fn count("),
        "should have count method: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 50. Callback function with typed parameters
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_50_typed_callback() {
    let source = r#"function on_click(handler: (string) => void): void {
  handler("clicked");
}

function main() {
  on_click((msg: string): void => {
    console.log(msg);
  });
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("fn on_click"),
        "should have on_click fn: {rust}"
    );
    assert!(
        rust.contains("Fn(") || rust.contains("fn(") || rust.contains("impl"),
        "should have function param type: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 51. Decorator on function
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_51_decorator() {
    let source = r#"@test
function test_addition() {
  const result: i32 = 2 + 2;
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("#[test]"),
        "decorator should produce #[test]: {rust}"
    );
    assert!(
        rust.contains("fn test_addition"),
        "should have test function: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 52. Generator function with yield
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_52_generator() {
    let source = r#"function* count_up(start: i32, end: i32): i32 {
  let i = start;
  while (i < end) {
    yield i;
    i += 1;
  }
}

function main() {
  for (const n of count_up(0, 3)) {
    console.log(n);
  }
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("Iterator"),
        "generator should produce Iterator impl: {rust}"
    );
    assert!(
        rust.contains("type Item"),
        "should have Item type: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 53. String method chains
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_53_string_methods() {
    let source = r#"function main() {
  const input: string = "  Hello, World!  ";
  const trimmed: string = input.trim();
  const upper: string = trimmed.toUpperCase();
  const has_hello: bool = trimmed.startsWith("Hello");
  console.log(trimmed);
  console.log(upper);
  console.log(has_hello);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("trim()"),
        "should have trim(): {rust}"
    );
    assert!(
        rust.contains("to_uppercase()"),
        "should have to_uppercase(): {rust}"
    );
    assert!(
        rust.contains("starts_with("),
        "should have starts_with(): {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 54. Using declaration (resource management)
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_54_using_declaration() {
    let source = r#"function openFile(path: string): string {
  return "file handle";
}

function main() {
  using file = openFile("data.txt");
  console.log(file);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("let file") || rust.contains("let _file"),
        "using should lower to let binding: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 55. Inline Rust escape hatch
// ---------------------------------------------------------------------------

#[test]
fn test_stress_advanced_55_inline_rust() {
    let source = r#"function main() {
  const x: i32 = 42;
  rust {
    println!("The answer is {}", x);
  }
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("println!(\"The answer is {}\", x)"),
        "inline Rust should be passed through: {rust}"
    );
    assert!(
        rust.contains("let x: i32 = 42"),
        "regular code should compile alongside: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ===========================================================================
// ===========================================================================
//
// GROUP 7: Real-world Composite (5 tests)
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// 56. Simple HTTP handler pattern (typed request/response)
// ---------------------------------------------------------------------------

#[test]
fn test_stress_composite_56_http_handler() {
    let source = r#"type Request = {
  method: string,
  path: string,
  body: string,
}

type Response = {
  status: i32,
  body: string,
}

function handle(req: Request): Response {
  const msg: string = `Received ${req.method} ${req.path}`;
  return { status: 200, body: msg };
}

function main() {
  const req: Request = { method: "GET", path: "/api/users", body: "" };
  const res: Response = handle(req);
  console.log(res.status);
  console.log(res.body);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Request"),
        "should have Request struct: {rust}"
    );
    assert!(
        rust.contains("struct Response"),
        "should have Response struct: {rust}"
    );
    assert!(
        rust.contains("fn handle("),
        "should have handle fn: {rust}"
    );
    assert!(
        rust.contains("format!("),
        "template literal should produce format!: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 57. Config parser: struct creation, field access, validation
// ---------------------------------------------------------------------------

#[test]
fn test_stress_composite_57_config_parser() {
    let source = r#"type AppConfig = {
  host: string,
  port: i32,
  debug: bool,
}

function validate_config(cfg: AppConfig): bool {
  if (cfg.port < 1 || cfg.port > 65535) {
    return false;
  }
  if (cfg.host.length === 0) {
    return false;
  }
  return true;
}

function default_config(): AppConfig {
  return { host: "localhost", port: 8080, debug: false };
}

function main() {
  const cfg: AppConfig = default_config();
  const valid: bool = validate_config(cfg);
  console.log(valid);
  console.log(cfg.host);
  console.log(cfg.port);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct AppConfig"),
        "should have AppConfig struct: {rust}"
    );
    assert!(
        rust.contains("fn validate_config"),
        "should have validate_config fn: {rust}"
    );
    assert!(
        rust.contains("fn default_config"),
        "should have default_config fn: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 58. State machine: enum states + transition function
// ---------------------------------------------------------------------------

#[test]
fn test_stress_composite_58_state_machine() {
    let source = r#"type State = "idle" | "loading" | "success" | "error"

function transition(current: State, action: string): State {
  switch (current) {
    case "idle": return "loading";
    case "loading": return "success";
    case "success": return "idle";
    case "error": return "idle";
  }
}

function main() {
  let state: State = "idle";
  state = transition(state, "start");
  console.log(state);
  state = transition(state, "complete");
  console.log(state);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("enum State"),
        "string union should produce enum: {rust}"
    );
    assert!(
        rust.contains("match"),
        "switch should produce match: {rust}"
    );
    assert!(
        rust.contains("fn transition"),
        "should have transition fn: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 59. Observable pattern: class with subscribe/notify
// ---------------------------------------------------------------------------

#[test]
fn test_stress_composite_59_observable() {
    let source = r#"class EventBus {
  listeners: Array<string>;

  constructor() {
    this.listeners = [];
  }

  subscribe(name: string): void {
    this.listeners.push(name);
  }

  count(): i32 {
    return this.listeners.length;
  }

  has_listeners(): bool {
    return this.listeners.length > 0;
  }
}

function main() {
  let bus: EventBus = new EventBus();
  bus.subscribe("click");
  bus.subscribe("hover");
  console.log(bus.count());
  console.log(bus.has_listeners());
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct EventBus"),
        "should have EventBus struct: {rust}"
    );
    assert!(
        rust.contains("fn subscribe("),
        "should have subscribe method: {rust}"
    );
    assert!(
        rust.contains("fn count("),
        "should have count method: {rust}"
    );
    assert!(
        rust.contains(".len()"),
        ".length should lower to .len(): {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ---------------------------------------------------------------------------
// 60. CLI argument parser: process flags, return options object
// ---------------------------------------------------------------------------

#[test]
fn test_stress_composite_60_cli_parser() {
    let source = r#"type Options = {
  verbose: bool,
  output: string,
  count: i32,
}

function default_options(): Options {
  return { verbose: false, output: "stdout", count: 1 };
}

function has_flag(args: Array<string>, flag: string): bool {
  for (const arg of args) {
    if (arg === flag) {
      return true;
    }
  }
  return false;
}

function main() {
  const args: Array<string> = ["--verbose", "--output"];
  const opts: Options = default_options();
  const is_verbose: bool = has_flag(args, "--verbose");
  console.log(is_verbose);
  console.log(opts.output);
  console.log(opts.count);
}"#;
    let rust = compile_to_rust(source);
    assert!(
        rust.contains("struct Options"),
        "should have Options struct: {rust}"
    );
    assert!(
        rust.contains("fn default_options"),
        "should have default_options fn: {rust}"
    );
    assert!(
        rust.contains("fn has_flag"),
        "should have has_flag fn: {rust}"
    );
    assert!(
        rust.contains("for"),
        "should have for loop: {rust}"
    );
    assert!(!rust.contains("todo!()"), "should not have todo: {rust}");
}

// ===========================================================================
// ===========================================================================
//
// COMPILATION E2E TESTS (require rustc — marked #[ignore])
//
// ===========================================================================
// ===========================================================================

// ---------------------------------------------------------------------------
// E2E: Data processing pipeline
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_stress_e2e_data_pipeline() {
    let source = r#"function main() {
  const nums: Array<i32> = [1, 2, 3, 4, 5];
  let total: i64 = 0;
  for (const n of nums) {
    total = total + n;
  }
  console.log(total);
}"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "15");
}

// ---------------------------------------------------------------------------
// E2E: Class with methods
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_stress_e2e_class_methods() {
    let source = r#"class Counter {
  count: i32;

  constructor(start: i32) {
    this.count = start;
  }

  increment(): void {
    this.count = this.count + 1;
  }

  get_value(): i32 {
    return this.count;
  }
}

function main() {
  let c: Counter = new Counter(0);
  c.increment();
  c.increment();
  c.increment();
  console.log(c.get_value());
}"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "3");
}

// ---------------------------------------------------------------------------
// E2E: Nested if/else + template literals
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_stress_e2e_nested_logic() {
    let source = r#"function classify(n: i64): string {
  if (n > 100) {
    return "large";
  } else if (n > 10) {
    return "medium";
  } else if (n > 0) {
    return "small";
  } else {
    return "non-positive";
  }
}

function main() {
  console.log(classify(200));
  console.log(classify(50));
  console.log(classify(5));
  console.log(classify(0));
}"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "large\nmedium\nsmall\nnon-positive");
}

// ---------------------------------------------------------------------------
// E2E: String methods
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_stress_e2e_string_methods() {
    let source = r#"function main() {
  const s: string = "hello world";
  console.log(s.toUpperCase());
  console.log(s.startsWith("hello"));
  console.log(s.trim());
}"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "HELLO WORLD\ntrue\nhello world");
}

// ---------------------------------------------------------------------------
// E2E: GCD algorithm
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_stress_e2e_gcd_algorithm() {
    let source = r#"function gcd(a: i32, b: i32): i32 {
  let x: i32 = a;
  let y: i32 = b;
  while (y != 0) {
    const temp: i32 = y;
    y = x % y;
    x = temp;
  }
  return x;
}

function main() {
  console.log(gcd(48, 18));
  console.log(gcd(100, 75));
}"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "6\n25");
}

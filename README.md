# RustScript

**Write TypeScript. Ship Rust.**

RustScript compiles TypeScript syntax to idiomatic Rust. No runtime, no GC, no lock-in. The generated `.rs` files are human-readable, compile with standard `rustc`, and publish as normal Rust crates. If you ever want out, eject to the generated Rust and keep going.

## Quick Example

RustScript's error handling is safer than TypeScript's — errors are typed, propagation is explicit, and the compiler enforces handling.

<table>
<tr><th>RustScript (.rts)</th><th>Generated Rust (.rs)</th></tr>
<tr>
<td>

```typescript
type User = { name: string, age: u32 }

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
}
```

</td>
<td>

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
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
            println!("Found: {}, age {}", user.name, user.age);
        }
        Err(err) => {
            println!("{}", err);
        }
    }
}
```

</td>
</tr>
</table>

`T | null` becomes `Option<T>`. `throws` becomes `Result<T, E>`. `try/catch` becomes `match`. The TypeScript developer never writes `Some`, `None`, `Ok`, or `Err` — the compiler handles it. The Rust developer reading the output sees nothing unusual.

## Getting Started

```bash
cargo install rustscript
rustscript init my-app
rustscript build
rustscript run
```

> **Note:** The `rsc` command is also available as a short alias for `rustscript`.

## Why RustScript

### For TypeScript developers

- **Familiar syntax** — functions, classes, interfaces, generics, async/await. If you know TypeScript, you can read and write RustScript on day one.
- **Type-safe error handling** — `throws` / `try` / `catch` compiles to `Result<T, E>` with typed errors and compiler-enforced handling. No more `catch (e: unknown)`.
- **Null safety** — `T | null` compiles to `Option<T>`. Optional chaining (`?.`) and nullish coalescing (`??`) work as expected, backed by Rust's exhaustive match.
- **Use any Rust crate** — `import { Router } from "axum"` lowers to `use axum::Router`. The entire crates.io ecosystem is your package registry.
- **Native binaries and WASM** — compile to a single static binary or a WASM module from the same source code.

### For Rust developers

- **Less ceremony for application code** — no lifetime annotations, no `mod.rs`, no `use` boilerplate. Write the logic, let the compiler handle the plumbing.
- **Auto-derive** — structs automatically get `#[derive(Debug, Clone, PartialEq, Eq)]`. Enums get `Copy` and `Hash` when applicable.
- **Tier 2 ownership inference** — function parameters that only read their value are automatically emitted as `&str` / `&T`. Clone insertion handles the rest.
- **TypeScript-familiar APIs** — `.map()` / `.filter()` / `.reduce()` on collections lower to zero-cost iterator chains. String methods like `.includes()`, `.startsWith()`, `.toUpperCase()` map to their Rust equivalents.
- **Inline Rust escape hatch** — drop to raw Rust with `rust { ... }` blocks when you need full control.

## Language Features

| RustScript | Rust | Notes |
|---|---|---|
| `const x = 42` | `let x = 42` | Immutable by default |
| `let x = 0` | `let mut x = 0` | `let` signals mutability |
| `string` | `String` | Always owned |
| `Array<T>` | `Vec<T>` | With `.map()`, `.filter()`, `.reduce()` |
| `Map<K, V>` | `HashMap<K, V>` | |
| `Set<T>` | `HashSet<T>` | |
| `T \| null` | `Option<T>` | Null checks → `match` / `if let` |
| `throws E` | `Result<T, E>` | `try/catch` → `match`, `throw` → `Err` |
| `interface Foo { }` | `trait Foo { }` | |
| `class Foo { }` | `struct Foo` + `impl Foo` | Constructor → `fn new()` |
| `class Foo implements Bar` | `impl Bar for Foo` | |
| `type Dir = "n" \| "s"` | `enum Dir { N, S }` | String unions → enums |
| `switch (x) { case "n": }` | `match x { Dir::N => }` | Exhaustive pattern matching |
| `async function f()` | `async fn f()` | Tokio runtime bundled |
| `await Promise.all([a, b])` | `tokio::join!(a, b)` | |
| `import { X } from "crate"` | `use crate::X` | Direct crate consumption |
| `export function f()` | `pub fn f()` | `export` → `pub` |
| `shared<T>` | `Arc<Mutex<T>>` | Concurrency sugar |
| `` `hello ${name}` `` | `format!("hello {}", name)` | Template literals |

## CLI

```bash
rustscript init [--template cli|web-server|wasm]   # scaffold a new project
rustscript build [--release] [--target wasm32-wasip1]  # compile to binary or WASM
rustscript run                                      # compile and run
rustscript check                                    # type check without building
rustscript test                                     # run tests
rustscript fmt [--check]                            # format .rts source files
rustscript lsp                                      # start the language server
```

## Tooling

- **LSP** — diagnostics, formatting, hover, go-to-definition via rust-analyzer proxy. Works with any editor that speaks LSP.
- **Formatter** — opinionated, zero-config. One style. No debates. `rustscript fmt`.
- **Project templates** — `rustscript init --template web-server` scaffolds a project with axum, tokio, and serde pre-configured. Templates for CLI and WASM apps too.
- **Error translation** — `rustc` errors are intercepted and re-rendered in RustScript terms, pointing at your `.rts` source positions.

## More Examples

### Data pipeline with iterator chaining

```typescript
type Product = { name: string, price: f64, inStock: bool }

function main() {
  const products: Array<Product> = [
    { name: "Widget", price: 29.99, inStock: true },
    { name: "Gadget", price: 49.99, inStock: false },
    { name: "Doohickey", price: 9.99, inStock: true },
  ];

  const total = products
    .filter(p => p.inStock)
    .map(p => p.price)
    .reduce((sum: f64, p: f64): f64 => sum + p, 0.0);

  console.log(`Total: $${total}`);  // Total: $39.98
}
```

The `.filter().map().reduce()` chain compiles to `iter().filter().cloned().map().fold()` — zero-cost Rust iterators, zero ceremony.

### Async with concurrent execution

```typescript
async function fetchUser(id: u32): string {
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
}
```

`async function main()` gets `#[tokio::main]`. `Promise.all` becomes `tokio::join!`. The runtime is bundled — async just works.

### Enums and pattern matching

```typescript
type TrafficLight = "red" | "yellow" | "green"

function next(light: TrafficLight): TrafficLight {
  switch (light) {
    case "red": return "green";
    case "green": return "yellow";
    case "yellow": return "red";
  }
}
```

String unions compile to Rust enums with `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]`. `switch` compiles to exhaustive `match` — miss a case and the compiler catches it.

## Project Status

RustScript is in active development. Phases 0 through 4 are complete, with 1,100+ tests passing across unit, snapshot, compilation, and end-to-end suites.

**What works today:**
- Full compilation pipeline (`.rts` → `.rs` → native binary)
- Core language: functions, classes, interfaces, generics, closures, enums, pattern matching
- Error handling: `T | null` → `Option`, `throws` → `Result`, `try/catch` → `match`
- Async/await with tokio, `Promise.all`, `spawn()`
- External crate consumption from crates.io
- String and iterator method sugar
- Tier 2 ownership inference (automatic `&str`/`&T` for read-only params)
- Auto-derive for structs and enums
- Inline Rust escape hatch
- `shared<T>` concurrency sugar
- WASM compilation target
- LSP, formatter, project templates, error translation

**What's next:**
- Tier 3 ownership (explicit lifetime annotations in TypeScript-friendly syntax)
- Web playground
- Full ecosystem shim generator
- Cross-function borrow inference
- Async lifetime interaction

## Contributing

RustScript is Apache 2.0 licensed.

The project has extensive design documentation:
- [`SPECIFICATION.md`](SPECIFICATION.md) — full language design and rationale
- [`PROCESS.md`](PROCESS.md) — development workflow and quality gates
- [`CONVENTIONS.md`](CONVENTIONS.md) — coding standards and architecture rules

## License

Apache 2.0

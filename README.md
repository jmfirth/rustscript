# RustScript

**Ship Rust. Write TypeScript.**

RustScript compiles TypeScript syntax to idiomatic Rust. No runtime, no GC, no lock-in. The generated `.rs` files are human-readable, compile with standard `rustc`, and use normal crates from crates.io. If you ever want out, run `rustscript eject` and keep going in pure Rust.

**[Website](https://rustscript.dev)** &middot; **[Playground](https://rustscript.dev/playground)** &middot; **[Docs](https://rustscript.dev/docs)** &middot; **[Crate Browser](https://rustscript.dev/crates)**

## Quick Start

```bash
cargo install rustscript
rustscript init my-app
cd my-app
rustscript run
```

> `rsc` is available as a short alias for `rustscript`.

## What It Looks Like

<table>
<tr><th>RustScript (.rts)</th><th>Generated Rust (.rs)</th></tr>
<tr>
<td>

```typescript
import { Serialize } from "serde";

type Book = {
  title: string,
  author: string,
  rating: f64,
} derives Serialize

function main() {
  const books: Array<Book> = [
    { title: "Dune", author: "Herbert", rating: 4.7 },
    { title: "Neuromancer", author: "Gibson", rating: 4.5 },
  ];

  const top = books.filter(b => b.rating > 4.6);
  console.log(JSON.stringify(top));
}
```

</td>
<td>

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
struct Book {
    pub title: String,
    pub author: String,
    pub rating: f64,
}

fn main() {
    let books: Vec<Book> = vec![
        Book { title: "Dune".to_string(),
               author: "Herbert".to_string(), rating: 4.7 },
        Book { title: "Neuromancer".to_string(),
               author: "Gibson".to_string(), rating: 4.5 },
    ];

    let top: Vec<Book> = books.iter()
        .filter(|b| b.rating > 4.6).cloned().collect();
    println!("{}", serde_json::to_string(&top).unwrap());
}
```

</td>
</tr>
</table>

## Why RustScript

**For TypeScript developers:** Write the syntax you know. Get native binaries, WASM, and the entire Rust crate ecosystem. No lifetimes, no borrow checker fights, no `mod.rs` boilerplate.

**For Rust developers:** Less ceremony for application code. Auto-derive, ownership inference, TypeScript-familiar collection methods (`.map()`, `.filter()`, `.reduce()`), and an inline `rust { }` escape hatch when you need full control.

## Language at a Glance

| RustScript | Rust | 
|---|---|
| `const x = 42` | `let x = 42` |
| `let x = 0` | `let mut x = 0` |
| `Array<T>` / `Map<K,V>` / `Set<T>` | `Vec<T>` / `HashMap<K,V>` / `HashSet<T>` |
| `T \| null` | `Option<T>` |
| `throws E` / `try` / `catch` | `Result<T,E>` / `match` |
| `interface Foo {}` | `trait Foo {}` |
| `class Foo extends Bar` | `struct` + `impl` + trait delegation |
| `type Dir = "n" \| "s"` | `enum Dir { N, S }` |
| `async function f()` | `async fn f()` + tokio |
| `await Promise.all([a, b])` | `tokio::join!(a, b)` |
| `import { X } from "crate"` | `use crate::X` + Cargo dep |
| `@command` | `#[tauri::command]` |
| `shared<T>` | `Arc<Mutex<T>>` |

## Examples

See the [`examples/`](examples/) directory for complete projects:

- **[Tauri Desktop App](examples/tauri_notes/)** — RustScript backend + React frontend with `@command` decorators and shared types
- **[REST API](examples/json_api/)** — Book catalog with axum + serde
- **[HTTP Client](examples/http_client/)** — Async reqwest with `Promise.all`
- **[CLI Tool](examples/cli_tool/)** — Task manager with clap

Every example compiles to a native binary with `rustscript build`.

## Crate Browser

Browse any Rust crate's public API translated to RustScript syntax:

**[rustscript.dev/crates](https://rustscript.dev/crates)** — axum, serde, tokio, clap, reqwest, and every other crate on crates.io.

## Tooling

- **VS Code extension** — LSP with diagnostics, hover, completions, and go-to-definition
- **Formatter** — `rustscript fmt`, zero-config, one style
- **Project templates** — `rustscript init --template web-server|cli|wasm`
- **Error translation** — rustc errors re-rendered in RustScript terms, pointing at `.rts` source
- **Type generator** — `rustscript types` emits `.d.ts` files for frontend/backend shared types
- **Watch mode** — `rustscript dev` rebuilds on save
- **Eject** — `rustscript eject` converts to pure Rust, no lock-in

## Project Status

RustScript is in beta. 2,600+ tests, 195 conformance tests (0 failures), 330+ built-in methods, 11 crates.

Full documentation at **[rustscript.dev/docs](https://rustscript.dev/docs)**.

## License

Apache 2.0

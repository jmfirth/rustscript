# Book Catalog REST API

A JSON REST API built with RustScript + axum + serde.

## What it demonstrates

- `import { Router } from "axum"` — Rust crate consumption via TypeScript imports
- `type Book = { ... } derives Serialize` — proc macro derives with the `derives` keyword
- `books.filter(b => b.rating > 4.6)` — closure-based collection methods
- `JSON.stringify(top)` — standard library methods compile to serde

## Run

```bash
rsc build
cargo run
# Server starts on :3000
```

## Source

See [src/main.rts](src/main.rts) for the full implementation.

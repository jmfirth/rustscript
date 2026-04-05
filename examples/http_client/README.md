# HTTP Client

An async HTTP client built with RustScript + reqwest.

## What it demonstrates

- `async function main()` — async/await with tokio runtime auto-configured
- `import { Client } from "reqwest"` — external crate consumption
- `await Promise.all([...])` — parallel async operations via `tokio::join!`
- `response.map(r => r.title)` — typed data processing with closures

## Run

```bash
rsc build
cargo run
# Fetches data from JSONPlaceholder API
```

## Source

See [src/main.rts](src/main.rts) for the full implementation.

# CLI Task Manager

A command-line task manager built with RustScript + clap.

## What it demonstrates

- `import { Parser } from "clap"` — derive-based CLI argument parsing
- `type Task = { ... }` — typed data structures
- `tasks.filter(t => t.status == "done")` — collection filtering with closures
- `switch (command)` — pattern matching on string enums

## Run

```bash
rsc build
cargo run
```

## Source

See [src/main.rts](src/main.rts) for the full implementation.

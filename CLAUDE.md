# RustScript (rsc)

A TypeScript-native authoring language that compiles to idiomatic Rust. Write TypeScript. Ship Rust.

See `SPECIFICATION.md` for the full language design.

---

## Project State

Pre-implementation. Specification and process documents are established. No code exists yet.

## Key Documents

| Document | Purpose | Read when |
|----------|---------|-----------|
| `SPECIFICATION.md` | Language design, syntax, semantics, compilation model | Before any design decision |
| `PLAN.md` | Task plan, dependencies, status tracking | Before starting any work |
| `PROCESS.md` | Agent workflow, quality gates, task lifecycle | Before any work |
| `CONVENTIONS.md` | Coding standards, style, architecture rules | Before writing code |
| `BOOTSTRAP.md` | TL/Orchestrator role activation | TL role only |
| `agents/` | Developer, reviewer, plan-reviewer role contracts | Agents read their own role doc |
| `tasks/` | Per-task specifications | Developer and reviewer agents |

## Compilation Pipeline

```
.rts → Parse → RustScript AST → Type Check → Ownership Infer → Lower → Rust IR → Emit → .rs → rustc/Cargo → binary
```

Crate structure will be defined during Phase 0 planning. The pipeline stages guide the decomposition.

## Architectural Invariants

These are non-negotiable constraints from the specification:

1. **Output is standard Rust.** Generated `.rs` files compile with unmodified `rustc`. No custom runtime, no wrapper types in public APIs, no special dependencies.
2. **Ecosystem compatibility is structural.** RustScript packages are Rust crates. `import { X } from "crate"` lowers to `use crate::X`. Types pass to Rust functions without conversion.
3. **Ownership inference may clone, never unsound.** The compiler may insert clones for correctness. It must never produce Rust that violates borrow checker rules.
4. **Generated Rust is human-readable.** The `.rs` output should look like code a Rust developer would write by hand. This is both the compilation path and the escape hatch.
5. **No lock-in.** A project can eject to the generated Rust and continue in pure Rust at any time.

## Commands

| Command | What it does |
|---------|-------------|
| `just check` | Format check + clippy |
| `just test` | Fast test suite (<10s) |
| `just test-all` | Full suite: unit + snapshot + compilation + e2e |
| `just build` | Debug build |
| `just release` | Release build |
| `just start` | Run rsc (pass-through args) |
| `just ci` | Full CI pipeline |
| `just doc` | Build and open docs |

## Code Standards

- `#![warn(clippy::pedantic)]` on all crates
- `cargo fmt` enforced — no exceptions
- No `unwrap()` or `expect()` in library code
- Doc comments on all public types, traits, functions
- `thiserror` for library errors, `anyhow` in binary crate only
- See `CONVENTIONS.md` for complete standards

## Testing Model

| Level | What | Where |
|-------|------|-------|
| Unit | Internal API correctness | `#[cfg(test)]` in each module |
| Snapshot | `.rts` → expected `.rs` golden files | `tests/snapshots/` |
| Compilation | Generated `.rs` compiles with rustc | `tests/compile/` |
| End-to-end | `.rts` → compile → run → expected output | `tests/e2e/` |
| Diagnostics | Invalid `.rts` → expected error messages | `tests/diagnostics/` |

Phase goals are measured by correctness test passage. See `PROCESS.md` for the full quality framework.

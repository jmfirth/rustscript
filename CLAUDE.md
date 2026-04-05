# RustScript (rsc)

A TypeScript-native authoring language that compiles to idiomatic Rust. Write TypeScript. Ship Rust.

See `SPECIFICATION.md` for the full language design.

---

## Project State

Pre-release complete. Full TypeScript syntax coverage, complete builtin library, Tier 2 error enrichment, 2,600+ tests. Zero conformance gaps. Remaining: Phase 7 (website/playground/docs).

## Key Documents

| Document | Purpose | Read when |
|----------|---------|-----------|
| `llms.txt` | **Compact language reference** — syntax, builtins, type mapping, crate integration | **Before writing any RustScript code** |
| `SPECIFICATION.md` | Language design, syntax, semantics, compilation model | Before any design decision |
| `CONVENTIONS.md` | Coding standards, style, architecture rules | Before writing code |

## Compilation Pipeline

```
.rts → Parse → RustScript AST → Type Check → Ownership Infer → Lower → Rust IR → Emit → .rs → rustc/Cargo → binary
```

### Crate Structure

| Crate | Responsibility |
|-------|---------------|
| `rsc-syntax` | AST types, Rust IR types, spans, diagnostics |
| `rsc-parser` | Lexer + recursive descent parser → RustScript AST |
| `rsc-typeck` | Type resolution, registry, bridge to Rust types |
| `rsc-lower` | AST → Rust IR (type lowering, ownership, builtins, transforms) |
| `rsc-emit` | Rust IR → `.rs` text + source maps |
| `rsc-driver` | Pipeline orchestration, Cargo integration, error translation |
| `rsc-cli` | Binary entry point, `rsc` commands |
| `rsc-fmt` | RustScript formatter |
| `rsc-lsp` | Language server (hover, completions, diagnostics) |

### Key lowering modules (`rsc-lower/src/transform/`)

| Module | Responsibility |
|--------|---------------|
| `mod.rs` | Transform struct, `lower_module` entry, utilities |
| `expr_lower.rs` | Expression lowering, numeric widening |
| `stmt_lower.rs` | Statement lowering, control flow |
| `type_lower.rs` | Type/enum/interface/mapped type lowering |
| `fn_lower.rs` | Function + generator lowering |
| `class_lower.rs` | Class inheritance, trait generation |
| `match_lower.rs` | Switch → match (string, integer, enum) |
| `union_lower.rs` | Union type registration + enum generation |
| `import_lower.rs` | Import classification |
| `use_collector.rs` | `use` declaration generation |

## Architectural Invariants

1. **Output is standard Rust.** Generated `.rs` files compile with unmodified `rustc`. No custom runtime.
2. **Ecosystem compatibility is structural.** `import { X } from "crate"` lowers to `use crate::X`.
3. **Ownership inference may clone, never unsound.** The compiler may insert clones for correctness.
4. **Generated Rust is human-readable.** Strong preference, not absolute (trait generation for inheritance is verbose but idiomatic).
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
| `just examples` | Validate all example projects |

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
| Snapshot | `.rts` → expected `.rs` patterns | `crates/rsc-driver/tests/snapshots.rs` |
| Stress | 65 real-world multi-feature patterns | `crates/rsc-driver/tests/stress_tests.rs` |
| Conformance | 190 combinatorial syntax × context tests | `crates/rsc-driver/tests/conformance.rs` |
| Compilation | Generated `.rs` compiles with rustc | `#[ignore]` tests, run with `--include-ignored` |
| End-to-end | `.rts` → compile → run → expected output | `crates/rsc-driver/tests/phase6_integration.rs` |
| Diagnostics | Invalid `.rts` → expected error messages | `crates/rsc-driver/tests/diagnostic_quality.rs` |

## Using cq (Code Query) Tools

The `cq` MCP tools provide semantic code intelligence. Use them instead of broad grep/glob when you need structured answers about the codebase. They're faster and more precise for symbol-level queries.

### When to use cq vs built-in tools

| Need | Use | Not |
|------|-----|-----|
| Find where a symbol is defined | `cq_def symbol` | `Grep "fn symbol"` |
| Read a function body | `cq_body symbol` | `Read` + scroll to find it |
| Get just the signature | `cq_sig symbol` | `cq_body` (wastes tokens on the body) |
| Find all call sites | `cq_callers symbol` | `Grep "symbol("` (misses method calls) |
| What does this function call? | `cq_deps symbol` | Reading the whole function |
| What's at this line? | `cq_context file:line` | `Read` with offset |
| Overview of a file | `cq_outline file` | `Read` whole file |
| Overview of a directory | `cq_tree` with scope | `Bash ls` + multiple reads |
| All structs in a crate | `cq_symbols --kind struct --scope crate` | `Grep "pub struct"` |
| Find unused code | `cq_dead` | Manual grep for refs |
| Type hierarchy | `cq_hierarchy TypeName` | Manual search through impls |
| Multi-level call graph | `cq_callchain symbol` | Manual callers-of-callers |
| Structural pattern search | `cq_search '(S-expr pattern)'` | Complex regex |

### Key patterns for this project

```
# Find where a type is defined
cq_def TypeKind --scope crates/rsc-syntax

# Get a lowering function's signature without reading 2000 lines
cq_sig lower_fn --scope crates/rsc-lower

# See what lower_expr depends on
cq_deps lower_expr --scope crates/rsc-lower/src/transform/expr_lower.rs

# All callers of a builtin method
cq_callers lower_console_log --scope crates/rsc-lower

# File outline to orient quickly
cq_outline crates/rsc-lower/src/transform/stmt_lower.rs

# Directory symbol tree
cq_tree --scope crates/rsc-lower/src/transform

# What function contains line 500 of the emitter?
cq_context crates/rsc-emit/src/emitter.rs:500

# Find all functions starting with "lower_"
cq_search '(function_item name: (identifier) @name (#match? @name "^lower_"))' --scope crates/rsc-lower

# Imports for a file
cq_imports crates/rsc-lower/src/transform/mod.rs
```

### Limitations

- `cq_callers` on heavily-used symbols (e.g., `compile_to_rust` in tests) can return very large results
- Resolution is syntactic (name-based) without a running LSP daemon — may include false positives for common names
- `cq_refs` and `cq_callers` report `completeness=best_effort` — they may miss indirect references
- Use `--scope` to narrow queries to specific crates/directories for speed and relevance

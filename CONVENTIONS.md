# RustScript (rsc) Coding Conventions

This document defines the rules all contributors (human and agent) follow. When in doubt, this document wins.

---

## 1. Rust Style

### Formatting
- `rustfmt` with default settings. No overrides in `rustfmt.toml`.
- Run `cargo fmt` before every commit. The pre-commit hook enforces this.

### Linting
- `#![warn(clippy::pedantic)]` in every crate's `lib.rs` or `main.rs`.
- Zero clippy warnings. `cargo clippy --workspace -- -D warnings` must pass.
- `#[allow(clippy::...)]` is permitted ONLY with a comment explaining why:
  ```rust
  #[allow(clippy::too_many_lines)]
  // Parser match arms for all expression types; splitting would obscure the grammar
  ```

### Naming

- Types: `PascalCase`
- Functions, methods, variables: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Crate names: `rsc-xxx` (kebab-case in Cargo.toml, `rsc_xxx` as Rust identifiers)
- Module files: `snake_case.rs`
- AST node types: `PascalCase` matching the grammar production (e.g., `FnDecl`, `IfExpr`, `BinaryOp`)
- Test functions: `test_<what>_<condition>_<expected>` (e.g., `test_parser_empty_function_produces_fn_decl`)

### Documentation

- All `pub` items get doc comments (`///`).
- Doc comments describe **what** and **why**, not **how** (the code shows how).
- Crate-level doc comments (`//!`) in every `lib.rs` with a one-line summary and the crate's role in the compilation pipeline.
- No doc comments on private items unless the logic is genuinely non-obvious.
- AST node doc comments should reference the corresponding RustScript syntax and the Rust lowering target.

---

## 2. Error Handling

### Library crates
- Use `thiserror` derive for all error enums.
- Each crate has its own error type in `error.rs`, re-exported from `lib.rs`.
- Each crate defines `pub type Result<T> = std::result::Result<T, XxxError>;`
- No `unwrap()` or `expect()` in library code. Ever.
- Use `?` for propagation. Use `map_err` when crossing crate boundaries.
- Error messages are lowercase, no trailing punctuation (Rust convention).

### Compiler diagnostics vs internal errors

These are two fundamentally different things. Never conflate them.

- **Compiler diagnostics** are user-facing messages about problems in `.rts` source code. They are structured data (source span, severity, message, optional suggestions) that accumulate during compilation and render with source context. A type error in `.rts` source is a diagnostic, not a Rust `Err`.
- **Internal errors** are bugs in the compiler itself — invariant violations, unexpected states, I/O failures. These use `thiserror` error types and propagate via `?`. An internal error means the compiler has a bug, not that the user's code is wrong.

### Panics
- `panic!()`, `todo!()`, `unimplemented!()`: never in merged library code.
- `unreachable!()`: acceptable when the code path is provably unreachable and the compiler can't see it.
- In tests: `unwrap()`, `expect()`, and `panic!()` are fine.

---

## 3. Dependencies

### Principles
- **Minimize dependency count.** Every dependency is a security and maintenance surface.
- **Prefer well-maintained, widely-used crates.** Check download counts and last publish date.
- **No feature bloat.** Enable only the features we use. Disable default features when appropriate.
- **Pin major versions** in Cargo.toml (e.g., `"1"` not `"*"`).

### Approved dependencies

| Crate | Purpose | Used in |
|-------|---------|---------|
| `thiserror` | Error derive macros | All library crates |
| `anyhow` | Top-level error handling | `rsc-cli` |
| `clap` (derive) | Argument parsing | `rsc-cli` |
| `codespan-reporting` | Diagnostic rendering with source spans | Diagnostic infrastructure |
| `tempfile` | Test temp files | dev-dependency |
| `insta` | Snapshot testing | dev-dependency |

Parser/lexer crate selection (e.g., `logos`, `winnow`, `chumsky`, or hand-written) is a Phase 0 design decision. The choice will be added to this table when made.

Adding a new dependency requires justification. Don't pull in a crate for something the standard library can do.

### Forbidden patterns
- No async runtime dependencies (tokio, async-std) in the compiler itself — the compiler is synchronous. Generated code may reference tokio.
- No `unsafe` without a `// SAFETY:` comment explaining the invariants.
- No `build.rs` scripts unless absolutely necessary (and documented why).
- No proc macros beyond `thiserror`, `clap`, and `serde` (when needed for AST serialization).

---

## 4. Architecture

### Crate boundaries
- Crate boundaries are defined during phase planning and documented in PLAN.md.
- No circular dependencies. The dependency graph is a DAG.
- Cross-crate communication uses well-defined trait interfaces or shared AST types.
- Private implementation details stay private. Only the designed public API is `pub`.

### Compiler-specific patterns

**AST design:**
- AST nodes carry source spans for diagnostic reporting.
- Use `enum` for node kinds with variant data, not trait objects.
- The RustScript AST and Rust IR are separate type hierarchies — lowering is a transformation between them, not an in-place mutation.
- Prefer `Box` for AST child nodes to start. Arena allocation is an optimization to pursue only if profiling shows allocation pressure.

**Visitor/walker pattern:**
- Define `Visitor` traits for AST traversal when multiple passes need to walk the tree.
- Visitors are the standard pattern for type checking, ownership inference, and lowering passes.
- Separate "visit" (read-only traversal) from "fold/transform" (produces new tree) operations.

**Lowering:**
- Lowering from RustScript AST to Rust IR is a pure function: input AST + context → output IR.
- No mutation of the input AST during lowering.
- Clone insertion happens during lowering based on ownership analysis.
- Each lowering step should be independently testable.

### Ownership and borrowing (in compiler code itself)
- Prefer borrowing over cloning. Clone only when ownership transfer is needed.
- Use `&str` in function parameters, `String` in struct fields that own data.
- Use `Cow<'_, str>` when a function might or might not need to allocate.
- AST nodes typically own their data (Strings, child Vecs). Passes borrow the AST.

### Type design
- Use newtypes to distinguish semantically different values of the same primitive type (e.g., `Span`, `NodeId`) — but only when confusion is a real risk. Don't over-newtype.
- Enums over booleans when a function has more than one boolean parameter.
- Prefer `Option` over sentinel values (no `-1` meaning "not found").

### Module organization
- One primary type per file. `parser.rs` contains `Parser`, `lexer.rs` contains `Lexer`.
- `mod.rs` is forbidden. Use `module_name.rs` with `mod module_name;` in the parent.
- Test modules: `#[cfg(test)] mod tests { ... }` at the bottom of the file they test.

---

## 5. Testing

### Philosophy
- **Test-first.** Write the test, see it fail, then implement.
- **Test behavior, not implementation.** Tests assert on observable outputs, not internal state.
- **100% coverage of public API.** Every `pub fn` has at least one test. Untested public API is a bug.

### Test organization
- Unit tests: `#[cfg(test)] mod tests` in each source file
- Snapshot tests: `tests/snapshots/` — `.rts` input files with `.rs` golden output
- Compilation tests: `tests/compile/` — verify generated `.rs` compiles with rustc
- End-to-end tests: `tests/e2e/` — `.rts` → compile → run → expected output
- Error diagnostic tests: `tests/diagnostics/` — invalid `.rts` → expected error messages
- Test helpers: `#[cfg(test)]` gated modules

### Test naming
```
test_<unit>_<scenario>_<expected_behavior>
```
Examples:
- `test_parser_empty_fn_produces_fn_decl_node`
- `test_lexer_string_literal_with_escapes`
- `test_lower_const_binding_emits_let`
- `test_ownership_moved_value_inserts_clone`
- `test_emit_struct_definition_matches_snapshot`

### Snapshot testing
- Use `insta` or equivalent for snapshot management.
- Each snapshot test has a `.rts` input and an expected `.rs` output.
- Snapshot updates require manual review — `cargo insta review`.
- Snapshot tests are part of the fast suite.
- Snapshot golden files are checked into the repo and reviewed like any other code.

### Test quality
- No `#[should_panic]` — test error returns instead.
- No `sleep()` in tests. Use deterministic synchronization.
- No filesystem side effects outside `tempfile` directories.
- Each test is independent. No shared mutable state between tests.
- Test both the happy path and edge cases (empty input, boundary values, error conditions).

### Coverage
- Target: 100% of public API surface area.
- Use `cargo-tarpaulin` or `cargo-llvm-cov` for measurement (configured via `just coverage`).
- Coverage of private functions is nice but not required — good public API coverage exercises most private code.

---

## 6. Performance

### Principles
- **Correctness first, then performance.** Don't optimize before profiling.
- **No premature allocation.** Use iterators and lazy evaluation where natural.
- **Compilation speed matters.** The compiler should be fast enough that developers don't notice it during normal development. Profile the full pipeline, not just individual passes.

### Specific guidelines
- Avoid `collect()` into a Vec when you can iterate directly.
- Use `&str` slicing instead of `String::clone()` where lifetime allows.
- String interning for identifiers is an optimization to consider if profiling shows allocation pressure — not before.
- Arena allocation for AST nodes is an optimization to consider if profiling shows tree walks are allocation-bound — not before.

---

## 7. Safety

- No `unsafe` without a `// SAFETY:` comment. Every unsafe block documents why it's sound.
- The compiler should have minimal (ideally zero) `unsafe` code. A compiler processes structured data and emits text — `unsafe` should rarely be necessary.
- Never trust input sizes. Validate before allocating. A malicious or malformed `.rts` file must not cause OOM.
- The parser must handle adversarial input gracefully — deeply nested expressions, extremely long lines, pathological repetition.

---

## 8. Git and Commits

- Branch naming: `task/NNN-short-name`
- Commit messages: imperative mood, one-line summary, optional blank line + body
- One logical change per commit
- No merge commits in feature branches (rebase workflow)
- `main` always builds and passes `just test`
- Commit hooks enforce `just check && just test` before commit

---

## 9. What NOT to Do

- Don't add features beyond what the current task specifies.
- Don't refactor code outside your task's scope.
- Don't add comments explaining obvious code.
- Don't add `// TODO` without a tracked task in PLAN.md.
- Don't use `String` where `&str` suffices.
- Don't use `Box<dyn Trait>` where a generic `<T: Trait>` works (unless you need type erasure).
- Don't add optional dependencies or feature flags without architectural justification.
- Don't suppress warnings with `#[allow]` — fix the warning.
- Don't write "defensive" code against impossible states. If a state is impossible, `unreachable!()` is appropriate.
- Don't conflate compiler diagnostics (user-facing) with internal errors (compiler bugs).
- Don't mutate AST nodes during lowering passes.

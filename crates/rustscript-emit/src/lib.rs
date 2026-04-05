//! Code emission for the `RustScript` compiler.
//!
//! Renders the Rust intermediate representation as formatted `.rs` source text.
//! This is the final compiler stage before handing off to `rustc` — it takes
//! [`RustFile`](rustscript_syntax::rust_ir::RustFile) IR and produces human-readable
//! `.rs` source.
//!
//! Also produces a line-level source map: for each line in the generated `.rs`,
//! records the corresponding `.rts` [`Span`](rustscript_syntax::span::Span) (if any).
#![warn(clippy::pedantic)]

mod emitter;

pub use emitter::EmitResult;

/// Emit Rust source code from Rust IR.
///
/// Produces a complete `.rs` file and a line-level source map. The emitter is
/// infallible: well-formed IR always produces valid output.
#[must_use]
pub fn emit(file: &rustscript_syntax::rust_ir::RustFile) -> EmitResult {
    emitter::emit(file)
}

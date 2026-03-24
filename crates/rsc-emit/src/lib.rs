//! Code emission for the `RustScript` compiler.
//!
//! Renders the Rust intermediate representation as formatted `.rs` source text.
//! This is the final compiler stage before handing off to `rustc` — it takes
//! [`RustFile`](rsc_syntax::rust_ir::RustFile) IR and produces human-readable
//! `.rs` source.
#![warn(clippy::pedantic)]

mod emitter;

/// Emit Rust source code from Rust IR.
///
/// Produces a complete `.rs` file as a string. The emitter is infallible:
/// well-formed IR always produces valid output.
#[must_use]
pub fn emit(file: &rsc_syntax::rust_ir::RustFile) -> String {
    emitter::emit(file)
}

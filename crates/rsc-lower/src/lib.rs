//! AST lowering for the `RustScript` compiler.
//!
//! Transforms the `RustScript` AST into a Rust intermediate representation,
//! performing type resolution, ownership inference, clone insertion, and
//! builtin method lowering. The entry point is [`lower`].
#![warn(clippy::pedantic)]

mod builtins;
mod context;
pub mod error;
mod ownership;
mod transform;

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::RustFile;

use transform::Transform;

/// Lower a `RustScript` AST to Rust IR.
///
/// Returns the Rust IR and any diagnostics encountered during lowering.
#[must_use]
pub fn lower(module: &ast::Module) -> (RustFile, Vec<Diagnostic>) {
    let mut transform = Transform::new();
    transform.lower_module(module)
}

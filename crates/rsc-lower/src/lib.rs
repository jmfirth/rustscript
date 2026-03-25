//! AST lowering for the `RustScript` compiler.
//!
//! Transforms the `RustScript` AST into a Rust intermediate representation,
//! performing type resolution, ownership inference, clone insertion, and
//! builtin method lowering. The entry point is [`lower`].
#![warn(clippy::pedantic)]

mod builtins;
mod context;
mod derive_inference;
pub mod error;
mod ownership;
mod transform;

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::rust_ir::RustFile;

use transform::Transform;

/// An external crate dependency discovered during lowering.
///
/// Collected when an import path references an external crate (not `"./"` local
/// or `"std/"` standard library). The driver uses these to populate the
/// generated `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CrateDependency {
    /// The crate name as it appears in `use` statements (underscored form).
    pub name: String,
}

/// Result of lowering a single module.
///
/// Groups the Rust IR, diagnostics, external crate dependencies, and
/// whether the module needs an async runtime.
pub struct LowerResult {
    /// The generated Rust IR.
    pub ir: RustFile,
    /// Diagnostics accumulated during lowering.
    pub diagnostics: Vec<Diagnostic>,
    /// External crate dependencies referenced by import statements.
    /// Deduplicated by crate name.
    pub crate_dependencies: Vec<CrateDependency>,
    /// Whether the source contains async functions that need a tokio runtime.
    pub needs_async_runtime: bool,
}

/// Lower a `RustScript` AST to Rust IR.
///
/// Returns a [`LowerResult`] containing the IR, diagnostics, and any external
/// crate dependencies discovered from import statements.
#[must_use]
pub fn lower(module: &ast::Module) -> LowerResult {
    let mut transform = Transform::new();
    let (ir, diagnostics, crate_deps, needs_async_runtime) = transform.lower_module(module);
    let crate_dependencies: Vec<CrateDependency> = crate_deps.into_iter().collect();
    LowerResult {
        ir,
        diagnostics,
        crate_dependencies,
        needs_async_runtime,
    }
}

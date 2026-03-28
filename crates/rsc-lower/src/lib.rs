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

use std::collections::HashMap;

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::external_fn::ExternalFnInfo;
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

/// Options controlling the lowering pass.
#[derive(Debug, Clone, Default)]
pub struct LowerOptions {
    /// When true, disables Tier 2 borrow inference and forces all function
    /// parameters to `Owned` mode (Tier 1 behavior). Useful for debugging
    /// when generated Rust has borrow-related compilation issues.
    pub no_borrow_inference: bool,
    /// External function signatures from rustdoc JSON, keyed by qualified name
    /// (e.g., `"axum::Router::route"` or `"serde_json::to_string"`).
    pub external_signatures: HashMap<String, ExternalFnInfo>,
}

/// Result of lowering a single module.
///
/// Groups the Rust IR, diagnostics, external crate dependencies, and
/// whether the module needs an async runtime, futures, `serde_json`, or rand crate.
#[allow(clippy::struct_excessive_bools)]
// Four boolean flags for independent crate dependency tracking
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
    /// Whether the source uses `for await` or `Promise.any` and needs the `futures` crate.
    pub needs_futures_crate: bool,
    /// Whether the source uses `JSON.stringify`/`JSON.parse` and needs `serde_json`.
    pub needs_serde_json: bool,
    /// Whether the source uses `Math.random()` and needs the `rand` crate.
    pub needs_rand: bool,
}

/// Lower a `RustScript` AST to Rust IR.
///
/// Returns a [`LowerResult`] containing the IR, diagnostics, and any external
/// crate dependencies discovered from import statements.
#[must_use]
pub fn lower(module: &ast::Module) -> LowerResult {
    lower_with_options(module, &LowerOptions::default())
}

/// Lower a `RustScript` AST to Rust IR with explicit options.
///
/// Like [`lower`], but accepts [`LowerOptions`] to control lowering behavior
/// (e.g., disabling borrow inference with `--no-borrow-inference`).
#[must_use]
pub fn lower_with_options(module: &ast::Module, options: &LowerOptions) -> LowerResult {
    let mut transform = Transform::new(options.no_borrow_inference);
    if !options.external_signatures.is_empty() {
        transform.set_external_signatures(options.external_signatures.clone());
    }
    let (
        ir,
        diagnostics,
        crate_deps,
        needs_async_runtime,
        needs_futures_crate,
        needs_serde_json,
        needs_rand,
    ) = transform.lower_module(module);
    let crate_dependencies: Vec<CrateDependency> = crate_deps.into_iter().collect();
    LowerResult {
        ir,
        diagnostics,
        crate_dependencies,
        needs_async_runtime,
        needs_futures_crate,
        needs_serde_json,
        needs_rand,
    }
}

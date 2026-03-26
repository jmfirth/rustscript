#![warn(clippy::pedantic)]
//! `RustScript` Language Server Protocol implementation.
//!
//! Provides an LSP server that integrates with editors to offer real-time
//! diagnostics, formatting, hover information, go-to-definition, and
//! completions for `.rts` files. Uses `tower-lsp` for protocol handling
//! and communicates over stdin/stdout.
//!
//! When available, proxies requests to rust-analyzer running on the generated
//! `.rs` code in `.rsc-build/`, translating positions and names between
//! `RustScript` and Rust.

pub mod builtin_hover;
pub mod diagnostics;
pub mod error;
pub mod name_map;
pub mod position_map;
pub mod ra_proxy;
pub mod server;

pub use server::RscLanguageServer;

use tower_lsp::{LspService, Server};

/// Start the LSP server on stdin/stdout.
///
/// This is the main entry point called by `rsc lsp`. It sets up the
/// `tower-lsp` service and runs the server until the client disconnects.
///
/// # Errors
///
/// Returns an error if the tokio runtime cannot be created.
pub fn run_server() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::new(RscLanguageServer::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    Ok(())
}

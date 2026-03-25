//! Error types for the `RustScript` compiler driver.

use std::path::PathBuf;

/// Errors that can occur during driver operations.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// No project found walking up from the given directory.
    #[error("project not found (looked for cargo.toml or src/ starting from {0})")]
    ProjectNotFound(PathBuf),

    /// Neither `src/index.rts` nor `src/main.rts` exists.
    #[error("main source file not found (expected src/index.rts or src/main.rts)")]
    MainSourceNotFound,

    /// Compilation produced error-level diagnostics.
    #[error("compilation failed with {0} error(s)")]
    CompilationFailed(usize),

    /// `cargo build` exited with a non-zero status.
    #[error("cargo build failed")]
    CargoBuildFailed,

    /// The target project directory already exists.
    #[error("project directory already exists: {0}")]
    ProjectExists(PathBuf),

    /// An invalid template name was provided.
    #[error("unknown template: {0}")]
    InvalidTemplate(String),

    /// Cannot run a WASM target directly (use a WASM runtime instead).
    #[error(
        "cannot run WASM target directly — use a WASM runtime (e.g., wasmtime) to execute the .wasm file"
    )]
    WasmRunUnsupported,

    /// An I/O error occurred.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A specialized `Result` type for driver operations.
pub type Result<T> = std::result::Result<T, DriverError>;

//! Error types for the LSP server crate.

/// An error that can occur within the LSP server.
#[derive(Debug, thiserror::Error)]
pub enum LspError {
    /// Failed to format a document.
    #[error("formatting failed: {0}")]
    FormatFailed(String),

    /// Failed to start rust-analyzer subprocess.
    #[error("rust-analyzer start failed: {0}")]
    RustAnalyzerStart(String),

    /// Communication error with rust-analyzer.
    #[error("rust-analyzer communication error: {0}")]
    RustAnalyzerComm(String),
}

/// A convenience alias for results within the LSP crate.
pub type Result<T> = std::result::Result<T, LspError>;

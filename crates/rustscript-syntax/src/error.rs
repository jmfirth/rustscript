//! Internal error types for the syntax crate.
//!
//! These represent compiler bugs or invariant violations — not user-facing
//! diagnostics. User-facing errors are handled via [`crate::diagnostic::Diagnostic`].

use crate::source::FileId;

/// Errors that can occur within the syntax infrastructure.
///
/// These are internal compiler errors — conditions that indicate a bug in
/// the compiler rather than a problem with the user's source code.
#[derive(Debug, thiserror::Error)]
pub enum SyntaxError {
    /// A source file could not be found at the given path.
    #[error("source file not found: {0}")]
    FileNotFound(String),

    /// A `FileId` was used that does not correspond to any loaded file.
    #[error("invalid file id: {0:?}")]
    InvalidFileId(FileId),
}

/// A convenience alias for results within the syntax crate.
pub type Result<T> = std::result::Result<T, SyntaxError>;

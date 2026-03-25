//! Error types for the formatter crate.
//!
//! [`FmtError`] represents failures during formatting — catastrophic parse
//! failures that prevent any formatting. Files with comments are not errors;
//! they are returned unchanged with a warning.

/// An error that can occur during formatting.
#[derive(Debug, thiserror::Error)]
pub enum FmtError {
    /// The source could not be parsed at all — no usable AST was produced.
    #[error("failed to parse source: {0}")]
    ParseFailed(String),
}

/// A convenience alias for results within the formatter crate.
pub type Result<T> = std::result::Result<T, FmtError>;

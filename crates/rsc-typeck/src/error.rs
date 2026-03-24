//! Error types for the type checking crate.

/// Errors that can occur during type checking.
#[derive(Debug, thiserror::Error)]
pub enum TypeckError {
    /// An internal type checking error (compiler bug).
    #[error("internal type checking error: {0}")]
    Internal(String),
}

/// A specialized `Result` type for type checking operations.
pub type Result<T> = std::result::Result<T, TypeckError>;

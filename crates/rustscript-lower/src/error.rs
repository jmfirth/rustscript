//! Internal error types for the lowering pass.
//!
//! These represent compiler bugs or invariant violations during lowering,
//! not user-facing diagnostics. User-facing type errors are
//! [`rustscript_syntax::diagnostic::Diagnostic`]s.

/// Errors that can occur within the lowering pass.
///
/// These are internal compiler errors indicating a bug in the lowering
/// implementation, not problems with the user's source code.
#[derive(Debug, thiserror::Error)]
pub enum LowerError {
    /// An unexpected internal condition was encountered during lowering.
    #[error("internal lowering error: {0}")]
    Internal(String),
}

/// A convenience alias for results within the lowering crate.
pub type Result<T> = std::result::Result<T, LowerError>;

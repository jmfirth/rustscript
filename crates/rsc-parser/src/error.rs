//! Internal error types for the parser crate.
//!
//! [`ParseError`] represents structural failures within the parser itself
//! (e.g., unexpected end of input). User-facing source errors are reported
//! as [`rsc_syntax::diagnostic::Diagnostic`]s, not as `ParseError`.

/// An internal parser error — indicates a structural failure, not a user mistake.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The parser reached the end of input when more tokens were expected.
    #[error("unexpected end of input")]
    UnexpectedEof,

    /// A catch-all for internal parser failures with a custom message.
    #[error("{0}")]
    Custom(String),
}

/// A convenience alias for results within the parser crate.
pub type Result<T> = std::result::Result<T, ParseError>;

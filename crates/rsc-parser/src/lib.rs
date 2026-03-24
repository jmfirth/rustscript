#![warn(clippy::pedantic)]
//! `RustScript` parser — lexer and parser for `.rts` source files.
//!
//! The lexer transforms raw source text into a token stream. The parser
//! (not yet implemented) will consume that stream to build a `RustScript` AST.

pub mod error;
pub mod lexer;
mod token;

// Re-export token types at crate level for intra-crate use.
// These are not part of the public API — the parser (Task 005) will
// be the primary consumer. They are `pub` only to satisfy Rust's
// dead-code and visibility analysis until the parser exists.
pub use token::{Token, TokenKind};

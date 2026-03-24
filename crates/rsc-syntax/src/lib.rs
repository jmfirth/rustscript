#![warn(clippy::pedantic)]
//! Core syntax types for the `RustScript` compiler.
//!
//! Provides source spans, diagnostic infrastructure, and error types
//! shared across all compiler passes.

pub mod diagnostic;
pub mod error;
pub mod source;
pub mod span;

#![warn(clippy::pedantic)]
//! Core syntax types for the `RustScript` compiler.
//!
//! Provides the `RustScript` AST, Rust IR, source spans, diagnostic
//! infrastructure, and error types shared across all compiler passes.

pub mod ast;
pub mod diagnostic;
pub mod error;
pub mod external_fn;
pub mod rust_ir;
pub mod source;
pub mod span;

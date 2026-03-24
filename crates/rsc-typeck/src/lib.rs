//! Type checking and type inference for the `RustScript` compiler.
//!
//! Resolves `RustScript` type annotations to the canonical [`Type`] representation,
//! infers types from expressions, and (in later tasks) validates generic constraints
//! and trait bounds.
#![warn(clippy::pedantic)]

pub mod bridge;
pub mod error;
pub mod resolve;
pub mod types;

// Re-export core types
pub use types::{PrimitiveType, Type};

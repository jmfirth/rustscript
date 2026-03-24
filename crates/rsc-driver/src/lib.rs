//! Pipeline orchestration for the `RustScript` compiler.
//!
//! Coordinates the full compilation pipeline: parse, lower, and emit,
//! threading diagnostics and errors between stages.
#![warn(clippy::pedantic)]

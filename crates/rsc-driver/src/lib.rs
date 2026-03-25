#![warn(clippy::pedantic)]
//! `RustScript` compiler driver — pipeline orchestration and Cargo integration.
//!
//! This crate wires the compiler pipeline together: parse, lower, emit,
//! write files, and invoke Cargo. It also handles project scaffolding
//! (`init_project`) and diagnostic aggregation.

pub mod error;
mod pipeline;
mod project;

pub use pipeline::{CompileResult, compile_source, compile_source_with_mods};
pub use project::{Project, init_project};

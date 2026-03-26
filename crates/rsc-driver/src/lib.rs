#![warn(clippy::pedantic)]
//! `RustScript` compiler driver — pipeline orchestration and Cargo integration.
//!
//! This crate wires the compiler pipeline together: parse, lower, emit,
//! write files, and invoke Cargo. It also handles project scaffolding
//! (`init_project`) and diagnostic aggregation.

pub mod deps;
pub mod error;
pub mod error_translation;
mod pipeline;
mod project;
mod templates;

pub use error_translation::translate_rustc_errors;
pub use pipeline::{
    CompileOptions, CompileResult, compile_source, compile_source_with_mods,
    compile_source_with_mods_and_options, compile_source_with_options,
};
pub use project::{Project, WasmTarget, init_project, parse_wasm_target};

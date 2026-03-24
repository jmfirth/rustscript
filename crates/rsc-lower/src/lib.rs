//! AST lowering for the `RustScript` compiler.
//!
//! Transforms the `RustScript` AST into a Rust intermediate representation,
//! performing ownership inference and inserting clones as needed.
#![warn(clippy::pedantic)]

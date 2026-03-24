#![warn(clippy::pedantic)]
//! `RustScript` parser — lexer and parser for `.rts` source files.
//!
//! The lexer transforms raw source text into a token stream. The parser
//! consumes that stream to build a `RustScript` AST. The primary entry
//! point is [`parse`], which tokenizes and parses in a single call.

pub mod error;
mod lexer;
mod parser;
mod token;

use rsc_syntax::ast;
use rsc_syntax::diagnostic::Diagnostic;
use rsc_syntax::source::FileId;

use lexer::Lexer;
use parser::Parser;

/// Parse `RustScript` source code into an AST.
///
/// Returns the AST and any diagnostics encountered during parsing.
/// The AST may be partial if errors were encountered (error recovery
/// allows parsing to continue past syntax errors).
#[must_use]
pub fn parse(source: &str, file_id: FileId) -> (ast::Module, Vec<Diagnostic>) {
    let lexer = Lexer::new(source, file_id);
    let (tokens, lexer_diagnostics) = lexer.tokenize();

    let mut parser = Parser::new(tokens, file_id);
    let module = parser.parse_module();
    let mut diagnostics = lexer_diagnostics;
    diagnostics.extend(parser.into_diagnostics());

    (module, diagnostics)
}

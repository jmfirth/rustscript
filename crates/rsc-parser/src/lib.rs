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

    let mut parser = Parser::new(tokens, file_id, source);
    let module = parser.parse_module();

    // Filter out lexer diagnostics that fall within regex literal spans.
    // The lexer cannot disambiguate `/` as regex vs division, so it may
    // emit "unexpected character" errors for content inside regex patterns.
    let regex_spans = parser.regex_literal_spans();
    let mut diagnostics: Vec<Diagnostic> = if regex_spans.is_empty() {
        lexer_diagnostics
    } else {
        lexer_diagnostics
            .into_iter()
            .filter(|diag| {
                !diag
                    .labels
                    .iter()
                    .any(|label| regex_spans.iter().any(|rs| rs.contains(label.span.start)))
            })
            .collect()
    };

    diagnostics.extend(parser.into_diagnostics());

    (module, diagnostics)
}

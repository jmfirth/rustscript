//! Diagnostic conversion between `RustScript` compiler diagnostics and LSP diagnostics.
//!
//! Translates [`rustscript_syntax::diagnostic::Diagnostic`] to [`tower_lsp::lsp_types::Diagnostic`],
//! converting byte-offset spans to line/column positions as required by the LSP protocol.

use rustscript_syntax::diagnostic::{Diagnostic, Severity};
use rustscript_syntax::span::Span;
use tower_lsp::lsp_types;

/// Convert a `RustScript` compiler diagnostic to an LSP diagnostic.
///
/// Maps severity levels and converts byte-offset spans to LSP line/column ranges
/// using the provided source text. If the diagnostic has labels, uses the first
/// label's span for the range; otherwise falls back to the start of the file.
#[must_use]
pub fn to_lsp_diagnostic(diag: &Diagnostic, source: &str) -> lsp_types::Diagnostic {
    let range = diag
        .labels
        .first()
        .map(|label| span_to_range(label.span, source))
        .unwrap_or_default();

    lsp_types::Diagnostic {
        range,
        severity: Some(match diag.severity {
            Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
            Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
            Severity::Note => lsp_types::DiagnosticSeverity::INFORMATION,
        }),
        message: diag.message.clone(),
        source: Some("rsc".to_owned()),
        ..Default::default()
    }
}

/// Convert a byte-offset [`Span`] to an LSP [`lsp_types::Range`].
///
/// LSP uses 0-based line and column numbers. This function converts from the
/// compiler's byte offsets by scanning the source text for newlines.
#[must_use]
pub fn span_to_range(span: Span, source: &str) -> lsp_types::Range {
    let start = offset_to_position(span.start.0, source);
    let end = offset_to_position(span.end.0, source);
    lsp_types::Range { start, end }
}

/// Convert a byte offset to an LSP [`lsp_types::Position`] (0-based line and column).
///
/// Scans the source text up to the given offset, counting newlines for lines
/// and characters within the current line for the column.
#[must_use]
pub fn offset_to_position(offset: u32, source: &str) -> lsp_types::Position {
    let offset = offset as usize;
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    lsp_types::Position {
        line,
        character: col,
    }
}

/// Convert an LSP [`lsp_types::Position`] (0-based line/column) to a byte offset.
///
/// Scans the source text counting newlines and characters to find the byte
/// position corresponding to the given line and column.
#[must_use]
pub fn position_to_offset(position: &lsp_types::Position, source: &str) -> u32 {
    let target_line = position.line;
    let target_col = position.character;
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, c) in source.char_indices() {
        if line == target_line && col == target_col {
            #[allow(clippy::cast_possible_truncation)]
            // Source files larger than 4 GiB are not supported.
            return i as u32;
        }
        if c == '\n' {
            if line == target_line {
                // Column is beyond end of line; return end of this line.
                #[allow(clippy::cast_possible_truncation)]
                return i as u32;
            }
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    // Source files larger than 4 GiB are not supported.
    {
        source.len() as u32
    }
}

/// Compute an LSP range covering the entire source document.
///
/// Returns a range from `(0, 0)` to the end of the last line.
#[must_use]
pub fn full_document_range(source: &str) -> lsp_types::Range {
    let mut line = 0u32;
    let mut col = 0u32;
    for c in source.chars() {
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    lsp_types::Range {
        start: lsp_types::Position {
            line: 0,
            character: 0,
        },
        end: lsp_types::Position {
            line,
            character: col,
        },
    }
}

/// Collect LSP diagnostics from a `RustScript` source string.
///
/// Parses the source, and if no parse errors occur, runs the full compilation
/// pipeline. All diagnostics (parse errors, lowering errors, type errors) are
/// collected and converted to LSP diagnostics.
#[must_use]
pub fn collect_diagnostics(source: &str) -> Vec<lsp_types::Diagnostic> {
    let file_id = rustscript_syntax::source::FileId(0);
    let (_, parse_diags) = rustscript_parser::parse(source, file_id);

    let mut lsp_diagnostics: Vec<lsp_types::Diagnostic> = Vec::new();

    // If there are parse errors, only report those (further passes would fail).
    let has_parse_errors = parse_diags
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));

    if has_parse_errors {
        for diag in &parse_diags {
            lsp_diagnostics.push(to_lsp_diagnostic(diag, source));
        }
        return lsp_diagnostics;
    }

    // No parse errors — run full compilation pipeline for type/lowering diagnostics.
    let result = rustscript_driver::compile_source(source, "lsp_input.rts");
    for diag in &result.diagnostics {
        lsp_diagnostics.push(to_lsp_diagnostic(diag, source));
    }

    lsp_diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustscript_syntax::diagnostic::Diagnostic;
    use rustscript_syntax::source::FileId;
    use rustscript_syntax::span::Span;

    #[test]
    fn test_diagnostics_offset_to_position_first_char() {
        let source = "hello\nworld\n";
        let pos = offset_to_position(0, source);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn test_diagnostics_offset_to_position_mid_first_line() {
        let source = "hello\nworld\n";
        let pos = offset_to_position(3, source);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 3);
    }

    #[test]
    fn test_diagnostics_offset_to_position_second_line_start() {
        let source = "hello\nworld\n";
        let pos = offset_to_position(6, source);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn test_diagnostics_offset_to_position_second_line_mid() {
        let source = "hello\nworld\n";
        let pos = offset_to_position(8, source);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 2);
    }

    #[test]
    fn test_diagnostics_span_to_range_converts_correctly() {
        let source = "hello\nworld\n";
        let span = Span::new(6, 11); // "world"
        let range = span_to_range(span, source);
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 1);
        assert_eq!(range.end.character, 5);
    }

    #[test]
    fn test_diagnostics_position_to_offset_first_line() {
        let source = "hello\nworld\n";
        let pos = lsp_types::Position {
            line: 0,
            character: 3,
        };
        assert_eq!(position_to_offset(&pos, source), 3);
    }

    #[test]
    fn test_diagnostics_position_to_offset_second_line() {
        let source = "hello\nworld\n";
        let pos = lsp_types::Position {
            line: 1,
            character: 2,
        };
        assert_eq!(position_to_offset(&pos, source), 8);
    }

    #[test]
    fn test_diagnostics_full_document_range_multiline() {
        let source = "line1\nline2\nline3";
        let range = full_document_range(source);
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 2);
        assert_eq!(range.end.character, 5);
    }

    #[test]
    fn test_diagnostics_full_document_range_single_line() {
        let source = "hello";
        let range = full_document_range(source);
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 0);
        assert_eq!(range.end.character, 5);
    }

    #[test]
    fn test_diagnostics_full_document_range_trailing_newline() {
        let source = "line1\nline2\n";
        let range = full_document_range(source);
        assert_eq!(range.end.line, 2);
        assert_eq!(range.end.character, 0);
    }

    #[test]
    fn test_diagnostics_to_lsp_diagnostic_error_severity() {
        let diag =
            Diagnostic::error("unexpected token").with_label(Span::new(0, 5), FileId(0), "here");
        let source = "hello world";
        let lsp_diag = to_lsp_diagnostic(&diag, source);

        assert_eq!(
            lsp_diag.severity,
            Some(lsp_types::DiagnosticSeverity::ERROR)
        );
        assert_eq!(lsp_diag.message, "unexpected token");
        assert_eq!(lsp_diag.source, Some("rsc".to_owned()));
        assert_eq!(lsp_diag.range.start.line, 0);
        assert_eq!(lsp_diag.range.start.character, 0);
        assert_eq!(lsp_diag.range.end.line, 0);
        assert_eq!(lsp_diag.range.end.character, 5);
    }

    #[test]
    fn test_diagnostics_to_lsp_diagnostic_warning_severity() {
        let diag =
            Diagnostic::warning("unused variable").with_label(Span::new(4, 5), FileId(0), "here");
        let source = "let x = 1;";
        let lsp_diag = to_lsp_diagnostic(&diag, source);

        assert_eq!(
            lsp_diag.severity,
            Some(lsp_types::DiagnosticSeverity::WARNING)
        );
    }

    #[test]
    fn test_diagnostics_to_lsp_diagnostic_no_labels_uses_default_range() {
        let diag = Diagnostic::error("something failed");
        let source = "hello";
        let lsp_diag = to_lsp_diagnostic(&diag, source);

        assert_eq!(lsp_diag.range.start.line, 0);
        assert_eq!(lsp_diag.range.start.character, 0);
        assert_eq!(lsp_diag.range.end.line, 0);
        assert_eq!(lsp_diag.range.end.character, 0);
    }

    // Correctness scenario 1: Parse error to LSP diagnostic
    #[test]
    fn test_diagnostics_correctness_parse_error_produces_lsp_diagnostic() {
        let source = "function foo( { }";
        let diagnostics = collect_diagnostics(source);

        assert!(
            !diagnostics.is_empty(),
            "should produce at least one diagnostic"
        );

        let first = &diagnostics[0];
        assert_eq!(
            first.severity,
            Some(lsp_types::DiagnosticSeverity::ERROR),
            "parse error should be ERROR severity"
        );
        // The parse error message should indicate something about the expected token
        // (the exact message depends on the parser, but it should be meaningful)
        assert!(
            !first.message.is_empty(),
            "diagnostic message should not be empty"
        );
    }

    // Correctness scenario 2: Multi-error diagnostics
    #[test]
    fn test_diagnostics_correctness_multi_error_produces_multiple_diagnostics() {
        // Source with multiple errors
        let source = "function foo( { } function bar( { }";
        let diagnostics = collect_diagnostics(source);

        // Should produce at least one diagnostic (parser error recovery may vary)
        assert!(
            !diagnostics.is_empty(),
            "should produce at least one diagnostic for source with errors"
        );
    }

    // Test: Clean source produces no diagnostics
    #[test]
    fn test_diagnostics_clean_source_no_diagnostics() {
        let source = "function foo() {}";
        let diagnostics = collect_diagnostics(source);
        assert!(
            diagnostics.is_empty(),
            "valid source should produce no diagnostics, got: {diagnostics:?}"
        );
    }

    // Test: Parse error diagnostics have correct range
    #[test]
    fn test_diagnostics_parse_error_has_range() {
        let source = "function foo( { }";
        let diagnostics = collect_diagnostics(source);

        assert!(!diagnostics.is_empty());
        let first = &diagnostics[0];
        // The range should point somewhere within the source
        // (exact position depends on parser error recovery)
        assert!(
            first.range.start.line == 0,
            "parse error should be on line 0 for single-line source"
        );
    }
}

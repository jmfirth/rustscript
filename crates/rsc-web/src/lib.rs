#![warn(clippy::pedantic)]
//! WASM bindings for the `RustScript` compiler.
//!
//! Exposes the compiler's pure functions via `wasm-bindgen` for use in the
//! playground (live compilation), crate docs (rustdoc translation), and
//! Monaco LSP features — all client-side in the browser.

pub mod translator;

use rsc_driver::rustdoc_parser::RustdocItemKind;
use rsc_syntax::diagnostic::Severity;
use rsc_syntax::source::compute_line_starts;
use rsc_syntax::span::BytePos;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Shared output types
// ---------------------------------------------------------------------------

/// JSON-serializable compilation result returned by [`compile`].
#[derive(Serialize)]
struct CompileOutput {
    /// The generated Rust source code (empty on error).
    rust_source: String,
    /// All diagnostics from all compiler passes.
    diagnostics: Vec<DiagnosticOutput>,
    /// Whether any error-level diagnostics were emitted.
    has_errors: bool,
}

/// A single diagnostic suitable for JSON serialization.
#[derive(Serialize)]
struct DiagnosticOutput {
    /// The human-readable message.
    message: String,
    /// `"error"`, `"warning"`, or `"info"`.
    severity: String,
    /// 1-based line number (if available).
    line: Option<u32>,
    /// 0-based column offset (if available).
    column: Option<u32>,
}

/// A translated rustdoc item for the docs viewer.
#[derive(Serialize)]
struct TranslatedItem {
    /// The item's name (e.g. `Router`).
    name: String,
    /// The item kind: `"function"`, `"struct"`, `"trait"`, or `"enum"`.
    kind: String,
    /// The `RustScript`-syntax signature.
    signature: String,
    /// Documentation string, if any.
    docs: Option<String>,
    /// Module path (reserved for future use).
    module: Option<String>,
}

// ---------------------------------------------------------------------------
// Helper: convert byte position to (line, column) using source text
// ---------------------------------------------------------------------------

/// Convert a `BytePos` to a 1-based line and 0-based column using
/// precomputed line starts from the source text.
fn byte_pos_to_line_col(pos: BytePos, line_starts: &[u32]) -> (u32, u32) {
    let offset = pos.0;
    let line_idx = line_starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1);
    let line_start = line_starts.get(line_idx).copied().unwrap_or(0);
    let col = offset.saturating_sub(line_start);

    #[allow(clippy::cast_possible_truncation)]
    // Line numbers and columns within a single source file will never overflow u32.
    let line_1based = (line_idx as u32) + 1;
    (line_1based, col)
}

/// Build [`DiagnosticOutput`] entries from compiler diagnostics, resolving
/// byte positions to line/column using the raw source text.
fn build_diagnostics(
    diagnostics: &[rsc_syntax::diagnostic::Diagnostic],
    source: &str,
) -> Vec<DiagnosticOutput> {
    let line_starts = compute_line_starts(source);

    diagnostics
        .iter()
        .map(|d| {
            let (line, column) = d.labels.first().map_or((None, None), |label| {
                let (l, c) = byte_pos_to_line_col(label.span.start, &line_starts);
                (Some(l), Some(c))
            });

            DiagnosticOutput {
                message: d.message.clone(),
                severity: match d.severity {
                    Severity::Error => "error".to_owned(),
                    Severity::Warning => "warning".to_owned(),
                    Severity::Note => "info".to_owned(),
                },
                line,
                column,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. compile(source) -> { rust_source, diagnostics, has_errors }
// ---------------------------------------------------------------------------

/// Compile `RustScript` source to Rust.
///
/// Returns a JSON object with `rust_source`, `diagnostics`, and `has_errors`.
#[wasm_bindgen]
#[allow(clippy::must_use_candidate)]
pub fn compile(source: &str) -> JsValue {
    let result = rsc_driver::compile_source(source, "playground.rts");

    let output = CompileOutput {
        rust_source: result.rust_source,
        diagnostics: build_diagnostics(&result.diagnostics, source),
        has_errors: result.has_errors,
    };

    serde_wasm_bindgen::to_value(&output).unwrap_or(JsValue::NULL)
}

// ---------------------------------------------------------------------------
// 2. get_diagnostics(source) -> [{ message, severity, line, column }]
// ---------------------------------------------------------------------------

/// Parse and lower a `RustScript` source, returning only diagnostics.
///
/// Faster than [`compile`] because it skips the emit stage.
#[wasm_bindgen]
#[allow(clippy::must_use_candidate)]
pub fn get_diagnostics(source: &str) -> JsValue {
    let line_starts = compute_line_starts(source);

    // Stage 1: Parse
    let file_id = rsc_syntax::source::FileId(0);
    let (module, parse_diagnostics) = rsc_parser::parse(source, file_id);

    let mut all_diagnostics = parse_diagnostics;

    // Stage 2: Lower (only if parsing succeeded without errors)
    let has_parse_errors = all_diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));

    if !has_parse_errors {
        let lower_result = rsc_lower::lower(&module);
        all_diagnostics.extend(lower_result.diagnostics);
    }

    let output: Vec<DiagnosticOutput> = all_diagnostics
        .iter()
        .map(|d| {
            let (line, column) = d.labels.first().map_or((None, None), |label| {
                let (l, c) = byte_pos_to_line_col(label.span.start, &line_starts);
                (Some(l), Some(c))
            });

            DiagnosticOutput {
                message: d.message.clone(),
                severity: match d.severity {
                    Severity::Error => "error".to_owned(),
                    Severity::Warning => "warning".to_owned(),
                    Severity::Note => "info".to_owned(),
                },
                line,
                column,
            }
        })
        .collect();

    serde_wasm_bindgen::to_value(&output).unwrap_or(JsValue::NULL)
}

// ---------------------------------------------------------------------------
// 3. hover(source, line, column) -> String
// ---------------------------------------------------------------------------

/// Return hover information for the symbol at the given position.
///
/// Delegates to the shared `rsc-hover` crate which contains all hover logic.
#[wasm_bindgen]
#[allow(clippy::must_use_candidate)]
pub fn hover(source: &str, line: u32, column: u32) -> String {
    rsc_hover::hover(source, line, column)
}

// ---------------------------------------------------------------------------
// 4. translate_rustdoc(json) -> [{ name, kind, signature, docs, module }]
// ---------------------------------------------------------------------------

/// Translate rustdoc JSON to `RustScript`-syntax item descriptions.
///
/// Takes a rustdoc JSON string (from `cargo doc --output-format json`),
/// parses it, and translates all items to `RustScript` syntax.
#[wasm_bindgen]
#[allow(clippy::must_use_candidate)]
pub fn translate_rustdoc(json: &str) -> JsValue {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json) else {
        return JsValue::NULL;
    };

    let Some(crate_data) = rsc_driver::rustdoc_parser::parse_rustdoc_json(&parsed) else {
        return JsValue::NULL;
    };

    let items: Vec<TranslatedItem> = crate_data
        .items
        .values()
        .map(|item| {
            let kind = match &item.kind {
                RustdocItemKind::Function(_) => "function",
                RustdocItemKind::Struct(_) => "struct",
                RustdocItemKind::Trait(_) => "trait",
                RustdocItemKind::Enum(_) => "enum",
            };
            TranslatedItem {
                name: item.name.clone(),
                kind: kind.to_owned(),
                signature: translator::translate_item_to_hover(item),
                docs: item.docs.clone(),
                module: None,
            }
        })
        .collect();

    serde_wasm_bindgen::to_value(&items).unwrap_or(JsValue::NULL)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_hello_world() {
        let source = "function main() { console.log(\"hello\"); }";
        let result = rsc_driver::compile_source(source, "test.rts");
        assert!(
            !result.has_errors,
            "compilation produced errors: {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
        assert!(result.rust_source.contains("println!"));
    }

    #[test]
    fn test_compile_with_error() {
        // Intentionally bad source to produce a parse error.
        let source = "function { }";
        let result = rsc_driver::compile_source(source, "test.rts");
        assert!(result.has_errors);
        assert!(!result.diagnostics.is_empty());
    }

    #[test]
    fn test_build_diagnostics_line_column() {
        let source = "let x: number = 42;\nlet y: string = \"hi\";";
        let diag = rsc_syntax::diagnostic::Diagnostic::error("test error").with_label(
            rsc_syntax::span::Span::new(20, 21),
            rsc_syntax::source::FileId(0),
            "here",
        );
        let output = build_diagnostics(&[diag], source);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].severity, "error");
        // Byte 20 is the start of line 2 (0-indexed line 1).
        assert_eq!(output[0].line, Some(2));
        assert_eq!(output[0].column, Some(0));
    }

    #[test]
    fn test_hover_builtin_console() {
        let source = "function main() { console.log(\"hello\"); }";
        // "console" starts at 0-based col 18, 1-based col 19
        let result = hover(source, 1, 19);
        assert!(
            result.contains("console"),
            "expected console hover, got: {result}"
        );
    }

    #[test]
    fn test_hover_builtin_log() {
        let source = "function main() { console.log(\"hello\"); }";
        // "log" starts at 0-based col 26, 1-based col 27
        let result = hover(source, 1, 27);
        assert!(result.contains("log"), "expected log hover, got: {result}");
    }

    #[test]
    fn test_hover_user_function() {
        let source = "function greet(name: string): string { return name; }";
        // "greet" starts at 0-based col 9, 1-based col 10
        let result = hover(source, 1, 10);
        assert!(
            result.contains("function greet"),
            "expected greet signature, got: {result}"
        );
    }

    #[test]
    fn test_hover_doc_comment() {
        let source =
            "/** Greets a person */\nfunction greet(name: string): string { return name; }";
        let result = hover(source, 2, 10);
        assert!(
            result.contains("Greets a person"),
            "expected doc comment in hover, got: {result}"
        );
        assert!(
            result.contains("function greet"),
            "expected signature in hover, got: {result}"
        );
    }

    #[test]
    fn test_hover_out_of_bounds() {
        let source = "let x = 1;";
        let result = hover(source, 99, 1);
        assert!(result.is_empty());
    }

    #[test]
    fn test_translate_rustdoc_invalid_json() {
        // Invalid JSON should fail to parse.
        let parsed: Result<serde_json::Value, _> = serde_json::from_str("not valid json");
        assert!(parsed.is_err());
    }

    #[test]
    fn test_translate_rustdoc_empty_index() {
        let json = r#"{"index": {}, "paths": {}}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let crate_data = rsc_driver::rustdoc_parser::parse_rustdoc_json(&parsed);
        // Valid rustdoc JSON with no items — should parse but have empty items.
        assert!(crate_data.is_some());
        assert!(crate_data.unwrap().items.is_empty());
    }

    #[test]
    fn test_byte_pos_to_line_col_basic() {
        let line_starts = vec![0, 10, 20];
        // Byte 0 = line 1, col 0
        assert_eq!(byte_pos_to_line_col(BytePos(0), &line_starts), (1, 0));
        // Byte 5 = line 1, col 5
        assert_eq!(byte_pos_to_line_col(BytePos(5), &line_starts), (1, 5));
        // Byte 10 = line 2, col 0
        assert_eq!(byte_pos_to_line_col(BytePos(10), &line_starts), (2, 0));
        // Byte 15 = line 2, col 5
        assert_eq!(byte_pos_to_line_col(BytePos(15), &line_starts), (2, 5));
        // Byte 20 = line 3, col 0
        assert_eq!(byte_pos_to_line_col(BytePos(20), &line_starts), (3, 0));
    }
}

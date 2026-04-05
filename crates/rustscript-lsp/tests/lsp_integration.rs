//! LSP integration tests (Phase 3).
//!
//! Tests the LSP diagnostic collection and position mapping without
//! starting the full LSP server. Verifies that compiler diagnostics
//! are correctly converted to LSP protocol types.

use rustscript_lsp::diagnostics::{collect_diagnostics, offset_to_position, span_to_range};
use rustscript_lsp::position_map::PositionMap;
use rustscript_syntax::span::Span;
use tower_lsp::lsp_types;

// ---------------------------------------------------------------------------
// 1. Error diagnostics produce LSP diagnostics
// ---------------------------------------------------------------------------

#[test]
fn test_lsp_integration_parse_error_produces_diagnostics() {
    let source = "function foo( { }";

    let diagnostics = collect_diagnostics(source);

    assert!(
        !diagnostics.is_empty(),
        "should produce at least one diagnostic for broken source"
    );

    let first = &diagnostics[0];
    assert_eq!(
        first.severity,
        Some(lsp_types::DiagnosticSeverity::ERROR),
        "parse error should be ERROR severity"
    );
    assert_eq!(
        first.source.as_deref(),
        Some("rsc"),
        "diagnostic source should be 'rsc'"
    );
}

// ---------------------------------------------------------------------------
// 2. Clean source produces no diagnostics
// ---------------------------------------------------------------------------

#[test]
fn test_lsp_integration_clean_source_no_diagnostics() {
    let source = "function foo() {}";
    let diagnostics = collect_diagnostics(source);

    assert!(
        diagnostics.is_empty(),
        "valid source should produce no diagnostics, got: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. Multi-line source: error position on correct line
// ---------------------------------------------------------------------------

#[test]
fn test_lsp_integration_multiline_error_position() {
    let source = "\
function foo() {
  const x = 1;
}

function bar( { }";

    let diagnostics = collect_diagnostics(source);

    assert!(
        !diagnostics.is_empty(),
        "should produce diagnostics for broken source"
    );

    // The error is on line 4 (0-indexed) where bar has broken syntax
    let has_error_on_later_line = diagnostics.iter().any(|d| d.range.start.line >= 3);
    assert!(
        has_error_on_later_line,
        "should have error on line >= 3, got lines: {:?}",
        diagnostics
            .iter()
            .map(|d| d.range.start.line)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// 4. Position map: compile and verify roundtrip
// ---------------------------------------------------------------------------

#[test]
fn test_lsp_integration_position_map_roundtrip() {
    let rts_source = "function add(a: i32, b: i32): i32 {\n  return a + b;\n}\n";
    let rs_source = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";

    let source_map = vec![
        Some(Span::new(0, 36)),  // .rs line 0 -> .rts line 0
        Some(Span::new(36, 52)), // .rs line 1 -> .rts line 1
        Some(Span::new(52, 54)), // .rs line 2 -> .rts line 2
    ];

    let map = PositionMap::new(source_map, rts_source.to_owned(), rs_source.to_owned());

    // Roundtrip: .rts line 1, col 2 -> .rs -> .rts
    let original = lsp_types::Position {
        line: 1,
        character: 2,
    };
    let rs_pos = map.rts_to_rs_position(original).expect("should map to .rs");
    let back = map
        .rs_to_rts_position(rs_pos)
        .expect("should map back to .rts");

    assert_eq!(back.line, original.line, "line should roundtrip");
    assert_eq!(
        back.character, original.character,
        "column should roundtrip"
    );
}

// ---------------------------------------------------------------------------
// 5. Position map: unmapped lines return None
// ---------------------------------------------------------------------------

#[test]
fn test_lsp_integration_position_map_unmapped_returns_none() {
    let source_map = vec![None, Some(Span::new(0, 10))];
    let map = PositionMap::new(source_map, "hello\nworld\n".to_owned(), "a\nb\n".to_owned());

    // Line 0 in .rs has no mapping
    let result = map.rs_to_rts_position(lsp_types::Position {
        line: 0,
        character: 0,
    });
    assert!(
        result.is_none(),
        "line with no source map entry should return None"
    );
}

// ---------------------------------------------------------------------------
// 6. Span to range conversion
// ---------------------------------------------------------------------------

#[test]
fn test_lsp_integration_span_to_range_multiline() {
    let source = "function foo() {\n  return 1;\n}\n";
    // "function foo() {\n" is 17 bytes (indices 0-16)
    // Line 1: offset 17 = ' ' (col 0), 18 = ' ' (col 1), 19 = 'r' (col 2)
    let span = Span::new(19, 28);

    let range = span_to_range(span, source);

    assert_eq!(range.start.line, 1, "start should be on line 1");
    assert_eq!(range.start.character, 2, "start column should be 2");
    assert_eq!(range.end.line, 1, "end should be on line 1");
}

// ---------------------------------------------------------------------------
// 7. Position conversion: offset to position for various offsets
// ---------------------------------------------------------------------------

#[test]
fn test_lsp_integration_offset_to_position_end_of_source() {
    let source = "abc\ndef";
    let pos = offset_to_position(7, source);
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 3);
}

// ---------------------------------------------------------------------------
// 8. URI translation roundtrip
// ---------------------------------------------------------------------------

#[test]
fn test_lsp_integration_uri_translation_roundtrip() {
    let map = PositionMap::new(vec![], String::new(), String::new());

    let rts_uri = tower_lsp::lsp_types::Url::parse("file:///project/src/main.rts").unwrap();
    let rs_uri = map
        .rts_to_rs_uri(&rts_uri)
        .expect("should translate to .rs URI");

    assert!(
        rs_uri.as_str().contains("/project/src/main.rs"),
        "should point to same directory with .rs extension, got: {rs_uri}"
    );

    let back = map
        .rs_to_rts_uri(&rs_uri)
        .expect("should translate back to .rts URI");
    assert!(
        back.as_str().contains("main.rts"),
        "should roundtrip to .rts, got: {back}"
    );
}

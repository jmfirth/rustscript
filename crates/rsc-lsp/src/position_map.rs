//! Bidirectional position mapping between `.rts` source and generated `.rs` files.
//!
//! Uses the line-level source map produced by the emitter to translate positions
//! between the original `RustScript` source and the generated Rust code. Translation
//! is approximate at line granularity — column positions are preserved as-is.

use std::collections::HashMap;

use rsc_syntax::source::compute_line_starts;
use rsc_syntax::span::Span;
use tower_lsp::lsp_types::{Position, Range, Url};

/// Bidirectional position map between `.rts` source lines and `.rs` generated lines.
///
/// Built from the emitter's source map (`Vec<Option<Span>>`), which maps each
/// generated `.rs` line to the `.rts` span it originated from. The reverse map
/// is computed at construction time.
pub struct PositionMap {
    /// Source map from emitter: `.rs` line index -> `.rts` span.
    rs_to_rts: Vec<Option<Span>>,
    /// Reverse map: `.rts` line -> `.rs` line (built from `rs_to_rts`).
    rts_to_rs: HashMap<u32, u32>,
    /// Original `.rts` source (for offset calculations).
    source_rts: String,
    /// Generated `.rs` source (for offset calculations).
    source_generated: String,
}

impl PositionMap {
    /// Create from an emitter source map.
    ///
    /// The `source_map` vector maps each generated `.rs` line (by index) to the
    /// `.rts` byte-offset span it originated from. The reverse map is built by
    /// converting each `.rts` span to a line number in the `.rts` source.
    #[must_use]
    pub fn new(
        source_map: Vec<Option<Span>>,
        rts_source: String,
        generated_source: String,
    ) -> Self {
        let mut rts_to_rs: HashMap<u32, u32> = HashMap::new();

        // Pre-compute the line start offsets for the .rts source.
        let rts_line_starts = compute_line_starts(&rts_source);

        for (rs_line_idx, maybe_span) in source_map.iter().enumerate() {
            if let Some(span) = maybe_span {
                let rts_line = offset_to_line(&rts_line_starts, span.start.0);
                #[allow(clippy::cast_possible_truncation)]
                let rs_line_u32 = rs_line_idx as u32;
                // First mapping wins — keep the earliest .rs line for each .rts line.
                rts_to_rs.entry(rts_line).or_insert(rs_line_u32);
            }
        }

        Self {
            rs_to_rts: source_map,
            rts_to_rs,
            source_rts: rts_source,
            source_generated: generated_source,
        }
    }

    /// Translate an `.rts` position to the corresponding `.rs` position.
    ///
    /// Returns `None` if the `.rts` line has no corresponding `.rs` line.
    /// Column is preserved as-is (line-level mapping).
    #[must_use]
    pub fn rts_to_rs_position(&self, pos: Position) -> Option<Position> {
        let target_rs_line = self.rts_to_rs.get(&pos.line)?;
        Some(Position {
            line: *target_rs_line,
            character: pos.character,
        })
    }

    /// Translate an `.rs` position back to the corresponding `.rts` position.
    ///
    /// Returns `None` if the `.rs` line has no `.rts` origin (compiler-generated).
    #[must_use]
    pub fn rs_to_rts_position(&self, pos: Position) -> Option<Position> {
        let idx = pos.line as usize;
        let span = self.rs_to_rts.get(idx).copied().flatten()?;

        let rts_line_starts = compute_line_starts(&self.source_rts);
        let target_rts_line = offset_to_line(&rts_line_starts, span.start.0);

        Some(Position {
            line: target_rts_line,
            character: pos.character,
        })
    }

    /// Translate an `.rs` range back to an `.rts` range.
    ///
    /// Translates both start and end positions independently. Returns `None`
    /// if either endpoint has no `.rts` mapping.
    #[must_use]
    pub fn rs_to_rts_range(&self, range: Range) -> Option<Range> {
        let start = self.rs_to_rts_position(range.start)?;
        let end = self.rs_to_rts_position(range.end)?;
        Some(Range { start, end })
    }

    /// Translate an `.rts` URI to the corresponding `.rs` URI (same directory).
    ///
    /// Converts `file:///path/to/file.rts` to `file:///path/to/file.rs`.
    /// In-place compilation means `.rs` files live alongside their `.rts` sources.
    #[must_use]
    pub fn rts_to_rs_uri(&self, uri: &Url) -> Option<Url> {
        let path = uri.to_file_path().ok()?;
        let rs_path = path.with_extension("rs");
        Url::from_file_path(rs_path).ok()
    }

    /// Translate an `.rs` URI back to the corresponding `.rts` URI (same directory).
    ///
    /// Converts `file:///path/to/file.rs` to `file:///path/to/file.rts`.
    /// In-place compilation means `.rs` files live alongside their `.rts` sources.
    #[must_use]
    pub fn rs_to_rts_uri(&self, uri: &Url) -> Option<Url> {
        let path = uri.to_file_path().ok()?;
        let rts_path = path.with_extension("rts");
        Url::from_file_path(rts_path).ok()
    }

    /// Access the `.rts` source text.
    #[must_use]
    pub fn rts_source(&self) -> &str {
        &self.source_rts
    }

    /// Access the generated `.rs` source text.
    #[must_use]
    pub fn rs_source(&self) -> &str {
        &self.source_generated
    }
}

/// Find the 0-based line number for a byte offset given pre-computed line starts.
fn offset_to_line(line_starts: &[u32], offset: u32) -> u32 {
    match line_starts.binary_search(&offset) {
        Ok(line) => {
            #[allow(clippy::cast_possible_truncation)]
            {
                line as u32
            }
        }
        Err(line) => {
            #[allow(clippy::cast_possible_truncation)]
            {
                (line.saturating_sub(1)) as u32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rts_source() -> String {
        // Lines:
        // 0: "function add(a: i32, b: i32): i32 {\n"
        // 1: "  return a + b;\n"
        // 2: "}\n"
        "function add(a: i32, b: i32): i32 {\n  return a + b;\n}\n".to_owned()
    }

    fn make_generated_source() -> String {
        // Lines:
        // 0: "fn add(a: i32, b: i32) -> i32 {\n"
        // 1: "    a + b\n"
        // 2: "}\n"
        "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n".to_owned()
    }

    fn make_source_map() -> Vec<Option<Span>> {
        // .rs line 0 -> .rts span covering line 0 (offset 0..36)
        // .rs line 1 -> .rts span covering line 1 (offset 36..52)
        // .rs line 2 -> .rts span covering line 2 (offset 52..54)
        vec![
            Some(Span::new(0, 36)),
            Some(Span::new(36, 52)),
            Some(Span::new(52, 54)),
        ]
    }

    // Test 1: Position translation .rts to .rs
    #[test]
    fn test_position_map_rts_to_rs_position() {
        let map = PositionMap::new(
            make_source_map(),
            make_rts_source(),
            make_generated_source(),
        );

        // .rts line 0, col 5 -> should map to .rs line 0, col 5
        let result = map.rts_to_rs_position(Position {
            line: 0,
            character: 5,
        });
        assert!(result.is_some());
        let pos = result.unwrap();
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 5);
    }

    // Test 2: Position translation .rs to .rts
    #[test]
    fn test_position_map_rs_to_rts_position() {
        let map = PositionMap::new(
            make_source_map(),
            make_rts_source(),
            make_generated_source(),
        );

        // .rs line 1, col 4 -> should map to .rts line 1
        let result = map.rs_to_rts_position(Position {
            line: 1,
            character: 4,
        });
        assert!(result.is_some());
        let pos = result.unwrap();
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 4);
    }

    // Test 3: Range translation
    #[test]
    fn test_position_map_rs_to_rts_range() {
        let map = PositionMap::new(
            make_source_map(),
            make_rts_source(),
            make_generated_source(),
        );

        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 2,
                character: 1,
            },
        };
        let result = map.rs_to_rts_range(range);
        assert!(result.is_some());
        let rts_range = result.unwrap();
        assert_eq!(rts_range.start.line, 0);
        assert_eq!(rts_range.end.line, 2);
    }

    // Test 4: URI translation .rts -> .rs (in-place, same directory)
    #[test]
    fn test_position_map_rts_to_rs_uri() {
        let map = PositionMap::new(vec![], String::new(), String::new());

        let rts_uri = Url::parse("file:///project/src/main.rts").unwrap();
        let rs_uri = map.rts_to_rs_uri(&rts_uri);
        assert!(rs_uri.is_some());
        let rs = rs_uri.unwrap();
        assert!(
            rs.as_str().contains("/project/src/main.rs"),
            "expected /project/src/main.rs in URI, got: {rs}"
        );
    }

    // Test 4b: URI translation .rts -> .rs for non-main files
    #[test]
    fn test_position_map_rts_to_rs_uri_utils() {
        let map = PositionMap::new(vec![], String::new(), String::new());

        let rts_uri = Url::parse("file:///project/src/utils.rts").unwrap();
        let rs_uri = map.rts_to_rs_uri(&rts_uri);
        assert!(rs_uri.is_some());
        let rs = rs_uri.unwrap();
        assert!(
            rs.as_str().contains("/project/src/utils.rs"),
            "expected /project/src/utils.rs in URI, got: {rs}"
        );
    }

    // Test 5: URI translation .rs -> .rts (in-place, same directory)
    #[test]
    fn test_position_map_rs_to_rts_uri() {
        let map = PositionMap::new(vec![], String::new(), String::new());

        let rs_uri = Url::parse("file:///project/src/main.rs").unwrap();
        let rts_uri = map.rs_to_rts_uri(&rs_uri);
        assert!(rts_uri.is_some());
        let rts = rts_uri.unwrap();
        assert!(
            rts.as_str().contains("/project/src/main.rts"),
            "expected /project/src/main.rts in URI, got: {rts}"
        );
    }

    // Test 5b: URI translation .rs -> .rts for non-main files
    #[test]
    fn test_position_map_rs_to_rts_uri_utils() {
        let map = PositionMap::new(vec![], String::new(), String::new());

        let rs_uri = Url::parse("file:///project/src/utils.rs").unwrap();
        let rts_uri = map.rs_to_rts_uri(&rs_uri);
        assert!(rts_uri.is_some());
        let rts = rts_uri.unwrap();
        assert!(
            rts.as_str().contains("/project/src/utils.rts"),
            "expected /project/src/utils.rts in URI, got: {rts}"
        );
    }

    // Test: URI roundtrip rts -> rs -> rts
    #[test]
    fn test_position_map_uri_roundtrip() {
        let map = PositionMap::new(vec![], String::new(), String::new());

        let original = Url::parse("file:///project/src/main.rts").unwrap();
        let rs_uri = map
            .rts_to_rs_uri(&original)
            .expect("should translate to .rs");
        let back = map
            .rs_to_rts_uri(&rs_uri)
            .expect("should translate back to .rts");
        assert_eq!(
            original.as_str(),
            back.as_str(),
            "URI should roundtrip: {original} -> {rs_uri} -> {back}"
        );
    }

    // Test 7: Position map construction produces valid bidirectional mappings
    #[test]
    fn test_position_map_construction_bidirectional() {
        let map = PositionMap::new(
            make_source_map(),
            make_rts_source(),
            make_generated_source(),
        );

        // Every .rts line that has a mapping should round-trip
        for rts_line in 0..3u32 {
            let pos = Position {
                line: rts_line,
                character: 0,
            };
            if let Some(rs_pos) = map.rts_to_rs_position(pos) {
                let back = map.rs_to_rts_position(rs_pos);
                assert!(back.is_some(), "round-trip failed for .rts line {rts_line}");
                assert_eq!(back.unwrap().line, rts_line);
            }
        }
    }

    // Test: Returns None for unmapped .rts line
    #[test]
    fn test_position_map_rts_to_rs_unmapped_returns_none() {
        let map = PositionMap::new(
            make_source_map(),
            make_rts_source(),
            make_generated_source(),
        );

        let result = map.rts_to_rs_position(Position {
            line: 99,
            character: 0,
        });
        assert!(result.is_none());
    }

    // Test: Returns None for .rs line with None in source map
    #[test]
    fn test_position_map_rs_to_rts_none_entry_returns_none() {
        let source_map = vec![None, Some(Span::new(0, 10))];
        let map = PositionMap::new(source_map, "hello\nworld\n".to_owned(), "a\nb\n".to_owned());

        let result = map.rs_to_rts_position(Position {
            line: 0,
            character: 0,
        });
        assert!(
            result.is_none(),
            "line with None source map entry should return None"
        );
    }

    // Correctness scenario 1: Position roundtrip
    #[test]
    fn test_position_map_correctness_position_roundtrip() {
        let map = PositionMap::new(
            make_source_map(),
            make_rts_source(),
            make_generated_source(),
        );

        // Translate .rts line 1, col 2 to .rs, then back to .rts
        let original = Position {
            line: 1,
            character: 2,
        };
        let rs_pos = map.rts_to_rs_position(original).expect("should map to .rs");
        let roundtrip = map
            .rs_to_rts_position(rs_pos)
            .expect("should map back to .rts");
        assert_eq!(roundtrip.line, original.line);
        assert_eq!(roundtrip.character, original.character);
    }

    // Correctness scenario 3: Definition response translation
    #[test]
    fn test_position_map_correctness_definition_response_translation() {
        // Simulate: rust-analyzer returns a definition at .rs line 0, col 3
        // and we translate it back to .rts
        let map = PositionMap::new(
            make_source_map(),
            make_rts_source(),
            make_generated_source(),
        );

        let rs_pos = Position {
            line: 0,
            character: 3,
        };
        let rts_pos = map
            .rs_to_rts_position(rs_pos)
            .expect("should translate back");
        assert_eq!(rts_pos.line, 0, "definition should be on .rts line 0");
        assert_eq!(rts_pos.character, 3, "column should be preserved");
    }

    // Test: compute_line_starts
    #[test]
    fn test_compute_line_starts() {
        let source = "abc\ndef\nghi";
        let starts = compute_line_starts(source);
        assert_eq!(starts, vec![0, 4, 8]);
    }

    // Test: offset_to_line
    #[test]
    fn test_offset_to_line_basic() {
        let starts = vec![0, 4, 8];
        assert_eq!(offset_to_line(&starts, 0), 0);
        assert_eq!(offset_to_line(&starts, 3), 0);
        assert_eq!(offset_to_line(&starts, 4), 1);
        assert_eq!(offset_to_line(&starts, 7), 1);
        assert_eq!(offset_to_line(&starts, 8), 2);
    }
}

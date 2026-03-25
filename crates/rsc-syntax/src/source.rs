//! Source file tracking and line/column resolution.
//!
//! Provides [`SourceFile`] for individual loaded source files with precomputed
//! line information, and [`SourceMap`] for managing all files in a compilation.

use crate::span::{BytePos, Span};

/// A unique identifier for a source file within a compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// A loaded source file with precomputed line start positions.
///
/// Created via [`SourceMap::add_file`] or directly with [`SourceFile::new`].
/// Stores the original source text and a precomputed index of line start
/// byte offsets for efficient line/column lookups.
pub struct SourceFile {
    id: FileId,
    name: String,
    source: String,
    /// Byte offsets of the start of each line. The first entry is always 0.
    line_starts: Vec<u32>,
}

impl SourceFile {
    /// Create a new source file, computing line start positions by scanning
    /// for newline characters.
    #[must_use]
    pub fn new(id: FileId, name: String, source: String) -> Self {
        let line_starts = compute_line_starts(&source);
        Self {
            id,
            name,
            source,
            line_starts,
        }
    }

    /// The unique identifier assigned to this file.
    #[must_use]
    pub fn id(&self) -> FileId {
        self.id
    }

    /// The display name of this file (typically a file path).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The full source text.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Convert a byte position to a `(line, column)` pair, both 0-based.
    ///
    /// If the position is beyond the end of the file, returns the last
    /// valid line and a column equal to the overshoot.
    #[must_use]
    pub fn line_col(&self, pos: BytePos) -> (usize, usize) {
        let offset = pos.0 as usize;
        // Binary search for the line containing this offset.
        // partition_point returns the first index where the predicate is false,
        // so we subtract 1 to get the line whose start is <= offset.
        let line = self
            .line_starts
            .partition_point(|&start| (start as usize) <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts.get(line).copied().unwrap_or(0) as usize;
        let col = offset - line_start;
        (line, col)
    }

    /// The number of lines in this source file.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// The byte offset of the start of the given line (0-based).
    ///
    /// Returns `None` if the line index is out of bounds.
    #[must_use]
    pub fn line_start(&self, line: usize) -> Option<BytePos> {
        self.line_starts.get(line).map(|&s| BytePos(s))
    }

    /// The text content of the given line (0-based), without the trailing newline.
    ///
    /// Returns `None` if the line index is out of bounds.
    #[must_use]
    pub fn line_source(&self, line: usize) -> Option<&str> {
        let start = *self.line_starts.get(line)? as usize;
        let end = self
            .line_starts
            .get(line + 1)
            .map_or(self.source.len(), |&s| s as usize);
        // Strip trailing newline if present.
        let text = &self.source[start..end];
        Some(text.trim_end_matches('\n').trim_end_matches('\r'))
    }

    /// Extract the source text covered by a span.
    ///
    /// # Panics
    ///
    /// Panics if the span extends beyond the source text. In library code
    /// this is only called with validated spans, so this is considered an
    /// internal invariant violation rather than a recoverable error.
    #[must_use]
    pub fn source_slice(&self, span: Span) -> &str {
        &self.source[span.start.0 as usize..span.end.0 as usize]
    }
}

/// Compute byte offsets of each line start. The first line always starts at 0.
///
/// This is used by `SourceMap::new()` and is also available for other crates
/// (e.g., the LSP position mapper) that need the same line-start index.
#[must_use]
pub fn compute_line_starts(source: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (i, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            // The next line starts at the byte after the newline.
            #[allow(clippy::cast_possible_truncation)]
            // Source files larger than 4 GiB are not supported.
            starts.push((i + 1) as u32);
        }
    }
    starts
}

/// Collection of all source files in a compilation.
///
/// Files are assigned sequential [`FileId`]s starting from 0.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Create an empty source map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a source file and return its assigned [`FileId`].
    pub fn add_file(&mut self, name: String, source: String) -> FileId {
        #[allow(clippy::cast_possible_truncation)]
        // More than 4 billion source files is not a realistic scenario.
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile::new(id, name, source));
        id
    }

    /// Retrieve a source file by its [`FileId`].
    ///
    /// Returns `None` if the id does not correspond to a loaded file.
    #[must_use]
    pub fn get_file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_file_new_computes_line_starts_multiline() {
        let src = "line one\nline two\nline three\n";
        let file = SourceFile::new(FileId(0), "test.rts".into(), src.into());
        // line 0 starts at 0, line 1 at 9, line 2 at 18, line 3 at 29 (trailing newline)
        assert_eq!(file.line_starts, vec![0, 9, 18, 29]);
        assert_eq!(file.line_count(), 4);
    }

    #[test]
    fn test_source_file_line_col_first_char() {
        let src = "hello\nworld\n";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        assert_eq!(file.line_col(BytePos(0)), (0, 0));
    }

    #[test]
    fn test_source_file_line_col_mid_line() {
        let src = "hello\nworld\n";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        // 'l' in "hello" is at offset 2, line 0, col 2
        assert_eq!(file.line_col(BytePos(2)), (0, 2));
    }

    #[test]
    fn test_source_file_line_col_line_boundary() {
        let src = "hello\nworld\n";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        // 'w' in "world" is at offset 6, line 1, col 0
        assert_eq!(file.line_col(BytePos(6)), (1, 0));
    }

    #[test]
    fn test_source_file_line_col_last_line() {
        let src = "hello\nworld\nfoo";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        // 'f' in "foo" at offset 12, line 2, col 0
        assert_eq!(file.line_col(BytePos(12)), (2, 0));
        // 'o' at offset 14, line 2, col 2
        assert_eq!(file.line_col(BytePos(14)), (2, 2));
    }

    #[test]
    fn test_source_file_line_col_single_line() {
        let src = "single line no newline";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        assert_eq!(file.line_col(BytePos(0)), (0, 0));
        assert_eq!(file.line_col(BytePos(7)), (0, 7));
    }

    #[test]
    fn test_source_file_line_col_empty_lines() {
        let src = "a\n\nb\n";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        // line 0: "a\n" starts at 0
        // line 1: "\n"  starts at 2 (empty line)
        // line 2: "b\n" starts at 3
        assert_eq!(file.line_col(BytePos(0)), (0, 0)); // 'a'
        assert_eq!(file.line_col(BytePos(2)), (1, 0)); // empty line start
        assert_eq!(file.line_col(BytePos(3)), (2, 0)); // 'b'
    }

    #[test]
    fn test_source_file_source_slice_extracts_correct_text() {
        let src = "let x = 42;\nlet y = 99;\n";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        // "x = 42" is at bytes 4..10
        assert_eq!(file.source_slice(Span::new(4, 10)), "x = 42");
    }

    #[test]
    fn test_source_file_line_source_returns_correct_text() {
        let src = "first\nsecond\nthird\n";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        assert_eq!(file.line_source(0), Some("first"));
        assert_eq!(file.line_source(1), Some("second"));
        assert_eq!(file.line_source(2), Some("third"));
    }

    #[test]
    fn test_source_file_line_source_out_of_bounds_returns_none() {
        let src = "one\ntwo\n";
        let file = SourceFile::new(FileId(0), "t.rts".into(), src.into());
        assert_eq!(file.line_source(99), None);
    }

    #[test]
    fn test_source_map_add_file_assigns_sequential_ids() {
        let mut sm = SourceMap::new();
        let id0 = sm.add_file("a.rts".into(), "a".into());
        let id1 = sm.add_file("b.rts".into(), "b".into());
        let id2 = sm.add_file("c.rts".into(), "c".into());
        assert_eq!(id0, FileId(0));
        assert_eq!(id1, FileId(1));
        assert_eq!(id2, FileId(2));
    }

    #[test]
    fn test_source_map_get_file_retrieves_correct_file() {
        let mut sm = SourceMap::new();
        let id = sm.add_file("test.rts".into(), "content".into());
        let file = sm.get_file(id);
        assert!(file.is_some());
        assert_eq!(file.unwrap().name(), "test.rts");
        assert_eq!(file.unwrap().source(), "content");
    }

    #[test]
    fn test_source_map_get_file_invalid_id_returns_none() {
        let sm = SourceMap::new();
        assert!(sm.get_file(FileId(0)).is_none());
        assert!(sm.get_file(FileId(999)).is_none());
    }
}

//! Source span types for tracking byte positions within source files.

/// A byte offset into a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BytePos(pub u32);

/// A span of source code — a half-open byte range `[start, end)`.
///
/// Used to track the origin of every AST node, diagnostic label, and
/// compiler-generated construct back to its position in the original
/// `.rts` source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// The inclusive start byte offset.
    pub start: BytePos,
    /// The exclusive end byte offset.
    pub end: BytePos,
}

impl Span {
    /// Create a new span from raw byte offsets.
    #[must_use]
    pub fn new(start: u32, end: u32) -> Self {
        Self {
            start: BytePos(start),
            end: BytePos(end),
        }
    }

    /// Create a sentinel span for compiler-generated nodes that have no
    /// meaningful source location.
    #[must_use]
    pub fn dummy() -> Self {
        Self {
            start: BytePos(0),
            end: BytePos(0),
        }
    }

    /// Merge two spans into one that covers both, taking the minimum start
    /// and maximum end.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        Self {
            start: if self.start.0 <= other.start.0 {
                self.start
            } else {
                other.start
            },
            end: if self.end.0 >= other.end.0 {
                self.end
            } else {
                other.end
            },
        }
    }

    /// Check whether a byte position falls within this span (half-open:
    /// the end position is excluded).
    #[must_use]
    pub fn contains(self, pos: BytePos) -> bool {
        pos.0 >= self.start.0 && pos.0 < self.end.0
    }

    /// The byte length of this span.
    #[must_use]
    pub fn len(self) -> u32 {
        self.end.0 - self.start.0
    }

    /// Whether this span has zero length (not the same as `is_dummy` — a
    /// zero-length span could be at any position).
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Whether this is a dummy span (start and end are both zero).
    #[must_use]
    pub fn is_dummy(self) -> bool {
        self.start.0 == 0 && self.end.0 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_new_creates_correct_start_end() {
        let span = Span::new(0, 5);
        assert_eq!(span.start, BytePos(0));
        assert_eq!(span.end, BytePos(5));
    }

    #[test]
    fn test_span_dummy_is_dummy_returns_true() {
        let span = Span::dummy();
        assert!(span.is_dummy());
        assert_eq!(span.start, BytePos(0));
        assert_eq!(span.end, BytePos(0));
    }

    #[test]
    fn test_span_non_dummy_is_dummy_returns_false() {
        let span = Span::new(1, 5);
        assert!(!span.is_dummy());
    }

    #[test]
    fn test_span_merge_non_overlapping_covers_both() {
        let a = Span::new(0, 5);
        let b = Span::new(10, 15);
        let merged = a.merge(b);
        assert_eq!(merged.start, BytePos(0));
        assert_eq!(merged.end, BytePos(15));
    }

    #[test]
    fn test_span_merge_overlapping() {
        let a = Span::new(3, 10);
        let b = Span::new(7, 20);
        let merged = a.merge(b);
        assert_eq!(merged.start, BytePos(3));
        assert_eq!(merged.end, BytePos(20));
    }

    #[test]
    fn test_span_contains_inside_returns_true() {
        let span = Span::new(5, 10);
        assert!(span.contains(BytePos(5)));
        assert!(span.contains(BytePos(7)));
        assert!(span.contains(BytePos(9)));
    }

    #[test]
    fn test_span_contains_outside_returns_false() {
        let span = Span::new(5, 10);
        assert!(!span.contains(BytePos(4)));
        assert!(!span.contains(BytePos(11)));
    }

    #[test]
    fn test_span_contains_end_excluded() {
        let span = Span::new(5, 10);
        assert!(!span.contains(BytePos(10)));
    }

    #[test]
    fn test_span_len_returns_byte_length() {
        let span = Span::new(3, 10);
        assert_eq!(span.len(), 7);
    }

    #[test]
    fn test_span_len_dummy_is_zero() {
        assert_eq!(Span::dummy().len(), 0);
    }
}

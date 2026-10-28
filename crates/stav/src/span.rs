//! Byte ranges into source text.

use std::ops::Range;

/// A half-open byte range `start..end` into a source text.
///
/// Both offsets always fall on UTF-8 character boundaries of the text the
/// span was produced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    /// Offset of the first byte.
    pub start: usize,
    /// Offset one past the last byte.
    pub end: usize,
}

impl Span {
    /// Returns the span as a range, suitable for slicing the source text.
    pub fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

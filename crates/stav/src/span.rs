//! Byte ranges into source text.

use std::ops::Range;

use crate::check::FileId;

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

/// A span in a particular file of a checked program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceSpan {
    /// The file the span is in.
    pub file: FileId,
    /// The bytes of the file the span covers.
    pub span: Span,
}

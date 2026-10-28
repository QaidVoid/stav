//! Errors reported about source text.

use crate::span::Span;

/// An error found in source text.
///
/// Every diagnostic has a stable code such as `E0105` that keeps its meaning
/// across releases, the span of the offending text and a message that says
/// what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The stable error code, for example `E0105`.
    pub code: &'static str,
    /// What is wrong, starting lowercase and without a trailing period.
    pub message: String,
    /// The source text the error is about.
    pub span: Span,
    /// A suggested fix, when one can be given.
    pub help: Option<String>,
}

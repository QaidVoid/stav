//! Helpers shared by the formatter tests. The integration tests declare
//! this module with `mod support`, and the unit tests of the formatter
//! include it by path, so both use one copy. The including module must
//! bring `parse`, `SyntaxElement` and `TokenKind` into scope.

use super::{SyntaxElement, TokenKind, parse};

/// Returns the nodes and tokens of `source` in order, and its comments in
/// order. Whitespace and line feeds are left out, and so are the separators
/// the formatter may add or remove: commas and the `|` before the first
/// variant of a sum type. Formatting must keep both lists unchanged.
pub(super) fn meaning(source: &str) -> (Vec<String>, Vec<String>) {
    let parsed = parse(source);
    let mut tree = Vec::new();
    let mut comments = Vec::new();
    let mut stack = vec![SyntaxElement::Node(parsed.tree.root())];
    while let Some(element) = stack.pop() {
        match element {
            SyntaxElement::Node(node) => {
                tree.push(format!("{:?}", node.kind()));
                let children: Vec<_> = node.children_with_tokens().collect();
                stack.extend(children.into_iter().rev());
            }
            SyntaxElement::Token(token) => match token.kind() {
                TokenKind::Comment | TokenKind::DocComment => {
                    comments.push(token.text().trim_end().to_owned());
                }
                TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comma => {}
                TokenKind::Pipe if tree.last().is_some_and(|last| last == "=") => {}
                _ => tree.push(token.text().to_owned()),
            },
        }
    }
    (tree, comments)
}

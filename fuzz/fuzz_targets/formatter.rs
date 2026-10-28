//! Formats arbitrary UTF-8 and checks that formatting never panics, leaves
//! sources with errors alone, and, for valid sources, produces valid text
//! with the same meaning and comments that formats to itself.

#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use stav::{SyntaxElement, TokenKind};

fuzz_target!(|source: &str| {
    let Ok(formatted) = stav::format(source) else {
        assert!(!stav::parse(source).diagnostics.is_empty());
        return;
    };
    assert!(stav::parse(&formatted).diagnostics.is_empty(), "{formatted}");
    assert_eq!(meaning(source), meaning(&formatted), "{formatted}");
    assert_eq!(stav::format(&formatted).as_deref(), Ok(formatted.as_str()));
});

/// Returns the nodes and tokens of `source` without layout, commas and the
/// leading `|` of sum types, and its comments.
fn meaning(source: &str) -> (Vec<String>, Vec<String>) {
    let parsed = stav::parse(source);
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

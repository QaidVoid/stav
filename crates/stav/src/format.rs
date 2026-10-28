//! Formatting source text into its canonical layout.
//!
//! The formatter walks the lossless syntax tree and describes the layout as
//! a document of text, line breaks and groups, which the printer in
//! [`doc`] lays out within the line width.
//!
//! # Comments
//!
//! A comment on its own line stays on its own line before the element that
//! follows it. A comment at the end of a line stays at the end of the line
//! of the element before it. Inside an expression, a comment ends its line
//! where it stands and the expression continues on the next line. Every
//! comment is followed by a line break, as in the source, so the tokens
//! around it keep their meaning.

mod doc;

use doc::{Doc, column, flat_width, group, if_break, if_flat, indent};

use crate::diagnostic::Diagnostic;
use crate::parser::parse;
use crate::syntax::{NodeKind as N, SyntaxElement, SyntaxNode, SyntaxToken};
use crate::token::TokenKind as T;

/// The longest key that a run of attributes, fields, entries, bindings or
/// arms still aligns on.
const MAX_ALIGNED_KEY: usize = 24;

/// Formats `source` into the canonical layout.
///
/// The layout indents by two spaces and puts one statement, field, variant
/// or arm on each line. Lists, maps, argument lists and parameter lists stay
/// on one line when they fit in 100 columns, and otherwise put one element
/// on each line with a trailing comma. Runs of consecutive attributes, map
/// entries, `let` bindings and `match` arms align their `=` or `=>`, and
/// runs of struct fields align their types, unless the longest key of the
/// run is longer than 24 characters. Bodies, struct declarations and sum
/// types that are written on one line stay on one line when they fit.
///
/// Formatting never changes what the source means and never reorders
/// anything. Comments are kept, blank lines between elements are kept and
/// collapsed to one, and strings are kept byte for byte, including
/// multi-line strings and the expressions they interpolate. Formatting the
/// result again returns it unchanged.
///
/// # Errors
///
/// Returns the lexical and syntax errors of `source` when it has any. A
/// source with errors is not formatted.
///
/// # Examples
///
/// ```
/// let source = "system {\n  hostname=\"forge\"\n  init = runit, timezone = \"UTC\"\n}\n";
/// let formatted = stav::format(source).expect("the source has no errors");
/// assert_eq!(
///     formatted,
///     "system {\n  hostname = \"forge\"\n  init     = runit\n  timezone = \"UTC\"\n}\n",
/// );
/// ```
pub fn format(source: &str) -> Result<String, Vec<Diagnostic>> {
    let parsed = parse(source);
    if !parsed.diagnostics.is_empty() {
        return Err(parsed.diagnostics);
    }
    Ok(doc::print(&file(parsed.tree.root())))
}

/// A child of a node that the layout cares about. Whitespace and line feeds
/// are dropped, because the formatter decides them.
#[derive(Clone, Copy)]
enum Part<'a> {
    Token(SyntaxToken<'a>),
    Node(SyntaxNode<'a>),
    Comment(&'a str),
}

fn parts_of<'a>(elements: impl IntoIterator<Item = SyntaxElement<'a>>) -> Vec<Part<'a>> {
    elements
        .into_iter()
        .filter_map(|element| match element {
            SyntaxElement::Node(node) => Some(Part::Node(node)),
            SyntaxElement::Token(token) => match token.kind() {
                T::Whitespace | T::Newline => None,
                T::Comment | T::DocComment => Some(Part::Comment(comment(token))),
                _ => Some(Part::Token(token)),
            },
        })
        .collect()
}

fn parts(node: SyntaxNode<'_>) -> Vec<Part<'_>> {
    parts_of(node.children_with_tokens())
}

fn comment(token: SyntaxToken<'_>) -> &str {
    token.text().trim_end()
}

fn is_token(part: Part<'_>, kind: T) -> bool {
    matches!(part, Part::Token(token) if token.kind() == kind)
}

/// Formats any node.
fn any(node: SyntaxNode<'_>) -> Doc<'_> {
    match node.kind() {
        N::File => file(node),
        N::Str | N::Interp => Doc::Text(node.text()),
        N::Struct => struct_decl(node),
        N::TypeDecl => type_decl(node),
        N::FnDecl => fn_decl(node),
        N::Body => braced(node, "{ }", false, false),
        N::MatchExpr => braced(node, "{}", true, false),
        N::List => listed(node, T::LBracket, T::RBracket, false),
        N::Map => listed(node, T::LBrace, T::RBrace, true),
        N::ArgList | N::ParamList => listed(node, T::LParen, T::RParen, false),
        N::IfExpr => {
            let mut docs = Vec::new();
            if_chain(node, &mut docs);
            group(docs)
        }
        N::LetExpr => let_expr(node),
        N::Attr | N::Field | N::MapEntry | N::Binding | N::MatchArm => item(node).doc(0),
        _ => inline(node),
    }
}

/// Formats a node whose parts all go on one line, separated by single
/// spaces where the grammar wants them.
fn inline(node: SyntaxNode<'_>) -> Doc<'_> {
    Doc::Concat(inline_docs(node.kind(), &parts(node)))
}

/// Formats `parts` on one line. A comment ends the line where it stands, and
/// the parts after it continue on the next line, indented one level. The
/// source had a line feed after the comment too, so the line break is valid
/// there.
fn inline_docs<'a>(parent: N, parts: &[Part<'a>]) -> Vec<Doc<'a>> {
    inline_docs_then(parent, parts, None)
}

/// Formats `parts` like [`inline_docs`], followed by `tail` after a space,
/// or at the start of the continuation line when a comment comes last.
fn inline_docs_then<'a>(parent: N, parts: &[Part<'a>], tail: Option<Doc<'a>>) -> Vec<Doc<'a>> {
    let mut docs = Vec::new();
    let mut continuation: Option<Vec<Doc<'a>>> = None;
    let mut previous = None;
    for (index, &part) in parts.iter().enumerate() {
        let out = continuation.as_mut().unwrap_or(&mut docs);
        let doc = match part {
            Part::Comment(text) => {
                let follows_comment = index > 0 && matches!(parts[index - 1], Part::Comment(_));
                out.push(if follows_comment {
                    Doc::Text(text)
                } else {
                    Doc::LineSuffix(text)
                });
                continuation
                    .get_or_insert_with(Vec::new)
                    .push(Doc::HardLine);
                previous = None;
                continue;
            }
            Part::Token(token) if token.kind() == T::Comma && closes_after(&parts[index + 1..]) => {
                continue;
            }
            Part::Token(token) => Doc::Text(token.text()),
            Part::Node(node) => any(node),
        };
        if previous.is_some_and(|previous| spaced(parent, previous, part)) {
            out.push(Doc::Spaces(1));
        }
        out.push(doc);
        previous = Some(part);
    }
    if let Some(tail) = tail {
        let out = continuation.as_mut().unwrap_or(&mut docs);
        if previous.is_some() {
            out.push(Doc::Spaces(1));
        }
        out.push(tail);
    }
    docs.extend(continuation.map(indent));
    docs
}

/// Returns whether the next part that is not a comment closes a delimited
/// list, which makes a comma before it a trailing comma.
fn closes_after(parts: &[Part<'_>]) -> bool {
    parts
        .iter()
        .find(|part| !matches!(part, Part::Comment(_)))
        .is_some_and(|&part| {
            is_token(part, T::RParen) || is_token(part, T::RBracket) || is_token(part, T::Gt)
        })
}

/// Returns whether a space separates two adjacent parts of a `parent` node.
fn spaced(parent: N, previous: Part<'_>, next: Part<'_>) -> bool {
    if matches!(parent, N::UnaryExpr | N::RangePredicate) {
        return false;
    }
    match next {
        Part::Token(token) => match token.kind() {
            T::Comma | T::Colon | T::LParen | T::RParen | T::RBracket => return false,
            T::Dot | T::QuestionDot => return ends_in_dotted_word(previous),
            T::Gt if parent == N::TypeArgList => return false,
            T::LBracket if parent == N::IndexExpr => return false,
            T::RBrace if parent == N::MapComp => return false,
            _ => {}
        },
        Part::Node(node) => {
            if matches!(node.kind(), N::ArgList | N::ParamList | N::TypeArgList) {
                return false;
            }
        }
        Part::Comment(_) => {}
    }
    match previous {
        Part::Token(token) => match token.kind() {
            T::LParen | T::LBracket | T::Dot | T::QuestionDot => false,
            T::Lt => parent != N::TypeArgList,
            T::LBrace => parent != N::MapComp,
            _ => true,
        },
        _ => true,
    }
}

/// Returns whether `part` ends in a path or a symbol word with a version.
/// Both may contain `.`, so a field access after them needs a space to keep
/// the `.` out of the word.
fn ends_in_dotted_word(part: Part<'_>) -> bool {
    let last = match part {
        Part::Token(token) => Some(token),
        Part::Node(node) => last_token(node),
        Part::Comment(_) => None,
    };
    last.is_some_and(|token| matches!(token.kind(), T::Path | T::SymWord))
}

fn last_token(mut node: SyntaxNode<'_>) -> Option<SyntaxToken<'_>> {
    loop {
        match node.children_with_tokens().last()? {
            SyntaxElement::Token(token) => return Some(token),
            SyntaxElement::Node(child) => node = child,
        }
    }
}

/// An element of a sequence, split into a key and the rest when its kind
/// takes part in alignment.
struct Item<'a> {
    key: Vec<Doc<'a>>,
    rest: Option<Vec<Doc<'a>>>,
}

impl<'a> Item<'a> {
    /// Returns the width of the key when the item can be aligned.
    fn key_width(&self) -> Option<usize> {
        self.rest.as_ref().and_then(|_| flat_width(&self.key))
    }

    /// Returns the item with `pad` spaces after its key when the enclosing
    /// group breaks.
    fn doc(self, pad: usize) -> Doc<'a> {
        let mut docs = self.key;
        if let Some(rest) = self.rest {
            if pad > 0 {
                docs.push(if_break(Doc::Spaces(pad)));
            }
            docs.push(Doc::Spaces(1));
            docs.extend(rest);
        }
        Doc::Concat(docs)
    }
}

/// Formats an element of a sequence. Attributes, map entries and bindings
/// align on `=`, match arms on `=>`, and fields on the type after `:`.
fn item(node: SyntaxNode<'_>) -> Item<'_> {
    let separator = match node.kind() {
        N::Attr | N::MapEntry | N::Binding => Some((T::Eq, 0)),
        N::Field => Some((T::Colon, 1)),
        N::MatchArm => Some((T::FatArrow, 0)),
        _ => None,
    };
    let parts = parts(node);
    if let Some((kind, offset)) = separator
        && let Some(at) = parts.iter().position(|&part| is_token(part, kind))
    {
        let split = at + offset;
        return Item {
            key: inline_docs(node.kind(), &parts[..split]),
            rest: Some(inline_docs(node.kind(), &parts[split..])),
        };
    }
    Item {
        key: vec![any(node)],
        rest: None,
    }
}

/// The elements of a sequence such as a body, a list or the arms of a
/// `match`, with the comments and blank lines between them.
struct Seq<'a> {
    /// A comment at the end of the line of the opening delimiter.
    head: Option<&'a str>,
    entries: Vec<Entry<'a>>,
    /// Whether the source text of the elements spans several lines.
    broken: bool,
}

struct Entry<'a> {
    /// Whether a blank line comes before the entry.
    blank: bool,
    kind: EntryKind<'a>,
}

enum EntryKind<'a> {
    Item {
        item: Item<'a>,
        /// A comment at the end of the item's line.
        trailing: Option<&'a str>,
    },
    /// A comment on its own line.
    Comment { text: &'a str, doc: bool },
}

/// Collects the entries of a sequence from the elements between its
/// delimiters. `after_opener` tells whether a comment before the first line
/// feed belongs to the line of an opening delimiter.
fn scan<'a>(elements: impl IntoIterator<Item = SyntaxElement<'a>>, after_opener: bool) -> Seq<'a> {
    let mut seq = Seq {
        head: None,
        entries: Vec::new(),
        broken: false,
    };
    let mut newlines = 0;
    let mut on_line = after_opener;
    for element in elements {
        seq.broken |= element.text().contains('\n');
        let kind = match element {
            SyntaxElement::Node(node) => EntryKind::Item {
                item: item(node),
                trailing: None,
            },
            SyntaxElement::Token(token) => match token.kind() {
                T::Whitespace | T::Comma | T::Pipe => continue,
                T::Newline => {
                    newlines += 1;
                    on_line = false;
                    continue;
                }
                T::Comment | T::DocComment => {
                    let text = comment(token);
                    if on_line {
                        match seq.entries.last_mut() {
                            Some(Entry {
                                kind:
                                    EntryKind::Item {
                                        trailing: slot @ None,
                                        ..
                                    },
                                ..
                            }) => {
                                *slot = Some(text);
                                continue;
                            }
                            None if seq.head.is_none() => {
                                seq.head = Some(text);
                                continue;
                            }
                            _ => {}
                        }
                    }
                    EntryKind::Comment {
                        text,
                        doc: token.kind() == T::DocComment,
                    }
                }
                _ => EntryKind::Item {
                    item: Item {
                        key: vec![Doc::Text(token.text())],
                        rest: None,
                    },
                    trailing: None,
                },
            },
        };
        let blank = newlines >= 2 && !seq.entries.is_empty();
        seq.entries.push(Entry { blank, kind });
        newlines = 0;
        on_line = true;
    }
    seq
}

/// Returns the padding of each entry that aligns runs of items. A run ends
/// at a blank line, at a comment, unless it is a doc comment and
/// `doc_comments_join` is set, and at an item that does not align.
fn pads(entries: &[Entry<'_>], doc_comments_join: bool) -> Vec<usize> {
    let mut pads = vec![0; entries.len()];
    let mut run = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.blank {
            close_run(&mut run, &mut pads);
        }
        match &entry.kind {
            EntryKind::Item { item, .. } => match item.key_width() {
                Some(width) => run.push((index, width)),
                None => close_run(&mut run, &mut pads),
            },
            EntryKind::Comment { doc: true, .. } if doc_comments_join => {}
            EntryKind::Comment { .. } => close_run(&mut run, &mut pads),
        }
    }
    close_run(&mut run, &mut pads);
    pads
}

fn close_run(run: &mut Vec<(usize, usize)>, pads: &mut [usize]) {
    if let Some(longest) = run.iter().map(|&(_, width)| width).max()
        && longest <= MAX_ALIGNED_KEY
    {
        for &(index, width) in run.iter() {
            pads[index] = longest - width;
        }
    }
    run.clear();
}

fn last_item(entries: &[Entry<'_>]) -> Option<usize> {
    entries
        .iter()
        .rposition(|entry| matches!(entry.kind, EntryKind::Item { .. }))
}

/// The children of a node before, between and after a pair of delimiters.
struct Delimited<'a> {
    before: Vec<SyntaxElement<'a>>,
    inner: Vec<SyntaxElement<'a>>,
    after: Vec<SyntaxElement<'a>>,
}

/// Splits the children of `node` at its first `open` and last `close`
/// token.
fn delimited(node: SyntaxNode<'_>, open: T, close: T) -> Option<Delimited<'_>> {
    let is = |element: &SyntaxElement<'_>, kind| matches!(element, SyntaxElement::Token(token) if token.kind() == kind);
    let children: Vec<_> = node.children_with_tokens().collect();
    let start = children.iter().position(|element| is(element, open))?;
    let end = children.iter().rposition(|element| is(element, close))?;
    (start < end).then(|| Delimited {
        before: children[..start].to_vec(),
        inner: children[start + 1..end].to_vec(),
        after: children[end + 1..].to_vec(),
    })
}

/// Formats a file: one item per line.
fn file(node: SyntaxNode<'_>) -> Doc<'_> {
    let seq = scan(node.children_with_tokens(), false);
    let pads = pads(&seq.entries, false);
    let mut docs = Vec::new();
    for (index, (entry, pad)) in seq.entries.into_iter().zip(pads).enumerate() {
        if index > 0 {
            docs.push(Doc::HardLine);
            if entry.blank {
                docs.push(Doc::HardLine);
            }
        }
        push_entry(&mut docs, entry.kind, pad);
    }
    Doc::Concat(docs)
}

fn push_entry<'a>(docs: &mut Vec<Doc<'a>>, kind: EntryKind<'a>, pad: usize) {
    match kind {
        EntryKind::Item { item, trailing } => {
            docs.push(item.doc(pad));
            docs.extend(trailing.map(Doc::LineSuffix));
        }
        EntryKind::Comment { text, .. } => {
            docs.push(Doc::Text(text));
            docs.push(Doc::BreakParent);
        }
    }
}

/// Formats a node that ends in a sequence of statements, fields or arms in
/// braces. The sequence stays on one line, separated by commas, when it
/// fits and its source text is on one line, unless `always_break` is set.
fn braced<'a>(node: SyntaxNode<'a>, empty: &'a str, always_break: bool, fields: bool) -> Doc<'a> {
    let Some(Delimited {
        before,
        inner,
        after,
    }) = delimited(node, T::LBrace, T::RBrace)
    else {
        return inline(node);
    };
    let seq = scan(inner, true);
    let braces = if seq.head.is_none() && seq.entries.is_empty() {
        Doc::Text(empty)
    } else {
        let pads = pads(&seq.entries, fields);
        let last = last_item(&seq.entries);
        let mut lines = Vec::new();
        for (index, (entry, pad)) in seq.entries.into_iter().zip(pads).enumerate() {
            if index > 0 && entry.blank {
                lines.push(if_break(Doc::Line));
            }
            lines.push(Doc::Line);
            match entry.kind {
                EntryKind::Item { item, trailing } => {
                    lines.push(item.doc(pad));
                    if last.is_some_and(|last| index < last) {
                        lines.push(if_flat(Doc::Text(",")));
                    }
                    lines.extend(trailing.map(Doc::LineSuffix));
                }
                kind @ EntryKind::Comment { .. } => push_entry(&mut lines, kind, pad),
            }
        }
        let mut inner = vec![Doc::Text("{")];
        if let Some(text) = seq.head {
            inner.push(Doc::LineSuffix(text));
        }
        inner.push(indent(lines));
        inner.push(Doc::Line);
        inner.push(Doc::Text("}"));
        if always_break || seq.broken {
            inner.push(Doc::BreakParent);
        }
        group(inner)
    };
    let mut docs = inline_docs_then(node.kind(), &parts_of(before), Some(braces));
    docs.extend(inline_docs(node.kind(), &parts_of(after)));
    Doc::Concat(docs)
}

fn struct_decl(node: SyntaxNode<'_>) -> Doc<'_> {
    braced(node, "{}", false, true)
}

/// Formats a list, a map, an argument list or a parameter list. It stays on
/// one line when it fits, and otherwise puts each element on its own line
/// with a trailing comma. On one line, `padded` puts spaces inside the
/// delimiters.
fn listed(node: SyntaxNode<'_>, open: T, close: T, padded: bool) -> Doc<'_> {
    let Some(Delimited {
        before,
        inner,
        after,
    }) = delimited(node, open, close)
    else {
        return inline(node);
    };
    let (open, close) = match open {
        T::LBracket => ("[", "]"),
        T::LBrace => ("{", "}"),
        _ => ("(", ")"),
    };
    let end_line = || if padded { Doc::Line } else { Doc::SoftLine };
    let mut docs = inline_docs(node.kind(), &parts_of(before));
    let seq = scan(inner, true);
    if seq.head.is_none() && seq.entries.is_empty() {
        docs.push(Doc::Text(open));
        docs.push(Doc::Text(close));
    } else {
        let pads = pads(&seq.entries, false);
        let last = last_item(&seq.entries);
        let mut lines = Vec::new();
        for (index, (entry, pad)) in seq.entries.into_iter().zip(pads).enumerate() {
            if index > 0 && entry.blank {
                lines.push(if_break(Doc::Line));
            }
            lines.push(if index == 0 { end_line() } else { Doc::Line });
            match entry.kind {
                EntryKind::Item { item, trailing } => {
                    lines.push(item.doc(pad));
                    let comma = Doc::Text(",");
                    lines.push(if Some(index) == last {
                        if_break(comma)
                    } else {
                        comma
                    });
                    lines.extend(trailing.map(Doc::LineSuffix));
                }
                kind @ EntryKind::Comment { .. } => push_entry(&mut lines, kind, pad),
            }
        }
        let mut inner = vec![Doc::Text(open)];
        if let Some(text) = seq.head {
            inner.push(Doc::LineSuffix(text));
        }
        inner.push(indent(lines));
        inner.push(end_line());
        inner.push(Doc::Text(close));
        docs.push(group(inner));
    }
    docs.extend(inline_docs(node.kind(), &parts_of(after)));
    Doc::Concat(docs)
}

/// Formats a sum type. The variants stay on one line when they fit and their
/// source text is on one line, and otherwise go one per line after `|`.
fn type_decl(node: SyntaxNode<'_>) -> Doc<'_> {
    let children: Vec<_> = node.children_with_tokens().collect();
    let Some(eq) = children.iter().position(
        |element| matches!(element, SyntaxElement::Token(token) if token.kind() == T::Eq),
    ) else {
        return inline(node);
    };
    let mut docs = inline_docs(N::TypeDecl, &parts_of(children[..=eq].iter().copied()));
    let seq = scan(children[eq + 1..].iter().copied(), true);
    if let Some(text) = seq.head {
        docs.push(Doc::LineSuffix(text));
    }
    let mut lines = Vec::new();
    let mut first = true;
    for (index, entry) in seq.entries.into_iter().enumerate() {
        if index > 0 && entry.blank {
            lines.push(if_break(Doc::Line));
        }
        lines.push(Doc::Line);
        if matches!(entry.kind, EntryKind::Item { .. }) {
            let pipe = Doc::Text("| ");
            lines.push(if first { if_break(pipe) } else { pipe });
            first = false;
        }
        push_entry(&mut lines, entry.kind, 0);
    }
    docs.push(indent(lines));
    if seq.broken {
        docs.push(Doc::BreakParent);
    }
    group(docs)
}

/// Formats a function declaration. A `match` or `part` body starts on the
/// line of `=` unless a comment comes before it. Any other body moves to the
/// next line when the declaration does not fit, when the source put it
/// there or when a comment on its own line comes before it.
fn fn_decl(node: SyntaxNode<'_>) -> Doc<'_> {
    let parts = parts(node);
    let Some(eq) = parts.iter().position(|&part| is_token(part, T::Eq)) else {
        return inline(node);
    };
    let mut docs = inline_docs(N::FnDecl, &parts[..=eq]);
    let mut commented = false;
    let mut lines = vec![Doc::Line];
    let mut own_line = false;
    let mut body = Vec::new();
    let after_eq = node
        .children_with_tokens()
        .skip_while(
            |element| !matches!(element, SyntaxElement::Token(token) if token.kind() == T::Eq),
        )
        .skip(1);
    for element in after_eq {
        match element {
            SyntaxElement::Token(token) if body.is_empty() => match token.kind() {
                T::Whitespace => {}
                T::Newline => own_line = true,
                T::Comment | T::DocComment => {
                    commented = true;
                    if own_line {
                        lines.push(Doc::Text(comment(token)));
                        lines.push(Doc::HardLine);
                    } else {
                        docs.push(Doc::LineSuffix(comment(token)));
                    }
                }
                _ => body.push(Part::Token(token)),
            },
            element => body.extend(parts_of([element])),
        }
    }
    let hugs = !commented
        && matches!(
            body.as_slice(),
            [Part::Node(node)] if matches!(node.kind(), N::MatchExpr | N::PartExpr)
        );
    let body = inline_docs(N::FnDecl, &body);
    if hugs {
        docs.push(Doc::Spaces(1));
        docs.extend(body);
        return Doc::Concat(docs);
    }
    lines.extend(body);
    docs.push(indent(lines));
    if line_feed_after(node, T::Eq) {
        docs.push(Doc::BreakParent);
    }
    group(docs)
}

/// Returns whether a line feed directly follows the first `kind` token among
/// the children of `node`, before the next node.
fn line_feed_after(node: SyntaxNode<'_>, kind: T) -> bool {
    node.children_with_tokens()
        .skip_while(
            |element| !matches!(element, SyntaxElement::Token(token) if token.kind() == kind),
        )
        .skip(1)
        .map_while(|element| match element {
            SyntaxElement::Token(token) => Some(token.kind()),
            SyntaxElement::Node(_) => None,
        })
        .any(|kind| kind == T::Newline)
}

/// Formats an `if` expression and the `else if` expressions chained to it
/// into `docs`, as one group. A comment between the parts of the chain ends
/// its line, and the next part starts the following line.
fn if_chain<'a>(node: SyntaxNode<'a>, docs: &mut Vec<Doc<'a>>) {
    let mut branch: Option<Vec<Doc<'a>>> = None;
    let mut after_comment = false;
    let gap = |docs: &mut Vec<Doc<'a>>, after_comment: &mut bool| {
        docs.push(if std::mem::take(after_comment) {
            Doc::HardLine
        } else {
            Doc::Spaces(1)
        });
    };
    for part in parts(node) {
        match (part, &mut branch) {
            (Part::Token(token), None) if token.kind() == T::LBrace => {
                gap(docs, &mut after_comment);
                docs.push(Doc::Text("{"));
                branch = Some(vec![Doc::Line]);
            }
            (Part::Token(token), Some(_)) if token.kind() == T::RBrace => {
                docs.extend(branch.take().map(indent));
                docs.push(Doc::Line);
                docs.push(Doc::Text("}"));
            }
            (Part::Token(token), None) if token.kind() == T::ElseKw => {
                gap(docs, &mut after_comment);
                docs.push(Doc::Text("else"));
            }
            (Part::Node(child), None) => {
                gap(docs, &mut after_comment);
                if child.kind() == N::IfExpr {
                    if_chain(child, docs);
                } else {
                    docs.push(any(child));
                }
            }
            (Part::Node(child), Some(inner)) => inner.push(any(child)),
            (Part::Token(token), None) => docs.push(Doc::Text(token.text())),
            (Part::Token(token), Some(inner)) => inner.push(Doc::Text(token.text())),
            (Part::Comment(text), Some(inner)) if inner.len() > 1 => {
                inner.push(Doc::LineSuffix(text));
            }
            // A comment before the value of a branch stays on the line of `{`.
            (Part::Comment(text), Some(_)) => docs.push(Doc::LineSuffix(text)),
            (Part::Comment(text), None) => {
                docs.push(Doc::LineSuffix(text));
                after_comment = true;
            }
        }
    }
}

/// Formats a `let ... in` expression. It stays on one line when it fits and
/// has no blank line between its bindings. Otherwise each binding gets its
/// own line, aligned after `let`, and `in` starts a line below `let`.
fn let_expr(node: SyntaxNode<'_>) -> Doc<'_> {
    let children: Vec<_> = node.children_with_tokens().collect();
    let position = |kind| {
        children.iter().position(
            |element| matches!(element, SyntaxElement::Token(token) if token.kind() == kind),
        )
    };
    let (Some(let_at), Some(in_at)) = (position(T::LetKw), position(T::InKw)) else {
        return inline(node);
    };
    if in_at < let_at {
        return inline(node);
    }
    let mut docs = inline_docs(N::LetExpr, &parts_of(children[..=let_at].iter().copied()));
    docs.push(Doc::Spaces(1));
    let seq = scan(children[let_at + 1..in_at].iter().copied(), true);
    // A comment after `let` ends its line, and the bindings start below,
    // where they would have started after `let `.
    if let Some(text) = seq.head {
        docs.push(Doc::LineSuffix(text));
        docs.push(Doc::HardLine);
        docs.push(Doc::Spaces("let ".len()));
    }
    let pads = pads(&seq.entries, false);
    let last = last_item(&seq.entries);
    let mut bindings = Vec::new();
    for (index, (entry, pad)) in seq.entries.into_iter().zip(pads).enumerate() {
        if index > 0 {
            if entry.blank {
                bindings.push(Doc::BreakParent);
                bindings.push(Doc::Line);
            }
            bindings.push(Doc::Line);
        }
        match entry.kind {
            EntryKind::Item { item, trailing } => {
                bindings.push(item.doc(pad));
                if last.is_some_and(|last| index < last) {
                    bindings.push(Doc::Text(","));
                }
                bindings.extend(trailing.map(Doc::LineSuffix));
            }
            kind @ EntryKind::Comment { .. } => push_entry(&mut bindings, kind, pad),
        }
    }
    docs.push(column(bindings));
    docs.push(Doc::Line);
    docs.extend(inline_docs(
        N::LetExpr,
        &parts_of(children[in_at..].iter().copied()),
    ));
    group(vec![column(docs)])
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;

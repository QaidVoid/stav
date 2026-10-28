//! The lossless syntax tree.
//!
//! A [`SyntaxTree`] holds every token of a source text, including whitespace,
//! comments, line feeds and text that could not be parsed. Nodes group tokens
//! into grammar constructs. Reading the tokens of the tree in order yields
//! the source text byte for byte.
//!
//! Whitespace and comments sit between nodes rather than at their edges: a
//! node starts at its first meaningful token and ends at its last one. Line
//! feeds that the grammar ignores, for example inside parentheses, are kept
//! in the node they occur in.

use std::fmt;

use crate::span::Span;
use crate::token::{Token, TokenKind};

/// The kind of a [`SyntaxNode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// A whole source file: its declarations and statements.
    File,
    /// Tokens that could not be parsed. A diagnostic always explains them,
    /// unless the lexer already reported the tokens themselves.
    Error,

    /// `import "path" as alias`.
    Import,
    /// `struct Name { fields }`.
    Struct,
    /// A field of a struct: `name: Type = default block kind`.
    Field,
    /// `type Name = variant | variant`.
    TypeDecl,
    /// A variant of a sum type, with an optional parameter list.
    Variant,
    /// `fn name(params) -> Type = body`.
    FnDecl,
    /// The parenthesized parameters of a function, lambda or variant.
    ParamList,
    /// A parameter: `name: Type = default`.
    Param,
    /// `let name: Type = value`, as a statement or a top-level declaration.
    Let,

    /// `{ statements }`, the body of a block, an `if`, a `for` or a `part`.
    Body,
    /// `path = value`.
    Attr,
    /// The dotted path of an attribute statement, such as `a.b."c"`.
    AttrPath,
    /// `kind label { body }`, with an optional label.
    Block,
    /// `include value`.
    Include,
    /// `if condition { body } else { body }` as a statement.
    IfStmt,
    /// `for name in collection { body }`.
    ForStmt,

    /// A literal token: an integer, a float, a path, `true`, `false` or
    /// `none`.
    Literal,
    /// A string in any of its forms, including its interpolations.
    Str,
    /// `#{ expression }` inside a string.
    Interp,
    /// An identifier in expression position.
    Name,
    /// A symbol word such as `llvm@19:static` in expression position.
    Symbol,
    /// `self`.
    SelfExpr,
    /// `[items]`.
    List,
    /// `[for x in xs if guard -> value]`.
    ListComp,
    /// `{ key = value }`, a map literal or a struct literal without a type
    /// name.
    Map,
    /// `key = value` inside a map or struct literal.
    MapEntry,
    /// `{for x in xs if guard -> key = value}`.
    MapComp,
    /// `Type { key = value }`.
    StructLit,
    /// `(expression)`.
    Paren,
    /// `callee(arguments)`.
    Call,
    /// The parenthesized arguments of a call or a predicate.
    ArgList,
    /// An argument, positional or named as in `name = value`.
    Arg,
    /// `base.field` or `base?.field`.
    FieldExpr,
    /// `base[index]`.
    IndexExpr,
    /// `lhs op rhs`.
    BinaryExpr,
    /// `!operand` or `-operand`.
    UnaryExpr,
    /// `if condition { value } else { value }` as an expression.
    IfExpr,
    /// `match scrutinee { arms }`.
    MatchExpr,
    /// `pattern if guard => value` inside a `match`.
    MatchArm,
    /// `let bindings in body`.
    LetExpr,
    /// `name: Type = value` inside a `let ... in` expression.
    Binding,
    /// `fn(params) -> Type = body`.
    Lambda,
    /// `part { body }`.
    PartExpr,

    /// The name of a type, optionally qualified: `Str` or `alias.Type`.
    TypePath,
    /// A type name with optional type arguments, such as `Map<Str, Int>`.
    NamedType,
    /// `<types>` after a type name.
    TypeArgList,
    /// `Fn(types) -> Type`.
    FnType,
    /// `Type where predicate && predicate`.
    RefinedType,
    /// A range predicate such as `1..=16`, `..5` or `0..`.
    RangePredicate,
    /// A named predicate such as `nonEmpty` or `matches(r"...")`.
    NamedPredicate,

    /// `_`.
    WildcardPat,
    /// A literal pattern: an integer, a string, `true`, `false` or `none`.
    LiteralPat,
    /// A name pattern: a binder or a nullary variant, optionally qualified.
    NamePat,
    /// A variant pattern with field patterns, such as `check(cmd, ..)`.
    VariantPat,
    /// `name` or `name = pattern` inside a variant pattern.
    FieldPat,
    /// `pattern | pattern`.
    OrPat,
}

/// A lossless syntax tree of one source text.
///
/// The tree owns a copy of the source. Its [`root`](Self::root) is always a
/// [`NodeKind::File`] node that spans the whole text.
#[derive(Debug, Clone)]
pub struct SyntaxTree {
    source: Box<str>,
    tokens: Vec<Token>,
    token_parents: Vec<usize>,
    nodes: Vec<NodeData>,
    elements: Vec<Element>,
}

#[derive(Debug, Clone)]
struct NodeData {
    kind: NodeKind,
    span: Span,
    parent: Option<usize>,
    first_child: usize,
    child_count: usize,
}

#[derive(Debug, Clone, Copy)]
enum Element {
    Node(usize),
    Token(usize),
}

impl SyntaxTree {
    /// Returns the root node, which covers the whole source text.
    pub fn root(&self) -> SyntaxNode<'_> {
        SyntaxNode {
            tree: self,
            index: self.nodes.len() - 1,
        }
    }

    fn element(&self, element: Element) -> SyntaxElement<'_> {
        match element {
            Element::Node(index) => SyntaxElement::Node(SyntaxNode { tree: self, index }),
            Element::Token(index) => SyntaxElement::Token(SyntaxToken { tree: self, index }),
        }
    }
}

/// A node of a [`SyntaxTree`]: a grammar construct and its children.
///
/// A node is a cheap handle that borrows the tree.
#[derive(Clone, Copy)]
pub struct SyntaxNode<'a> {
    tree: &'a SyntaxTree,
    index: usize,
}

impl<'a> SyntaxNode<'a> {
    /// Returns what construct the node is.
    pub fn kind(self) -> NodeKind {
        self.data().kind
    }

    /// Returns the bytes the node covers, from the start of its first token
    /// to the end of its last token. A node without tokens has an empty span
    /// where it would have started.
    pub fn span(self) -> Span {
        self.data().span
    }

    /// Returns the source text the node covers.
    pub fn text(self) -> &'a str {
        &self.tree.source[self.span().range()]
    }

    /// Returns the node that contains this one, or `None` for the root.
    pub fn parent(self) -> Option<SyntaxNode<'a>> {
        let tree = self.tree;
        self.data().parent.map(|index| SyntaxNode { tree, index })
    }

    /// Returns the child nodes in source order, without tokens.
    pub fn children(self) -> impl Iterator<Item = SyntaxNode<'a>> {
        self.children_with_tokens()
            .filter_map(|element| match element {
                SyntaxElement::Node(node) => Some(node),
                SyntaxElement::Token(_) => None,
            })
    }

    /// Returns the child nodes and tokens in source order.
    pub fn children_with_tokens(self) -> impl Iterator<Item = SyntaxElement<'a>> {
        let tree = self.tree;
        let data = self.data();
        tree.elements[data.first_child..data.first_child + data.child_count]
            .iter()
            .map(move |&element| tree.element(element))
    }

    fn data(self) -> &'a NodeData {
        &self.tree.nodes[self.index]
    }
}

impl fmt::Debug for SyntaxNode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let span = self.span();
        write!(f, "{:?}@{}..{}", self.kind(), span.start, span.end)
    }
}

impl fmt::Display for SyntaxNode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

/// A token of a [`SyntaxTree`].
///
/// A token is a cheap handle that borrows the tree.
#[derive(Clone, Copy)]
pub struct SyntaxToken<'a> {
    tree: &'a SyntaxTree,
    index: usize,
}

impl<'a> SyntaxToken<'a> {
    /// Returns what the token is.
    pub fn kind(self) -> TokenKind {
        self.tree.tokens[self.index].kind
    }

    /// Returns the bytes the token covers.
    pub fn span(self) -> Span {
        self.tree.tokens[self.index].span
    }

    /// Returns the source text of the token.
    pub fn text(self) -> &'a str {
        &self.tree.source[self.span().range()]
    }

    /// Returns the node that contains the token.
    pub fn parent(self) -> SyntaxNode<'a> {
        SyntaxNode {
            tree: self.tree,
            index: self.tree.token_parents[self.index],
        }
    }
}

impl fmt::Debug for SyntaxToken<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let span = self.span();
        write!(
            f,
            "{:?}@{}..{} {:?}",
            self.kind(),
            span.start,
            span.end,
            self.text()
        )
    }
}

impl fmt::Display for SyntaxToken<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

/// A child of a [`SyntaxNode`]: either a node or a token.
#[derive(Debug, Clone, Copy)]
pub enum SyntaxElement<'a> {
    /// A child node.
    Node(SyntaxNode<'a>),
    /// A child token.
    Token(SyntaxToken<'a>),
}

impl<'a> SyntaxElement<'a> {
    /// Returns the bytes the element covers.
    pub fn span(self) -> Span {
        match self {
            Self::Node(node) => node.span(),
            Self::Token(token) => token.span(),
        }
    }

    /// Returns the source text of the element.
    pub fn text(self) -> &'a str {
        match self {
            Self::Node(node) => node.text(),
            Self::Token(token) => token.text(),
        }
    }
}

/// Builds a [`SyntaxTree`] bottom up.
///
/// Children wait on a stack until the node that contains them is finished,
/// then move into one contiguous run of `elements`.
pub(crate) struct Builder {
    nodes: Vec<NodeData>,
    elements: Vec<Element>,
    token_parents: Vec<usize>,
    pending: Vec<Element>,
    open: Vec<(NodeKind, usize)>,
}

impl Builder {
    pub(crate) fn new(token_count: usize) -> Self {
        Self {
            nodes: Vec::new(),
            elements: Vec::with_capacity(token_count),
            token_parents: vec![0; token_count],
            pending: Vec::new(),
            open: Vec::new(),
        }
    }

    pub(crate) fn start_node(&mut self, kind: NodeKind) {
        self.open.push((kind, self.pending.len()));
    }

    /// Returns a position that [`Builder::start_node_at`] can later wrap
    /// into a node, together with everything added after it.
    pub(crate) fn checkpoint(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn start_node_at(&mut self, checkpoint: usize, kind: NodeKind) {
        self.open.push((kind, checkpoint.min(self.pending.len())));
    }

    pub(crate) fn token(&mut self, index: usize) {
        self.pending.push(Element::Token(index));
    }

    /// Finishes the innermost open node. `tokens` gives the spans of the
    /// children, and `empty_at` is the offset of a node without tokens.
    pub(crate) fn finish_node(&mut self, tokens: &[Token], empty_at: usize) {
        let Some((kind, first)) = self.open.pop() else {
            return;
        };
        let index = self.nodes.len();
        let first_child = self.elements.len();
        let mut span: Option<Span> = None;
        for element in self.pending.drain(first..) {
            let child_span = match element {
                Element::Node(child) => {
                    self.nodes[child].parent = Some(index);
                    self.nodes[child].span
                }
                Element::Token(token) => {
                    self.token_parents[token] = index;
                    tokens[token].span
                }
            };
            span = Some(match span {
                Some(span) => Span {
                    start: span.start,
                    end: child_span.end,
                },
                None => child_span,
            });
            self.elements.push(element);
        }
        self.nodes.push(NodeData {
            kind,
            span: span.unwrap_or(Span {
                start: empty_at,
                end: empty_at,
            }),
            parent: None,
            first_child,
            child_count: self.elements.len() - first_child,
        });
        self.pending.push(Element::Node(index));
    }

    /// Returns the tree. The last finished node becomes the root, and it
    /// spans the whole source.
    pub(crate) fn finish(mut self, source: &str, tokens: Vec<Token>) -> SyntaxTree {
        if let Some(root) = self.nodes.last_mut() {
            root.span = Span {
                start: 0,
                end: source.len(),
            };
        }
        SyntaxTree {
            source: source.into(),
            tokens,
            token_parents: self.token_parents,
            nodes: self.nodes,
            elements: self.elements,
        }
    }
}

//! A typed view of the syntax tree.
//!
//! Each type here wraps a [`SyntaxNode`] of one [`NodeKind`] and offers
//! accessors for the parts of its construct. The wrappers are cheap copies
//! of a node handle. Accessors return `None` or skip an element when that
//! part is missing, which happens only in a tree with syntax errors.
//!
//! Start from [`Parsed::file`](crate::Parsed::file), or wrap any node with
//! the `cast` function of the type that matches its kind.

use crate::diagnostic::Diagnostic;
use crate::span::Span;
use crate::syntax::{NodeKind, SyntaxElement, SyntaxNode, SyntaxToken};
use crate::token::TokenKind;

trait AstNode<'a>: Sized {
    fn cast(node: SyntaxNode<'a>) -> Option<Self>;
}

macro_rules! node {
    ($(#[$doc:meta])* $name:ident = $kind:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub struct $name<'a>(SyntaxNode<'a>);

        impl<'a> $name<'a> {
            /// Returns the node as this type when it has the matching kind.
            pub fn cast(node: SyntaxNode<'a>) -> Option<Self> {
                (node.kind() == NodeKind::$kind).then_some(Self(node))
            }

            /// Returns the underlying syntax node.
            pub fn syntax(self) -> SyntaxNode<'a> {
                self.0
            }
        }

        impl<'a> AstNode<'a> for $name<'a> {
            fn cast(node: SyntaxNode<'a>) -> Option<Self> {
                Self::cast(node)
            }
        }
    };
}

macro_rules! choice {
    (
        $(#[$doc:meta])*
        $name:ident { $($(#[$variant_doc:meta])* $variant:ident($ty:ident),)* }
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub enum $name<'a> {
            $($(#[$variant_doc])* $variant($ty<'a>),)*
        }

        impl<'a> $name<'a> {
            /// Returns the node as this type when it has one of the matching
            /// kinds.
            pub fn cast(node: SyntaxNode<'a>) -> Option<Self> {
                $(
                    if let Some(inner) = $ty::cast(node) {
                        return Some(Self::$variant(inner));
                    }
                )*
                None
            }

            /// Returns the underlying syntax node.
            pub fn syntax(self) -> SyntaxNode<'a> {
                match self {
                    $(Self::$variant(inner) => inner.syntax(),)*
                }
            }
        }

        impl<'a> AstNode<'a> for $name<'a> {
            fn cast(node: SyntaxNode<'a>) -> Option<Self> {
                Self::cast(node)
            }
        }
    };
}

fn child<'a, N: AstNode<'a>>(node: SyntaxNode<'a>) -> Option<N> {
    node.children().find_map(N::cast)
}

fn children<'a, N: AstNode<'a>>(node: SyntaxNode<'a>) -> impl Iterator<Item = N> {
    node.children().filter_map(N::cast)
}

fn tokens(node: SyntaxNode<'_>) -> impl Iterator<Item = SyntaxToken<'_>> {
    node.children_with_tokens()
        .filter_map(|element| match element {
            SyntaxElement::Token(token) => Some(token),
            SyntaxElement::Node(_) => None,
        })
}

fn token(node: SyntaxNode<'_>, kind: TokenKind) -> Option<SyntaxToken<'_>> {
    tokens(node).find(|token| token.kind() == kind)
}

fn is_name(token: &SyntaxToken<'_>) -> bool {
    matches!(token.kind(), TokenKind::Ident | TokenKind::TypeIdent)
}

/// Returns the direct identifier tokens of `node`.
fn names(node: SyntaxNode<'_>) -> impl Iterator<Item = SyntaxToken<'_>> {
    tokens(node).filter(is_name)
}

/// Returns the first child of type `N` that comes after the first direct
/// token of kind `after` and before any later direct token in `before`.
fn child_after<'a, N: AstNode<'a>>(
    node: SyntaxNode<'a>,
    after: TokenKind,
    before: &[TokenKind],
) -> Option<N> {
    let mut seen = false;
    for element in node.children_with_tokens() {
        match element {
            SyntaxElement::Token(token) if !seen => seen = token.kind() == after,
            SyntaxElement::Token(token) if before.contains(&token.kind()) => return None,
            SyntaxElement::Node(node) if seen => {
                if let Some(found) = N::cast(node) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// Returns the first identifier token after the first direct token of kind
/// `after`.
fn name_after(node: SyntaxNode<'_>, after: TokenKind) -> Option<SyntaxToken<'_>> {
    tokens(node)
        .skip_while(|token| token.kind() != after)
        .skip(1)
        .find(is_name)
}

/// Returns the name of a field, parameter or binding, which may be a symbol
/// word written by mistake.
fn binding_name(node: SyntaxNode<'_>) -> Option<SyntaxToken<'_>> {
    tokens(node).find(|token| is_name(token) || token.kind() == TokenKind::SymWord)
}

/// Returns the first key, a name or a string, among `elements`.
fn first_key<'a>(mut elements: impl Iterator<Item = SyntaxElement<'a>>) -> Option<Key<'a>> {
    elements.find_map(Key::from_element)
}

/// Returns the first token of `node`, looking into its first child nodes.
fn first_token(node: SyntaxNode<'_>) -> Option<SyntaxToken<'_>> {
    let mut node = node;
    loop {
        match node.children_with_tokens().next()? {
            SyntaxElement::Token(token) => return Some(token),
            SyntaxElement::Node(child) => node = child,
        }
    }
}

/// The kinds of files, which differ in the items they may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// A file evaluated as the body of a root struct. It may contain every
    /// kind of item.
    Entry,
    /// A file loaded through `import`. It contains imports and
    /// declarations, including top-level `let`, but no other statements.
    Library,
    /// A file loaded through `include "path"`. It contains imports, `let`
    /// and statements, but no `struct`, `type` or `fn` declarations.
    Part,
}

node! {
    /// A whole source file.
    SourceFile = File
}

impl<'a> SourceFile<'a> {
    pub(crate) fn from_root(root: SyntaxNode<'a>) -> Self {
        Self(root)
    }

    /// Returns the declarations and statements of the file in order.
    pub fn items(self) -> impl Iterator<Item = Item<'a>> {
        children(self.0)
    }

    /// Checks that the file contains only the items a file of `kind` may
    /// contain, and returns an error for each item that does not belong.
    ///
    /// A library cannot contain statements other than `let` (`E0209`), and
    /// a part file cannot declare a `struct`, `type` or `fn` (`E0208`).
    pub fn validate(self, kind: FileKind) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for item in self.items() {
            let node = item.syntax();
            let span = first_token(node).map_or(node.span(), SyntaxToken::span);
            match (kind, item) {
                (FileKind::Library, Item::Stmt(stmt)) if !matches!(stmt, Stmt::Let(_)) => {
                    diagnostics.push(
                        Diagnostic::error("E0209", span, "a library cannot contain statements")
                            .with_help("move the statement into an entry file or a part file"),
                    );
                }
                (FileKind::Part, Item::Struct(_) | Item::Type(_) | Item::Fn(_)) => {
                    let keyword = first_token(node).map_or("", SyntaxToken::text);
                    diagnostics.push(
                        Diagnostic::error(
                            "E0208",
                            span,
                            format!("a part file cannot contain a `{keyword}` declaration"),
                        )
                        .with_help("move the declaration into a library and import it"),
                    );
                }
                _ => {}
            }
        }
        diagnostics
    }
}

choice! {
    /// An item at the top level of a file.
    Item {
        /// `import "path" as alias`.
        Import(Import),
        /// A struct declaration.
        Struct(StructDecl),
        /// A sum type declaration.
        Type(TypeDecl),
        /// A function declaration.
        Fn(FnDecl),
        /// A statement. A top-level `let` is also a declaration that
        /// importing files can reach.
        Stmt(Stmt),
    }
}

node! {
    /// `import "path" as alias`.
    Import = Import
}

impl<'a> Import<'a> {
    /// Returns the string that names the imported file or module.
    pub fn path(self) -> Option<Str<'a>> {
        child(self.0)
    }

    /// Returns the name the module is bound to.
    pub fn alias(self) -> Option<SyntaxToken<'a>> {
        name_after(self.0, TokenKind::AsKw)
    }
}

node! {
    /// `struct Name { fields }`.
    StructDecl = Struct
}

impl<'a> StructDecl<'a> {
    /// Returns the name of the struct.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        names(self.0).next()
    }

    /// Returns the fields in declaration order.
    pub fn fields(self) -> impl Iterator<Item = Field<'a>> {
        children(self.0)
    }
}

node! {
    /// A struct field: `name: Type = default block kind`.
    Field = Field
}

impl<'a> Field<'a> {
    /// Returns the name of the field.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        binding_name(self.0)
    }

    /// Returns the type of the field.
    pub fn ty(self) -> Option<Type<'a>> {
        child(self.0)
    }

    /// Returns the default value, when the field declares one.
    pub fn default(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }

    /// Returns the block kind of a `block kind` clause.
    pub fn block_kind(self) -> Option<SyntaxToken<'a>> {
        name_after(self.0, TokenKind::BlockKw)
    }
}

node! {
    /// `type Name = variant | variant`.
    TypeDecl = TypeDecl
}

impl<'a> TypeDecl<'a> {
    /// Returns the name of the type.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        names(self.0).next()
    }

    /// Returns the variants in declaration order.
    pub fn variants(self) -> impl Iterator<Item = Variant<'a>> {
        children(self.0)
    }
}

node! {
    /// A variant of a sum type, such as `info` or `notify(fd: Int)`.
    Variant = Variant
}

impl<'a> Variant<'a> {
    /// Returns the name of the variant.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        names(self.0).next()
    }

    /// Returns the payload fields, or `None` for a variant without
    /// parentheses.
    pub fn params(self) -> Option<ParamList<'a>> {
        child(self.0)
    }
}

node! {
    /// `fn name(params) -> Type = body`.
    FnDecl = FnDecl
}

impl<'a> FnDecl<'a> {
    /// Returns the name of the function.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        names(self.0).next()
    }

    /// Returns the parameters.
    pub fn params(self) -> Option<ParamList<'a>> {
        child(self.0)
    }

    /// Returns the declared return type.
    pub fn return_type(self) -> Option<Type<'a>> {
        child_after(self.0, TokenKind::Arrow, &[])
    }

    /// Returns the expression the function evaluates to.
    pub fn body(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }
}

node! {
    /// The parenthesized parameters of a function, lambda or variant.
    ParamList = ParamList
}

impl<'a> ParamList<'a> {
    /// Returns the parameters in order.
    pub fn params(self) -> impl Iterator<Item = Param<'a>> {
        children(self.0)
    }
}

node! {
    /// A parameter: `name: Type = default`.
    Param = Param
}

impl<'a> Param<'a> {
    /// Returns the name of the parameter.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        binding_name(self.0)
    }

    /// Returns the type of the parameter.
    pub fn ty(self) -> Option<Type<'a>> {
        child(self.0)
    }

    /// Returns the default value, when the parameter declares one.
    pub fn default(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }
}

node! {
    /// `let name: Type = value`, as a statement or a top-level declaration.
    Let = Let
}

impl<'a> Let<'a> {
    /// Returns the bound name.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        binding_name(self.0)
    }

    /// Returns the type annotation, when there is one.
    pub fn ty(self) -> Option<Type<'a>> {
        child(self.0)
    }

    /// Returns the bound value.
    pub fn value(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }
}

choice! {
    /// A statement of a body, or of the top level of an entry or part file.
    Stmt {
        /// `let name = value`.
        Let(Let),
        /// `path = value`.
        Attr(Attr),
        /// `kind label { body }`.
        Block(Block),
        /// `include value`.
        Include(Include),
        /// `if condition { body } else { body }`.
        If(IfStmt),
        /// `for name in collection { body }`.
        For(ForStmt),
    }
}

node! {
    /// `{ statements }`.
    Body = Body
}

impl<'a> Body<'a> {
    /// Returns the statements in order.
    pub fn statements(self) -> impl Iterator<Item = Stmt<'a>> {
        children(self.0)
    }
}

node! {
    /// `path = value`.
    Attr = Attr
}

impl<'a> Attr<'a> {
    /// Returns the attribute path on the left of `=`.
    pub fn path(self) -> Option<AttrPath<'a>> {
        child(self.0)
    }

    /// Returns the contributed value.
    pub fn value(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }
}

node! {
    /// A dotted attribute path such as `a.b."c"`.
    AttrPath = AttrPath
}

impl<'a> AttrPath<'a> {
    /// Returns the keys of the path in order.
    pub fn keys(self) -> impl Iterator<Item = Key<'a>> {
        self.0.children_with_tokens().filter_map(Key::from_element)
    }
}

/// A key written as a name or as a string: an attribute path segment, a
/// block label or a map key.
#[derive(Debug, Clone, Copy)]
pub enum Key<'a> {
    /// A key written as an identifier.
    Name(SyntaxToken<'a>),
    /// A key written as a string.
    Str(Str<'a>),
}

impl<'a> Key<'a> {
    fn from_element(element: SyntaxElement<'a>) -> Option<Self> {
        match element {
            SyntaxElement::Token(token) if is_name(&token) => Some(Self::Name(token)),
            SyntaxElement::Node(node) => Str::cast(node).map(Self::Str),
            SyntaxElement::Token(_) => None,
        }
    }

    /// Returns the bytes the key covers.
    pub fn span(self) -> Span {
        match self {
            Self::Name(token) => token.span(),
            Self::Str(string) => string.syntax().span(),
        }
    }
}

node! {
    /// `kind label { body }`, with an optional label.
    Block = Block
}

impl<'a> Block<'a> {
    /// Returns the block kind, the identifier the block starts with.
    pub fn kind(self) -> Option<SyntaxToken<'a>> {
        names(self.0).next()
    }

    /// Returns the label, when the block has one.
    pub fn label(self) -> Option<Key<'a>> {
        first_key(
            self.0
                .children_with_tokens()
                .skip_while(|element| !matches!(element, SyntaxElement::Token(t) if is_name(t)))
                .skip(1),
        )
    }

    /// Returns the body of the block.
    pub fn body(self) -> Option<Body<'a>> {
        child(self.0)
    }
}

node! {
    /// `include value`. With a plain string, the value names a part file.
    Include = Include
}

impl<'a> Include<'a> {
    /// Returns the included value.
    pub fn value(self) -> Option<Expr<'a>> {
        child(self.0)
    }
}

node! {
    /// `if condition { body } else { body }` as a statement.
    IfStmt = IfStmt
}

impl<'a> IfStmt<'a> {
    /// Returns the condition.
    pub fn condition(self) -> Option<Expr<'a>> {
        child(self.0)
    }

    /// Returns the body evaluated when the condition holds.
    pub fn then_body(self) -> Option<Body<'a>> {
        child(self.0)
    }

    /// Returns what follows `else`, when there is an `else`.
    pub fn else_branch(self) -> Option<ElseBranch<'a>> {
        child_after(self.0, TokenKind::ElseKw, &[])
    }
}

choice! {
    /// What follows `else` in an `if` statement.
    ElseBranch {
        /// `else { body }`.
        Body(Body),
        /// `else if ...`.
        If(IfStmt),
    }
}

node! {
    /// `for name in collection { body }` or `for key, value in map { body }`.
    ForStmt = ForStmt
}

impl<'a> ForStmt<'a> {
    /// Returns the one or two names bound for each element.
    pub fn names(self) -> impl Iterator<Item = SyntaxToken<'a>> {
        loop_names(self.0)
    }

    /// Returns the collection iterated over.
    pub fn collection(self) -> Option<Expr<'a>> {
        child(self.0)
    }

    /// Returns the body evaluated for each element.
    pub fn body(self) -> Option<Body<'a>> {
        child(self.0)
    }
}

fn loop_names(node: SyntaxNode<'_>) -> impl Iterator<Item = SyntaxToken<'_>> {
    tokens(node)
        .skip_while(|token| token.kind() != TokenKind::ForKw)
        .take_while(|token| token.kind() != TokenKind::InKw)
        .filter(is_name)
}

choice! {
    /// An expression.
    Expr {
        /// An integer, float, path, `true`, `false` or `none`.
        Literal(Literal),
        /// A string.
        Str(Str),
        /// An identifier.
        Name(Name),
        /// A symbol word such as `llvm@19:static`.
        Symbol(Symbol),
        /// `self`.
        SelfExpr(SelfExpr),
        /// `[items]`.
        List(List),
        /// `[for x in xs -> value]`.
        ListComp(ListComp),
        /// `{ key = value }`.
        Map(Map),
        /// `{for x in xs -> key = value}`.
        MapComp(MapComp),
        /// `Type { key = value }`.
        StructLit(StructLit),
        /// `(expression)`.
        Paren(Paren),
        /// `callee(arguments)`.
        Call(Call),
        /// `base.field` or `base?.field`.
        Field(FieldExpr),
        /// `base[index]`.
        Index(IndexExpr),
        /// `lhs op rhs`.
        Binary(BinaryExpr),
        /// `!operand` or `-operand`.
        Unary(UnaryExpr),
        /// `if condition { value } else { value }`.
        If(IfExpr),
        /// `match scrutinee { arms }`.
        Match(MatchExpr),
        /// `let bindings in body`.
        Let(LetExpr),
        /// `fn(params) = body`.
        Lambda(Lambda),
        /// `part { body }`.
        Part(PartExpr),
    }
}

node! {
    /// An integer, float, path, `true`, `false` or `none`.
    Literal = Literal
}

/// The kind of a [`Literal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiteralKind {
    /// An integer such as `42` or `0x1f`.
    Int,
    /// A float such as `0.5`.
    Float,
    /// A relative path such as `./fix.patch`.
    Path,
    /// `true`.
    True,
    /// `false`.
    False,
    /// `none`.
    None,
}

impl<'a> Literal<'a> {
    /// Returns the literal token.
    pub fn token(self) -> Option<SyntaxToken<'a>> {
        tokens(self.0).next()
    }

    /// Returns what kind of literal this is.
    pub fn kind(self) -> Option<LiteralKind> {
        Some(match self.token()?.kind() {
            TokenKind::Int => LiteralKind::Int,
            TokenKind::Float => LiteralKind::Float,
            TokenKind::Path => LiteralKind::Path,
            TokenKind::TrueKw => LiteralKind::True,
            TokenKind::FalseKw => LiteralKind::False,
            TokenKind::NoneKw => LiteralKind::None,
            _ => return None,
        })
    }
}

node! {
    /// A string in any of its forms.
    Str = Str
}

/// The form of a [`Str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StrKind {
    /// `"..."`, with escapes and interpolation.
    Plain,
    /// `"""..."""` across lines, with interpolation.
    Multi,
    /// `r"..."`, without escapes or interpolation.
    Raw,
    /// `r"""..."""` across lines, without escapes or interpolation.
    RawMulti,
}

/// A part of the value of a [`Str`].
#[derive(Debug, Clone, Copy)]
pub enum StrPart<'a> {
    /// Literal text, taken exactly as written.
    Text(SyntaxToken<'a>),
    /// An escape that stands for one character.
    Escape(SyntaxToken<'a>),
    /// `#{ expression }`.
    Interp(Interp<'a>),
}

impl<'a> Str<'a> {
    /// Returns the form of the string.
    pub fn kind(self) -> Option<StrKind> {
        Some(match tokens(self.0).next()?.kind() {
            TokenKind::StrStart => StrKind::Plain,
            TokenKind::MStrStart => StrKind::Multi,
            TokenKind::RawStrStart => StrKind::Raw,
            TokenKind::RawMStrStart => StrKind::RawMulti,
            _ => return None,
        })
    }

    /// Returns the parts that make up the value, in order. The delimiters
    /// and the indentation of multi-line strings are left out.
    pub fn parts(self) -> impl Iterator<Item = StrPart<'a>> {
        self.0
            .children_with_tokens()
            .filter_map(|element| match element {
                SyntaxElement::Token(token) => match token.kind() {
                    TokenKind::StrText => Some(StrPart::Text(token)),
                    TokenKind::StrEscape => Some(StrPart::Escape(token)),
                    _ => None,
                },
                SyntaxElement::Node(node) => Interp::cast(node).map(StrPart::Interp),
            })
    }

    /// Returns the value of a string without interpolation. Returns `None`
    /// when the string interpolates or contains an invalid escape.
    pub fn value(self) -> Option<String> {
        let mut value = String::new();
        for part in self.parts() {
            match part {
                StrPart::Text(token) => value.push_str(token.text()),
                StrPart::Escape(token) => value.push(unescape(token.text())?),
                StrPart::Interp(_) => return None,
            }
        }
        Some(value)
    }
}

/// Returns the character an escape such as `\n` or `\u{1F600}` stands for.
pub(crate) fn unescape(escape: &str) -> Option<char> {
    let rest = escape.strip_prefix('\\')?;
    if let Some(hex) = rest.strip_prefix("u{") {
        let digits = hex.strip_suffix('}')?;
        if digits.is_empty() || digits.len() > 6 {
            return None;
        }
        return u32::from_str_radix(digits, 16)
            .ok()
            .and_then(char::from_u32);
    }
    let mut chars = rest.chars();
    let c = match chars.next()? {
        '\\' => '\\',
        '"' => '"',
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        '#' => '#',
        _ => return None,
    };
    chars.next().is_none().then_some(c)
}

node! {
    /// `#{ expression }` inside a string.
    Interp = Interp
}

impl<'a> Interp<'a> {
    /// Returns the interpolated expression.
    pub fn expr(self) -> Option<Expr<'a>> {
        child(self.0)
    }
}

node! {
    /// An identifier in expression position. What it refers to is decided
    /// by name resolution: a binding, a variant, a symbol or a reference.
    Name = Name
}

impl<'a> Name<'a> {
    /// Returns the identifier token.
    pub fn token(self) -> Option<SyntaxToken<'a>> {
        tokens(self.0).next()
    }
}

node! {
    /// A symbol word such as `llvm@19.1:static`, a compact literal of a
    /// host symbol type.
    Symbol = Symbol
}

impl<'a> Symbol<'a> {
    /// Returns the symbol word token.
    pub fn token(self) -> Option<SyntaxToken<'a>> {
        tokens(self.0).next()
    }

    /// Returns the identifier before the version and variant parts.
    pub fn name(self) -> &'a str {
        let text = self.text();
        text.find(['@', ':']).map_or(text, |end| &text[..end])
    }

    /// Returns the version part after `@`, when there is one.
    pub fn version(self) -> Option<&'a str> {
        let text = self.text();
        let start = text.find('@')? + 1;
        let end = text[start..]
            .find(':')
            .map_or(text.len(), |end| start + end);
        Some(&text[start..end])
    }

    /// Returns the variant part after `:`, when there is one.
    pub fn variant(self) -> Option<&'a str> {
        let text = self.text();
        text.find(':').map(|colon| &text[colon + 1..])
    }

    fn text(self) -> &'a str {
        self.token().map_or("", SyntaxToken::text)
    }
}

node! {
    /// `self`, the merged value of an enclosing block.
    SelfExpr = SelfExpr
}

node! {
    /// `[items]`.
    List = List
}

impl<'a> List<'a> {
    /// Returns the items in order.
    pub fn items(self) -> impl Iterator<Item = Expr<'a>> {
        children(self.0)
    }
}

node! {
    /// `[for x in xs if guard -> value]`.
    ListComp = ListComp
}

impl<'a> ListComp<'a> {
    /// Returns the one or two names bound for each element.
    pub fn names(self) -> impl Iterator<Item = SyntaxToken<'a>> {
        loop_names(self.0)
    }

    /// Returns the collection iterated over.
    pub fn source(self) -> Option<Expr<'a>> {
        child_after(
            self.0,
            TokenKind::InKw,
            &[TokenKind::IfKw, TokenKind::Arrow],
        )
    }

    /// Returns the condition after `if`, when there is one.
    pub fn guard(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::IfKw, &[TokenKind::Arrow])
    }

    /// Returns the value produced for each element.
    pub fn value(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Arrow, &[])
    }
}

node! {
    /// `{ key = value }`: a map literal, or a struct literal whose type comes
    /// from the expected type.
    Map = Map
}

impl<'a> Map<'a> {
    /// Returns the entries in order.
    pub fn entries(self) -> impl Iterator<Item = MapEntry<'a>> {
        children(self.0)
    }
}

node! {
    /// `key = value` inside a map or struct literal.
    MapEntry = MapEntry
}

impl<'a> MapEntry<'a> {
    /// Returns the key.
    pub fn key(self) -> Option<Key<'a>> {
        first_key(self.0.children_with_tokens())
    }

    /// Returns the value.
    pub fn value(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }
}

node! {
    /// `{for x in xs if guard -> key = value}`.
    MapComp = MapComp
}

impl<'a> MapComp<'a> {
    /// Returns the one or two names bound for each element.
    pub fn names(self) -> impl Iterator<Item = SyntaxToken<'a>> {
        loop_names(self.0)
    }

    /// Returns the collection iterated over.
    pub fn source(self) -> Option<Expr<'a>> {
        child_after(
            self.0,
            TokenKind::InKw,
            &[TokenKind::IfKw, TokenKind::Arrow],
        )
    }

    /// Returns the condition after `if`, when there is one.
    pub fn guard(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::IfKw, &[TokenKind::Arrow])
    }

    /// Returns the key produced for each element.
    pub fn key(self) -> Option<Key<'a>> {
        first_key(
            self.0
                .children_with_tokens()
                .skip_while(|element| {
                    !matches!(element, SyntaxElement::Token(t) if t.kind() == TokenKind::Arrow)
                })
                .skip(1),
        )
    }

    /// Returns the value produced for each element.
    pub fn value(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }
}

node! {
    /// `Type { key = value }`.
    StructLit = StructLit
}

impl<'a> StructLit<'a> {
    /// Returns the name of the struct type.
    pub fn type_path(self) -> Option<TypePath<'a>> {
        child(self.0)
    }

    /// Returns the field values in order.
    pub fn entries(self) -> impl Iterator<Item = MapEntry<'a>> {
        child::<Map<'a>>(self.0).into_iter().flat_map(Map::entries)
    }
}

node! {
    /// `(expression)`.
    Paren = Paren
}

impl<'a> Paren<'a> {
    /// Returns the expression in the parentheses.
    pub fn expr(self) -> Option<Expr<'a>> {
        child(self.0)
    }
}

node! {
    /// `callee(arguments)`. Priority markers such as `force(x)` are calls
    /// too.
    Call = Call
}

impl<'a> Call<'a> {
    /// Returns the called expression.
    pub fn callee(self) -> Option<Expr<'a>> {
        child(self.0)
    }

    /// Returns the argument list.
    pub fn args(self) -> Option<ArgList<'a>> {
        child(self.0)
    }
}

node! {
    /// The parenthesized arguments of a call or a predicate.
    ArgList = ArgList
}

impl<'a> ArgList<'a> {
    /// Returns the arguments in order.
    pub fn args(self) -> impl Iterator<Item = Arg<'a>> {
        children(self.0)
    }
}

node! {
    /// An argument: `value` or `name = value`.
    Arg = Arg
}

impl<'a> Arg<'a> {
    /// Returns the name of a named argument.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        token(self.0, TokenKind::Eq).and_then(|_| names(self.0).next())
    }

    /// Returns the value of the argument.
    pub fn value(self) -> Option<Expr<'a>> {
        child(self.0)
    }
}

node! {
    /// `base.field` or `base?.field`.
    FieldExpr = FieldExpr
}

impl<'a> FieldExpr<'a> {
    /// Returns the expression whose field is read.
    pub fn base(self) -> Option<Expr<'a>> {
        child(self.0)
    }

    /// Returns the name of the field.
    pub fn field(self) -> Option<SyntaxToken<'a>> {
        names(self.0).next()
    }

    /// Returns whether the access is `?.`, which yields `none` when the base
    /// is `none`.
    pub fn is_optional(self) -> bool {
        token(self.0, TokenKind::QuestionDot).is_some()
    }
}

node! {
    /// `base[index]`.
    IndexExpr = IndexExpr
}

impl<'a> IndexExpr<'a> {
    /// Returns the indexed expression.
    pub fn base(self) -> Option<Expr<'a>> {
        child(self.0)
    }

    /// Returns the index.
    pub fn index(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::LBracket, &[])
    }
}

node! {
    /// `lhs op rhs`.
    BinaryExpr = BinaryExpr
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `??`
    Coalesce,
    /// `||`
    Or,
    /// `&&`
    And,
    /// `==`
    Eq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    LtEq,
    /// `>`
    Gt,
    /// `>=`
    GtEq,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `++`
    Concat,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
}

impl BinaryOp {
    fn from_token(kind: TokenKind) -> Option<Self> {
        Some(match kind {
            TokenKind::QuestionQuestion => Self::Coalesce,
            TokenKind::OrOr => Self::Or,
            TokenKind::AndAnd => Self::And,
            TokenKind::EqEq => Self::Eq,
            TokenKind::NotEq => Self::NotEq,
            TokenKind::Lt => Self::Lt,
            TokenKind::LtEq => Self::LtEq,
            TokenKind::Gt => Self::Gt,
            TokenKind::GtEq => Self::GtEq,
            TokenKind::Plus => Self::Add,
            TokenKind::Minus => Self::Sub,
            TokenKind::PlusPlus => Self::Concat,
            TokenKind::Star => Self::Mul,
            TokenKind::Slash => Self::Div,
            TokenKind::Percent => Self::Rem,
            _ => return None,
        })
    }
}

impl<'a> BinaryExpr<'a> {
    /// Returns the left operand.
    pub fn lhs(self) -> Option<Expr<'a>> {
        child(self.0)
    }

    /// Returns the operator token.
    pub fn op_token(self) -> Option<SyntaxToken<'a>> {
        tokens(self.0).find(|token| BinaryOp::from_token(token.kind()).is_some())
    }

    /// Returns the operator.
    pub fn op(self) -> Option<BinaryOp> {
        BinaryOp::from_token(self.op_token()?.kind())
    }

    /// Returns the right operand.
    pub fn rhs(self) -> Option<Expr<'a>> {
        let op = self.op_token()?.kind();
        child_after(self.0, op, &[])
    }
}

node! {
    /// `!operand` or `-operand`.
    UnaryExpr = UnaryExpr
}

/// A unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `!`
    Not,
    /// `-`
    Neg,
}

impl<'a> UnaryExpr<'a> {
    /// Returns the operator.
    pub fn op(self) -> Option<UnaryOp> {
        match tokens(self.0).next()?.kind() {
            TokenKind::Bang => Some(UnaryOp::Not),
            TokenKind::Minus => Some(UnaryOp::Neg),
            _ => None,
        }
    }

    /// Returns the operand.
    pub fn operand(self) -> Option<Expr<'a>> {
        child(self.0)
    }
}

node! {
    /// `if condition { value } else { value }` as an expression.
    IfExpr = IfExpr
}

impl<'a> IfExpr<'a> {
    /// Returns the condition.
    pub fn condition(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::IfKw, &[TokenKind::LBrace])
    }

    /// Returns the value when the condition holds.
    pub fn then_branch(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::LBrace, &[TokenKind::RBrace])
    }

    /// Returns the value otherwise: the expression in the `else` braces, or
    /// a nested `if` expression.
    pub fn else_branch(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::ElseKw, &[])
    }
}

node! {
    /// `match scrutinee { arms }`.
    MatchExpr = MatchExpr
}

impl<'a> MatchExpr<'a> {
    /// Returns the matched expression.
    pub fn scrutinee(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::MatchKw, &[TokenKind::LBrace])
    }

    /// Returns the arms in order.
    pub fn arms(self) -> impl Iterator<Item = MatchArm<'a>> {
        children(self.0)
    }
}

node! {
    /// `pattern if guard => value`.
    MatchArm = MatchArm
}

impl<'a> MatchArm<'a> {
    /// Returns the pattern.
    pub fn pattern(self) -> Option<Pattern<'a>> {
        child(self.0)
    }

    /// Returns the condition after `if`, when there is one.
    pub fn guard(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::IfKw, &[TokenKind::FatArrow])
    }

    /// Returns the value of the arm.
    pub fn value(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::FatArrow, &[])
    }
}

node! {
    /// `let bindings in body`.
    LetExpr = LetExpr
}

impl<'a> LetExpr<'a> {
    /// Returns the bindings in order.
    pub fn bindings(self) -> impl Iterator<Item = Binding<'a>> {
        children(self.0)
    }

    /// Returns the expression after `in`.
    pub fn body(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::InKw, &[])
    }
}

node! {
    /// `name: Type = value` inside a `let ... in` expression.
    Binding = Binding
}

impl<'a> Binding<'a> {
    /// Returns the bound name.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        binding_name(self.0)
    }

    /// Returns the type annotation, when there is one.
    pub fn ty(self) -> Option<Type<'a>> {
        child(self.0)
    }

    /// Returns the bound value.
    pub fn value(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }
}

node! {
    /// `fn(params) -> Type = body`, with an optional return type.
    Lambda = Lambda
}

impl<'a> Lambda<'a> {
    /// Returns the parameters.
    pub fn params(self) -> Option<ParamList<'a>> {
        child(self.0)
    }

    /// Returns the declared return type, when there is one.
    pub fn return_type(self) -> Option<Type<'a>> {
        child_after(self.0, TokenKind::Arrow, &[])
    }

    /// Returns the expression the lambda evaluates to.
    pub fn body(self) -> Option<Expr<'a>> {
        child_after(self.0, TokenKind::Eq, &[])
    }
}

node! {
    /// `part { body }`.
    PartExpr = PartExpr
}

impl<'a> PartExpr<'a> {
    /// Returns the body of the part.
    pub fn body(self) -> Option<Body<'a>> {
        child(self.0)
    }
}

choice! {
    /// A type.
    Type {
        /// A type name with optional arguments, such as `Map<Str, Int>`.
        Named(NamedType),
        /// `Fn(types) -> Type`.
        Fn(FnType),
        /// `Type where predicate`.
        Refined(RefinedType),
    }
}

node! {
    /// The name of a type, such as `Str` or `alias.Type`.
    TypePath = TypePath
}

impl<'a> TypePath<'a> {
    /// Returns the segments of the name in order.
    pub fn segments(self) -> impl Iterator<Item = SyntaxToken<'a>> {
        names(self.0)
    }

    /// Returns the last segment, the name of the type itself.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        self.segments().last()
    }
}

node! {
    /// A type name with optional type arguments, such as `List<Str>`.
    NamedType = NamedType
}

impl<'a> NamedType<'a> {
    /// Returns the name of the type.
    pub fn path(self) -> Option<TypePath<'a>> {
        child(self.0)
    }

    /// Returns the type arguments in order.
    pub fn args(self) -> impl Iterator<Item = Type<'a>> {
        self.0
            .children()
            .find(|node| node.kind() == NodeKind::TypeArgList)
            .into_iter()
            .flat_map(children)
    }
}

node! {
    /// `Fn(types) -> Type`.
    FnType = FnType
}

impl<'a> FnType<'a> {
    /// Returns the parameter types in order.
    pub fn params(self) -> impl Iterator<Item = Type<'a>> {
        self.0
            .children_with_tokens()
            .take_while(|element| {
                !matches!(element, SyntaxElement::Token(t) if t.kind() == TokenKind::Arrow)
            })
            .filter_map(|element| match element {
                SyntaxElement::Node(node) => Type::cast(node),
                SyntaxElement::Token(_) => None,
            })
    }

    /// Returns the return type.
    pub fn return_type(self) -> Option<Type<'a>> {
        child_after(self.0, TokenKind::Arrow, &[])
    }
}

node! {
    /// `Type where predicate && predicate`.
    RefinedType = RefinedType
}

impl<'a> RefinedType<'a> {
    /// Returns the refined type.
    pub fn base(self) -> Option<Type<'a>> {
        child(self.0)
    }

    /// Returns the predicates in order.
    pub fn predicates(self) -> impl Iterator<Item = Predicate<'a>> {
        children(self.0)
    }
}

choice! {
    /// A predicate of a refinement type.
    Predicate {
        /// A range such as `1..=16`.
        Range(RangePredicate),
        /// A named predicate such as `nonEmpty` or `matches(r"...")`.
        Named(NamedPredicate),
    }
}

node! {
    /// A range predicate such as `1..=16`, `..5` or `0..`.
    RangePredicate = RangePredicate
}

impl<'a> RangePredicate<'a> {
    /// Returns the lower bound, when there is one.
    pub fn start(self) -> Option<Expr<'a>> {
        self.0
            .children_with_tokens()
            .take_while(|element| !matches!(element, SyntaxElement::Token(t) if is_range_op(t)))
            .find_map(|element| match element {
                SyntaxElement::Node(node) => Expr::cast(node),
                SyntaxElement::Token(_) => None,
            })
    }

    /// Returns the upper bound, when there is one.
    pub fn end(self) -> Option<Expr<'a>> {
        self.0
            .children_with_tokens()
            .skip_while(|element| !matches!(element, SyntaxElement::Token(t) if is_range_op(t)))
            .find_map(|element| match element {
                SyntaxElement::Node(node) => Expr::cast(node),
                SyntaxElement::Token(_) => None,
            })
    }

    /// Returns whether the range includes its upper bound, as `..=` does.
    pub fn is_inclusive(self) -> bool {
        token(self.0, TokenKind::DotDotEq).is_some()
    }
}

fn is_range_op(token: &SyntaxToken<'_>) -> bool {
    matches!(token.kind(), TokenKind::DotDot | TokenKind::DotDotEq)
}

node! {
    /// A named predicate such as `nonEmpty` or `matches(r"...")`.
    NamedPredicate = NamedPredicate
}

impl<'a> NamedPredicate<'a> {
    /// Returns the segments of the predicate name in order.
    pub fn segments(self) -> impl Iterator<Item = SyntaxToken<'a>> {
        names(self.0)
    }

    /// Returns the extra arguments, when the predicate has parentheses.
    pub fn args(self) -> Option<ArgList<'a>> {
        child(self.0)
    }
}

choice! {
    /// A pattern of a `match` arm.
    Pattern {
        /// `_`.
        Wildcard(WildcardPat),
        /// An integer, string, `true`, `false` or `none`.
        Literal(LiteralPat),
        /// A binder or a nullary variant.
        Name(NamePat),
        /// A variant with field patterns.
        Variant(VariantPat),
        /// Alternatives separated by `|`.
        Or(OrPat),
    }
}

node! {
    /// `_`, which matches anything.
    WildcardPat = WildcardPat
}

node! {
    /// A literal pattern: an integer, possibly negative, a string, `true`,
    /// `false` or `none`.
    LiteralPat = LiteralPat
}

impl<'a> LiteralPat<'a> {
    /// Returns the literal as an expression: a [`Literal`], a [`Str`], or a
    /// [`UnaryExpr`] negating an integer.
    pub fn expr(self) -> Option<Expr<'a>> {
        child(self.0)
    }
}

node! {
    /// A name pattern, optionally qualified. It matches a variant when the
    /// scrutinee's type has one of that name, and binds the value otherwise.
    NamePat = NamePat
}

impl<'a> NamePat<'a> {
    /// Returns the segments of the name in order.
    pub fn segments(self) -> impl Iterator<Item = SyntaxToken<'a>> {
        names(self.0)
    }
}

node! {
    /// A variant pattern with field patterns, such as `check(cmd, ..)`.
    VariantPat = VariantPat
}

impl<'a> VariantPat<'a> {
    /// Returns the segments of the variant name in order.
    pub fn segments(self) -> impl Iterator<Item = SyntaxToken<'a>> {
        names(self.0)
    }

    /// Returns the field patterns in order.
    pub fn fields(self) -> impl Iterator<Item = FieldPat<'a>> {
        children(self.0)
    }

    /// Returns whether the pattern ends with `..`, which ignores the fields
    /// it does not mention.
    pub fn has_rest(self) -> bool {
        token(self.0, TokenKind::DotDot).is_some()
    }
}

node! {
    /// `field` or `field = pattern` inside a variant pattern.
    FieldPat = FieldPat
}

impl<'a> FieldPat<'a> {
    /// Returns the name of the payload field.
    pub fn name(self) -> Option<SyntaxToken<'a>> {
        names(self.0).next()
    }

    /// Returns the nested pattern. Without one, the field is bound to a
    /// variable of the same name.
    pub fn pattern(self) -> Option<Pattern<'a>> {
        child(self.0)
    }
}

node! {
    /// Alternatives separated by `|`.
    OrPat = OrPat
}

impl<'a> OrPat<'a> {
    /// Returns the alternatives in order.
    pub fn alternatives(self) -> impl Iterator<Item = Pattern<'a>> {
        children(self.0)
    }
}

#[cfg(test)]
#[path = "ast_tests.rs"]
mod tests;

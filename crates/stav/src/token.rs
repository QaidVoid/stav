//! Tokens produced by the lexer.

use crate::span::Span;

/// One token of source text: its kind and the exact bytes it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// The bytes of the source text the token covers.
    pub span: Span,
}

impl Token {
    /// Returns the text of the token.
    ///
    /// # Panics
    ///
    /// Panics if `source` is not the text the token was lexed from and the
    /// span is out of bounds or splits a character there.
    pub fn text(self, source: &str) -> &str {
        &source[self.span.range()]
    }
}

/// The kind of a [`Token`].
///
/// A kind describes the shape of the text. A token with the right shape but
/// an invalid value, such as an integer that does not fit in 64 bits, keeps
/// its kind and comes with a diagnostic. Text that forms no token at all is
/// a [`TokenKind::Error`] token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// A run of spaces and tabs.
    Whitespace,
    /// A comment from `#` up to the end of the line, excluding the line feed.
    Comment,
    /// A comment starting with `##` followed by a space or the end of the
    /// line. It documents the declaration or field that follows it.
    DocComment,
    /// A single line feed. Consecutive line feeds, with whitespace and
    /// comments between them, are separate tokens.
    Newline,

    /// An identifier such as `name`, `xdg-utils` or `openssl1.1`.
    ///
    /// Identifiers are ASCII. A `-` belongs to an identifier when a letter,
    /// digit or `_` follows it directly, and a `.` belongs to it when a digit
    /// follows it directly. `a-b` is therefore one identifier and `a - b` is
    /// a subtraction.
    Ident,
    /// An identifier that starts with an uppercase ASCII letter, such as
    /// `Str` or `Apache-2.0`.
    TypeIdent,
    /// An identifier directly followed by a version part, a variant part or
    /// both, such as `name@1.2`, `name:static` or `name@1.2:static`.
    ///
    /// The version part is `@` followed by letters, digits, `.`, `_` and `~`.
    /// The variant part is `:` followed by a lowercase letter and then
    /// lowercase letters, digits and `_`. A `:` followed by anything else,
    /// for example a space, is a [`TokenKind::Colon`].
    SymWord,
    /// An integer such as `42`, `0x1f`, `0o755`, `0b1010` or `1_000`.
    ///
    /// A `_` may separate two digits. Anywhere else it makes the literal an
    /// [`TokenKind::Error`] token.
    Int,
    /// A float such as `0.5`, `1.0e-3` or `2e10`.
    ///
    /// A `_` may separate two digits, as in an [`TokenKind::Int`].
    Float,
    /// A relative path such as `./fix.patch` or `../files/a.conf`.
    Path,

    /// `"`, opening a single-line string.
    StrStart,
    /// `"""`, opening a multi-line string.
    MStrStart,
    /// `r"`, opening a raw single-line string.
    RawStrStart,
    /// `r"""`, opening a raw multi-line string.
    RawMStrStart,
    /// Literal text inside a string. Its bytes are part of the string value
    /// exactly as written.
    StrText,
    /// A backslash escape inside a string, such as `\n` or `\u{1F600}`. It
    /// stands for the one character it denotes. In multi-line strings the
    /// only escape is `\#` directly before `{`, standing for `#`.
    StrEscape,
    /// Layout of a multi-line string that is not part of its value: the line
    /// feed after the opening delimiter, the common indentation at the start
    /// of each line, and the whitespace of lines that hold only whitespace.
    StrLayout,
    /// `#{`, starting an interpolated expression inside a string.
    InterpStart,
    /// `}`, ending an interpolated expression.
    InterpEnd,
    /// `"`, closing a single-line or raw single-line string.
    StrEnd,
    /// `"""`, closing a multi-line or raw multi-line string.
    MStrEnd,

    /// The keyword `let`.
    LetKw,
    /// The keyword `fn`.
    FnKw,
    /// The keyword `struct`.
    StructKw,
    /// The keyword `type`.
    TypeKw,
    /// The keyword `match`.
    MatchKw,
    /// The keyword `if`.
    IfKw,
    /// The keyword `else`.
    ElseKw,
    /// The keyword `for`.
    ForKw,
    /// The keyword `in`.
    InKw,
    /// The keyword `import`.
    ImportKw,
    /// The keyword `include`.
    IncludeKw,
    /// The keyword `as`.
    AsKw,
    /// The keyword `part`.
    PartKw,
    /// The keyword `self`.
    SelfKw,
    /// The keyword `true`.
    TrueKw,
    /// The keyword `false`.
    FalseKw,
    /// The keyword `none`.
    NoneKw,
    /// The keyword `where`.
    WhereKw,
    /// The keyword `block`.
    BlockKw,

    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
    /// `=`
    Eq,
    /// `.`
    Dot,
    /// `?.`
    QuestionDot,
    /// `??`
    QuestionQuestion,
    /// `:`
    Colon,
    /// `->`
    Arrow,
    /// `=>`
    FatArrow,
    /// `..`
    DotDot,
    /// `..=`
    DotDotEq,
    /// `|`
    Pipe,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `++`
    PlusPlus,
    /// `==`
    EqEq,
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
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `!`
    Bang,

    /// Text that forms no valid token. A diagnostic always explains it.
    Error,
    /// The end of the source. It is always the last token and covers no text.
    Eof,
}

impl TokenKind {
    /// Returns whether the token carries no meaning for the parser and only
    /// matters for reproducing the source: whitespace and comments.
    ///
    /// Line feeds are not trivia, because they separate statements and
    /// entries.
    pub fn is_trivia(self) -> bool {
        matches!(self, Self::Whitespace | Self::Comment | Self::DocComment)
    }

    pub(crate) fn keyword(word: &str) -> Option<Self> {
        Some(match word {
            "let" => Self::LetKw,
            "fn" => Self::FnKw,
            "struct" => Self::StructKw,
            "type" => Self::TypeKw,
            "match" => Self::MatchKw,
            "if" => Self::IfKw,
            "else" => Self::ElseKw,
            "for" => Self::ForKw,
            "in" => Self::InKw,
            "import" => Self::ImportKw,
            "include" => Self::IncludeKw,
            "as" => Self::AsKw,
            "part" => Self::PartKw,
            "self" => Self::SelfKw,
            "true" => Self::TrueKw,
            "false" => Self::FalseKw,
            "none" => Self::NoneKw,
            "where" => Self::WhereKw,
            "block" => Self::BlockKw,
            _ => return None,
        })
    }
}

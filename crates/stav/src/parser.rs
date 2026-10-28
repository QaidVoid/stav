//! Parsing source text into a lossless syntax tree.
//!
//! The parser is a recursive descent parser over the tokens of the lexer.
//! It never fails: text that does not fit the grammar ends up in
//! [`NodeKind::Error`] nodes or leaves a node incomplete, and a diagnostic
//! explains it.
//!
//! # Line feeds
//!
//! Line feeds separate statements, entries and arms, so the lexer keeps
//! them as tokens. A line feed is ignored:
//!
//! - inside `( )` and inside `#{ }`, unless a nested `{ }` or `[ ]` makes
//!   line feeds significant again,
//! - after a token that cannot end an expression, such as `=`, `,`, `{`,
//!   `->`, a binary operator other than `>`, or one of the keywords `in`,
//!   `else`, `where`, `import`, `include` and `as`,
//! - before a token that cannot start a statement: `.`, `?.`, `??`, `&&`,
//!   `||`, `++`, `|`, `=>` and `else`.
//!
//! A line feed after `>`, `..` or `..=` is kept, because a type such as
//! `Option<Str>` or a refinement such as `Int where 1..` can end a line.
//! Some constructs also skip line feeds where nothing else could follow:
//! inside `< >` of type arguments, before the `in` of a `let` expression,
//! between the parts of a comprehension, and before the closing `}` of an
//! `if` expression branch or the closing `]` of an index.
//!
//! # Errors
//!
//! After an error, the parser reports nothing more until it passes the next
//! separator (a line feed or a `,`), so that one mistake produces one
//! diagnostic. Unclosed delimiters are reported at the opening delimiter.
//! Nesting deeper than [`MAX_DEPTH`] levels is reported once and the nested
//! text is skipped, so that deeply nested input cannot overflow the stack.

mod expressions;
mod items;

use crate::ast::SourceFile;
use crate::diagnostic::Diagnostic;
use crate::lexer::lex;
use crate::span::Span;
use crate::syntax::{Builder, NodeKind, SyntaxTree};
use crate::token::{Token, TokenKind};

/// The deepest nesting the parser accepts. Each nested expression,
/// statement, type or pattern is a level, and so is each operator of a
/// chain such as `a + b + c` or `a.b.c`, because each one nests the tree
/// one level deeper.
pub const MAX_DEPTH: usize = 256;

/// The syntax tree and all errors of one source text.
#[derive(Debug, Clone)]
pub struct Parsed {
    /// The lossless syntax tree. It reproduces the source text exactly,
    /// whether or not there were errors.
    pub tree: SyntaxTree,
    /// The lexical and syntax errors, ordered by the start of their span.
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    /// Returns the typed view of the root of the tree.
    pub fn file(&self) -> SourceFile<'_> {
        SourceFile::from_root(self.tree.root())
    }
}

/// Parses `source` into a lossless syntax tree.
///
/// Parsing never fails and never panics. Every byte of the source ends up in
/// the tree, and every lexical and syntax error comes back as a diagnostic.
/// The grammar accepts everything that any kind of file may contain. Use
/// [`SourceFile::validate`] to check the items allowed in a particular kind
/// of file.
///
/// # Examples
///
/// ```
/// use stav::ast::{Item, Stmt};
///
/// let source = "port = 8080\nservice web { exec = [\"/bin/web\"] }\n";
/// let parsed = stav::parse(source);
/// assert!(parsed.diagnostics.is_empty());
/// assert_eq!(parsed.tree.root().text(), source);
///
/// let items: Vec<Item> = parsed.file().items().collect();
/// let Item::Stmt(Stmt::Block(block)) = items[1] else {
///     panic!("expected a block");
/// };
/// assert_eq!(block.kind().map(|t| t.text()), Some("service"));
/// ```
pub fn parse(source: &str) -> Parsed {
    let lexed = lex(source);
    let mut parser = Parser::new(source, lexed.tokens);
    parser.file();
    let Parser {
        mut tokens,
        builder,
        diagnostics: syntax_diagnostics,
        ..
    } = parser;
    tokens.pop();
    let tree = builder.finish(source, tokens);
    let mut diagnostics = lexed.diagnostics;
    diagnostics.extend(syntax_diagnostics);
    diagnostics.sort_by_key(|d| d.span.start);
    Parsed { tree, diagnostics }
}

pub(crate) struct Parser<'s> {
    source: &'s str,
    /// The tokens of the lexer, ending with [`TokenKind::Eof`].
    tokens: Vec<Token>,
    layout: Layout,
    /// The index of the first token not yet in the tree.
    pos: usize,
    /// Tokens at this index and later are hidden, so that the contents of
    /// an interpolation cannot run past its end.
    limit: usize,
    builder: Builder,
    diagnostics: Vec<Diagnostic>,
    /// Whether an error was reported since the last separator.
    recovering: bool,
    depth: usize,
    /// The token indexes of the open `(`, `[` and `{`, innermost last.
    delims: Vec<usize>,
    /// Whether the file has had an item other than `import`.
    past_imports: bool,
}

impl<'s> Parser<'s> {
    fn new(source: &'s str, tokens: Vec<Token>) -> Self {
        let layout = Layout::new(&tokens);
        let limit = tokens.len() - 1;
        Self {
            source,
            builder: Builder::new(tokens.len()),
            tokens,
            layout,
            pos: 0,
            limit,
            diagnostics: Vec::new(),
            recovering: false,
            depth: 0,
            delims: Vec::new(),
            past_imports: false,
        }
    }

    fn is_skipped(&self, index: usize) -> bool {
        let kind = self.tokens[index].kind;
        kind.is_trivia() || (kind == TokenKind::Newline && self.layout.ignored[index])
    }

    /// Returns the index of the first token at or after `from` that the
    /// grammar sees. An index at or past the limit stands for the end.
    fn next_index(&self, from: usize) -> usize {
        let mut index = from;
        while index < self.limit && self.is_skipped(index) {
            index += 1;
        }
        index
    }

    /// Returns the index one past the last token of the string or
    /// interpolation that starts at `pos`, within the limit.
    fn end_of_string(&self) -> usize {
        self.layout.ends[self.pos].max(self.pos + 1).min(self.limit)
    }

    fn nth_index(&self, n: usize) -> usize {
        let mut index = self.next_index(self.pos);
        for _ in 0..n {
            if index >= self.limit {
                break;
            }
            index = self.next_index(index + 1);
        }
        index
    }

    /// Returns the kind of the `n`th token ahead that the grammar sees.
    fn nth(&self, n: usize) -> TokenKind {
        let index = self.nth_index(n);
        if index >= self.limit {
            TokenKind::Eof
        } else {
            self.tokens[index].kind
        }
    }

    fn current(&self) -> TokenKind {
        self.nth(0)
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current() == kind
    }

    fn current_text(&self) -> &'s str {
        self.text(self.nth_index(0))
    }

    fn text(&self, index: usize) -> &'s str {
        self.source
            .get(self.tokens[index].span.range())
            .unwrap_or_default()
    }

    fn current_span(&self) -> Span {
        self.tokens[self.nth_index(0)].span
    }

    /// Adds the skipped tokens before the next visible token to the tree.
    fn flush(&mut self) {
        let next = self.next_index(self.pos);
        for index in self.pos..next {
            self.builder.token(index);
        }
        self.pos = next;
    }

    /// Adds the next visible token to the tree. A string start adds the
    /// whole string, so that its parts never reach the grammar.
    fn bump(&mut self) {
        self.flush();
        if self.pos >= self.limit {
            return;
        }
        if is_string_start(self.tokens[self.pos].kind) {
            let end = self.end_of_string();
            while self.pos < end {
                self.builder.token(self.pos);
                self.pos += 1;
            }
        } else {
            self.builder.token(self.pos);
            self.pos += 1;
        }
    }

    /// Adds the token at `pos` to the tree, whatever its kind.
    fn bump_raw(&mut self) {
        if self.pos < self.limit {
            self.builder.token(self.pos);
            self.pos += 1;
        }
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        let found = self.at(kind);
        if found {
            self.bump();
        }
        found
    }

    fn start_node(&mut self, kind: NodeKind) {
        self.flush();
        self.builder.start_node(kind);
    }

    fn checkpoint(&mut self) -> usize {
        self.flush();
        self.builder.checkpoint()
    }

    fn start_node_at(&mut self, checkpoint: usize, kind: NodeKind) {
        self.builder.start_node_at(checkpoint, kind);
    }

    fn finish_node(&mut self) {
        let empty_at = self.tokens[self.pos.min(self.tokens.len() - 1)].span.start;
        self.builder.finish_node(&self.tokens, empty_at);
    }

    /// Bumps the opening delimiter at the current position and returns its
    /// token index.
    fn open(&mut self) -> usize {
        let index = self.nth_index(0);
        self.bump();
        self.delims.push(index);
        index
    }

    /// Bumps the closing delimiter for the opener at `opener`, or reports
    /// the opener as unclosed.
    fn close(&mut self, opener: usize, closer: TokenKind) {
        if !self.eat(closer) {
            let open = self.tokens[opener];
            let close = closer_text(closer);
            if self.at(TokenKind::Eof) || self.closes_outer(self.current()) {
                self.error(
                    "E0210",
                    open.span,
                    format!("unclosed `{}`", opener_text(open.kind)),
                    Some(format!("add `{close}` to close it")),
                );
            } else {
                self.expected(&format!("`{close}`"));
            }
        }
        if self.delims.last() == Some(&opener) {
            self.delims.pop();
        }
    }

    /// Returns whether `kind` closes a delimiter that encloses the innermost
    /// open one.
    fn closes_outer(&self, kind: TokenKind) -> bool {
        let Some(opener) = opener_of(kind) else {
            return false;
        };
        let outer = self.delims.len().saturating_sub(1);
        self.delims[..outer]
            .iter()
            .any(|&index| self.tokens[index].kind == opener)
    }

    /// Enters one level of nesting. Returns `false`, after reporting the
    /// nesting and skipping the nested text, when the limit is reached.
    fn enter(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            self.too_deep();
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn too_deep(&mut self) {
        self.error(
            "E0211",
            self.current_span(),
            format!("nesting is deeper than the limit of {MAX_DEPTH} levels"),
            Some("split the code into smaller parts with `let` bindings".to_owned()),
        );
        self.skip_to_separator();
    }

    /// Skips tokens up to the next separator or closing delimiter that is
    /// not nested in the skipped text, and wraps them in an error node.
    fn skip_to_separator(&mut self) {
        use TokenKind as T;
        let mut balance = 0usize;
        let mut started = false;
        loop {
            match self.current() {
                T::Eof => break,
                T::LBrace | T::LBracket | T::LParen => balance += 1,
                T::RBrace | T::RBracket | T::RParen => {
                    if balance == 0 {
                        break;
                    }
                    balance -= 1;
                }
                T::Newline | T::Comma if balance == 0 => break,
                _ => {}
            }
            if !started {
                self.start_node(NodeKind::Error);
                started = true;
            }
            self.bump();
        }
        if started {
            self.finish_node();
        }
    }

    /// Wraps the current token in an error node.
    fn bump_error(&mut self) {
        if self.at(TokenKind::Eof) {
            return;
        }
        self.start_node(NodeKind::Error);
        self.bump();
        self.finish_node();
    }

    /// Parses the elements of a `{ }` or `[ ]` sequence after its opener,
    /// separated by line feeds and commas, and then its closer.
    fn sequence(
        &mut self,
        opener: usize,
        closer: TokenKind,
        what: &str,
        mut element: impl FnMut(&mut Self),
    ) {
        loop {
            self.eat_separators();
            let kind = self.current();
            if kind == closer || kind == TokenKind::Eof || self.closes_outer(kind) {
                break;
            }
            if is_closer(kind) {
                self.expected(what);
                self.bump_error();
                continue;
            }
            self.element(&mut element);
            let kind = self.current();
            let separated = matches!(kind, TokenKind::Newline | TokenKind::Comma | TokenKind::Eof);
            if !separated && kind != closer && !self.closes_outer(kind) {
                self.expected(&format!("a line break, `,` or `{}`", closer_text(closer)));
                self.skip_to_separator();
            }
        }
        self.close(opener, closer);
    }

    /// Parses the elements of a `( )` list after its opener, separated by
    /// single commas, and then its closer.
    fn comma_list(
        &mut self,
        opener: usize,
        closer: TokenKind,
        what: &str,
        mut element: impl FnMut(&mut Self),
    ) {
        loop {
            let kind = self.current();
            if kind == closer || kind == TokenKind::Eof || self.closes_outer(kind) {
                break;
            }
            if is_closer(kind) || kind == TokenKind::Comma {
                self.expected(what);
                self.bump_error();
                continue;
            }
            self.element(&mut element);
            let kind = self.current();
            if kind == TokenKind::Comma {
                self.bump();
                self.recovering = false;
            } else if kind != closer && kind != TokenKind::Eof && !self.closes_outer(kind) {
                self.expected(&format!("`,` or `{}`", closer_text(closer)));
                self.skip_to_separator();
            }
        }
        self.close(opener, closer);
    }

    /// Parses one element of a sequence, and skips a token when the element
    /// consumed nothing, so that every sequence makes progress.
    fn element(&mut self, element: &mut impl FnMut(&mut Self)) {
        let start = self.next_index(self.pos);
        element(self);
        if self.next_index(self.pos) == start {
            self.bump_error();
        }
    }

    /// Bumps line feeds and commas. Passing one ends error recovery.
    fn eat_separators(&mut self) {
        while matches!(self.current(), TokenKind::Newline | TokenKind::Comma) {
            self.bump();
            self.recovering = false;
        }
    }

    /// Bumps line feeds when the first token after them is one of `kinds`.
    fn skip_newlines_before(&mut self, kinds: &[TokenKind]) {
        let mut index = self.next_index(self.pos);
        let mut count = 0;
        while index < self.limit && self.tokens[index].kind == TokenKind::Newline {
            index = self.next_index(index + 1);
            count += 1;
        }
        if count > 0 && index < self.limit && kinds.contains(&self.tokens[index].kind) {
            for _ in 0..count {
                self.bump();
            }
        }
    }

    /// Reports an error that leaves the parser unsure of the structure.
    /// Errors are suppressed until the next separator.
    fn error(&mut self, code: &'static str, span: Span, message: String, help: Option<String>) {
        if !self.recovering {
            self.diagnostics.push(Diagnostic {
                code,
                message,
                span,
                help,
            });
        }
        self.recovering = true;
    }

    /// Reports an error after which parsing continues as if the text were
    /// valid.
    fn soft_error(
        &mut self,
        code: &'static str,
        span: Span,
        message: String,
        help: Option<String>,
    ) {
        if !self.recovering {
            self.diagnostics.push(Diagnostic {
                code,
                message,
                span,
                help,
            });
        }
    }

    /// Reports that `what` was expected at the current token.
    fn expected(&mut self, what: &str) {
        self.unexpected(
            "E0210",
            |found| format!("expected {what}, found {found}"),
            None,
        );
    }

    /// Reports the current token with a message built from its description.
    /// A lexical error token has its own diagnostic already, so it only
    /// starts error recovery.
    fn unexpected(
        &mut self,
        code: &'static str,
        message: impl FnOnce(String) -> String,
        help: Option<String>,
    ) {
        let index = self.real_next();
        let token = self.tokens[index];
        if token.kind == TokenKind::Error {
            self.recovering = true;
            return;
        }
        let found = self.describe(index);
        self.error(code, token.span, message(found), help);
    }

    /// Bumps a name, or reports that `what` was expected.
    fn name(&mut self, what: &str) {
        if is_name(self.current()) {
            self.bump();
        } else {
            self.expected(what);
        }
    }

    /// Returns the index of the next token the grammar would see without
    /// the limit, for describing what was found.
    fn real_next(&self) -> usize {
        let last = self.tokens.len() - 1;
        let mut index = self.next_index(self.pos);
        while index < last && self.is_skipped(index) {
            index += 1;
        }
        index.min(last)
    }

    fn describe(&self, index: usize) -> String {
        use TokenKind as T;
        let token = self.tokens[index];
        match token.kind {
            T::Newline => "a line break".to_owned(),
            T::Eof => "the end of the file".to_owned(),
            T::StrStart | T::MStrStart | T::RawStrStart | T::RawMStrStart => "a string".to_owned(),
            T::InterpEnd => "`}`".to_owned(),
            _ => format!("`{}`", self.text(index)),
        }
    }
}

/// Facts about the token stream that depend on more than one token: which
/// line feeds the grammar ignores, and where strings and interpolations
/// end.
struct Layout {
    ignored: Vec<bool>,
    /// For the start of a string, the index one past its last token. For
    /// `#{`, the index of its `}`, or of the token before which it was cut
    /// off by the end of the line or file.
    ends: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    Paren,
    Brace,
    Bracket,
    Interp(usize),
    Str { start: usize, single_line: bool },
}

impl Open {
    fn bracket_slot(self) -> Option<usize> {
        match self {
            Self::Paren => Some(0),
            Self::Brace => Some(1),
            Self::Bracket => Some(2),
            _ => None,
        }
    }
}

impl Layout {
    fn new(tokens: &[Token]) -> Self {
        use TokenKind as T;
        let mut layout = Self {
            ignored: vec![false; tokens.len()],
            ends: vec![0; tokens.len()],
        };
        // Each entry counts the open brackets of each kind from the nearest
        // string or interpolation up to and including it, so that a closer
        // finds its opener in constant time and never crosses a string.
        let mut stack: Vec<(Open, [usize; 3])> = Vec::new();
        let mut single_line_strings = 0usize;
        let mut previous: Option<TokenKind> = None;
        let mut pending: Vec<usize> = Vec::new();

        for (index, token) in tokens.iter().enumerate() {
            let kind = token.kind;
            if kind.is_trivia() {
                continue;
            }
            if kind == T::Newline {
                while single_line_strings > 0 {
                    layout.pop(&mut stack, &mut single_line_strings, index);
                }
                let in_parens = matches!(stack.last(), Some((Open::Paren | Open::Interp(_), _)));
                if in_parens || previous.is_some_and(continues_after) {
                    layout.ignored[index] = true;
                } else {
                    pending.push(index);
                }
                continue;
            }
            let continues = continues_before(kind);
            for newline in pending.drain(..) {
                layout.ignored[newline] = continues;
            }
            let push = |stack: &mut Vec<(Open, [usize; 3])>, open: Open| {
                let mut counts = match (open, stack.last()) {
                    (Open::Interp(_) | Open::Str { .. }, _) | (_, None) => [0; 3],
                    (_, Some(&(_, counts))) => counts,
                };
                if let Some(slot) = open.bracket_slot() {
                    counts[slot] += 1;
                }
                stack.push((open, counts));
            };
            match kind {
                T::LParen => push(&mut stack, Open::Paren),
                T::LBrace => push(&mut stack, Open::Brace),
                T::LBracket => push(&mut stack, Open::Bracket),
                T::RParen | T::RBrace | T::RBracket => {
                    let slot = match kind {
                        T::RParen => 0,
                        T::RBrace => 1,
                        _ => 2,
                    };
                    if stack.last().is_some_and(|&(_, counts)| counts[slot] > 0) {
                        while let Some((open, _)) = stack.pop() {
                            if open.bracket_slot() == Some(slot) {
                                break;
                            }
                        }
                    }
                }
                T::InterpStart => push(&mut stack, Open::Interp(index)),
                T::InterpEnd => {
                    if stack
                        .iter()
                        .any(|&(open, _)| matches!(open, Open::Interp(_)))
                    {
                        while let Some(open) =
                            layout.pop(&mut stack, &mut single_line_strings, index)
                        {
                            if matches!(open, Open::Interp(_)) {
                                break;
                            }
                        }
                    }
                }
                T::StrStart | T::RawStrStart => {
                    single_line_strings += 1;
                    push(
                        &mut stack,
                        Open::Str {
                            start: index,
                            single_line: true,
                        },
                    );
                }
                T::MStrStart | T::RawMStrStart => push(
                    &mut stack,
                    Open::Str {
                        start: index,
                        single_line: false,
                    },
                ),
                T::StrEnd | T::MStrEnd => {
                    if stack
                        .iter()
                        .any(|&(open, _)| matches!(open, Open::Str { .. }))
                    {
                        while let Some(open) =
                            layout.pop(&mut stack, &mut single_line_strings, index + 1)
                        {
                            if matches!(open, Open::Str { .. }) {
                                break;
                            }
                        }
                    }
                }
                T::Eof => {
                    while layout
                        .pop(&mut stack, &mut single_line_strings, index)
                        .is_some()
                    {}
                }
                _ => {}
            }
            previous = Some(kind);
        }
        layout
    }

    /// Pops the innermost open construct. A string or interpolation popped
    /// here ends at `end`.
    fn pop(
        &mut self,
        stack: &mut Vec<(Open, [usize; 3])>,
        single_line_strings: &mut usize,
        end: usize,
    ) -> Option<Open> {
        let (open, _) = stack.pop()?;
        match open {
            Open::Interp(start) => self.ends[start] = end,
            Open::Str { start, single_line } => {
                self.ends[start] = end;
                if single_line {
                    *single_line_strings -= 1;
                }
            }
            _ => {}
        }
        Some(open)
    }
}

/// Returns whether a line feed after `kind` is ignored, because an
/// expression cannot end with it.
fn continues_after(kind: TokenKind) -> bool {
    use TokenKind as T;
    matches!(
        kind,
        T::LBrace
            | T::LBracket
            | T::LParen
            | T::Comma
            | T::Eq
            | T::Dot
            | T::QuestionDot
            | T::QuestionQuestion
            | T::Colon
            | T::Arrow
            | T::FatArrow
            | T::Pipe
            | T::Plus
            | T::Minus
            | T::Star
            | T::Slash
            | T::Percent
            | T::PlusPlus
            | T::EqEq
            | T::NotEq
            | T::Lt
            | T::LtEq
            | T::GtEq
            | T::AndAnd
            | T::OrOr
            | T::Bang
            | T::InKw
            | T::ElseKw
            | T::WhereKw
            | T::ImportKw
            | T::IncludeKw
            | T::AsKw
    )
}

/// Returns whether a line feed before `kind` is ignored, because a
/// statement cannot start with it.
fn continues_before(kind: TokenKind) -> bool {
    use TokenKind as T;
    matches!(
        kind,
        T::Dot
            | T::QuestionDot
            | T::QuestionQuestion
            | T::AndAnd
            | T::OrOr
            | T::PlusPlus
            | T::ElseKw
            | T::Pipe
            | T::FatArrow
    )
}

fn is_string_start(kind: TokenKind) -> bool {
    use TokenKind as T;
    matches!(
        kind,
        T::StrStart | T::MStrStart | T::RawStrStart | T::RawMStrStart
    )
}

/// Returns whether `kind` is an identifier, which the grammar accepts as a
/// name whatever its case.
fn is_name(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Ident | TokenKind::TypeIdent)
}

fn opener_of(closer: TokenKind) -> Option<TokenKind> {
    use TokenKind as T;
    match closer {
        T::RParen => Some(T::LParen),
        T::RBracket => Some(T::LBracket),
        T::RBrace => Some(T::LBrace),
        _ => None,
    }
}

fn is_closer(kind: TokenKind) -> bool {
    opener_of(kind).is_some()
}

fn opener_text(kind: TokenKind) -> &'static str {
    use TokenKind as T;
    match kind {
        T::LParen => "(",
        T::LBracket => "[",
        _ => "{",
    }
}

fn closer_text(kind: TokenKind) -> &'static str {
    use TokenKind as T;
    match kind {
        T::RParen => ")",
        T::RBracket => "]",
        _ => "}",
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;

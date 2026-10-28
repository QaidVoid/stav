//! Lexical analysis, turning source text into tokens.
//!
//! The lexer is lossless. Every byte of the source belongs to exactly one
//! token, including whitespace, comments and line feeds, so concatenating the
//! text of all tokens reproduces the source.
//!
//! # Strings
//!
//! A string is a sequence of tokens rather than a single token, so that a
//! parser can see its parts:
//!
//! ```text
//! string = start { StrText | StrEscape | StrLayout | Error | interpolation } [ end ]
//! interpolation = InterpStart { token } [ InterpEnd ]
//! ```
//!
//! The start is one of [`TokenKind::StrStart`], [`TokenKind::MStrStart`],
//! [`TokenKind::RawStrStart`] and [`TokenKind::RawMStrStart`], and the end is
//! [`TokenKind::StrEnd`] or [`TokenKind::MStrEnd`]. The tokens of an
//! interpolation are ordinary tokens and may contain further strings. The
//! end tokens are missing only when the string is unterminated, which is
//! always reported.
//!
//! The value of a string is the concatenation of its [`TokenKind::StrText`]
//! tokens, the characters its [`TokenKind::StrEscape`] tokens stand for and
//! the values of its interpolations. [`TokenKind::StrLayout`] tokens are
//! skipped. For multi-line strings, the lexer has already split off the
//! indentation of the closing delimiter from every line, so the value needs
//! no further processing.
//!
//! A single-line string and its interpolations end at the end of the line.
//! An interpolation inside a multi-line string may span several lines.

use crate::diagnostic::Diagnostic;
use crate::span::Span;
use crate::token::{Token, TokenKind};

const MAX_IDENT_LEN: usize = 255;

/// The tokens and lexical errors of one source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lexed {
    /// Every token of the source in order. Spans are contiguous and start at
    /// offset zero. The last token is always [`TokenKind::Eof`].
    pub tokens: Vec<Token>,
    /// The lexical errors, ordered by the start of their span.
    pub diagnostics: Vec<Diagnostic>,
}

/// Splits `source` into tokens.
///
/// Lexing never fails and never panics. Text that cannot be lexed becomes
/// [`TokenKind::Error`] tokens or tokens with a diagnostic, and lexing
/// continues after it.
///
/// # Examples
///
/// ```
/// use stav::{TokenKind, lex};
///
/// let source = "port = 8080 # default\n";
/// let lexed = lex(source);
/// assert!(lexed.diagnostics.is_empty());
///
/// let kinds: Vec<TokenKind> = lexed
///     .tokens
///     .iter()
///     .map(|t| t.kind)
///     .filter(|k| !k.is_trivia())
///     .collect();
/// assert_eq!(
///     kinds,
///     [TokenKind::Ident, TokenKind::Eq, TokenKind::Int, TokenKind::Newline, TokenKind::Eof],
/// );
///
/// let text: String = lexed.tokens.iter().map(|t| t.text(source)).collect();
/// assert_eq!(text, source);
/// ```
pub fn lex(source: &str) -> Lexed {
    let mut lexer = Lexer {
        src: source,
        bytes: source.as_bytes(),
        pos: 0,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
        modes: Vec::new(),
        single_line_from: None,
        layout_splits: Vec::new(),
    };
    lexer.run();
    let mut diagnostics = lexer.diagnostics;
    diagnostics.sort_by_key(|d| d.span.start);
    Lexed {
        tokens: lexer.tokens,
        diagnostics,
    }
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
    /// The strings and interpolations the lexer is inside of, innermost
    /// last. Nesting is tracked here rather than by recursion so that deeply
    /// nested input cannot overflow the stack.
    modes: Vec<Mode>,
    /// The index in `modes` of the outermost open single-line string. Every
    /// mode from there on ends at the next line feed.
    single_line_from: Option<usize>,
    /// Indentation tokens of multi-line strings that are deeper than the
    /// indentation of the closing `"""`, given as the token start and the
    /// length of the closing indentation. That length stays layout, and only
    /// the excess indentation beyond it becomes literal text. The splits are
    /// applied in one pass at the end, because rewriting tokens when each
    /// string closes is quadratic in the nesting depth.
    layout_splits: Vec<(usize, usize)>,
}

enum Mode {
    /// Inside `#{ }`, counting the `{` that are still open within it.
    Interp {
        open_braces: u32,
    },
    Str(StrFrame),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StrForm {
    Plain,
    Multi,
    Raw,
    RawMulti,
}

impl StrForm {
    fn opener_len(self) -> usize {
        match self {
            Self::Plain => 1,
            Self::Raw => 2,
            Self::Multi => 3,
            Self::RawMulti => 4,
        }
    }

    fn is_multi(self) -> bool {
        matches!(self, Self::Multi | Self::RawMulti)
    }

    fn is_raw(self) -> bool {
        matches!(self, Self::Raw | Self::RawMulti)
    }
}

struct StrFrame {
    form: StrForm,
    start: usize,
    /// The lines of a multi-line string that started inside its literal
    /// text, in order.
    lines: Vec<Line>,
    bad_opener: bool,
}

/// A line of a multi-line string and the end of its leading whitespace.
#[derive(Clone, Copy)]
struct Line {
    start: usize,
    indent_end: usize,
}

impl<'a> Lexer<'a> {
    fn run(&mut self) {
        if let Some(cr) = self.src.find('\r') {
            // One error covers every carriage return in the file, because
            // they nearly always come from line endings throughout it.
            self.error(
                "E0114",
                cr,
                cr + 1,
                "carriage return is not allowed".to_owned(),
                Some("save the file with LF line endings".to_owned()),
            );
        }
        if self.src.starts_with('\u{feff}') {
            self.pos = '\u{feff}'.len_utf8();
            self.push(TokenKind::Error, 0);
            self.error(
                "E0101",
                0,
                self.pos,
                "byte order mark is not allowed".to_owned(),
                Some("save the file as UTF-8 without a byte order mark".to_owned()),
            );
        }
        while self.pos < self.bytes.len() {
            if self.single_line_from.is_some() && self.peek(0) == Some(b'\n') {
                self.end_single_line_strings();
                continue;
            }
            match self.modes.last() {
                Some(Mode::Str(frame)) => match frame.form {
                    StrForm::Plain | StrForm::Raw => self.lex_single_line(frame.form),
                    StrForm::Multi | StrForm::RawMulti => self.lex_multi_line(frame.form),
                },
                _ => self.lex_code(),
            }
        }
        while let Some(mode) = self.pop_mode() {
            if let Mode::Str(frame) = mode {
                self.unterminated(&frame, false);
            }
        }
        self.apply_layout_splits();
        let end = self.bytes.len();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span { start: end, end },
        });
    }

    fn lex_code(&mut self) {
        use TokenKind as T;

        let start = self.pos;
        let c = self.current();
        let next = self.peek(1);
        let (kind, len) = match c {
            ' ' | '\t' => {
                self.eat_while(|b| b == b' ' || b == b'\t');
                return self.push(T::Whitespace, start);
            }
            '\n' => (T::Newline, 1),
            '#' => return self.lex_comment(),
            '"' if self.at("\"\"\"") => return self.open_string(StrForm::Multi),
            '"' => return self.open_string(StrForm::Plain),
            'r' if self.at("r\"\"\"") => return self.open_string(StrForm::RawMulti),
            'r' if next == Some(b'"') => return self.open_string(StrForm::Raw),
            '0'..='9' => return self.lex_number(),
            c if is_ident_start(c) => return self.lex_word(),
            '.' if self.at("./") || self.at("../") => return self.lex_path(),
            '.' if self.at("..=") => (T::DotDotEq, 3),
            '.' if next == Some(b'.') => (T::DotDot, 2),
            '.' => (T::Dot, 1),
            '?' if next == Some(b'.') => (T::QuestionDot, 2),
            '?' if next == Some(b'?') => (T::QuestionQuestion, 2),
            '{' => {
                if let Some(Mode::Interp { open_braces }) = self.modes.last_mut() {
                    *open_braces = open_braces.saturating_add(1);
                }
                (T::LBrace, 1)
            }
            '}' => match self.modes.last_mut() {
                Some(Mode::Interp { open_braces: 0 }) => {
                    self.pop_mode();
                    (T::InterpEnd, 1)
                }
                Some(Mode::Interp { open_braces }) => {
                    *open_braces -= 1;
                    (T::RBrace, 1)
                }
                _ => (T::RBrace, 1),
            },
            '[' => (T::LBracket, 1),
            ']' => (T::RBracket, 1),
            '(' => (T::LParen, 1),
            ')' => (T::RParen, 1),
            ',' => (T::Comma, 1),
            ':' => (T::Colon, 1),
            '=' if next == Some(b'=') => (T::EqEq, 2),
            '=' if next == Some(b'>') => (T::FatArrow, 2),
            '=' => (T::Eq, 1),
            '-' if next == Some(b'>') => (T::Arrow, 2),
            '-' => (T::Minus, 1),
            '+' if next == Some(b'+') => (T::PlusPlus, 2),
            '+' => (T::Plus, 1),
            '*' => (T::Star, 1),
            '/' => (T::Slash, 1),
            '%' => (T::Percent, 1),
            '<' if next == Some(b'=') => (T::LtEq, 2),
            '<' => (T::Lt, 1),
            '>' if next == Some(b'=') => (T::GtEq, 2),
            '>' => (T::Gt, 1),
            '!' if next == Some(b'=') => (T::NotEq, 2),
            '!' => (T::Bang, 1),
            '&' if next == Some(b'&') => (T::AndAnd, 2),
            '|' if next == Some(b'|') => (T::OrOr, 2),
            '|' => (T::Pipe, 1),
            c => return self.lex_unexpected(c),
        };
        self.pos += len;
        self.push(kind, start);
    }

    fn lex_unexpected(&mut self, c: char) {
        let start = self.pos;
        self.pos += c.len_utf8();
        self.push(TokenKind::Error, start);
        let end = self.pos;
        match c {
            // Every carriage return is already covered by one file-level error.
            '\r' => {}
            '@' => self.error(
                "E0105",
                start,
                end,
                "`@` is only allowed inside a symbol word".to_owned(),
                Some("write the name and version without spaces, like `name@1.2`".to_owned()),
            ),
            '&' => self.error(
                "E0113",
                start,
                end,
                "unexpected character `&`".to_owned(),
                Some("write `&&` for logical and".to_owned()),
            ),
            c if c.is_whitespace() => self.error(
                "E0102",
                start,
                end,
                format!("invalid whitespace character U+{:04X}", c as u32),
                Some("use spaces or tabs".to_owned()),
            ),
            c => self.error(
                "E0113",
                start,
                end,
                format!("unexpected character {}", describe_char(c)),
                None,
            ),
        }
    }

    fn lex_comment(&mut self) {
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
            self.bump_char();
        }
        let text = self.slice(start, self.pos);
        let doc = text
            .strip_prefix("##")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '));
        let kind = if doc {
            TokenKind::DocComment
        } else {
            TokenKind::Comment
        };
        self.push(kind, start);
    }

    fn lex_word(&mut self) {
        let start = self.pos;
        self.pos += 1;
        self.eat_word_rest();
        let ident_end = self.pos;
        if let Some(keyword) = TokenKind::keyword(self.slice(start, ident_end)) {
            return self.push(keyword, start);
        }
        if ident_end - start > MAX_IDENT_LEN {
            self.error(
                "E0104",
                start,
                ident_end,
                format!(
                    "identifier is {} bytes long, more than the limit of {MAX_IDENT_LEN}",
                    ident_end - start
                ),
                Some(format!("shorten the identifier to {MAX_IDENT_LEN} bytes")),
            );
        }
        let kind = if self.eat_symbol_suffix() {
            TokenKind::SymWord
        } else if self.bytes[start].is_ascii_uppercase() {
            TokenKind::TypeIdent
        } else {
            TokenKind::Ident
        };
        self.push(kind, start);
    }

    fn eat_word_rest(&mut self) {
        loop {
            match (self.peek(0), self.peek(1)) {
                (Some(b), _) if is_ident_char(b) => self.pos += 1,
                (Some(b'-'), Some(b)) if is_ident_char(b) => self.pos += 2,
                (Some(b'.'), Some(b)) if b.is_ascii_digit() => {
                    self.pos += 2;
                    self.eat_while(|b| b.is_ascii_digit());
                }
                _ => break,
            }
        }
    }

    fn eat_symbol_suffix(&mut self) -> bool {
        let variant_follows =
            |lexer: &Self| lexer.peek(0) == Some(b':') && lexer.peek(1).is_some_and(is_lower);
        if self.peek(0) == Some(b'@') {
            let version_len = self.bytes[self.pos + 1..]
                .iter()
                .take_while(|&&b| is_version_char(b))
                .count();
            if version_len == 0 {
                return false;
            }
            self.pos += 1 + version_len;
            if variant_follows(self) {
                self.pos += 2;
                self.eat_while(is_variant_char);
            }
            true
        } else if variant_follows(self) {
            self.pos += 2;
            self.eat_while(is_variant_char);
            true
        } else {
            false
        }
    }

    fn lex_number(&mut self) {
        let start = self.pos;
        let radix = match (self.peek(0), self.peek(1)) {
            (Some(b'0'), Some(b'x')) => 16,
            (Some(b'0'), Some(b'o')) => 8,
            (Some(b'0'), Some(b'b')) => 2,
            _ => 10,
        };
        let mut is_float = false;
        if radix != 10 {
            if self
                .peek(2)
                .is_some_and(|b| b == b'_' || is_digit_in(b, radix))
            {
                self.pos += 2;
                self.eat_while(|b| b == b'_' || is_digit_in(b, radix));
            } else {
                // Without digits the prefix letter is reported as a name
                // glued to the number `0`.
                self.pos += 1;
            }
        } else {
            self.eat_while(is_dec_or_underscore);
            if self.peek(0) == Some(b'.') && self.peek(1).is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
                self.eat_while(is_dec_or_underscore);
                is_float = true;
            }
            if matches!(self.peek(0), Some(b'e' | b'E')) {
                let sign = usize::from(matches!(self.peek(1), Some(b'+' | b'-')));
                if self.peek(1 + sign).is_some_and(|b| b.is_ascii_digit()) {
                    self.pos += 1 + sign;
                    self.eat_while(is_dec_or_underscore);
                    is_float = true;
                }
            }
        }

        if self.peek(0).is_some_and(is_ident_char) {
            self.eat_word_rest();
            self.eat_symbol_suffix();
            self.push(TokenKind::Error, start);
            let text = self.slice(start, self.pos);
            return self.error(
                "E0103",
                start,
                self.pos,
                format!("`{text}` is not a valid token"),
                Some(
                    "separate the number from the name, or start the name with a letter".to_owned(),
                ),
            );
        }
        let text = self.slice(start, self.pos);
        if !separators_between_digits(text, radix) {
            self.push(TokenKind::Error, start);
            return self.error(
                "E0103",
                start,
                self.pos,
                format!("`{text}` is not a valid token"),
                Some("write `_` only between two digits".to_owned()),
            );
        }
        if is_float {
            self.push(TokenKind::Float, start);
            if parse_float(text).is_none() {
                self.error(
                    "E0107",
                    start,
                    self.pos,
                    format!("float `{text}` is too large to be finite"),
                    None,
                );
            }
        } else {
            self.push(TokenKind::Int, start);
            if parse_int(text).is_none() {
                self.error(
                    "E0107",
                    start,
                    self.pos,
                    format!("integer `{text}` does not fit in a signed 64-bit integer"),
                    Some(format!("the largest integer is {}", i64::MAX)),
                );
            }
        }
    }

    fn push_mode(&mut self, mode: Mode) {
        let single_line = matches!(
            mode,
            Mode::Str(StrFrame {
                form: StrForm::Plain | StrForm::Raw,
                ..
            })
        );
        if single_line && self.single_line_from.is_none() {
            self.single_line_from = Some(self.modes.len());
        }
        self.modes.push(mode);
    }

    fn pop_mode(&mut self) -> Option<Mode> {
        let mode = self.modes.pop();
        if self.single_line_from == Some(self.modes.len()) {
            self.single_line_from = None;
        }
        mode
    }

    /// Ends, at a line feed, every string and interpolation from the
    /// outermost open single-line string inward. Each of those strings is
    /// unterminated, even a multi-line string nested inside an
    /// interpolation, because it lies within a single line.
    fn end_single_line_strings(&mut self) {
        let Some(from) = self.single_line_from else {
            return;
        };
        while self.modes.len() > from {
            if let Some(Mode::Str(frame)) = self.pop_mode() {
                self.unterminated(&frame, true);
            }
        }
    }

    fn open_string(&mut self, form: StrForm) {
        let start = self.pos;
        self.pos += form.opener_len();
        let kind = match form {
            StrForm::Plain => TokenKind::StrStart,
            StrForm::Multi => TokenKind::MStrStart,
            StrForm::Raw => TokenKind::RawStrStart,
            StrForm::RawMulti => TokenKind::RawMStrStart,
        };
        let mut frame = StrFrame {
            form,
            start,
            lines: Vec::new(),
            bad_opener: false,
        };
        self.push(kind, start);
        // Inside a single-line string, the line feed after the opener ends
        // the enclosing string instead of starting the content, so a
        // multi-line string there can never be valid. Text after the opener
        // is reported at the opener and lexed as content, so that a closing
        // `"""` on the same line still ends the string.
        if form.is_multi() && self.single_line_from.is_some() && self.peek(0) != Some(b'\n') {
            frame.bad_opener = true;
            self.bad_opener(start);
        }
        if form.is_multi() && self.single_line_from.is_none() {
            if self.peek(0) != Some(b'\n') {
                frame.bad_opener = true;
                let rest = self.pos;
                while self.peek(0).is_some_and(|b| b != b'\n') && !self.at("\"\"\"") {
                    self.bump_char();
                }
                if self.pos > rest {
                    self.push(TokenKind::Error, rest);
                }
                self.bad_opener(start);
            }
            if self.peek(0) == Some(b'\n') {
                let line_feed = self.pos;
                self.pos += 1;
                self.push(TokenKind::StrLayout, line_feed);
                frame.lines.push(self.begin_line());
            }
        }
        self.push_mode(Mode::Str(frame));
    }

    fn bad_opener(&mut self, start: usize) {
        self.error(
            "E0110",
            start,
            self.pos,
            "the opening `\"\"\"` must be followed directly by a line break".to_owned(),
            Some("start the content of the string on the next line".to_owned()),
        );
    }

    fn open_interp(&mut self) {
        let start = self.pos;
        self.pos += 2;
        self.push(TokenKind::InterpStart, start);
        self.push_mode(Mode::Interp { open_braces: 0 });
    }

    fn lex_single_line(&mut self, form: StrForm) {
        let start = self.pos;
        let plain = form == StrForm::Plain;
        let interp_follows = |lexer: &Self| lexer.at("#{");
        match self.peek(0) {
            None => {}
            Some(b'"') => {
                self.pos += 1;
                self.push(TokenKind::StrEnd, start);
                self.pop_mode();
            }
            Some(b'\\') if plain => self.lex_escape(),
            Some(b'#') if plain && interp_follows(self) => self.open_interp(),
            Some(_) => {
                while let Some(b) = self.peek(0) {
                    let special = b == b'\\' || interp_follows(self);
                    if b == b'"' || b == b'\n' || (plain && special) {
                        break;
                    }
                    self.bump_char();
                }
                self.push(TokenKind::StrText, start);
            }
        }
    }

    fn lex_escape(&mut self) {
        let start = self.pos;
        self.pos += 1;
        if matches!(self.peek(0), None | Some(b'\n')) {
            // The string is reported as unterminated, so the lone backslash
            // gets no error of its own.
            return self.push(TokenKind::StrText, start);
        }
        let c = self.current();
        self.pos += c.len_utf8();
        let valid = match c {
            '\\' | '"' | 'n' | 't' | 'r' | '0' | '#' => true,
            'u' => self.eat_unicode_escape(),
            _ => false,
        };
        self.push(TokenKind::StrEscape, start);
        if !valid {
            let text = self.slice(start, self.pos);
            let (message, help) = if c == 'u' {
                (
                    format!("invalid unicode escape `{text}`"),
                    "write `\\u{` followed by 1 to 6 hex digits of a unicode scalar value and `}`",
                )
            } else {
                (
                    format!("unknown escape `{text}`"),
                    "write `\\\\` for a literal backslash",
                )
            };
            self.error("E0109", start, self.pos, message, Some(help.to_owned()));
        }
    }

    fn eat_unicode_escape(&mut self) -> bool {
        if self.peek(0) != Some(b'{') {
            return false;
        }
        self.pos += 1;
        let digits_start = self.pos;
        self.eat_while(|b| b.is_ascii_hexdigit());
        let digits = self.slice(digits_start, self.pos);
        if self.peek(0) != Some(b'}') {
            return false;
        }
        self.pos += 1;
        digits.len() <= 6
            && u32::from_str_radix(digits, 16)
                .ok()
                .and_then(char::from_u32)
                .is_some()
    }

    fn lex_multi_line(&mut self, form: StrForm) {
        let start = self.pos;
        let raw = form.is_raw();
        if self.at("\"\"\"") {
            return self.close_multi_line();
        }
        if !raw && self.at("#{") {
            return self.open_interp();
        }
        if !raw && self.at("\\#{") {
            self.pos += 2;
            return self.push(TokenKind::StrEscape, start);
        }
        let mut line_ended = false;
        while let Some(b) = self.peek(0) {
            // Inside a single-line string, the line feed ends that string
            // and everything nested in it, which `run` takes care of.
            if b == b'\n' && self.single_line_from.is_some() {
                break;
            }
            if b == b'\n' {
                self.pos += 1;
                line_ended = true;
                break;
            }
            if self.at("\"\"\"") || (!raw && (self.at("#{") || self.at("\\#{"))) {
                break;
            }
            self.bump_char();
        }
        if self.pos > start {
            self.push(TokenKind::StrText, start);
        }
        if line_ended {
            let line = self.begin_line();
            if let Some(Mode::Str(frame)) = self.modes.last_mut() {
                frame.lines.push(line);
            }
        }
    }

    fn begin_line(&mut self) -> Line {
        let start = self.pos;
        self.eat_while(|b| b == b' ' || b == b'\t');
        if self.pos > start {
            self.push(TokenKind::StrLayout, start);
        }
        Line {
            start,
            indent_end: self.pos,
        }
    }

    fn close_multi_line(&mut self) {
        let start = self.pos;
        self.pos += 3;
        self.push(TokenKind::MStrEnd, start);
        let Some(Mode::Str(mut frame)) = self.pop_mode() else {
            return;
        };
        match frame.lines.last().copied() {
            Some(closing) if closing.indent_end == start => {
                frame.lines.pop();
                self.strip_indentation(&frame, closing);
            }
            _ if frame.bad_opener => {}
            _ => self.error(
                "E0111",
                start,
                self.pos,
                "the closing `\"\"\"` must be the first text on its line".to_owned(),
                Some("move the closing `\"\"\"` to a line of its own".to_owned()),
            ),
        }
    }

    /// Checks that every line of a multi-line string starts with the
    /// indentation of the closing line, and schedules that indentation to be
    /// split off as layout so that only the deeper indentation remains
    /// literal text.
    fn strip_indentation(&mut self, frame: &StrFrame, closing: Line) {
        let prefix = self.slice(closing.start, closing.indent_end);
        for line in &frame.lines {
            if self.bytes.get(line.indent_end) == Some(&b'\n') {
                continue;
            }
            let indent = self.slice(line.start, line.indent_end);
            if !indent.starts_with(prefix) {
                let first_char = self
                    .src
                    .get(line.indent_end..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(0, char::len_utf8);
                self.error(
                    "E0112",
                    line.start,
                    line.indent_end + first_char,
                    "line does not start with the indentation of the closing `\"\"\"`".to_owned(),
                    Some(format!(
                        "start every line of the string with {}",
                        describe_indent(prefix)
                    )),
                );
            } else if indent.len() > prefix.len() {
                self.layout_splits.push((line.start, prefix.len()));
            }
        }
    }

    fn apply_layout_splits(&mut self) {
        if self.layout_splits.is_empty() {
            return;
        }
        self.layout_splits.sort_unstable();
        let mut splits = self.layout_splits.iter().peekable();
        let old = std::mem::take(&mut self.tokens);
        self.tokens.reserve(old.len() + self.layout_splits.len());
        for token in old {
            let Span { start, end } = token.span;
            let keep = match splits.peek() {
                Some(&&(at, keep)) if at == start && token.kind == TokenKind::StrLayout => keep,
                _ => {
                    self.tokens.push(token);
                    continue;
                }
            };
            splits.next();
            let mid = start + keep;
            if keep > 0 {
                self.tokens.push(Token {
                    kind: TokenKind::StrLayout,
                    span: Span { start, end: mid },
                });
            }
            self.tokens.push(Token {
                kind: TokenKind::StrText,
                span: Span { start: mid, end },
            });
        }
        debug_assert!(
            splits.next().is_none(),
            "every layout split starts at a layout token"
        );
    }

    /// Reports `frame` as unterminated. `at_line_feed` tells whether the
    /// line feed that ends a single-line string ended it, in which case the
    /// fix is to close that single-line string, whatever the form of
    /// `frame`.
    fn unterminated(&mut self, frame: &StrFrame, at_line_feed: bool) {
        let help = if frame.form.is_multi() && !at_line_feed {
            "close the string with `\"\"\"` on a line of its own"
        } else {
            "close the string with `\"` before the end of the line"
        };
        self.error(
            "E0108",
            frame.start,
            frame.start + frame.form.opener_len(),
            "unterminated string".to_owned(),
            Some(help.to_owned()),
        );
    }

    fn lex_path(&mut self) {
        let start = self.pos;
        self.pos += if self.at("./") { 2 } else { 3 };
        self.eat_while(is_path_char);
        self.push(TokenKind::Path, start);
    }

    fn current(&self) -> char {
        self.src
            .get(self.pos..)
            .and_then(|rest| rest.chars().next())
            .unwrap_or('\0')
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn at(&self, text: &str) -> bool {
        self.bytes
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with(text.as_bytes()))
    }

    fn bump_char(&mut self) {
        self.pos += self.current().len_utf8();
    }

    fn eat_while(&mut self, predicate: impl Fn(u8) -> bool) {
        while self.peek(0).is_some_and(&predicate) {
            self.pos += 1;
        }
    }

    fn slice(&self, start: usize, end: usize) -> &'a str {
        self.src.get(start..end).unwrap_or_default()
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            span: Span {
                start,
                end: self.pos,
            },
        });
    }

    fn error(
        &mut self,
        code: &'static str,
        start: usize,
        end: usize,
        message: String,
        help: Option<String>,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, Span { start, end }, message).with_optional_help(help));
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_lower(b: u8) -> bool {
    b.is_ascii_lowercase()
}

fn is_version_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~')
}

fn is_variant_char(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
}

fn is_dec_or_underscore(b: u8) -> bool {
    b.is_ascii_digit() || b == b'_'
}

fn is_digit_in(b: u8, radix: u32) -> bool {
    char::from(b).is_digit(radix)
}

/// Returns whether every `_` in a number literal has a digit on both sides.
fn separators_between_digits(text: &str, radix: u32) -> bool {
    let bytes = text.as_bytes();
    let digit_at = |i: usize| bytes.get(i).is_some_and(|&b| is_digit_in(b, radix));
    bytes
        .iter()
        .enumerate()
        .all(|(i, &b)| b != b'_' || (i > 0 && digit_at(i - 1) && digit_at(i + 1)))
}

fn is_path_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+' | b'@' | b'/')
}

fn describe_indent(indent: &str) -> String {
    let count = |b: u8| indent.bytes().filter(|&x| x == b).count();
    let plural = |n: usize, word: &str| {
        if n == 1 {
            format!("1 {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    match (count(b' '), count(b'\t')) {
        (spaces, 0) => plural(spaces, "space"),
        (0, tabs) => plural(tabs, "tab"),
        _ => "the same spaces and tabs as the line of the closing `\"\"\"`".to_owned(),
    }
}

/// Shows a character in backticks, or as its code point when it would be
/// invisible or misleading there.
fn describe_char(c: char) -> String {
    if c.is_control() || c.is_whitespace() || is_invisible_format(c) {
        format!("U+{:04X}", c as u32)
    } else {
        format!("`{c}`")
    }
}

/// Returns whether `c` is a Unicode format character (general category Cf)
/// or a variation selector, which render as nothing on their own.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{ad}'
            | '\u{600}'..='\u{605}'
            | '\u{61c}'
            | '\u{6dd}'
            | '\u{70f}'
            | '\u{890}'..='\u{891}'
            | '\u{8e2}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

/// Returns the value of an integer literal, or `None` when it is out of
/// range or malformed.
pub(crate) fn parse_int(text: &str) -> Option<i64> {
    let (digits, radix) = match text.get(..2) {
        Some("0x") => (&text[2..], 16),
        Some("0o") => (&text[2..], 8),
        Some("0b") => (&text[2..], 2),
        _ => (text, 10),
    };
    i64::from_str_radix(&digits.replace('_', ""), radix).ok()
}

/// Returns the value of a float literal, or `None` when it is malformed or
/// not finite.
pub(crate) fn parse_float(text: &str) -> Option<f64> {
    text.replace('_', "")
        .parse::<f64>()
        .ok()
        .filter(|f| f.is_finite())
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod tests;

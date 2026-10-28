//! Lexical analysis, turning source text into tokens.
//!
//! The lexer is lossless. Every byte of the source belongs to exactly one
//! token, including whitespace, comments and line feeds, so concatenating the
//! text of all tokens reproduces the source.

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
}

impl<'a> Lexer<'a> {
    fn run(&mut self) {
        if let Some(cr) = self.src.find('\r') {
            self.error(
                "E0101",
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
            self.lex_code();
        }
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
            '0'..='9' => return self.lex_number(),
            c if is_ident_start(c) => return self.lex_word(),
            '.' if self.at("./") || self.at("../") => return self.lex_path(),
            '.' if self.at("..=") => (T::DotDotEq, 3),
            '.' if next == Some(b'.') => (T::DotDot, 2),
            '.' => (T::Dot, 1),
            '?' if next == Some(b'.') => (T::QuestionDot, 2),
            '?' if next == Some(b'?') => (T::QuestionQuestion, 2),
            '{' => (T::LBrace, 1),
            '}' => (T::RBrace, 1),
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
                None,
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
            if self.peek(2).is_some_and(|b| is_digit_in(b, radix)) {
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
        if text.ends_with('_') {
            self.push(TokenKind::Error, start);
            return self.error(
                "E0103",
                start,
                self.pos,
                format!("`{text}` is not a valid token"),
                Some("remove the trailing `_`".to_owned()),
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
                    format!("integer `{text}` does not fit in 64 bits"),
                    Some(format!("the largest integer is {}", i64::MAX)),
                );
            }
        }
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
        self.diagnostics.push(Diagnostic {
            code,
            message,
            span: Span { start, end },
            help,
        });
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

fn is_path_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+' | b'@' | b'=' | b',' | b'/')
}

fn describe_char(c: char) -> String {
    if c.is_control() {
        format!("U+{:04X}", c as u32)
    } else {
        format!("`{c}`")
    }
}

fn parse_int(text: &str) -> Option<i64> {
    let (digits, radix) = match text.get(..2) {
        Some("0x") => (&text[2..], 16),
        Some("0o") => (&text[2..], 8),
        Some("0b") => (&text[2..], 2),
        _ => (text, 10),
    };
    i64::from_str_radix(&digits.replace('_', ""), radix).ok()
}

fn parse_float(text: &str) -> Option<f64> {
    text.replace('_', "")
        .parse::<f64>()
        .ok()
        .filter(|f| f.is_finite())
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod tests;

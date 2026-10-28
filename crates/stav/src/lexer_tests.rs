use std::fmt::Write;

use insta::assert_snapshot;
use proptest::prelude::*;

use crate::{TokenKind, lex};

fn render(source: &str) -> String {
    let lexed = lex(source);
    let mut out = String::new();
    for token in &lexed.tokens {
        let span = format!("{}..{}", token.span.start, token.span.end);
        writeln!(out, "{span:<8} {:?} {:?}", token.text(source), token.kind).unwrap();
    }
    for d in &lexed.diagnostics {
        writeln!(
            out,
            "error[{}] {}..{}: {}",
            d.code, d.span.start, d.span.end, d.message
        )
        .unwrap();
        if let Some(help) = &d.help {
            writeln!(out, "  help: {help}").unwrap();
        }
    }
    out
}

fn codes(source: &str) -> Vec<&'static str> {
    lex(source).diagnostics.iter().map(|d| d.code).collect()
}

fn assert_lossless(source: &str) {
    let lexed = lex(source);
    let (eof, rest) = lexed.tokens.split_last().expect("lexer always emits eof");
    assert_eq!(eof.kind, TokenKind::Eof);
    assert_eq!((eof.span.start, eof.span.end), (source.len(), source.len()));
    let mut offset = 0;
    for token in rest {
        assert_ne!(token.kind, TokenKind::Eof);
        assert_eq!(token.span.start, offset, "tokens must be contiguous");
        assert!(
            token.span.end > token.span.start,
            "tokens must not be empty"
        );
        assert!(source.get(token.span.range()).is_some());
        offset = token.span.end;
    }
    assert_eq!(offset, source.len());
    for d in &lexed.diagnostics {
        assert!(source.get(d.span.range()).is_some());
    }
    assert!(
        lexed
            .diagnostics
            .windows(2)
            .all(|w| w[0].span.start <= w[1].span.start)
    );
}

#[test]
fn identifiers_absorb_hyphens_and_dotted_digits() {
    assert_snapshot!(render("xdg-utils openssl1.1 on-failure a.b a-1 a - 1 foo- _x"), @r#"
    0..9     "xdg-utils" Ident
    9..10    " " Whitespace
    10..20   "openssl1.1" Ident
    20..21   " " Whitespace
    21..31   "on-failure" Ident
    31..32   " " Whitespace
    32..33   "a" Ident
    33..34   "." Dot
    34..35   "b" Ident
    35..36   " " Whitespace
    36..39   "a-1" Ident
    39..40   " " Whitespace
    40..41   "a" Ident
    41..42   " " Whitespace
    42..43   "-" Minus
    43..44   " " Whitespace
    44..45   "1" Int
    45..46   " " Whitespace
    46..49   "foo" Ident
    49..50   "-" Minus
    50..51   " " Whitespace
    51..53   "_x" Ident
    53..53   "" Eof
    "#);
}

#[test]
fn identifiers_starting_uppercase_are_type_identifiers() {
    assert_snapshot!(render("Str Apache-2.0 port: Int"), @r#"
    0..3     "Str" TypeIdent
    3..4     " " Whitespace
    4..14    "Apache-2.0" TypeIdent
    14..15   " " Whitespace
    15..19   "port" Ident
    19..20   ":" Colon
    20..21   " " Whitespace
    21..24   "Int" TypeIdent
    24..24   "" Eof
    "#);
}

#[test]
fn keywords_are_not_identifiers() {
    assert_snapshot!(
        render("let fn struct type match if else for in import include as part self true false none where block self-test lets"),
        @r#"
    0..3     "let" LetKw
    3..4     " " Whitespace
    4..6     "fn" FnKw
    6..7     " " Whitespace
    7..13    "struct" StructKw
    13..14   " " Whitespace
    14..18   "type" TypeKw
    18..19   " " Whitespace
    19..24   "match" MatchKw
    24..25   " " Whitespace
    25..27   "if" IfKw
    27..28   " " Whitespace
    28..32   "else" ElseKw
    32..33   " " Whitespace
    33..36   "for" ForKw
    36..37   " " Whitespace
    37..39   "in" InKw
    39..40   " " Whitespace
    40..46   "import" ImportKw
    46..47   " " Whitespace
    47..54   "include" IncludeKw
    54..55   " " Whitespace
    55..57   "as" AsKw
    57..58   " " Whitespace
    58..62   "part" PartKw
    62..63   " " Whitespace
    63..67   "self" SelfKw
    67..68   " " Whitespace
    68..72   "true" TrueKw
    72..73   " " Whitespace
    73..78   "false" FalseKw
    78..79   " " Whitespace
    79..83   "none" NoneKw
    83..84   " " Whitespace
    84..89   "where" WhereKw
    89..90   " " Whitespace
    90..95   "block" BlockKw
    95..96   " " Whitespace
    96..105  "self-test" Ident
    105..106 " " Whitespace
    106..110 "lets" Ident
    110..110 "" Eof
    "#
    );
}

#[test]
fn identifiers_with_version_or_variant_are_symbol_words() {
    assert_snapshot!(
        render("lib@19 tool:static lib@19.1:static lib@1.0~rc_2 port:Int lib@19:Big port: str"),
        @r#"
    0..6     "lib@19" SymWord
    6..7     " " Whitespace
    7..18    "tool:static" SymWord
    18..19   " " Whitespace
    19..34   "lib@19.1:static" SymWord
    34..35   " " Whitespace
    35..47   "lib@1.0~rc_2" SymWord
    47..48   " " Whitespace
    48..52   "port" Ident
    52..53   ":" Colon
    53..56   "Int" TypeIdent
    56..57   " " Whitespace
    57..63   "lib@19" SymWord
    63..64   ":" Colon
    64..67   "Big" TypeIdent
    67..68   " " Whitespace
    68..72   "port" Ident
    72..73   ":" Colon
    73..74   " " Whitespace
    74..77   "str" Ident
    77..77   "" Eof
    "#
    );
}

#[test]
fn at_sign_outside_symbol_word_is_an_error() {
    assert_snapshot!(render("lib @19\nlib@ 19\nlib@"), @r#"
    0..3     "lib" Ident
    3..4     " " Whitespace
    4..5     "@" Error
    5..7     "19" Int
    7..8     "\n" Newline
    8..11    "lib" Ident
    11..12   "@" Error
    12..13   " " Whitespace
    13..15   "19" Int
    15..16   "\n" Newline
    16..19   "lib" Ident
    19..20   "@" Error
    20..20   "" Eof
    error[E0105] 4..5: `@` is only allowed inside a symbol word
      help: write the name and version without spaces, like `name@1.2`
    error[E0105] 11..12: `@` is only allowed inside a symbol word
      help: write the name and version without spaces, like `name@1.2`
    error[E0105] 19..20: `@` is only allowed inside a symbol word
      help: write the name and version without spaces, like `name@1.2`
    "#);
}

#[test]
fn numbers_in_every_notation() {
    assert_snapshot!(
        render("42 0o755 0x1f 0b1010 1_000_000 0.5 1.0e-3 2_500.25 1E+5 007"),
        @r#"
    0..2     "42" Int
    2..3     " " Whitespace
    3..8     "0o755" Int
    8..9     " " Whitespace
    9..13    "0x1f" Int
    13..14   " " Whitespace
    14..20   "0b1010" Int
    20..21   " " Whitespace
    21..30   "1_000_000" Int
    30..31   " " Whitespace
    31..34   "0.5" Float
    34..35   " " Whitespace
    35..41   "1.0e-3" Float
    41..42   " " Whitespace
    42..50   "2_500.25" Float
    50..51   " " Whitespace
    51..55   "1E+5" Float
    55..56   " " Whitespace
    56..59   "007" Int
    59..59   "" Eof
    "#
    );
}

#[test]
fn number_dots_without_digits_are_punctuation() {
    assert_snapshot!(render("1..5 1..=5 1.max"), @r#"
    0..1     "1" Int
    1..3     ".." DotDot
    3..4     "5" Int
    4..5     " " Whitespace
    5..6     "1" Int
    6..9     "..=" DotDotEq
    9..10    "5" Int
    10..11   " " Whitespace
    11..12   "1" Int
    12..13   "." Dot
    13..16   "max" Ident
    16..16   "" Eof
    "#);
}

#[test]
fn number_glued_to_name_is_one_invalid_token() {
    assert_snapshot!(render("0ad 0ad@0.27 0b102 0x 1e 1_ 0x_1"), @r#"
    0..3     "0ad" Error
    3..4     " " Whitespace
    4..12    "0ad@0.27" Error
    12..13   " " Whitespace
    13..18   "0b102" Error
    18..19   " " Whitespace
    19..21   "0x" Error
    21..22   " " Whitespace
    22..24   "1e" Error
    24..25   " " Whitespace
    25..27   "1_" Error
    27..28   " " Whitespace
    28..32   "0x_1" Error
    32..32   "" Eof
    error[E0103] 0..3: `0ad` is not a valid token
      help: separate the number from the name, or start the name with a letter
    error[E0103] 4..12: `0ad@0.27` is not a valid token
      help: separate the number from the name, or start the name with a letter
    error[E0103] 13..18: `0b102` is not a valid token
      help: separate the number from the name, or start the name with a letter
    error[E0103] 19..21: `0x` is not a valid token
      help: separate the number from the name, or start the name with a letter
    error[E0103] 22..24: `1e` is not a valid token
      help: separate the number from the name, or start the name with a letter
    error[E0103] 25..27: `1_` is not a valid token
      help: write `_` only between two digits
    error[E0103] 28..32: `0x_1` is not a valid token
      help: write `_` only between two digits
    "#);
}

#[test]
fn digit_separators_must_sit_between_digits() {
    for source in ["1_000", "0x1_f", "0b1_0", "1_0.2_5e1_0"] {
        assert!(codes(source).is_empty(), "{source}");
    }
    for source in [
        "1_", "1__0", "1_.5", "1_e5", "1e_5", "0x_1f", "0x1__f", "0x1f_",
    ] {
        let lexed = lex(source);
        assert_eq!(codes(source), ["E0103"], "{source}");
        assert_eq!(lexed.tokens[0].kind, TokenKind::Error, "{source}");
        assert_eq!(lexed.tokens[0].text(source), source);
    }
}

#[test]
fn numbers_out_of_range_are_reported() {
    assert_snapshot!(
        render("9223372036854775807 9223372036854775808 0x8000000000000000 1e308 1e309"),
        @r#"
    0..19    "9223372036854775807" Int
    19..20   " " Whitespace
    20..39   "9223372036854775808" Int
    39..40   " " Whitespace
    40..58   "0x8000000000000000" Int
    58..59   " " Whitespace
    59..64   "1e308" Float
    64..65   " " Whitespace
    65..70   "1e309" Float
    70..70   "" Eof
    error[E0107] 20..39: integer `9223372036854775808` does not fit in a signed 64-bit integer
      help: the largest integer is 9223372036854775807
    error[E0107] 40..58: integer `0x8000000000000000` does not fit in a signed 64-bit integer
      help: the largest integer is 9223372036854775807
    error[E0107] 65..70: float `1e309` is too large to be finite
    "#
    );
}

#[test]
fn punctuation_and_operators_take_the_longest_match() {
    assert_snapshot!(
        render("{}[](),=.?.??:->=>.....=|+-*/%++==!=<<=>>=&&||!"),
        @r#"
    0..1     "{" LBrace
    1..2     "}" RBrace
    2..3     "[" LBracket
    3..4     "]" RBracket
    4..5     "(" LParen
    5..6     ")" RParen
    6..7     "," Comma
    7..8     "=" Eq
    8..9     "." Dot
    9..11    "?." QuestionDot
    11..13   "??" QuestionQuestion
    13..14   ":" Colon
    14..16   "->" Arrow
    16..18   "=>" FatArrow
    18..20   ".." DotDot
    20..22   ".." DotDot
    22..23   "." Dot
    23..24   "=" Eq
    24..25   "|" Pipe
    25..26   "+" Plus
    26..27   "-" Minus
    27..28   "*" Star
    28..29   "/" Slash
    29..30   "%" Percent
    30..32   "++" PlusPlus
    32..34   "==" EqEq
    34..36   "!=" NotEq
    36..37   "<" Lt
    37..39   "<=" LtEq
    39..40   ">" Gt
    40..42   ">=" GtEq
    42..44   "&&" AndAnd
    44..46   "||" OrOr
    46..47   "!" Bang
    47..47   "" Eof
    "#
    );
}

#[test]
fn lone_question_mark_and_ampersand_are_errors() {
    assert_snapshot!(render("a ? b & c"), @r#"
    0..1     "a" Ident
    1..2     " " Whitespace
    2..3     "?" Error
    3..4     " " Whitespace
    4..5     "b" Ident
    5..6     " " Whitespace
    6..7     "&" Error
    7..8     " " Whitespace
    8..9     "c" Ident
    9..9     "" Eof
    error[E0113] 2..3: unexpected character `?`
    error[E0113] 6..7: unexpected character `&`
      help: write `&&` for logical and
    "#);
}

#[test]
fn comments_run_to_the_end_of_the_line() {
    assert_snapshot!(render("# line\n## doc\n##\n###x\n##x\na # trailing"), @r####"
    0..6     "# line" Comment
    6..7     "\n" Newline
    7..13    "## doc" DocComment
    13..14   "\n" Newline
    14..16   "##" DocComment
    16..17   "\n" Newline
    17..21   "###x" Comment
    21..22   "\n" Newline
    22..25   "##x" Comment
    25..26   "\n" Newline
    26..27   "a" Ident
    27..28   " " Whitespace
    28..38   "# trailing" Comment
    38..38   "" Eof
    "####);
}

#[test]
fn every_line_feed_is_a_token() {
    assert_snapshot!(render("a\n\n  # note\nb\n"), @r##"
    0..1     "a" Ident
    1..2     "\n" Newline
    2..3     "\n" Newline
    3..5     "  " Whitespace
    5..11    "# note" Comment
    11..12   "\n" Newline
    12..13   "b" Ident
    13..14   "\n" Newline
    14..14   "" Eof
    "##);
}

#[test]
fn relative_paths_are_literals() {
    assert_snapshot!(render("./fix.patch ../files/a.conf ./a+b@c-d_e [./a,./b] p=./c x/y"), @r#"
    0..11    "./fix.patch" Path
    11..12   " " Whitespace
    12..27   "../files/a.conf" Path
    27..28   " " Whitespace
    28..39   "./a+b@c-d_e" Path
    39..40   " " Whitespace
    40..41   "[" LBracket
    41..44   "./a" Path
    44..45   "," Comma
    45..48   "./b" Path
    48..49   "]" RBracket
    49..50   " " Whitespace
    50..51   "p" Ident
    51..52   "=" Eq
    52..55   "./c" Path
    55..56   " " Whitespace
    56..57   "x" Ident
    57..58   "/" Slash
    58..59   "y" Ident
    59..59   "" Eof
    "#);
}

#[test]
fn byte_order_mark_and_carriage_returns_are_rejected_once() {
    assert_snapshot!(render("\u{feff}a\r\nb\r\n"), @r#"
    0..3     "\u{feff}" Error
    3..4     "a" Ident
    4..5     "\r" Error
    5..6     "\n" Newline
    6..7     "b" Ident
    7..8     "\r" Error
    8..9     "\n" Newline
    9..9     "" Eof
    error[E0101] 0..3: byte order mark is not allowed
      help: save the file as UTF-8 without a byte order mark
    error[E0114] 4..5: carriage return is not allowed
      help: save the file with LF line endings
    "#);
}

#[test]
fn foreign_whitespace_and_characters_are_errors() {
    assert_snapshot!(render("a\u{a0}b \u{20ac} \u{0} \u{200b} \u{feff}"), @r#"
    0..1     "a" Ident
    1..3     "\u{a0}" Error
    3..4     "b" Ident
    4..5     " " Whitespace
    5..8     "€" Error
    8..9     " " Whitespace
    9..10    "\0" Error
    10..11   " " Whitespace
    11..14   "\u{200b}" Error
    14..15   " " Whitespace
    15..18   "\u{feff}" Error
    18..18   "" Eof
    error[E0102] 1..3: invalid whitespace character U+00A0
      help: use spaces or tabs
    error[E0113] 5..8: unexpected character `€`
    error[E0113] 9..10: unexpected character U+0000
    error[E0113] 11..14: unexpected character U+200B
    error[E0113] 15..18: unexpected character U+FEFF
    "#);
}

#[test]
fn identifier_length_is_limited() {
    assert!(codes(&"a".repeat(255)).is_empty());
    let long = "a".repeat(256);
    let lexed = lex(&long);
    assert_eq!(codes(&long), ["E0104"]);
    assert_eq!(lexed.tokens[0].kind, TokenKind::Ident);
    assert_eq!(lexed.tokens[0].text(&long), long);
    assert_eq!(
        lexed.diagnostics[0].help.as_deref(),
        Some("shorten the identifier to 255 bytes")
    );
    assert_eq!(codes(&format!("{}@1", "a".repeat(256))), ["E0104"]);
}

#[test]
fn empty_source_is_only_eof() {
    assert_snapshot!(render(""), @r#"0..0     "" Eof"#);
}

/// Builds the value of every string in `source` from its literal parts, in
/// the order the strings start. Each interpolation is written as `{}` and
/// each escape as `<escape>`.
fn string_values(source: &str) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    // Open strings as indexes into `values`, and `None` for interpolations.
    let mut open: Vec<Option<usize>> = Vec::new();
    for token in lex(source).tokens {
        let current = open.last().copied().flatten();
        match token.kind {
            TokenKind::StrStart
            | TokenKind::MStrStart
            | TokenKind::RawStrStart
            | TokenKind::RawMStrStart => {
                open.push(Some(values.len()));
                values.push(String::new());
            }
            TokenKind::StrEnd | TokenKind::MStrEnd | TokenKind::InterpEnd => {
                open.pop();
            }
            TokenKind::InterpStart => {
                if let Some(i) = current {
                    values[i].push_str("{}");
                }
                open.push(None);
            }
            TokenKind::StrText => {
                let i = current.expect("string text is inside a string");
                values[i].push_str(token.text(source));
            }
            TokenKind::StrEscape => {
                let i = current.expect("an escape is inside a string");
                write!(values[i], "<{}>", token.text(source)).unwrap();
            }
            _ => {}
        }
    }
    values
}

fn string_value(source: &str) -> String {
    string_values(source)
        .into_iter()
        .next()
        .expect("the source contains a string")
}

#[test]
fn plain_string_escapes_are_tokens() {
    assert_snapshot!(render(r#""a\n\t\\\"\#{x}\u{1F600}\0\r""#), @r#"
    0..1     "\"" StrStart
    1..2     "a" StrText
    2..4     "\\n" StrEscape
    4..6     "\\t" StrEscape
    6..8     "\\\\" StrEscape
    8..10    "\\\"" StrEscape
    10..12   "\\#" StrEscape
    12..15   "{x}" StrText
    15..24   "\\u{1F600}" StrEscape
    24..26   "\\0" StrEscape
    26..28   "\\r" StrEscape
    28..29   "\"" StrEnd
    29..29   "" Eof
    "#);
}

#[test]
fn unknown_and_invalid_escapes_are_reported() {
    assert_snapshot!(
        render(r#""\q \u{} \u{110000} \u{D800} \u{1234567} \u12 \u{41""#),
        @r#"
    0..1     "\"" StrStart
    1..3     "\\q" StrEscape
    3..4     " " StrText
    4..8     "\\u{}" StrEscape
    8..9     " " StrText
    9..19    "\\u{110000}" StrEscape
    19..20   " " StrText
    20..28   "\\u{D800}" StrEscape
    28..29   " " StrText
    29..40   "\\u{1234567}" StrEscape
    40..41   " " StrText
    41..43   "\\u" StrEscape
    43..46   "12 " StrText
    46..51   "\\u{41" StrEscape
    51..52   "\"" StrEnd
    52..52   "" Eof
    error[E0109] 1..3: unknown escape `\q`
      help: write `\\` for a literal backslash
    error[E0109] 4..8: invalid unicode escape `\u{}`
      help: write `\u{` followed by 1 to 6 hex digits of a unicode scalar value and `}`
    error[E0109] 9..19: invalid unicode escape `\u{110000}`
      help: write `\u{` followed by 1 to 6 hex digits of a unicode scalar value and `}`
    error[E0109] 20..28: invalid unicode escape `\u{D800}`
      help: write `\u{` followed by 1 to 6 hex digits of a unicode scalar value and `}`
    error[E0109] 29..40: invalid unicode escape `\u{1234567}`
      help: write `\u{` followed by 1 to 6 hex digits of a unicode scalar value and `}`
    error[E0109] 41..43: invalid unicode escape `\u`
      help: write `\u{` followed by 1 to 6 hex digits of a unicode scalar value and `}`
    error[E0109] 46..51: invalid unicode escape `\u{41`
      help: write `\u{` followed by 1 to 6 hex digits of a unicode scalar value and `}`
    "#
    );
}

#[test]
fn interpolation_exposes_inner_tokens() {
    assert_snapshot!(render(r#""http://#{host}:#{port + 1}/${PATH} #x""#), @r##"
    0..1     "\"" StrStart
    1..8     "http://" StrText
    8..10    "#{" InterpStart
    10..14   "host" Ident
    14..15   "}" InterpEnd
    15..16   ":" StrText
    16..18   "#{" InterpStart
    18..22   "port" Ident
    22..23   " " Whitespace
    23..24   "+" Plus
    24..25   " " Whitespace
    25..26   "1" Int
    26..27   "}" InterpEnd
    27..38   "/${PATH} #x" StrText
    38..39   "\"" StrEnd
    39..39   "" Eof
    "##);
}

#[test]
fn interpolation_tracks_nested_braces_and_strings() {
    assert_snapshot!(render(r##""#{ { a = "#{b}" }.a }!""##), @r##"
    0..1     "\"" StrStart
    1..3     "#{" InterpStart
    3..4     " " Whitespace
    4..5     "{" LBrace
    5..6     " " Whitespace
    6..7     "a" Ident
    7..8     " " Whitespace
    8..9     "=" Eq
    9..10    " " Whitespace
    10..11   "\"" StrStart
    11..13   "#{" InterpStart
    13..14   "b" Ident
    14..15   "}" InterpEnd
    15..16   "\"" StrEnd
    16..17   " " Whitespace
    17..18   "}" RBrace
    18..19   "." Dot
    19..20   "a" Ident
    20..21   " " Whitespace
    21..22   "}" InterpEnd
    22..23   "!" StrText
    23..24   "\"" StrEnd
    24..24   "" Eof
    "##);
}

#[test]
fn single_line_string_ends_at_line_feed() {
    assert_snapshot!(render("x = \"abc\ny = \"d\\\n"), @r#"
    0..1     "x" Ident
    1..2     " " Whitespace
    2..3     "=" Eq
    3..4     " " Whitespace
    4..5     "\"" StrStart
    5..8     "abc" StrText
    8..9     "\n" Newline
    9..10    "y" Ident
    10..11   " " Whitespace
    11..12   "=" Eq
    12..13   " " Whitespace
    13..14   "\"" StrStart
    14..15   "d" StrText
    15..16   "\\" StrText
    16..17   "\n" Newline
    17..17   "" Eof
    error[E0108] 4..5: unterminated string
      help: close the string with `"` before the end of the line
    error[E0108] 13..14: unterminated string
      help: close the string with `"` before the end of the line
    "#);
}

#[test]
fn single_line_interpolation_ends_at_line_feed() {
    assert_snapshot!(render("x = \"a #{ b\ny = 1"), @r##"
    0..1     "x" Ident
    1..2     " " Whitespace
    2..3     "=" Eq
    3..4     " " Whitespace
    4..5     "\"" StrStart
    5..7     "a " StrText
    7..9     "#{" InterpStart
    9..10    " " Whitespace
    10..11   "b" Ident
    11..12   "\n" Newline
    12..13   "y" Ident
    13..14   " " Whitespace
    14..15   "=" Eq
    15..16   " " Whitespace
    16..17   "1" Int
    17..17   "" Eof
    error[E0108] 4..5: unterminated string
      help: close the string with `"` before the end of the line
    "##);
}

#[test]
fn single_line_string_ends_at_line_feed_inside_nested_multi_line_string() {
    assert_snapshot!(render("x = \"#{ \"\"\"\n  y\n  \"\"\" }\""), @r##"
    0..1     "x" Ident
    1..2     " " Whitespace
    2..3     "=" Eq
    3..4     " " Whitespace
    4..5     "\"" StrStart
    5..7     "#{" InterpStart
    7..8     " " Whitespace
    8..11    "\"\"\"" MStrStart
    11..12   "\n" Newline
    12..14   "  " Whitespace
    14..15   "y" Ident
    15..16   "\n" Newline
    16..18   "  " Whitespace
    18..21   "\"\"\"" MStrStart
    21..24   " }\"" Error
    24..24   "" Eof
    error[E0108] 4..5: unterminated string
      help: close the string with `"` before the end of the line
    error[E0108] 8..11: unterminated string
      help: close the string with `"` before the end of the line
    error[E0110] 18..24: the opening `"""` must be followed directly by a line break
      help: start the content of the string on the next line
    error[E0108] 18..21: unterminated string
      help: close the string with `"""` on a line of its own
    "##);
}

#[test]
fn line_feed_in_multi_line_string_inside_single_line_string_ends_both() {
    assert_snapshot!(render("x = \"#{ \"\"\"a\nb = 1"), @r##"
    0..1     "x" Ident
    1..2     " " Whitespace
    2..3     "=" Eq
    3..4     " " Whitespace
    4..5     "\"" StrStart
    5..7     "#{" InterpStart
    7..8     " " Whitespace
    8..11    "\"\"\"" MStrStart
    11..12   "a" StrText
    12..13   "\n" Newline
    13..14   "b" Ident
    14..15   " " Whitespace
    15..16   "=" Eq
    16..17   " " Whitespace
    17..18   "1" Int
    18..18   "" Eof
    error[E0108] 4..5: unterminated string
      help: close the string with `"` before the end of the line
    error[E0110] 8..11: a multi-line string cannot appear inside a single-line string
      help: close the enclosing string before the multi-line string starts
    error[E0108] 8..11: unterminated string
      help: close the string with `"` before the end of the line
    "##);
}

#[test]
fn multi_line_string_inside_single_line_string_reports_its_opener() {
    assert_snapshot!(render("x = \"a #{ \"\"\"x\"\"\" }\""), @r##"
    0..1     "x" Ident
    1..2     " " Whitespace
    2..3     "=" Eq
    3..4     " " Whitespace
    4..5     "\"" StrStart
    5..7     "a " StrText
    7..9     "#{" InterpStart
    9..10    " " Whitespace
    10..13   "\"\"\"" MStrStart
    13..14   "x" StrText
    14..17   "\"\"\"" MStrEnd
    17..18   " " Whitespace
    18..19   "}" InterpEnd
    19..20   "\"" StrEnd
    20..20   "" Eof
    error[E0110] 10..13: a multi-line string cannot appear inside a single-line string
      help: close the enclosing string before the multi-line string starts
    "##);
}

#[test]
fn single_line_string_ends_at_line_feed_inside_nested_single_line_string() {
    assert_snapshot!(render("\"a #{ \"b #{ c\nd"), @r##"
    0..1     "\"" StrStart
    1..3     "a " StrText
    3..5     "#{" InterpStart
    5..6     " " Whitespace
    6..7     "\"" StrStart
    7..9     "b " StrText
    9..11    "#{" InterpStart
    11..12   " " Whitespace
    12..13   "c" Ident
    13..14   "\n" Newline
    14..15   "d" Ident
    15..15   "" Eof
    error[E0108] 0..1: unterminated string
      help: close the string with `"` before the end of the line
    error[E0108] 6..7: unterminated string
      help: close the string with `"` before the end of the line
    "##);
}

#[test]
fn strings_unterminated_at_end_of_input_are_reported() {
    assert_snapshot!(render("\"a #{ \"b"), @r##"
    0..1     "\"" StrStart
    1..3     "a " StrText
    3..5     "#{" InterpStart
    5..6     " " Whitespace
    6..7     "\"" StrStart
    7..8     "b" StrText
    8..8     "" Eof
    error[E0108] 0..1: unterminated string
      help: close the string with `"` before the end of the line
    error[E0108] 6..7: unterminated string
      help: close the string with `"` before the end of the line
    "##);
}

#[test]
fn multi_line_string_strips_closing_indentation() {
    let source = "text = \"\"\"\n    line one\n      indented\n    \n    line three\n    \"\"\"\n";
    assert_eq!(string_value(source), "line one\n  indented\n\nline three\n");
    assert_snapshot!(render(source), @r#"
    0..4     "text" Ident
    4..5     " " Whitespace
    5..6     "=" Eq
    6..7     " " Whitespace
    7..10    "\"\"\"" MStrStart
    10..11   "\n" StrLayout
    11..15   "    " StrLayout
    15..24   "line one\n" StrText
    24..28   "    " StrLayout
    28..30   "  " StrText
    30..39   "indented\n" StrText
    39..43   "    " StrLayout
    43..44   "\n" StrText
    44..48   "    " StrLayout
    48..59   "line three\n" StrText
    59..63   "    " StrLayout
    63..66   "\"\"\"" MStrEnd
    66..67   "\n" Newline
    67..67   "" Eof
    "#);
}

#[test]
fn whitespace_only_lines_become_empty_lines() {
    let source = "\"\"\"\n  a\n\n \n\t\t\t\n  b\n  \"\"\"";
    assert_eq!(string_value(source), "a\n\n\n\nb\n");
    assert!(lex(source).diagnostics.is_empty());
}

#[test]
fn multi_line_string_without_content_is_empty() {
    assert_eq!(string_value("\"\"\"\n  \"\"\""), "");
    assert_eq!(string_value("\"\"\"\n\"\"\""), "");
    assert!(codes("\"\"\"\n  \"\"\"").is_empty());
}

#[test]
fn unindented_closing_keeps_all_indentation() {
    assert_eq!(string_value("\"\"\"\n  a\n b\n\"\"\""), "  a\n b\n");
}

#[test]
fn multi_line_string_keeps_backslashes_except_before_interpolation() {
    let source = "\"\"\"\n  a\\n \\\n  \\#{x} \\#y ${y} #{z}\n  \"\"\"";
    assert_eq!(string_value(source), "a\\n \\\n<\\#>{x} \\#y ${y} {}\n");
    assert_snapshot!(render(source), @r##"
    0..3     "\"\"\"" MStrStart
    3..4     "\n" StrLayout
    4..6     "  " StrLayout
    6..12    "a\\n \\\n" StrText
    12..14   "  " StrLayout
    14..16   "\\#" StrEscape
    16..29   "{x} \\#y ${y} " StrText
    29..31   "#{" InterpStart
    31..32   "z" Ident
    32..33   "}" InterpEnd
    33..34   "\n" StrText
    34..36   "  " StrLayout
    36..39   "\"\"\"" MStrEnd
    39..39   "" Eof
    "##);
}

#[test]
fn multi_line_interpolation_may_span_lines() {
    let source = "\"\"\"\n  a #{f(\n    x,\n  )} b\n  c\n  \"\"\"";
    assert_eq!(string_value(source), "a {} b\nc\n");
    assert!(lex(source).diagnostics.is_empty());
}

#[test]
fn nested_multi_line_strings_strip_their_own_indentation() {
    let source = "x = \"\"\"\n    outer #{\"\"\"\n        inner\n      \"\"\"} end\n  \"\"\"\n";
    assert_eq!(string_values(source), ["  outer {} end\n", "  inner\n"]);
    assert!(lex(source).diagnostics.is_empty());
    assert_snapshot!(render(source), @r##"
    0..1     "x" Ident
    1..2     " " Whitespace
    2..3     "=" Eq
    3..4     " " Whitespace
    4..7     "\"\"\"" MStrStart
    7..8     "\n" StrLayout
    8..10    "  " StrLayout
    10..12   "  " StrText
    12..18   "outer " StrText
    18..20   "#{" InterpStart
    20..23   "\"\"\"" MStrStart
    23..24   "\n" StrLayout
    24..30   "      " StrLayout
    30..32   "  " StrText
    32..38   "inner\n" StrText
    38..44   "      " StrLayout
    44..47   "\"\"\"" MStrEnd
    47..48   "}" InterpEnd
    48..53   " end\n" StrText
    53..55   "  " StrLayout
    55..58   "\"\"\"" MStrEnd
    58..59   "\n" Newline
    59..59   "" Eof
    "##);
}

#[test]
fn nested_multi_line_strings_split_their_indentation_in_order() {
    let source = "x = \"\"\"\n    a #{\"\"\"\n      b\n    \"\"\"} c\n  \"\"\"\n";
    assert_eq!(string_values(source), ["  a {} c\n", "  b\n"]);
    assert!(lex(source).diagnostics.is_empty());
}

/// A well-formed multi-line string for property tests.
#[derive(Debug, Clone)]
struct MultiLine {
    indent: usize,
    lines: Vec<ContentLine>,
}

/// A content line: indentation beyond the closing indentation, text, and
/// optionally an interpolated multi-line string followed by more text.
#[derive(Debug, Clone)]
struct ContentLine {
    extra: usize,
    text: String,
    nested: Option<(MultiLine, String)>,
}

impl MultiLine {
    fn source(&self) -> String {
        let mut out = "\"\"\"\n".to_owned();
        for line in &self.lines {
            out.push_str(&" ".repeat(self.indent + line.extra));
            out.push_str(&line.text);
            if let Some((nested, after)) = &line.nested {
                write!(out, "#{{{}}}{after}", nested.source()).unwrap();
            }
            out.push('\n');
        }
        out.push_str(&" ".repeat(self.indent));
        out.push_str("\"\"\"");
        out
    }

    /// Appends the value of the string and then of every string nested in
    /// it, in source order, with `{}` for each interpolation.
    fn values(&self, out: &mut Vec<String>) {
        let slot = out.len();
        out.push(String::new());
        let mut value = String::new();
        for line in &self.lines {
            if line.nested.is_none() && line.text.trim().is_empty() {
                value.push('\n');
                continue;
            }
            value.push_str(&" ".repeat(line.extra));
            value.push_str(&line.text);
            if let Some((nested, after)) = &line.nested {
                value.push_str("{}");
                nested.values(out);
                value.push_str(after);
            }
            value.push('\n');
        }
        out[slot] = value;
    }
}

fn multi_line() -> impl Strategy<Value = MultiLine> {
    let text = "([a-z][a-z ]{0,5})?";
    let line = (0..4usize, text).prop_map(|(extra, text)| ContentLine {
        extra,
        text,
        nested: None,
    });
    let leaf = (0..6usize, prop::collection::vec(line, 0..4))
        .prop_map(|(indent, lines)| MultiLine { indent, lines });
    leaf.prop_recursive(3, 12, 4, move |inner| {
        let line = (0..4usize, text, prop::option::of((inner, "[a-z ]{0,4}"))).prop_map(
            |(extra, text, nested)| ContentLine {
                extra,
                text,
                nested,
            },
        );
        (0..6usize, prop::collection::vec(line, 0..4))
            .prop_map(|(indent, lines)| MultiLine { indent, lines })
    })
}

#[test]
fn opening_delimiter_must_end_its_line() {
    assert_snapshot!(render("\"\"\"abc\n  x\n  \"\"\" \"\"\"abc\"\"\""), @r#"
    0..3     "\"\"\"" MStrStart
    3..6     "abc" Error
    6..7     "\n" StrLayout
    7..9     "  " StrLayout
    9..11    "x\n" StrText
    11..13   "  " StrLayout
    13..16   "\"\"\"" MStrEnd
    16..17   " " Whitespace
    17..20   "\"\"\"" MStrStart
    20..23   "abc" Error
    23..26   "\"\"\"" MStrEnd
    26..26   "" Eof
    error[E0110] 0..6: the opening `"""` must be followed directly by a line break
      help: start the content of the string on the next line
    error[E0110] 17..23: the opening `"""` must be followed directly by a line break
      help: start the content of the string on the next line
    "#);
}

#[test]
fn closing_delimiter_must_start_its_line() {
    assert_snapshot!(render("\"\"\"\n  abc\"\"\""), @r#"
    0..3     "\"\"\"" MStrStart
    3..4     "\n" StrLayout
    4..6     "  " StrLayout
    6..9     "abc" StrText
    9..12    "\"\"\"" MStrEnd
    12..12   "" Eof
    error[E0111] 9..12: the closing `"""` must be the first text on its line
      help: move the closing `"""` to a line of its own
    "#);
}

#[test]
fn lines_must_start_with_the_closing_indentation() {
    let source = "\"\"\"\n    one\n   two\n\tthree\n     \"\"\"";
    assert_snapshot!(render(source), @r#"
    0..3     "\"\"\"" MStrStart
    3..4     "\n" StrLayout
    4..8     "    " StrLayout
    8..12    "one\n" StrText
    12..15   "   " StrLayout
    15..19   "two\n" StrText
    19..20   "\t" StrLayout
    20..26   "three\n" StrText
    26..31   "     " StrLayout
    31..34   "\"\"\"" MStrEnd
    34..34   "" Eof
    error[E0112] 4..9: line does not start with the indentation of the closing `"""`
      help: start every line of the string with 5 spaces
    error[E0112] 12..16: line does not start with the indentation of the closing `"""`
      help: start every line of the string with 5 spaces
    error[E0112] 19..21: line does not start with the indentation of the closing `"""`
      help: start every line of the string with 5 spaces
    "#);
}

#[test]
fn mixed_indentation_is_compared_byte_for_byte() {
    let source = "\"\"\"\n\t  ok\n  \tbad\n\t  \"\"\"";
    assert_eq!(codes(source), ["E0112"]);
    assert_eq!(
        lex(source).diagnostics[0].help.as_deref(),
        Some(
            "start every line of the string with the same spaces and tabs as the line of the closing `\"\"\"`"
        )
    );
}

#[test]
fn raw_strings_have_no_escapes_or_interpolation() {
    assert_snapshot!(render(r#"r"a\d+#{x}" r"open"#), @r#"
    0..2     "r\"" RawStrStart
    2..10    "a\\d+#{x}" StrText
    10..11   "\"" StrEnd
    11..12   " " Whitespace
    12..14   "r\"" RawStrStart
    14..18   "open" StrText
    18..18   "" Eof
    error[E0108] 12..14: unterminated string
      help: close the string with `"` before the end of the line
    "#);
}

#[test]
fn raw_multi_line_strings_strip_indentation() {
    let source = "r\"\"\"\n  \\d #{x}\n    y\n  \"\"\"";
    assert_eq!(string_value(source), "\\d #{x}\n  y\n");
    assert_snapshot!(render(source), @r#"
    0..4     "r\"\"\"" RawMStrStart
    4..5     "\n" StrLayout
    5..7     "  " StrLayout
    7..15    "\\d #{x}\n" StrText
    15..17   "  " StrLayout
    17..19   "  " StrText
    19..21   "y\n" StrText
    21..23   "  " StrLayout
    23..26   "\"\"\"" MStrEnd
    26..26   "" Eof
    "#);
}

#[test]
fn only_a_lone_r_before_a_quote_starts_a_raw_string() {
    assert_snapshot!(render("r rx\"\" r\"\""), @r#"
    0..1     "r" Ident
    1..2     " " Whitespace
    2..4     "rx" Ident
    4..5     "\"" StrStart
    5..6     "\"" StrEnd
    6..7     " " Whitespace
    7..9     "r\"" RawStrStart
    9..10    "\"" StrEnd
    10..10   "" Eof
    "#);
}

#[test]
fn deeply_nested_interpolations_do_not_overflow() {
    let source = "\"\"\"\n#{".repeat(100_000);
    let lexed = lex(&source);
    assert_eq!(lexed.diagnostics.len(), 100_000);
    assert_lossless(&source);
}

const FRAGMENTS: &[&str] = &[
    " ", "\t", "\n", "\r", "#", "## ", "\"", "\"\"\"", "r\"", "\\", "\\u{", "#{", "}", "{", "a",
    "Z", "-", ".", "@", ":", "0", "9", "x", "e", "_", "./", "../", "\u{a0}", "\u{feff}",
    "\u{20ac}", "?", "&", "=", ">", "1.5",
];

proptest! {
    #[test]
    fn arbitrary_text_lexes_losslessly(source in any::<String>()) {
        assert_lossless(&source);
    }

    #[test]
    fn nested_multi_line_strings_have_their_written_values(string in multi_line()) {
        let source = format!("x = {}\n", string.source());
        let mut expected = Vec::new();
        string.values(&mut expected);
        assert!(lex(&source).diagnostics.is_empty(), "{source}");
        assert_eq!(string_values(&source), expected, "{source}");
    }

    #[test]
    fn stav_like_text_lexes_losslessly(
        parts in prop::collection::vec(prop::sample::select(FRAGMENTS), 0..64),
    ) {
        assert_lossless(&parts.concat());
    }
}

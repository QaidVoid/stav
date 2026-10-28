use std::fmt::Write;

use insta::assert_snapshot;
use proptest::prelude::*;

use crate::{TokenKind, lex};

fn render(source: &str) -> String {
    let lexed = lex(source);
    let mut out = String::new();
    for token in &lexed.tokens {
        let span = format!("{}..{}", token.span.start, token.span.end);
        writeln!(
            out,
            "{:<16} {span:<8} {:?}",
            format!("{:?}", token.kind),
            token.text(source)
        )
        .unwrap();
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
    Ident            0..9     "xdg-utils"
    Whitespace       9..10    " "
    Ident            10..20   "openssl1.1"
    Whitespace       20..21   " "
    Ident            21..31   "on-failure"
    Whitespace       31..32   " "
    Ident            32..33   "a"
    Dot              33..34   "."
    Ident            34..35   "b"
    Whitespace       35..36   " "
    Ident            36..39   "a-1"
    Whitespace       39..40   " "
    Ident            40..41   "a"
    Whitespace       41..42   " "
    Minus            42..43   "-"
    Whitespace       43..44   " "
    Int              44..45   "1"
    Whitespace       45..46   " "
    Ident            46..49   "foo"
    Minus            49..50   "-"
    Whitespace       50..51   " "
    Ident            51..53   "_x"
    Eof              53..53   ""
    "#);
}

#[test]
fn identifiers_starting_uppercase_are_type_identifiers() {
    assert_snapshot!(render("Str Apache-2.0 port: Int"), @r#"
    TypeIdent        0..3     "Str"
    Whitespace       3..4     " "
    TypeIdent        4..14    "Apache-2.0"
    Whitespace       14..15   " "
    Ident            15..19   "port"
    Colon            19..20   ":"
    Whitespace       20..21   " "
    TypeIdent        21..24   "Int"
    Eof              24..24   ""
    "#);
}

#[test]
fn keywords_are_not_identifiers() {
    assert_snapshot!(
        render("let fn struct type match if else for in import include as part self true false none where block self-test lets"),
        @r#"
    LetKw            0..3     "let"
    Whitespace       3..4     " "
    FnKw             4..6     "fn"
    Whitespace       6..7     " "
    StructKw         7..13    "struct"
    Whitespace       13..14   " "
    TypeKw           14..18   "type"
    Whitespace       18..19   " "
    MatchKw          19..24   "match"
    Whitespace       24..25   " "
    IfKw             25..27   "if"
    Whitespace       27..28   " "
    ElseKw           28..32   "else"
    Whitespace       32..33   " "
    ForKw            33..36   "for"
    Whitespace       36..37   " "
    InKw             37..39   "in"
    Whitespace       39..40   " "
    ImportKw         40..46   "import"
    Whitespace       46..47   " "
    IncludeKw        47..54   "include"
    Whitespace       54..55   " "
    AsKw             55..57   "as"
    Whitespace       57..58   " "
    PartKw           58..62   "part"
    Whitespace       62..63   " "
    SelfKw           63..67   "self"
    Whitespace       67..68   " "
    TrueKw           68..72   "true"
    Whitespace       72..73   " "
    FalseKw          73..78   "false"
    Whitespace       78..79   " "
    NoneKw           79..83   "none"
    Whitespace       83..84   " "
    WhereKw          84..89   "where"
    Whitespace       89..90   " "
    BlockKw          90..95   "block"
    Whitespace       95..96   " "
    Ident            96..105  "self-test"
    Whitespace       105..106 " "
    Ident            106..110 "lets"
    Eof              110..110 ""
    "#
    );
}

#[test]
fn identifiers_with_version_or_variant_are_symbol_words() {
    assert_snapshot!(
        render("lib@19 tool:static lib@19.1:static lib@1.0~rc_2 port:Int lib@19:Big port: str"),
        @r#"
    SymWord          0..6     "lib@19"
    Whitespace       6..7     " "
    SymWord          7..18    "tool:static"
    Whitespace       18..19   " "
    SymWord          19..34   "lib@19.1:static"
    Whitespace       34..35   " "
    SymWord          35..47   "lib@1.0~rc_2"
    Whitespace       47..48   " "
    Ident            48..52   "port"
    Colon            52..53   ":"
    TypeIdent        53..56   "Int"
    Whitespace       56..57   " "
    SymWord          57..63   "lib@19"
    Colon            63..64   ":"
    TypeIdent        64..67   "Big"
    Whitespace       67..68   " "
    Ident            68..72   "port"
    Colon            72..73   ":"
    Whitespace       73..74   " "
    Ident            74..77   "str"
    Eof              77..77   ""
    "#
    );
}

#[test]
fn at_sign_outside_symbol_word_is_an_error() {
    assert_snapshot!(render("lib @19\nlib@ 19\nlib@"), @r#"
    Ident            0..3     "lib"
    Whitespace       3..4     " "
    Error            4..5     "@"
    Int              5..7     "19"
    Newline          7..8     "\n"
    Ident            8..11    "lib"
    Error            11..12   "@"
    Whitespace       12..13   " "
    Int              13..15   "19"
    Newline          15..16   "\n"
    Ident            16..19   "lib"
    Error            19..20   "@"
    Eof              20..20   ""
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
    Int              0..2     "42"
    Whitespace       2..3     " "
    Int              3..8     "0o755"
    Whitespace       8..9     " "
    Int              9..13    "0x1f"
    Whitespace       13..14   " "
    Int              14..20   "0b1010"
    Whitespace       20..21   " "
    Int              21..30   "1_000_000"
    Whitespace       30..31   " "
    Float            31..34   "0.5"
    Whitespace       34..35   " "
    Float            35..41   "1.0e-3"
    Whitespace       41..42   " "
    Float            42..50   "2_500.25"
    Whitespace       50..51   " "
    Float            51..55   "1E+5"
    Whitespace       55..56   " "
    Int              56..59   "007"
    Eof              59..59   ""
    "#
    );
}

#[test]
fn number_dots_without_digits_are_punctuation() {
    assert_snapshot!(render("1..5 1..=5 1.max"), @r#"
    Int              0..1     "1"
    DotDot           1..3     ".."
    Int              3..4     "5"
    Whitespace       4..5     " "
    Int              5..6     "1"
    DotDotEq         6..9     "..="
    Int              9..10    "5"
    Whitespace       10..11   " "
    Int              11..12   "1"
    Dot              12..13   "."
    Ident            13..16   "max"
    Eof              16..16   ""
    "#);
}

#[test]
fn number_glued_to_name_is_one_invalid_token() {
    assert_snapshot!(render("0ad 0ad@0.27 0b102 0x 1e 1_ 0x_1"), @r#"
    Error            0..3     "0ad"
    Whitespace       3..4     " "
    Error            4..12    "0ad@0.27"
    Whitespace       12..13   " "
    Error            13..18   "0b102"
    Whitespace       18..19   " "
    Error            19..21   "0x"
    Whitespace       21..22   " "
    Error            22..24   "1e"
    Whitespace       24..25   " "
    Error            25..27   "1_"
    Whitespace       27..28   " "
    Error            28..32   "0x_1"
    Eof              32..32   ""
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
      help: remove the trailing `_`
    error[E0103] 28..32: `0x_1` is not a valid token
      help: separate the number from the name, or start the name with a letter
    "#);
}

#[test]
fn numbers_out_of_range_are_reported() {
    assert_snapshot!(
        render("9223372036854775807 9223372036854775808 0x8000000000000000 1e308 1e309"),
        @r#"
    Int              0..19    "9223372036854775807"
    Whitespace       19..20   " "
    Int              20..39   "9223372036854775808"
    Whitespace       39..40   " "
    Int              40..58   "0x8000000000000000"
    Whitespace       58..59   " "
    Float            59..64   "1e308"
    Whitespace       64..65   " "
    Float            65..70   "1e309"
    Eof              70..70   ""
    error[E0107] 20..39: integer `9223372036854775808` does not fit in 64 bits
      help: the largest integer is 9223372036854775807
    error[E0107] 40..58: integer `0x8000000000000000` does not fit in 64 bits
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
    LBrace           0..1     "{"
    RBrace           1..2     "}"
    LBracket         2..3     "["
    RBracket         3..4     "]"
    LParen           4..5     "("
    RParen           5..6     ")"
    Comma            6..7     ","
    Eq               7..8     "="
    Dot              8..9     "."
    QuestionDot      9..11    "?."
    QuestionQuestion 11..13   "??"
    Colon            13..14   ":"
    Arrow            14..16   "->"
    FatArrow         16..18   "=>"
    DotDot           18..20   ".."
    DotDot           20..22   ".."
    Dot              22..23   "."
    Eq               23..24   "="
    Pipe             24..25   "|"
    Plus             25..26   "+"
    Minus            26..27   "-"
    Star             27..28   "*"
    Slash            28..29   "/"
    Percent          29..30   "%"
    PlusPlus         30..32   "++"
    EqEq             32..34   "=="
    NotEq            34..36   "!="
    Lt               36..37   "<"
    LtEq             37..39   "<="
    Gt               39..40   ">"
    GtEq             40..42   ">="
    AndAnd           42..44   "&&"
    OrOr             44..46   "||"
    Bang             46..47   "!"
    Eof              47..47   ""
    "#
    );
}

#[test]
fn lone_question_mark_and_ampersand_are_errors() {
    assert_snapshot!(render("a ? b & c"), @r#"
    Ident            0..1     "a"
    Whitespace       1..2     " "
    Error            2..3     "?"
    Whitespace       3..4     " "
    Ident            4..5     "b"
    Whitespace       5..6     " "
    Error            6..7     "&"
    Whitespace       7..8     " "
    Ident            8..9     "c"
    Eof              9..9     ""
    error[E0113] 2..3: unexpected character `?`
    error[E0113] 6..7: unexpected character `&`
      help: write `&&` for logical and
    "#);
}

#[test]
fn comments_run_to_the_end_of_the_line() {
    assert_snapshot!(render("# line\n## doc\n##\n###x\n##x\na # trailing"), @r####"
    Comment          0..6     "# line"
    Newline          6..7     "\n"
    DocComment       7..13    "## doc"
    Newline          13..14   "\n"
    DocComment       14..16   "##"
    Newline          16..17   "\n"
    Comment          17..21   "###x"
    Newline          21..22   "\n"
    Comment          22..25   "##x"
    Newline          25..26   "\n"
    Ident            26..27   "a"
    Whitespace       27..28   " "
    Comment          28..38   "# trailing"
    Eof              38..38   ""
    "####);
}

#[test]
fn every_line_feed_is_a_token() {
    assert_snapshot!(render("a\n\n  # note\nb\n"), @r##"
    Ident            0..1     "a"
    Newline          1..2     "\n"
    Newline          2..3     "\n"
    Whitespace       3..5     "  "
    Comment          5..11    "# note"
    Newline          11..12   "\n"
    Ident            12..13   "b"
    Newline          13..14   "\n"
    Eof              14..14   ""
    "##);
}

#[test]
fn relative_paths_are_literals() {
    assert_snapshot!(render("./fix.patch ../files/a.conf ./a+b@c=d,e x/y"), @r#"
    Path             0..11    "./fix.patch"
    Whitespace       11..12   " "
    Path             12..27   "../files/a.conf"
    Whitespace       27..28   " "
    Path             28..39   "./a+b@c=d,e"
    Whitespace       39..40   " "
    Ident            40..41   "x"
    Slash            41..42   "/"
    Ident            42..43   "y"
    Eof              43..43   ""
    "#);
}

#[test]
fn byte_order_mark_and_carriage_returns_are_rejected_once() {
    assert_snapshot!(render("\u{feff}a\r\nb\r\n"), @r#"
    Error            0..3     "\u{feff}"
    Ident            3..4     "a"
    Error            4..5     "\r"
    Newline          5..6     "\n"
    Ident            6..7     "b"
    Error            7..8     "\r"
    Newline          8..9     "\n"
    Eof              9..9     ""
    error[E0101] 0..3: byte order mark is not allowed
      help: save the file as UTF-8 without a byte order mark
    error[E0101] 4..5: carriage return is not allowed
      help: save the file with LF line endings
    "#);
}

#[test]
fn foreign_whitespace_and_characters_are_errors() {
    assert_snapshot!(render("a\u{a0}b \u{20ac} \u{0}"), @r#"
    Ident            0..1     "a"
    Error            1..3     "\u{a0}"
    Ident            3..4     "b"
    Whitespace       4..5     " "
    Error            5..8     "€"
    Whitespace       8..9     " "
    Error            9..10    "\0"
    Eof              10..10   ""
    error[E0102] 1..3: invalid whitespace character U+00A0
      help: use spaces or tabs
    error[E0113] 5..8: unexpected character `€`
    error[E0113] 9..10: unexpected character U+0000
    "#);
}

#[test]
fn identifier_length_is_limited() {
    assert!(codes(&"a".repeat(255)).is_empty());
    assert_eq!(codes(&"a".repeat(256)), ["E0104"]);
    assert_eq!(codes(&format!("{}@1", "a".repeat(256))), ["E0104"]);
}

#[test]
fn empty_source_is_only_eof() {
    assert_snapshot!(render(""), @r#"Eof              0..0     """#);
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
    fn stav_like_text_lexes_losslessly(
        parts in prop::collection::vec(prop::sample::select(FRAGMENTS), 0..64),
    ) {
        assert_lossless(&parts.concat());
    }
}

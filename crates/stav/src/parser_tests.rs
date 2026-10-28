use std::fmt::Write;

use insta::assert_snapshot;
use proptest::prelude::*;

use crate::ast::FileKind;
use crate::{MAX_DEPTH, SyntaxElement, SyntaxNode, TokenKind, parse};

fn render(source: &str) -> String {
    let parsed = parse(source);
    let mut out = String::new();
    dump(parsed.tree.root(), 0, &mut out);
    out.push_str(&render_diagnostics(&parsed.diagnostics));
    out
}

fn dump(node: SyntaxNode<'_>, depth: usize, out: &mut String) {
    let span = node.span();
    let indent = depth * 2;
    writeln!(
        out,
        "{:indent$}{:?}@{}..{}",
        "",
        node.kind(),
        span.start,
        span.end
    )
    .unwrap();
    for element in node.children_with_tokens() {
        match element {
            SyntaxElement::Node(child) => dump(child, depth + 1, out),
            SyntaxElement::Token(token) if token.kind() == TokenKind::Whitespace => {}
            SyntaxElement::Token(token) => {
                writeln!(
                    out,
                    "{:indent$}{:?} {:?}",
                    "",
                    token.kind(),
                    token.text(),
                    indent = indent + 2
                )
                .unwrap();
            }
        }
    }
}

fn render_diagnostics(diagnostics: &[crate::Diagnostic]) -> String {
    let mut out = String::new();
    for d in diagnostics {
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

/// Renders only the diagnostics, for tests where the tree is not the
/// point.
fn errors(source: &str) -> String {
    render_diagnostics(&parse(source).diagnostics)
}

fn codes(source: &str) -> Vec<&'static str> {
    parse(source).diagnostics.iter().map(|d| d.code).collect()
}

/// Checks the invariants every tree must hold, whatever the input: the
/// tokens reproduce the source, every node spans exactly its tokens, the
/// children of a node are ordered and disjoint, nesting is bounded and
/// diagnostics point into the source. Returns the depth.
fn assert_valid_tree(source: &str) -> usize {
    let parsed = parse(source);
    let root = parsed.tree.root();
    assert_eq!(root.kind(), crate::NodeKind::File);
    assert_eq!((root.span().start, root.span().end), (0, source.len()));

    let mut text = String::new();
    let mut max_depth = 0;
    check_node(root, 0, &mut max_depth, &mut text);
    assert_eq!(text, source);

    for d in &parsed.diagnostics {
        assert!(source.get(d.span.range()).is_some(), "{d:?}");
    }
    assert!(
        parsed
            .diagnostics
            .windows(2)
            .all(|w| w[0].span.start <= w[1].span.start)
    );
    max_depth
}

/// Checks that `node` spans exactly the tokens under it, from the start of
/// the first to the end of the last, or is empty when it has none, and that
/// its children follow each other without overlapping. Appends the text of
/// its tokens to `text` and returns their extent.
fn check_node(
    node: SyntaxNode<'_>,
    depth: usize,
    max_depth: &mut usize,
    text: &mut String,
) -> Option<(usize, usize)> {
    *max_depth = (*max_depth).max(depth);
    let mut extent: Option<(usize, usize)> = None;
    let mut previous_end = 0;
    for element in node.children_with_tokens() {
        let span = element.span();
        assert!(span.start <= span.end, "{element:?}");
        assert!(
            previous_end <= span.start,
            "{element:?} overlaps in {node:?}"
        );
        previous_end = span.end;
        let tokens = match element {
            SyntaxElement::Token(token) => {
                let parent = token.parent();
                assert_eq!((parent.kind(), parent.span()), (node.kind(), node.span()));
                text.push_str(token.text());
                Some((span.start, span.end))
            }
            SyntaxElement::Node(child) => {
                let parent = child.parent().expect("a child node has a parent");
                assert_eq!((parent.kind(), parent.span()), (node.kind(), node.span()));
                check_node(child, depth + 1, max_depth, text)
            }
        };
        if let Some((start, end)) = tokens {
            extent = Some((extent.map_or(start, |(first, _)| first), end));
        }
    }
    let span = node.span();
    match extent {
        Some(extent) => assert_eq!((span.start, span.end), extent, "{node:?}"),
        None => assert_eq!(span.start, span.end, "{node:?}"),
    }
    extent
}

#[test]
fn attributes_and_blocks_form_a_body() {
    assert_snapshot!(render(r##"port = 8080
a.b."c d" = 1
service gateway { exec = ["/usr/bin/gateway"], user = "root" }
fs "/" {}
server "#{name}" {
  boot { kernel = kernel-lts }
}
"##), @r##"
    File@0..151
      Attr@0..11
        AttrPath@0..4
          Ident "port"
        Eq "="
        Literal@7..11
          Int "8080"
      Newline "\n"
      Attr@12..25
        AttrPath@12..21
          Ident "a"
          Dot "."
          Ident "b"
          Dot "."
          Str@16..21
            StrStart "\""
            StrText "c d"
            StrEnd "\""
        Eq "="
        Literal@24..25
          Int "1"
      Newline "\n"
      Block@26..88
        Ident "service"
        Ident "gateway"
        Body@42..88
          LBrace "{"
          Attr@44..71
            AttrPath@44..48
              Ident "exec"
            Eq "="
            List@51..71
              LBracket "["
              Str@52..70
                StrStart "\""
                StrText "/usr/bin/gateway"
                StrEnd "\""
              RBracket "]"
          Comma ","
          Attr@73..86
            AttrPath@73..77
              Ident "user"
            Eq "="
            Str@80..86
              StrStart "\""
              StrText "root"
              StrEnd "\""
          RBrace "}"
      Newline "\n"
      Block@89..98
        Ident "fs"
        Str@92..95
          StrStart "\""
          StrText "/"
          StrEnd "\""
        Body@96..98
          LBrace "{"
          RBrace "}"
      Newline "\n"
      Block@99..150
        Ident "server"
        Str@106..115
          StrStart "\""
          Interp@107..114
            InterpStart "#{"
            Name@109..113
              Ident "name"
            InterpEnd "}"
          StrEnd "\""
        Body@116..150
          LBrace "{"
          Newline "\n"
          Block@120..148
            Ident "boot"
            Body@125..148
              LBrace "{"
              Attr@127..146
                AttrPath@127..133
                  Ident "kernel"
                Eq "="
                Name@136..146
                  Ident "kernel-lts"
              RBrace "}"
          Newline "\n"
          RBrace "}"
      Newline "\n"
    "##);
}

#[test]
fn declarations_at_the_top_level() {
    assert_snapshot!(render(r##"import "std/desktop" as desktop
## A service.
struct Service {
  kind:  ServiceKind = longrun
  exec:  List<Str> where nonEmpty
  user:  Option<Str>
  units: Map<Str, Unit> = {} block unit
}
type Ready =
  | immediate
  | notify(fd: Int where 0..=1023)
  | check(cmd: List<Str>, interval: Int = 1)
fn gateway(port: Int = 22) -> Part<System> = part {
  packages = [gatewayd]
}
let hosts: List<Str> = ["a"]
"##), @r###"
    File@0..405
      Import@0..31
        ImportKw "import"
        Str@7..20
          StrStart "\""
          StrText "std/desktop"
          StrEnd "\""
        AsKw "as"
        Ident "desktop"
      Newline "\n"
      DocComment "## A service."
      Newline "\n"
      Struct@46..190
        StructKw "struct"
        TypeIdent "Service"
        LBrace "{"
        Newline "\n"
        Field@65..93
          Ident "kind"
          Colon ":"
          NamedType@72..83
            TypePath@72..83
              TypeIdent "ServiceKind"
          Eq "="
          Name@86..93
            Ident "longrun"
        Newline "\n"
        Field@96..127
          Ident "exec"
          Colon ":"
          RefinedType@103..127
            NamedType@103..112
              TypePath@103..107
                TypeIdent "List"
              TypeArgList@107..112
                Lt "<"
                NamedType@108..111
                  TypePath@108..111
                    TypeIdent "Str"
                Gt ">"
            WhereKw "where"
            NamedPredicate@119..127
              Ident "nonEmpty"
        Newline "\n"
        Field@130..148
          Ident "user"
          Colon ":"
          NamedType@137..148
            TypePath@137..143
              TypeIdent "Option"
            TypeArgList@143..148
              Lt "<"
              NamedType@144..147
                TypePath@144..147
                  TypeIdent "Str"
              Gt ">"
        Newline "\n"
        Field@151..188
          Ident "units"
          Colon ":"
          NamedType@158..172
            TypePath@158..161
              TypeIdent "Map"
            TypeArgList@161..172
              Lt "<"
              NamedType@162..165
                TypePath@162..165
                  TypeIdent "Str"
              Comma ","
              NamedType@167..171
                TypePath@167..171
                  TypeIdent "Unit"
              Gt ">"
          Eq "="
          Map@175..177
            LBrace "{"
            RBrace "}"
          BlockKw "block"
          Ident "unit"
        Newline "\n"
        RBrace "}"
      Newline "\n"
      TypeDecl@191..297
        TypeKw "type"
        TypeIdent "Ready"
        Eq "="
        Newline "\n"
        Pipe "|"
        Variant@208..217
          Ident "immediate"
        Newline "\n"
        Pipe "|"
        Variant@222..252
          Ident "notify"
          ParamList@228..252
            LParen "("
            Param@229..251
              Ident "fd"
              Colon ":"
              RefinedType@233..251
                NamedType@233..236
                  TypePath@233..236
                    TypeIdent "Int"
                WhereKw "where"
                RangePredicate@243..251
                  Literal@243..244
                    Int "0"
                  DotDotEq "..="
                  Literal@247..251
                    Int "1023"
            RParen ")"
        Newline "\n"
        Pipe "|"
        Variant@257..297
          Ident "check"
          ParamList@262..297
            LParen "("
            Param@263..277
              Ident "cmd"
              Colon ":"
              NamedType@268..277
                TypePath@268..272
                  TypeIdent "List"
                TypeArgList@272..277
                  Lt "<"
                  NamedType@273..276
                    TypePath@273..276
                      TypeIdent "Str"
                  Gt ">"
            Comma ","
            Param@279..296
              Ident "interval"
              Colon ":"
              NamedType@289..292
                TypePath@289..292
                  TypeIdent "Int"
              Eq "="
              Literal@295..296
                Int "1"
            RParen ")"
      Newline "\n"
      FnDecl@298..375
        FnKw "fn"
        Ident "gateway"
        ParamList@308..324
          LParen "("
          Param@309..323
            Ident "port"
            Colon ":"
            NamedType@315..318
              TypePath@315..318
                TypeIdent "Int"
            Eq "="
            Literal@321..323
              Int "22"
          RParen ")"
        Arrow "->"
        NamedType@328..340
          TypePath@328..332
            TypeIdent "Part"
          TypeArgList@332..340
            Lt "<"
            NamedType@333..339
              TypePath@333..339
                TypeIdent "System"
            Gt ">"
        Eq "="
        PartExpr@343..375
          PartKw "part"
          Body@348..375
            LBrace "{"
            Newline "\n"
            Attr@352..373
              AttrPath@352..360
                Ident "packages"
              Eq "="
              List@363..373
                LBracket "["
                Name@364..372
                  Ident "gatewayd"
                RBracket "]"
            Newline "\n"
            RBrace "}"
      Newline "\n"
      Let@376..404
        LetKw "let"
        Ident "hosts"
        Colon ":"
        NamedType@387..396
          TypePath@387..391
            TypeIdent "List"
          TypeArgList@391..396
            Lt "<"
            NamedType@392..395
              TypePath@392..395
                TypeIdent "Str"
            Gt ">"
        Eq "="
        List@399..404
          LBracket "["
          Str@400..403
            StrStart "\""
            StrText "a"
            StrEnd "\""
          RBracket "]"
      Newline "\n"
    "###);
}

#[test]
fn statements_include_branch_and_repeat() {
    assert_snapshot!(render(r##"system {
  let base = 1
  include "./hardware.stav"
  include desktop.graphical(compositor = tiling)
  if tls {
    port = 443
  } else if plain { port = 80 }
  else { port = 8080 }
  for k, v in env { vars = [k, v] }
}
"##), @r#"
    File@0..220
      Block@0..219
        Ident "system"
        Body@7..219
          LBrace "{"
          Newline "\n"
          Let@11..23
            LetKw "let"
            Ident "base"
            Eq "="
            Literal@22..23
              Int "1"
          Newline "\n"
          Include@26..51
            IncludeKw "include"
            Str@34..51
              StrStart "\""
              StrText "./hardware.stav"
              StrEnd "\""
          Newline "\n"
          Include@54..100
            IncludeKw "include"
            Call@62..100
              FieldExpr@62..79
                Name@62..69
                  Ident "desktop"
                Dot "."
                Ident "graphical"
              ArgList@79..100
                LParen "("
                Arg@80..99
                  Ident "compositor"
                  Eq "="
                  Name@93..99
                    Ident "tiling"
                RParen ")"
          Newline "\n"
          IfStmt@103..181
            IfKw "if"
            Name@106..109
              Ident "tls"
            Body@110..130
              LBrace "{"
              Newline "\n"
              Attr@116..126
                AttrPath@116..120
                  Ident "port"
                Eq "="
                Literal@123..126
                  Int "443"
              Newline "\n"
              RBrace "}"
            ElseKw "else"
            IfStmt@136..181
              IfKw "if"
              Name@139..144
                Ident "plain"
              Body@145..158
                LBrace "{"
                Attr@147..156
                  AttrPath@147..151
                    Ident "port"
                  Eq "="
                  Literal@154..156
                    Int "80"
                RBrace "}"
              Newline "\n"
              ElseKw "else"
              Body@166..181
                LBrace "{"
                Attr@168..179
                  AttrPath@168..172
                    Ident "port"
                  Eq "="
                  Literal@175..179
                    Int "8080"
                RBrace "}"
          Newline "\n"
          ForStmt@184..217
            ForKw "for"
            Ident "k"
            Comma ","
            Ident "v"
            InKw "in"
            Name@196..199
              Ident "env"
            Body@200..217
              LBrace "{"
              Attr@202..215
                AttrPath@202..206
                  Ident "vars"
                Eq "="
                List@209..215
                  LBracket "["
                  Name@210..211
                    Ident "k"
                  Comma ","
                  Name@213..214
                    Ident "v"
                  RBracket "]"
              RBrace "}"
          Newline "\n"
          RBrace "}"
      Newline "\n"
    "#);
}

#[test]
fn strings_in_every_form() {
    let source = concat!(
        "a = \"x\\n#{y}\\#{z}\"\n",
        "b = \"\"\"\n  line #{c}\n  \\#{d}\n  \"\"\"\n",
        "c = r\"\\d+#{x}\"\n",
        "d = r\"\"\"\n  raw\n  \"\"\"\n",
        "e = \"#{\"#{f}\"}\"\n",
    );
    assert_snapshot!(render(source), @r##"
    File@0..105
      Attr@0..18
        AttrPath@0..1
          Ident "a"
        Eq "="
        Str@4..18
          StrStart "\""
          StrText "x"
          StrEscape "\\n"
          Interp@8..12
            InterpStart "#{"
            Name@10..11
              Ident "y"
            InterpEnd "}"
          StrEscape "\\#"
          StrText "{z}"
          StrEnd "\""
      Newline "\n"
      Attr@19..52
        AttrPath@19..20
          Ident "b"
        Eq "="
        Str@23..52
          MStrStart "\"\"\""
          StrLayout "\n"
          StrLayout "  "
          StrText "line "
          Interp@34..38
            InterpStart "#{"
            Name@36..37
              Ident "c"
            InterpEnd "}"
          StrText "\n"
          StrLayout "  "
          StrEscape "\\#"
          StrText "{d}\n"
          StrLayout "  "
          MStrEnd "\"\"\""
      Newline "\n"
      Attr@53..67
        AttrPath@53..54
          Ident "c"
        Eq "="
        Str@57..67
          RawStrStart "r\""
          StrText "\\d+#{x}"
          StrEnd "\""
      Newline "\n"
      Attr@68..88
        AttrPath@68..69
          Ident "d"
        Eq "="
        Str@72..88
          RawMStrStart "r\"\"\""
          StrLayout "\n"
          StrLayout "  "
          StrText "raw\n"
          StrLayout "  "
          MStrEnd "\"\"\""
      Newline "\n"
      Attr@89..104
        AttrPath@89..90
          Ident "e"
        Eq "="
        Str@93..104
          StrStart "\""
          Interp@94..103
            InterpStart "#{"
            Str@96..102
              StrStart "\""
              Interp@97..101
                InterpStart "#{"
                Name@99..100
                  Ident "f"
                InterpEnd "}"
              StrEnd "\""
            InterpEnd "}"
          StrEnd "\""
      Newline "\n"
    "##);
}

#[test]
fn literals_names_and_symbol_words() {
    assert_snapshot!(
        render("a = [1, 0x1f, 2.5, ./fix.patch, true, false, none, self.x, compiler@19.1:static, MIT, Level.info, app-utils]\n"),
        @r#"
    File@0..109
      Attr@0..108
        AttrPath@0..1
          Ident "a"
        Eq "="
        List@4..108
          LBracket "["
          Literal@5..6
            Int "1"
          Comma ","
          Literal@8..12
            Int "0x1f"
          Comma ","
          Literal@14..17
            Float "2.5"
          Comma ","
          Literal@19..30
            Path "./fix.patch"
          Comma ","
          Literal@32..36
            TrueKw "true"
          Comma ","
          Literal@38..43
            FalseKw "false"
          Comma ","
          Literal@45..49
            NoneKw "none"
          Comma ","
          FieldExpr@51..57
            SelfExpr@51..55
              SelfKw "self"
            Dot "."
            Ident "x"
          Comma ","
          Symbol@59..79
            SymWord "compiler@19.1:static"
          Comma ","
          Name@81..84
            TypeIdent "MIT"
          Comma ","
          FieldExpr@86..96
            Name@86..91
              TypeIdent "Level"
            Dot "."
            Ident "info"
          Comma ","
          Name@98..107
            Ident "app-utils"
          RBracket "]"
      Newline "\n"
    "#
    );
}

#[test]
fn collections_and_struct_literals() {
    assert_snapshot!(render(r##"a = []
b = {}
c = { port = 80, "mem.reclaim" = 10, tls = { enabled = true } }
d = Model { id = "x", provider = local }
e = svc.Ready { fd = 3 }
f = [for x in xs if x > 0 -> x * 2]
g = {for k, v in m -> "#{k}" = v}
h = [
  1,
  2
]
"##), @r##"
    File@0..231
      Attr@0..6
        AttrPath@0..1
          Ident "a"
        Eq "="
        List@4..6
          LBracket "["
          RBracket "]"
      Newline "\n"
      Attr@7..13
        AttrPath@7..8
          Ident "b"
        Eq "="
        Map@11..13
          LBrace "{"
          RBrace "}"
      Newline "\n"
      Attr@14..77
        AttrPath@14..15
          Ident "c"
        Eq "="
        Map@18..77
          LBrace "{"
          MapEntry@20..29
            Ident "port"
            Eq "="
            Literal@27..29
              Int "80"
          Comma ","
          MapEntry@31..49
            Str@31..44
              StrStart "\""
              StrText "mem.reclaim"
              StrEnd "\""
            Eq "="
            Literal@47..49
              Int "10"
          Comma ","
          MapEntry@51..75
            Ident "tls"
            Eq "="
            Map@57..75
              LBrace "{"
              MapEntry@59..73
                Ident "enabled"
                Eq "="
                Literal@69..73
                  TrueKw "true"
              RBrace "}"
          RBrace "}"
      Newline "\n"
      Attr@78..118
        AttrPath@78..79
          Ident "d"
        Eq "="
        StructLit@82..118
          TypePath@82..87
            TypeIdent "Model"
          Map@88..118
            LBrace "{"
            MapEntry@90..98
              Ident "id"
              Eq "="
              Str@95..98
                StrStart "\""
                StrText "x"
                StrEnd "\""
            Comma ","
            MapEntry@100..116
              Ident "provider"
              Eq "="
              Name@111..116
                Ident "local"
            RBrace "}"
      Newline "\n"
      Attr@119..143
        AttrPath@119..120
          Ident "e"
        Eq "="
        StructLit@123..143
          TypePath@123..132
            Ident "svc"
            Dot "."
            TypeIdent "Ready"
          Map@133..143
            LBrace "{"
            MapEntry@135..141
              Ident "fd"
              Eq "="
              Literal@140..141
                Int "3"
            RBrace "}"
      Newline "\n"
      Attr@144..179
        AttrPath@144..145
          Ident "f"
        Eq "="
        ListComp@148..179
          LBracket "["
          ForKw "for"
          Ident "x"
          InKw "in"
          Name@158..160
            Ident "xs"
          IfKw "if"
          BinaryExpr@164..169
            Name@164..165
              Ident "x"
            Gt ">"
            Literal@168..169
              Int "0"
          Arrow "->"
          BinaryExpr@173..178
            Name@173..174
              Ident "x"
            Star "*"
            Literal@177..178
              Int "2"
          RBracket "]"
      Newline "\n"
      Attr@180..213
        AttrPath@180..181
          Ident "g"
        Eq "="
        MapComp@184..213
          LBrace "{"
          ForKw "for"
          Ident "k"
          Comma ","
          Ident "v"
          InKw "in"
          Name@197..198
            Ident "m"
          Arrow "->"
          Str@202..208
            StrStart "\""
            Interp@203..207
              InterpStart "#{"
              Name@205..206
                Ident "k"
              InterpEnd "}"
            StrEnd "\""
          Eq "="
          Name@211..212
            Ident "v"
          RBrace "}"
      Newline "\n"
      Attr@214..230
        AttrPath@214..215
          Ident "h"
        Eq "="
        List@218..230
          LBracket "["
          Newline "\n"
          Literal@222..223
            Int "1"
          Comma ","
          Newline "\n"
          Literal@227..228
            Int "2"
          Newline "\n"
          RBracket "]"
      Newline "\n"
    "##);
}

#[test]
fn calls_field_access_and_indexing() {
    assert_snapshot!(
        render("a = f(1, x = 2)(3).b?.c[0] ?? force(d)\nb = default(\"x\")\n"),
        @r#"
    File@0..56
      Attr@0..38
        AttrPath@0..1
          Ident "a"
        Eq "="
        BinaryExpr@4..38
          IndexExpr@4..26
            FieldExpr@4..23
              FieldExpr@4..20
                Call@4..18
                  Call@4..15
                    Name@4..5
                      Ident "f"
                    ArgList@5..15
                      LParen "("
                      Arg@6..7
                        Literal@6..7
                          Int "1"
                      Comma ","
                      Arg@9..14
                        Ident "x"
                        Eq "="
                        Literal@13..14
                          Int "2"
                      RParen ")"
                  ArgList@15..18
                    LParen "("
                    Arg@16..17
                      Literal@16..17
                        Int "3"
                    RParen ")"
                Dot "."
                Ident "b"
              QuestionDot "?."
              Ident "c"
            LBracket "["
            Literal@24..25
              Int "0"
            RBracket "]"
          QuestionQuestion "??"
          Call@30..38
            Name@30..35
              Ident "force"
            ArgList@35..38
              LParen "("
              Arg@36..37
                Name@36..37
                  Ident "d"
              RParen ")"
      Newline "\n"
      Attr@39..55
        AttrPath@39..40
          Ident "b"
        Eq "="
        Call@43..55
          Name@43..50
            Ident "default"
          ArgList@50..55
            LParen "("
            Arg@51..54
              Str@51..54
                StrStart "\""
                StrText "x"
                StrEnd "\""
            RParen ")"
      Newline "\n"
    "#
    );
}

#[test]
fn conditional_match_let_lambda_and_part_expressions() {
    assert_snapshot!(render(r##"a = if x { 1 } else if y { 2 } else { 3 }
b = match r {
  immediate => "now"
  notify(fd) => "fd"
  check(cmd, interval = i, ..) if i > 10 => "slow"
  _ => "other"
}
c = let x = 1, y: Int = 2 in x + y
d = fn(x: Int) -> Int = x + 1
e = part { a = 1 }
"##), @r#"
    File@0..250
      Attr@0..41
        AttrPath@0..1
          Ident "a"
        Eq "="
        IfExpr@4..41
          IfKw "if"
          Name@7..8
            Ident "x"
          LBrace "{"
          Literal@11..12
            Int "1"
          RBrace "}"
          ElseKw "else"
          IfExpr@20..41
            IfKw "if"
            Name@23..24
              Ident "y"
            LBrace "{"
            Literal@27..28
              Int "2"
            RBrace "}"
            ElseKw "else"
            LBrace "{"
            Literal@38..39
              Int "3"
            RBrace "}"
      Newline "\n"
      Attr@42..165
        AttrPath@42..43
          Ident "b"
        Eq "="
        MatchExpr@46..165
          MatchKw "match"
          Name@52..53
            Ident "r"
          LBrace "{"
          Newline "\n"
          MatchArm@58..76
            NamePat@58..67
              Ident "immediate"
            FatArrow "=>"
            Str@71..76
              StrStart "\""
              StrText "now"
              StrEnd "\""
          Newline "\n"
          MatchArm@79..97
            VariantPat@79..89
              Ident "notify"
              LParen "("
              FieldPat@86..88
                Ident "fd"
              RParen ")"
            FatArrow "=>"
            Str@93..97
              StrStart "\""
              StrText "fd"
              StrEnd "\""
          Newline "\n"
          MatchArm@100..148
            VariantPat@100..128
              Ident "check"
              LParen "("
              FieldPat@106..109
                Ident "cmd"
              Comma ","
              FieldPat@111..123
                Ident "interval"
                Eq "="
                NamePat@122..123
                  Ident "i"
              Comma ","
              DotDot ".."
              RParen ")"
            IfKw "if"
            BinaryExpr@132..138
              Name@132..133
                Ident "i"
              Gt ">"
              Literal@136..138
                Int "10"
            FatArrow "=>"
            Str@142..148
              StrStart "\""
              StrText "slow"
              StrEnd "\""
          Newline "\n"
          MatchArm@151..163
            WildcardPat@151..152
              Ident "_"
            FatArrow "=>"
            Str@156..163
              StrStart "\""
              StrText "other"
              StrEnd "\""
          Newline "\n"
          RBrace "}"
      Newline "\n"
      Attr@166..200
        AttrPath@166..167
          Ident "c"
        Eq "="
        LetExpr@170..200
          LetKw "let"
          Binding@174..179
            Ident "x"
            Eq "="
            Literal@178..179
              Int "1"
          Comma ","
          Binding@181..191
            Ident "y"
            Colon ":"
            NamedType@184..187
              TypePath@184..187
                TypeIdent "Int"
            Eq "="
            Literal@190..191
              Int "2"
          InKw "in"
          BinaryExpr@195..200
            Name@195..196
              Ident "x"
            Plus "+"
            Name@199..200
              Ident "y"
      Newline "\n"
      Attr@201..230
        AttrPath@201..202
          Ident "d"
        Eq "="
        Lambda@205..230
          FnKw "fn"
          ParamList@207..215
            LParen "("
            Param@208..214
              Ident "x"
              Colon ":"
              NamedType@211..214
                TypePath@211..214
                  TypeIdent "Int"
            RParen ")"
          Arrow "->"
          NamedType@219..222
            TypePath@219..222
              TypeIdent "Int"
          Eq "="
          BinaryExpr@225..230
            Name@225..226
              Ident "x"
            Plus "+"
            Literal@229..230
              Int "1"
      Newline "\n"
      Attr@231..249
        AttrPath@231..232
          Ident "e"
        Eq "="
        PartExpr@235..249
          PartKw "part"
          Body@240..249
            LBrace "{"
            Attr@242..247
              AttrPath@242..243
                Ident "a"
              Eq "="
              Literal@246..247
                Int "1"
            RBrace "}"
      Newline "\n"
    "#);
}

#[test]
fn patterns_of_every_form() {
    assert_snapshot!(render(r##"a = match v {
  0 | -1 | "s" | true | none => 1
  Level.info => 2
  x => 3
  notify(fd = 3 | 4, ..) => 4
}
"##), @r#"
    File@0..107
      Attr@0..106
        AttrPath@0..1
          Ident "a"
        Eq "="
        MatchExpr@4..106
          MatchKw "match"
          Name@10..11
            Ident "v"
          LBrace "{"
          Newline "\n"
          MatchArm@16..47
            OrPat@16..42
              LiteralPat@16..17
                Literal@16..17
                  Int "0"
              Pipe "|"
              LiteralPat@20..22
                UnaryExpr@20..22
                  Minus "-"
                  Literal@21..22
                    Int "1"
              Pipe "|"
              LiteralPat@25..28
                Str@25..28
                  StrStart "\""
                  StrText "s"
                  StrEnd "\""
              Pipe "|"
              LiteralPat@31..35
                Literal@31..35
                  TrueKw "true"
              Pipe "|"
              LiteralPat@38..42
                Literal@38..42
                  NoneKw "none"
            FatArrow "=>"
            Literal@46..47
              Int "1"
          Newline "\n"
          MatchArm@50..65
            NamePat@50..60
              TypeIdent "Level"
              Dot "."
              Ident "info"
            FatArrow "=>"
            Literal@64..65
              Int "2"
          Newline "\n"
          MatchArm@68..74
            NamePat@68..69
              Ident "x"
            FatArrow "=>"
            Literal@73..74
              Int "3"
          Newline "\n"
          MatchArm@77..104
            VariantPat@77..99
              Ident "notify"
              LParen "("
              FieldPat@84..94
                Ident "fd"
                Eq "="
                OrPat@89..94
                  LiteralPat@89..90
                    Literal@89..90
                      Int "3"
                  Pipe "|"
                  LiteralPat@93..94
                    Literal@93..94
                      Int "4"
              Comma ","
              DotDot ".."
              RParen ")"
            FatArrow "=>"
            Literal@103..104
              Int "4"
          Newline "\n"
          RBrace "}"
      Newline "\n"
    "#);
}

#[test]
fn types_with_arguments_refinements_and_functions() {
    assert_snapshot!(render(r##"struct A {
  a: Map<Str, List<Set<Int>>>
  b: alias.Type
  c: Fn(Int, Str) -> Bool
  d: Float where -1.5..=2.0 && nonEmpty && matches(r"^a")
  e: Int where ..5
  f: Int where 0..
  g: List<
    Str,
  >
}
"##), @r#"
    File@0..205
      Struct@0..204
        StructKw "struct"
        TypeIdent "A"
        LBrace "{"
        Newline "\n"
        Field@13..40
          Ident "a"
          Colon ":"
          NamedType@16..40
            TypePath@16..19
              TypeIdent "Map"
            TypeArgList@19..40
              Lt "<"
              NamedType@20..23
                TypePath@20..23
                  TypeIdent "Str"
              Comma ","
              NamedType@25..39
                TypePath@25..29
                  TypeIdent "List"
                TypeArgList@29..39
                  Lt "<"
                  NamedType@30..38
                    TypePath@30..33
                      TypeIdent "Set"
                    TypeArgList@33..38
                      Lt "<"
                      NamedType@34..37
                        TypePath@34..37
                          TypeIdent "Int"
                      Gt ">"
                  Gt ">"
              Gt ">"
        Newline "\n"
        Field@43..56
          Ident "b"
          Colon ":"
          NamedType@46..56
            TypePath@46..56
              Ident "alias"
              Dot "."
              TypeIdent "Type"
        Newline "\n"
        Field@59..82
          Ident "c"
          Colon ":"
          FnType@62..82
            TypeIdent "Fn"
            LParen "("
            NamedType@65..68
              TypePath@65..68
                TypeIdent "Int"
            Comma ","
            NamedType@70..73
              TypePath@70..73
                TypeIdent "Str"
            RParen ")"
            Arrow "->"
            NamedType@78..82
              TypePath@78..82
                TypeIdent "Bool"
        Newline "\n"
        Field@85..140
          Ident "d"
          Colon ":"
          RefinedType@88..140
            NamedType@88..93
              TypePath@88..93
                TypeIdent "Float"
            WhereKw "where"
            RangePredicate@100..110
              UnaryExpr@100..104
                Minus "-"
                Literal@101..104
                  Float "1.5"
              DotDotEq "..="
              Literal@107..110
                Float "2.0"
            AndAnd "&&"
            NamedPredicate@114..122
              Ident "nonEmpty"
            AndAnd "&&"
            NamedPredicate@126..140
              Ident "matches"
              ArgList@133..140
                LParen "("
                Arg@134..139
                  Str@134..139
                    RawStrStart "r\""
                    StrText "^a"
                    StrEnd "\""
                RParen ")"
        Newline "\n"
        Field@143..159
          Ident "e"
          Colon ":"
          RefinedType@146..159
            NamedType@146..149
              TypePath@146..149
                TypeIdent "Int"
            WhereKw "where"
            RangePredicate@156..159
              DotDot ".."
              Literal@158..159
                Int "5"
        Newline "\n"
        Field@162..178
          Ident "f"
          Colon ":"
          RefinedType@165..178
            NamedType@165..168
              TypePath@165..168
                TypeIdent "Int"
            WhereKw "where"
            RangePredicate@175..178
              Literal@175..176
                Int "0"
              DotDot ".."
        Newline "\n"
        Field@181..202
          Ident "g"
          Colon ":"
          NamedType@184..202
            TypePath@184..188
              TypeIdent "List"
            TypeArgList@188..202
              Lt "<"
              Newline "\n"
              NamedType@194..197
                TypePath@194..197
                  TypeIdent "Str"
              Comma ","
              Newline "\n"
              Gt ">"
        Newline "\n"
        RBrace "}"
      Newline "\n"
    "#);
}

#[test]
fn line_feeds_continue_expressions_only_where_they_cannot_end() {
    assert_snapshot!(render(r##"a = 1 +
  2
b = x
  .y
  ?? z
c = f(
  1,
  2
)
d = 1
- 2
e = if c { 1 }
  else { 2 }
type T =
  | a
  | b
"##), @r#"
    File@0..107
      Attr@0..11
        AttrPath@0..1
          Ident "a"
        Eq "="
        BinaryExpr@4..11
          Literal@4..5
            Int "1"
          Plus "+"
          Newline "\n"
          Literal@10..11
            Int "2"
      Newline "\n"
      Attr@12..29
        AttrPath@12..13
          Ident "b"
        Eq "="
        BinaryExpr@16..29
          FieldExpr@16..22
            Name@16..17
              Ident "x"
            Newline "\n"
            Dot "."
            Ident "y"
          Newline "\n"
          QuestionQuestion "??"
          Name@28..29
            Ident "z"
      Newline "\n"
      Attr@30..47
        AttrPath@30..31
          Ident "c"
        Eq "="
        Call@34..47
          Name@34..35
            Ident "f"
          ArgList@35..47
            LParen "("
            Newline "\n"
            Arg@39..40
              Literal@39..40
                Int "1"
            Comma ","
            Newline "\n"
            Arg@44..45
              Literal@44..45
                Int "2"
            Newline "\n"
            RParen ")"
      Newline "\n"
      Attr@48..53
        AttrPath@48..49
          Ident "d"
        Eq "="
        Literal@52..53
          Int "1"
      Newline "\n"
      Error@54..55
        Minus "-"
      Error@56..57
        Int "2"
      Newline "\n"
      Attr@58..85
        AttrPath@58..59
          Ident "e"
        Eq "="
        IfExpr@62..85
          IfKw "if"
          Name@65..66
            Ident "c"
          LBrace "{"
          Literal@69..70
            Int "1"
          RBrace "}"
          Newline "\n"
          ElseKw "else"
          LBrace "{"
          Literal@82..83
            Int "2"
          RBrace "}"
      Newline "\n"
      TypeDecl@86..106
        TypeKw "type"
        TypeIdent "T"
        Eq "="
        Newline "\n"
        Pipe "|"
        Variant@99..100
          Ident "a"
        Newline "\n"
        Pipe "|"
        Variant@105..106
          Ident "b"
      Newline "\n"
    error[E0210] 54..55: expected a declaration or a statement, found `-`
    "#);
}

#[test]
fn types_and_open_ranges_can_end_a_line() {
    let source = "struct A {\n  a: Option<Str>\n  b: Int where 1..\n  c: Int\n}\n";
    let parsed = parse(source);
    assert!(parsed.diagnostics.is_empty());
    let fields: Vec<usize> = parsed
        .tree
        .root()
        .children()
        .map(|item| {
            item.children()
                .filter(|node| node.kind() == crate::NodeKind::Field)
                .count()
        })
        .collect();
    assert_eq!(fields, [3]);
}

#[test]
fn type_name_before_brace_is_no_struct_literal_in_conditions() {
    assert_snapshot!(render("if Flag { a = 1 }\nfor x in Items { b = x }\nc = match Mode { _ => 1 }\n"), @r#"
    File@0..69
      IfStmt@0..17
        IfKw "if"
        Name@3..7
          TypeIdent "Flag"
        Body@8..17
          LBrace "{"
          Attr@10..15
            AttrPath@10..11
              Ident "a"
            Eq "="
            Literal@14..15
              Int "1"
          RBrace "}"
      Newline "\n"
      ForStmt@18..42
        ForKw "for"
        Ident "x"
        InKw "in"
        Name@27..32
          TypeIdent "Items"
        Body@33..42
          LBrace "{"
          Attr@35..40
            AttrPath@35..36
              Ident "b"
            Eq "="
            Name@39..40
              Ident "x"
          RBrace "}"
      Newline "\n"
      Attr@43..68
        AttrPath@43..44
          Ident "c"
        Eq "="
        MatchExpr@47..68
          MatchKw "match"
          Name@53..57
            TypeIdent "Mode"
          LBrace "{"
          MatchArm@60..66
            WildcardPat@60..61
              Ident "_"
            FatArrow "=>"
            Literal@65..66
              Int "1"
          RBrace "}"
      Newline "\n"
    "#);
}

#[test]
fn comments_stay_between_nodes() {
    assert_snapshot!(render("# head\n## doc\nstruct A { # trailing\n  a: Int # field\n}\n"), @r###"
    File@0..55
      Comment "# head"
      Newline "\n"
      DocComment "## doc"
      Newline "\n"
      Struct@14..54
        StructKw "struct"
        TypeIdent "A"
        LBrace "{"
        Comment "# trailing"
        Newline "\n"
        Field@38..44
          Ident "a"
          Colon ":"
          NamedType@41..44
            TypePath@41..44
              TypeIdent "Int"
        Comment "# field"
        Newline "\n"
        RBrace "}"
      Newline "\n"
    "###);
}

#[test]
fn empty_source_is_an_empty_file() {
    assert_snapshot!(render(""), @"File@0..0");
}

#[test]
fn independent_errors_are_all_reported() {
    assert_snapshot!(
        errors("a = [1 2]\nb = f(x = 1, 2)\nc = 3 4\nd = )\nstruct e {}\nf = 1 +\n"),
        @"
    error[E0210] 7..8: expected a line break, `,` or `]`, found `2`
    error[E0203] 23..24: positional argument after a named argument
      help: move positional arguments before named ones
    error[E0210] 32..33: expected a line break, found `4`
    error[E0210] 38..39: expected an expression, found `)`
    error[E0205] 47..48: type name `e` must start with an uppercase letter
      help: rename it to `E`
    error[E0210] 60..60: expected an expression, found the end of the file
    "
    );
}

#[test]
fn one_mistake_gives_one_error() {
    assert_snapshot!(errors("a = f(1 2 3 4)\nb = [1, = = =, 2]\nc = { x y z }\n"), @"
    error[E0210] 8..9: expected `,` or `)`, found `2`
    error[E0210] 23..24: expected an expression, found `=`
    error[E0210] 41..42: expected `=`, found `y`
    ");
}

#[test]
fn stray_commas_are_reported_once_per_run() {
    assert_snapshot!(errors("a = [,]\nb = [1,,2]\nc = { , }\nd = [1,\n,\n2]\ne = [1,,,2]\n"), @"
    error[E0210] 5..6: expected an expression, found `,`
    error[E0210] 15..16: expected an expression, found `,`
    error[E0210] 25..26: expected a map entry, found `,`
    error[E0210] 37..38: expected an expression, found `,`
    error[E0210] 49..50: expected an expression, found `,`
    ");
    assert_snapshot!(errors(",,,\na = 1,\nb = 2,,\n"), @"
    error[E0210] 0..1: expected a declaration or a statement, found `,`
    error[E0210] 17..18: expected a declaration or a statement, found `,`
    ");
    assert!(
        parse("a = [1,\n  2,\n]\nb = { x = 1, y = 2, }\nc = 1,\n")
            .diagnostics
            .is_empty()
    );
}

#[test]
fn statement_needs_equals_or_brace_after_identifier() {
    assert_snapshot!(errors("system {\n  hostname \"forge\"\n  service gateway\n  \"key\" { }\n}\n"), @"
    error[E0201] 27..28: expected `{` after the block label, found a line break
      help: add a body in braces, or write `hostname = ...` to set an attribute
    error[E0201] 45..46: expected `{` after the block label, found a line break
      help: add a body in braces, or write `service = ...` to set an attribute
    error[E0201] 54..55: expected `=` after the attribute key, found `{`
    ");
}

#[test]
fn blocks_are_rejected_inside_literals() {
    assert_snapshot!(render("a = { service gateway { exec = [] } }\n"), @r#"
    File@0..38
      Attr@0..37
        AttrPath@0..1
          Ident "a"
        Eq "="
        Map@4..37
          LBrace "{"
          Block@6..35
            Ident "service"
            Ident "gateway"
            Body@22..35
              LBrace "{"
              Attr@24..33
                AttrPath@24..28
                  Ident "exec"
                Eq "="
                List@31..33
                  LBracket "["
                  RBracket "]"
              RBrace "}"
          RBrace "}"
      Newline "\n"
    error[E0202] 6..13: a block cannot appear inside a literal
      help: write `key = { ... }` to set a nested value
    "#);
}

#[test]
fn named_arguments_follow_positional_ones_and_never_repeat() {
    assert_snapshot!(errors("a = f(x = 1, 2, x = 3)\n"), @"
    error[E0203] 13..14: positional argument after a named argument
      help: move positional arguments before named ones
    error[E0204] 16..17: argument `x` is given more than once
      help: remove one of the arguments
    ");
}

#[test]
fn misplaced_arguments_and_rest_patterns_are_reported_once_per_list() {
    assert_snapshot!(errors("a = f(a = 1, x, y)\nb = match c {\n  f(.., a, b) => 1\n}\n"), @"
    error[E0203] 13..14: positional argument after a named argument
      help: move positional arguments before named ones
    error[E0210] 41..42: `..` must come last in a variant pattern
      help: move `..` to the end
    ");
}

#[test]
fn type_names_start_uppercase() {
    assert_snapshot!(errors("struct service {}\ntype level = a | b\nstruct _x {}\n"), @"
    error[E0205] 7..14: type name `service` must start with an uppercase letter
      help: rename it to `Service`
    error[E0205] 23..28: type name `level` must start with an uppercase letter
      help: rename it to `Level`
    error[E0205] 44..46: type name `_x` must start with an uppercase letter
    ");
}

#[test]
fn annotations_need_a_space_after_the_colon() {
    assert_snapshot!(errors("struct A {\n  port:Int\n  name:str\n}\nfn f(x:Int) -> Int = x\n"), @"
    error[E0206] 13..21: expected `name: Type`, found `port:Int` without a space
      help: write `port: Int` with a space after `:`
    error[E0206] 24..32: expected a name, found symbol word `name:str`
      help: write `name: str` with a space after `:`
    error[E0206] 40..45: expected `name: Type`, found `x:Int` without a space
      help: write `x: Int` with a space after `:`
    ");
}

#[test]
fn import_alias_must_be_a_name() {
    assert_snapshot!(errors("import \"a\" as compiler@19:static\nimport \"b\" as 1\n"), @"
    error[E0206] 14..32: expected a name, found symbol word `compiler@19:static`
    error[E0210] 47..48: expected an alias, found `1`
    ");
}

#[test]
fn comparisons_do_not_chain() {
    assert_snapshot!(errors("a = x < y < z\nb = x == y && y == z\nc = x == y != z\n"), @"
    error[E0207] 10..11: comparison operators cannot be chained
      help: join the comparisons with `&&`, as in `a < b && b < c`
    error[E0207] 46..48: comparison operators cannot be chained
      help: join the comparisons with `&&`, as in `a < b && b < c`
    ");
}

#[test]
fn declarations_and_imports_belong_at_the_top_of_a_file() {
    assert_snapshot!(
        errors("import \"a\" as a\nx = 1\nimport \"b\" as b\nsystem {\n  struct A {}\n  fn f() -> Int = 1\n}\n"),
        @"
    error[E0213] 22..28: `import` must come before every other item
      help: move the import to the start of the file
    error[E0208] 49..55: `struct` is only allowed at the top level of a file
      help: move it to the top level of the file
    error[E0208] 63..65: `fn` is only allowed at the top level of a file
      help: move it to the top level of the file
    "
    );
}

#[test]
fn file_kinds_restrict_their_items() {
    let source = "import \"a\" as a\nstruct A {}\nlet x = 1\nport = 1\nservice s {}\n";
    let parsed = parse(source);
    assert!(parsed.diagnostics.is_empty());
    let file = parsed.file();
    assert!(file.validate(FileKind::Entry).is_empty());
    assert_snapshot!(render_diagnostics(&file.validate(FileKind::Library)), @"
    error[E0209] 38..42: a library cannot contain statements
      help: move the statement into an entry file or a part file
    error[E0209] 47..54: a library cannot contain statements
      help: move the statement into an entry file or a part file
    ");
    assert_snapshot!(render_diagnostics(&file.validate(FileKind::Part)), @"
    error[E0214] 16..22: a part file cannot contain a `struct` declaration
      help: move the declaration into a library and import it
    ");
}

#[test]
fn unclosed_delimiters_are_reported_at_the_opener() {
    assert_snapshot!(errors("a = [1, 2\n"), @"
    error[E0210] 4..5: unclosed `[`
      help: add `]` to close it
    ");
    assert_snapshot!(errors("system {\n  x = f(1\n}\nb = 2\n"), @"
    error[E0210] 16..17: unclosed `(`
      help: add `)` to close it
    ");
    assert_snapshot!(errors("system {\n  x = 1\n"), @"
    error[E0210] 7..8: unclosed `{`
      help: add `}` to close it
    ");
}

#[test]
fn stray_closers_are_skipped() {
    assert_snapshot!(render("a = 1 )\n}\nb = 2\n"), @r#"
    File@0..16
      Attr@0..5
        AttrPath@0..1
          Ident "a"
        Eq "="
        Literal@4..5
          Int "1"
      Error@6..7
        RParen ")"
      Newline "\n"
      Error@8..9
        RBrace "}"
      Newline "\n"
      Attr@10..15
        AttrPath@10..11
          Ident "b"
        Eq "="
        Literal@14..15
          Int "2"
      Newline "\n"
    error[E0210] 6..7: expected a line break, found `)`
    error[E0210] 8..9: expected a declaration or a statement, found `}`
    "#);
}

#[test]
fn if_expression_needs_else() {
    assert_snapshot!(errors("a = if x { 1 }\n"), @"
    error[E0210] 14..15: expected `else`, found a line break
      help: an `if` expression needs an `else` branch
    ");
}

#[test]
fn compound_expressions_need_parentheses_as_operands() {
    assert_snapshot!(errors("a = 1 + if x { 1 } else { 2 }\nb = (if x { 1 } else { 2 }) + 1\n"), @"
    error[E0210] 8..10: an `if` expression cannot be an operand
      help: wrap it in parentheses
    ");
}

#[test]
fn import_paths_and_attribute_keys_cannot_interpolate() {
    assert_snapshot!(
        errors("import \"std/#{x}\" as y\n\"#{k}\" = 1\na.\"#{k}\" = 2\nb = { \"#{k}\" = 3 }\n"),
        @"
    error[E0212] 12..16: an import path cannot contain interpolation
      help: write the path as a plain string
    error[E0212] 24..28: an attribute key cannot contain interpolation
      help: write the key as a plain string
    error[E0212] 37..41: an attribute key cannot contain interpolation
      help: write the key as a plain string
    "
    );
}

#[test]
fn lexical_errors_do_not_cascade() {
    assert_snapshot!(errors("a = [lib @19, 2]\nb = \"open\nc = \"#{x\nd = 1\n"), @r#"
    error[E0105] 9..10: `@` is only allowed inside a symbol word
      help: write the name and version without spaces, like `name@1.2`
    error[E0108] 21..22: unterminated string
      help: close the string with `"` before the end of the line
    error[E0108] 31..32: unterminated string
      help: close the string with `"` before the end of the line
    "#);
}

#[test]
fn cut_off_interpolation_ends_at_its_last_token() {
    assert_valid_tree("a = \"x#{y");
    assert_snapshot!(render("a = \"x#{y"), @r##"
    File@0..9
      Attr@0..9
        AttrPath@0..1
          Ident "a"
        Eq "="
        Str@4..9
          StrStart "\""
          StrText "x"
          Interp@6..9
            InterpStart "#{"
            Name@8..9
              Ident "y"
    error[E0108] 4..5: unterminated string
      help: close the string with `"` before the end of the line
    "##);
    let source = "a = \"x#{y\nb = 2\n";
    assert_valid_tree(source);
    assert_snapshot!(render(source), @r##"
    File@0..16
      Attr@0..9
        AttrPath@0..1
          Ident "a"
        Eq "="
        Str@4..9
          StrStart "\""
          StrText "x"
          Interp@6..9
            InterpStart "#{"
            Name@8..9
              Ident "y"
      Newline "\n"
      Attr@10..15
        AttrPath@10..11
          Ident "b"
        Eq "="
        Literal@14..15
          Int "2"
      Newline "\n"
    error[E0108] 4..5: unterminated string
      help: close the string with `"` before the end of the line
    "##);
}

#[test]
fn empty_and_overfull_interpolations_are_reported() {
    assert_snapshot!(errors("a = \"#{}\"\nb = \"#{x y}\"\n"), @"
    error[E0210] 7..8: expected an expression, found `}`
    error[E0210] 19..20: expected `}`, found `y`
    ");
}

#[test]
fn match_arms_and_variant_patterns_recover() {
    assert_snapshot!(errors("a = match x {\n  1.5 => 1\n  f(.., a) => 2\n  y 3\n  z => 4\n}\n"), @"
    error[E0210] 16..19: expected a match arm, found `1.5`
    error[E0210] 33..34: `..` must come last in a variant pattern
      help: move `..` to the end
    error[E0210] 45..46: expected `=>`, found `3`
    ");
}

#[test]
fn deep_nesting_is_reported_once_without_overflow() {
    for (open, close) in [
        ("[", "]"),
        ("(", ")"),
        ("{a=", "}"),
        ("\"#{", "}\""),
        ("-", ""),
    ] {
        let source = format!("a = {}1{}\n", open.repeat(100_000), close.repeat(100_000));
        let parsed = parse(&source);
        let deep: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|d| d.code == "E0211")
            .collect();
        assert_eq!(deep.len(), 1, "{open}");
        assert!(assert_valid_tree(&source) <= 5 * MAX_DEPTH, "{open}");
    }
}

#[test]
fn deep_nesting_of_any_construct_stays_within_the_stack() {
    for (prefix, open) in [
        ("a = ", "f(x = match y { _ => "),
        ("a = ", "if a { "),
        ("a = ", "fn(x: Int) -> Int = let y = "),
        ("a = ", "part { b = "),
        ("a = ", "{for x in xs -> \"#{"),
        ("a = match x { ", "f(a = "),
        ("struct A { a: ", "Map<Str, Fn("),
        ("struct A { a: ", "Int where p(x = Int "),
    ] {
        let source = format!("{prefix}{}", open.repeat(20_000));
        assert!(!codes(&source).is_empty(), "{open}");
        assert!(assert_valid_tree(&source) <= 5 * MAX_DEPTH, "{open}");
    }
}

#[test]
fn long_operator_chains_are_bounded() {
    let source = format!("a = 1{}\n", " + 1".repeat(100_000));
    assert_eq!(codes(&source), ["E0211"]);
    assert!(assert_valid_tree(&source) <= 5 * MAX_DEPTH);
}

#[test]
fn deep_statement_nesting_is_bounded() {
    let source = format!("{}{}", "a {\n".repeat(50_000), "}\n".repeat(50_000));
    assert_eq!(codes(&source), ["E0211"]);
    assert!(assert_valid_tree(&source) <= 5 * MAX_DEPTH);
}

#[test]
fn nesting_below_the_limit_is_accepted() {
    let depth = MAX_DEPTH / 2;
    let source = format!("a = {}1{}\n", "[".repeat(depth), "]".repeat(depth));
    assert!(parse(&source).diagnostics.is_empty());
}

const FRAGMENTS: &[&str] = &[
    " ", "\n", "#c\n", "\"", "\"\"\"\n", "r\"", "#{", "}", "{", "[", "]", "(", ")", ",", "=", ".",
    "?.", "??", ":", "->", "=>", "..", "..=", "|", "+", "-", "*", "<", ">", "==", "&&", "!", "a",
    "B", "_", "x@1", "1", "1.5", "./p", "let", "fn", "struct", "type", "match", "if", "else",
    "for", "in", "import", "include", "as", "part", "self", "true", "none", "where", "block", "@",
    "\\", "\u{a0}",
];

proptest! {
    #[test]
    fn arbitrary_text_parses_into_a_lossless_tree(source in any::<String>()) {
        assert_valid_tree(&source);
    }

    #[test]
    fn stav_like_text_parses_into_a_lossless_tree(
        parts in prop::collection::vec(prop::sample::select(FRAGMENTS), 0..96),
    ) {
        assert_valid_tree(&parts.concat());
    }
}

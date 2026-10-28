use std::fmt::Write;

use insta::assert_snapshot;
use proptest::prelude::*;

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
/// tokens reproduce the source, node spans match their tokens, nesting is
/// bounded and diagnostics point into the source. Returns the depth.
fn assert_valid_tree(source: &str) -> usize {
    let parsed = parse(source);
    let root = parsed.tree.root();
    assert_eq!(root.kind(), crate::NodeKind::File);
    assert_eq!((root.span().start, root.span().end), (0, source.len()));

    let mut text = String::new();
    let mut max_depth = 0;
    let mut stack = vec![(root, 0usize)];
    while let Some((node, depth)) = stack.pop() {
        max_depth = max_depth.max(depth);
        let mut first = None;
        let mut last = None;
        for element in node.children_with_tokens() {
            let span = element.span();
            first.get_or_insert(span.start);
            last = Some(span.end);
        }
        if let (Some(start), Some(end)) = (first, last) {
            if node.parent().is_some() {
                assert_eq!((node.span().start, node.span().end), (start, end));
            }
        } else {
            assert_eq!(node.span().start, node.span().end);
        }
        let children: Vec<_> = node.children_with_tokens().collect();
        for element in children.into_iter().rev() {
            if let SyntaxElement::Node(child) = element {
                assert_eq!(child.parent().map(|p| p.span()), Some(node.span()));
                stack.push((child, depth + 1));
            }
        }
    }
    collect_text(root, &mut text);
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

/// Concatenates the tokens of the tree in order, without recursion.
fn collect_text(root: SyntaxNode<'_>, out: &mut String) {
    let mut stack = vec![SyntaxElement::Node(root)];
    while let Some(element) = stack.pop() {
        match element {
            SyntaxElement::Token(token) => {
                assert_eq!(token.parent().kind(), token.parent().kind());
                out.push_str(token.text());
            }
            SyntaxElement::Node(node) => {
                let children: Vec<_> = node.children_with_tokens().collect();
                stack.extend(children.into_iter().rev());
            }
        }
    }
}

#[test]
fn attributes_and_blocks_form_a_body() {
    assert_snapshot!(render(r##"port = 8080
a.b."c d" = 1
service sshd { exec = ["/usr/bin/sshd"], user = "root" }
fs "/" {}
server "#{name}" {
  boot { kernel = linux-lts }
}
"##), @r##"
    File@0..144
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
      Block@26..82
        Ident "service"
        Ident "sshd"
        Body@39..82
          LBrace "{"
          Attr@41..65
            AttrPath@41..45
              Ident "exec"
            Eq "="
            List@48..65
              LBracket "["
              Str@49..64
                StrStart "\""
                StrText "/usr/bin/sshd"
                StrEnd "\""
              RBracket "]"
          Comma ","
          Attr@67..80
            AttrPath@67..71
              Ident "user"
            Eq "="
            Str@74..80
              StrStart "\""
              StrText "root"
              StrEnd "\""
          RBrace "}"
      Newline "\n"
      Block@83..92
        Ident "fs"
        Str@86..89
          StrStart "\""
          StrText "/"
          StrEnd "\""
        Body@90..92
          LBrace "{"
          RBrace "}"
      Newline "\n"
      Block@93..143
        Ident "server"
        Str@100..109
          StrStart "\""
          Interp@101..108
            InterpStart "#{"
            Name@103..107
              Ident "name"
            InterpEnd "}"
          StrEnd "\""
        Body@110..143
          LBrace "{"
          Newline "\n"
          Block@114..141
            Ident "boot"
            Body@119..141
              LBrace "{"
              Attr@121..139
                AttrPath@121..127
                  Ident "kernel"
                Eq "="
                Name@130..139
                  Ident "linux-lts"
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
fn sshd(port: Int = 22) -> Part<System> = part {
  packages = [openssh]
}
let hosts: List<Str> = ["a"]
"##), @r###"
    File@0..401
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
      FnDecl@298..371
        FnKw "fn"
        Ident "sshd"
        ParamList@305..321
          LParen "("
          Param@306..320
            Ident "port"
            Colon ":"
            NamedType@312..315
              TypePath@312..315
                TypeIdent "Int"
            Eq "="
            Literal@318..320
              Int "22"
          RParen ")"
        Arrow "->"
        NamedType@325..337
          TypePath@325..329
            TypeIdent "Part"
          TypeArgList@329..337
            Lt "<"
            NamedType@330..336
              TypePath@330..336
                TypeIdent "System"
            Gt ">"
        Eq "="
        PartExpr@340..371
          PartKw "part"
          Body@345..371
            LBrace "{"
            Newline "\n"
            Attr@349..369
              AttrPath@349..357
                Ident "packages"
              Eq "="
              List@360..369
                LBracket "["
                Name@361..368
                  Ident "openssh"
                RBracket "]"
            Newline "\n"
            RBrace "}"
      Newline "\n"
      Let@372..400
        LetKw "let"
        Ident "hosts"
        Colon ":"
        NamedType@383..392
          TypePath@383..387
            TypeIdent "List"
          TypeArgList@387..392
            Lt "<"
            NamedType@388..391
              TypePath@388..391
                TypeIdent "Str"
            Gt ">"
        Eq "="
        List@395..400
          LBracket "["
          Str@396..399
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
  include desktop.wayland(compositor = niri)
  if tls {
    port = 443
  } else if plain { port = 80 }
  else { port = 8080 }
  for k, v in env { vars = [k, v] }
}
"##), @r#"
    File@0..216
      Block@0..215
        Ident "system"
        Body@7..215
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
          Include@54..96
            IncludeKw "include"
            Call@62..96
              FieldExpr@62..77
                Name@62..69
                  Ident "desktop"
                Dot "."
                Ident "wayland"
              ArgList@77..96
                LParen "("
                Arg@78..95
                  Ident "compositor"
                  Eq "="
                  Name@91..95
                    Ident "niri"
                RParen ")"
          Newline "\n"
          IfStmt@99..177
            IfKw "if"
            Name@102..105
              Ident "tls"
            Body@106..126
              LBrace "{"
              Newline "\n"
              Attr@112..122
                AttrPath@112..116
                  Ident "port"
                Eq "="
                Literal@119..122
                  Int "443"
              Newline "\n"
              RBrace "}"
            ElseKw "else"
            IfStmt@132..177
              IfKw "if"
              Name@135..140
                Ident "plain"
              Body@141..154
                LBrace "{"
                Attr@143..152
                  AttrPath@143..147
                    Ident "port"
                  Eq "="
                  Literal@150..152
                    Int "80"
                RBrace "}"
              Newline "\n"
              ElseKw "else"
              Body@162..177
                LBrace "{"
                Attr@164..175
                  AttrPath@164..168
                    Ident "port"
                  Eq "="
                  Literal@171..175
                    Int "8080"
                RBrace "}"
          Newline "\n"
          ForStmt@180..213
            ForKw "for"
            Ident "k"
            Comma ","
            Ident "v"
            InKw "in"
            Name@192..195
              Ident "env"
            Body@196..213
              LBrace "{"
              Attr@198..211
                AttrPath@198..202
                  Ident "vars"
                Eq "="
                List@205..211
                  LBracket "["
                  Name@206..207
                    Ident "k"
                  Comma ","
                  Name@209..210
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
        render("a = [1, 0x1f, 2.5, ./fix.patch, true, false, none, self.x, llvm@19.1:static, MIT, Level.info, xdg-utils]\n"),
        @r#"
    File@0..105
      Attr@0..104
        AttrPath@0..1
          Ident "a"
        Eq "="
        List@4..104
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
          Symbol@59..75
            SymWord "llvm@19.1:static"
          Comma ","
          Name@77..80
            TypeIdent "MIT"
          Comma ","
          FieldExpr@82..92
            Name@82..87
              TypeIdent "Level"
            Dot "."
            Ident "info"
          Comma ","
          Name@94..103
            Ident "xdg-utils"
          RBracket "]"
      Newline "\n"
    "#
    );
}

#[test]
fn collections_and_struct_literals() {
    assert_snapshot!(render(r##"a = []
b = {}
c = { port = 80, "vm.swappiness" = 10, tls = { enabled = true } }
d = Model { id = "x", provider = local }
e = svc.Ready { fd = 3 }
f = [for x in xs if x > 0 -> x * 2]
g = {for k, v in m -> "#{k}" = v}
h = [
  1,
  2
]
"##), @r##"
    File@0..233
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
      Attr@14..79
        AttrPath@14..15
          Ident "c"
        Eq "="
        Map@18..79
          LBrace "{"
          MapEntry@20..29
            Ident "port"
            Eq "="
            Literal@27..29
              Int "80"
          Comma ","
          MapEntry@31..51
            Str@31..46
              StrStart "\""
              StrText "vm.swappiness"
              StrEnd "\""
            Eq "="
            Literal@49..51
              Int "10"
          Comma ","
          MapEntry@53..77
            Ident "tls"
            Eq "="
            Map@59..77
              LBrace "{"
              MapEntry@61..75
                Ident "enabled"
                Eq "="
                Literal@71..75
                  TrueKw "true"
              RBrace "}"
          RBrace "}"
      Newline "\n"
      Attr@80..120
        AttrPath@80..81
          Ident "d"
        Eq "="
        StructLit@84..120
          TypePath@84..89
            TypeIdent "Model"
          Map@90..120
            LBrace "{"
            MapEntry@92..100
              Ident "id"
              Eq "="
              Str@97..100
                StrStart "\""
                StrText "x"
                StrEnd "\""
            Comma ","
            MapEntry@102..118
              Ident "provider"
              Eq "="
              Name@113..118
                Ident "local"
            RBrace "}"
      Newline "\n"
      Attr@121..145
        AttrPath@121..122
          Ident "e"
        Eq "="
        StructLit@125..145
          TypePath@125..134
            Ident "svc"
            Dot "."
            TypeIdent "Ready"
          Map@135..145
            LBrace "{"
            MapEntry@137..143
              Ident "fd"
              Eq "="
              Literal@142..143
                Int "3"
            RBrace "}"
      Newline "\n"
      Attr@146..181
        AttrPath@146..147
          Ident "f"
        Eq "="
        ListComp@150..181
          LBracket "["
          ForKw "for"
          Ident "x"
          InKw "in"
          Name@160..162
            Ident "xs"
          IfKw "if"
          BinaryExpr@166..171
            Name@166..167
              Ident "x"
            Gt ">"
            Literal@170..171
              Int "0"
          Arrow "->"
          BinaryExpr@175..180
            Name@175..176
              Ident "x"
            Star "*"
            Literal@179..180
              Int "2"
          RBracket "]"
      Newline "\n"
      Attr@182..215
        AttrPath@182..183
          Ident "g"
        Eq "="
        MapComp@186..215
          LBrace "{"
          ForKw "for"
          Ident "k"
          Comma ","
          Ident "v"
          InKw "in"
          Name@199..200
            Ident "m"
          Arrow "->"
          Str@204..210
            StrStart "\""
            Interp@205..209
              InterpStart "#{"
              Name@207..208
                Ident "k"
              InterpEnd "}"
            StrEnd "\""
          Eq "="
          Name@213..214
            Ident "v"
          RBrace "}"
      Newline "\n"
      Attr@216..232
        AttrPath@216..217
          Ident "h"
        Eq "="
        List@220..232
          LBracket "["
          Newline "\n"
          Literal@224..225
            Int "1"
          Comma ","
          Newline "\n"
          Literal@229..230
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
fn statement_needs_equals_or_brace_after_identifier() {
    assert_snapshot!(errors("system {\n  hostname \"forge\"\n  service sshd\n  \"key\" { }\n}\n"), @"
    error[E0201] 27..28: expected `{` after the block label, found a line break
      help: add a body in braces, or write `hostname = ...` to set an attribute
    error[E0201] 42..43: expected `{` after the block label, found a line break
      help: add a body in braces, or write `service = ...` to set an attribute
    error[E0201] 51..52: expected `=` after the attribute key, found `{`
    ");
}

#[test]
fn blocks_are_rejected_inside_literals() {
    assert_snapshot!(render("a = { service sshd { exec = [] } }\n"), @r#"
    File@0..35
      Attr@0..34
        AttrPath@0..1
          Ident "a"
        Eq "="
        Map@4..34
          LBrace "{"
          Block@6..32
            Ident "service"
            Ident "sshd"
            Body@19..32
              LBrace "{"
              Attr@21..30
                AttrPath@21..25
                  Ident "exec"
                Eq "="
                List@28..30
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
    error[E0208] 22..28: `import` must come before every other item
      help: move the import to the start of the file
    error[E0208] 49..55: `struct` is only allowed at the top level of a file
      help: move it to the top level of the file
    error[E0208] 63..65: `fn` is only allowed at the top level of a file
      help: move it to the top level of the file
    "
    );
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

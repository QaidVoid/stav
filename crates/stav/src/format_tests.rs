use proptest::prelude::*;

use crate::{MAX_DEPTH, SyntaxElement, TokenKind, format, parse};

/// Formats `source`, which must be free of errors, and checks that the
/// result keeps the meaning and comments of the source and is stable.
fn fmt(source: &str) -> String {
    let formatted = match format(source) {
        Ok(text) => text,
        Err(diagnostics) => panic!("{source:?} has errors: {diagnostics:?}"),
    };
    assert_preserved(source, &formatted);
    formatted
}

fn assert_preserved(source: &str, formatted: &str) {
    let reparsed = parse(formatted);
    assert!(
        reparsed.diagnostics.is_empty(),
        "the output of {source:?} has errors: {:?}\n{formatted}",
        reparsed.diagnostics
    );
    assert_eq!(
        meaning(source),
        meaning(formatted),
        "formatting changed the meaning of {source:?}\n{formatted}"
    );
    let again = format(formatted).expect("the output has no errors");
    assert_eq!(
        again, formatted,
        "formatting is not idempotent for {source:?}"
    );
}

/// Returns the nodes and tokens of `source` in order, and its comments in
/// order. Whitespace and line feeds are left out, and so are the separators
/// the formatter may add or remove: commas and the `|` before the first
/// variant of a sum type.
fn meaning(source: &str) -> (Vec<String>, Vec<String>) {
    let parsed = parse(source);
    let mut tree = Vec::new();
    let mut comments = Vec::new();
    let mut stack = vec![SyntaxElement::Node(parsed.tree.root())];
    while let Some(element) = stack.pop() {
        match element {
            SyntaxElement::Node(node) => {
                tree.push(format!("{:?}", node.kind()));
                let children: Vec<_> = node.children_with_tokens().collect();
                stack.extend(children.into_iter().rev());
            }
            SyntaxElement::Token(token) => match token.kind() {
                TokenKind::Comment | TokenKind::DocComment => {
                    comments.push(token.text().trim_end().to_owned());
                }
                TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comma => {}
                TokenKind::Pipe if tree.last().is_some_and(|last| last == "=") => {}
                _ => tree.push(token.text().to_owned()),
            },
        }
    }
    (tree, comments)
}

#[test]
fn spaces_operators_and_punctuation() {
    let source = "x=a+b*-c\ny = f( a,b = [1 ,2] ) . g ?. h [ 0 ]\nz=!(a&&b)||c??d\n";
    assert_eq!(
        fmt(source),
        "x = a + b * -c\ny = f(a, b = [1, 2]).g?.h[0]\nz = !(a && b) || c ?? d\n"
    );
}

#[test]
fn annotations_have_one_space_after_the_colon() {
    let source = "let port :Int = 1\nfn f(a :Int,b: Str) -> Int = a\nstruct A { x :List< Str > }\n";
    assert_eq!(
        fmt(source),
        "let port: Int = 1\nfn f(a: Int, b: Str) -> Int = a\nstruct A { x: List<Str> }\n"
    );
}

#[test]
fn symbol_words_and_ranges_stay_tight() {
    let source = "packages = [ripgrep:static, llvm@19.1:static]\nstruct A { n: Int where -5 .. = 5 && 0 ..\n}\n";
    let source = source.replace(".. =", "..=");
    assert_eq!(
        fmt(&source),
        "packages = [ripgrep:static, llvm@19.1:static]\nstruct A {\n  n: Int where -5..=5 && 0..\n}\n"
    );
}

#[test]
fn field_access_on_paths_and_versioned_symbols_keeps_a_space() {
    let source = "a = ./a .b\nb = x@1 .y\nc = (./a).b\n";
    assert_eq!(fmt(source), source);
}

#[test]
fn indents_nested_bodies_by_two_spaces() {
    let source = "system {\n\tusers {\n user alice {\n uid = 1000\n }\n}\n}\n";
    assert_eq!(
        fmt(source),
        "system {\n  users {\n    user alice {\n      uid = 1000\n    }\n  }\n}\n"
    );
}

#[test]
fn aligns_equals_in_runs_of_attributes() {
    let source = "system {\n  hostname = \"forge\"\n  init = runit\n  alternatives.sh = dash\n}\n";
    assert_eq!(
        fmt(source),
        "system {\n  hostname        = \"forge\"\n  init            = runit\n  alternatives.sh = dash\n}\n"
    );
}

#[test]
fn blank_lines_comments_and_other_statements_end_alignment_runs() {
    let source = "a = 1\nlong_name = 2\n\nb = 3\nlonger_name = 4\n# note\nc = 5\nlonger = 6\nlet d = 7\ne = 8\n";
    assert_eq!(
        fmt(source),
        "a         = 1\nlong_name = 2\n\nb           = 3\nlonger_name = 4\n# note\nc      = 5\nlonger = 6\nlet d = 7\ne = 8\n"
    );
}

#[test]
fn runs_with_a_key_longer_than_24_characters_do_not_align() {
    let source = "a = 1\nan_attribute_name_too_long_ = 2\n";
    assert_eq!(fmt(source), source);
    let source = "a = 1\nexactly_twenty_four_char = 2\n";
    assert_eq!(
        fmt(source),
        "a                        = 1\nexactly_twenty_four_char = 2\n"
    );
}

#[test]
fn aligns_field_types_across_doc_comments_but_not_comments() {
    let source = "struct User {\n  ## The id.\n  uid: Int\n  ## The name.\n  name: Str = \"\"\n  # plain\n  home: Str\n  description: Str\n}\n";
    assert_eq!(
        fmt(source),
        "struct User {\n  ## The id.\n  uid:  Int\n  ## The name.\n  name: Str = \"\"\n  # plain\n  home:        Str\n  description: Str\n}\n"
    );
}

#[test]
fn aligns_match_arms_and_let_bindings() {
    let source = "fn f(c: C) -> Int = match c { sway => 1, river => 2 }\nfn g() -> Int =\n  let a = 1, long = 2 in a + long + some_function_with_a_long_name(a, long, another_argument_that_is_long)\n";
    assert_eq!(
        fmt(source),
        "fn f(c: C) -> Int = match c {\n  sway  => 1\n  river => 2\n}\nfn g() -> Int =\n  let a    = 1,\n      long = 2\n  in a + long + some_function_with_a_long_name(a, long, another_argument_that_is_long)\n"
    );
}

#[test]
fn keeps_bodies_on_one_line_only_when_written_and_fitting_there() {
    assert_eq!(
        fmt("unit api { exec = [\"/usr/bin/api\"],after = [db] }\n"),
        "unit api { exec = [\"/usr/bin/api\"], after = [db] }\n"
    );
    assert_eq!(
        fmt("unit api {\n  exec = [\"/usr/bin/api\"] }\n"),
        "unit api {\n  exec = [\"/usr/bin/api\"]\n}\n"
    );
    let long = format!(
        "unit api {{ exec = [\"{}\"], after = [db] }}\n",
        "x".repeat(80)
    );
    assert_eq!(
        fmt(&long),
        format!(
            "unit api {{\n  exec  = [\"{}\"]\n  after = [db]\n}}\n",
            "x".repeat(80)
        )
    );
}

#[test]
fn commas_between_statements_become_line_breaks() {
    let source = "system {\n  a = 1, b = 2\n  c { }, d = 3\n}\n";
    assert_eq!(
        fmt(source),
        "system {\n  a = 1\n  b = 2\n  c { }\n  d = 3\n}\n"
    );
}

#[test]
fn empty_bodies_and_literals() {
    assert_eq!(
        fmt("build make {}\nx = { }\ny = [ ]\nz = f( )\nstruct A {\n}\n"),
        "build make { }\nx = {}\ny = []\nz = f()\nstruct A {}\n"
    );
}

#[test]
fn lists_maps_and_arguments_break_one_element_per_line_with_trailing_commas() {
    let items: Vec<String> = (0..12).map(|i| format!("package-{i}")).collect();
    let source = format!(
        "packages = [{}]\n\nx = {{ a = 1, b = [2] }}\ninclude f(user = alice, list = [{}])\n",
        items.join(", "),
        items[..8].join(", ")
    );
    let broken: String = items.iter().map(|item| format!("  {item},\n")).collect();
    assert_eq!(
        fmt(&source),
        format!(
            "packages = [\n{broken}]\n\nx = {{ a = 1, b = [2] }}\ninclude f(\n  user = alice,\n  list = [{}],\n)\n",
            items[..8].join(", ")
        )
    );
}

#[test]
fn short_multi_line_lists_and_maps_join_without_trailing_commas() {
    assert_eq!(
        fmt("x = [\n  1,\n  2,\n]\ny = {\n  a = 1,\n}\n"),
        "x = [1, 2]\ny = { a = 1 }\n"
    );
}

#[test]
fn broken_maps_align_their_entries() {
    let source = format!("x = {{ a = \"{}\", long = 2 }}\n", "v".repeat(90));
    assert_eq!(
        fmt(&source),
        format!(
            "x = {{\n  a    = \"{}\",\n  long = 2,\n}}\n",
            "v".repeat(90)
        )
    );
}

#[test]
fn sum_types_stay_on_one_line_only_when_written_and_fitting_there() {
    assert_eq!(fmt("type L = | a|b | c | d\n"), "type L = a | b | c | d\n");
    assert_eq!(
        fmt("type B =\n  | cargo(features: List<Str> = []) | make\n"),
        "type B =\n  | cargo(features: List<Str> = [])\n  | make\n"
    );
    let long = "type R = immediate | notify(fd: Int where 0..=1023) | check(cmd: List<Str>, interval: Int = 1, timeout: Int = 60)\n";
    assert_eq!(
        fmt(long),
        "type R =\n  | immediate\n  | notify(fd: Int where 0..=1023)\n  | check(cmd: List<Str>, interval: Int = 1, timeout: Int = 60)\n"
    );
}

#[test]
fn match_arms_always_go_one_per_line() {
    assert_eq!(
        fmt("x = match y { a | b if c => 1, _ => 2 }\n"),
        "x = match y {\n  a | b if c => 1\n  _          => 2\n}\n"
    );
}

#[test]
fn function_bodies_follow_the_equals_sign_unless_moved() {
    assert_eq!(
        fmt("fn f(x: Int) -> Int = x + 1\n"),
        "fn f(x: Int) -> Int = x + 1\n"
    );
    assert_eq!(
        fmt("fn f(x: Int) -> Int =\n x + 1\n"),
        "fn f(x: Int) -> Int =\n  x + 1\n"
    );
    assert_eq!(
        fmt("fn f(x: Int) -> Part<A> =\n  part { a = x }\n"),
        "fn f(x: Int) -> Part<A> = part { a = x }\n"
    );
    let long = format!("fn f(x: Int) -> Str = \"{}\"\n", "s".repeat(80));
    assert_eq!(
        fmt(&long),
        format!("fn f(x: Int) -> Str =\n  \"{}\"\n", "s".repeat(80))
    );
}

#[test]
fn long_parameter_lists_break_with_trailing_commas() {
    let source = "fn wayland(user: Ref<User>, compositor: Compositor = niri, terminal: Pkg = foot, fonts: Set<Pkg> = []) -> Part<System> = part {\n  packages = fonts\n}\n";
    assert_eq!(
        fmt(source),
        "fn wayland(\n  user: Ref<User>,\n  compositor: Compositor = niri,\n  terminal: Pkg = foot,\n  fonts: Set<Pkg> = [],\n) -> Part<System> = part {\n  packages = fonts\n}\n"
    );
}

#[test]
fn if_expressions_break_into_blocks_when_too_long() {
    assert_eq!(
        fmt("x = if a {1} else if b {2} else {3}\n"),
        "x = if a { 1 } else if b { 2 } else { 3 }\n"
    );
    let value = "v".repeat(40);
    let source = format!("x = if condition {{ \"{value}\" }} else {{ \"{value}\" }}\n");
    assert_eq!(
        fmt(&source),
        format!("x = if condition {{\n  \"{value}\"\n}} else {{\n  \"{value}\"\n}}\n")
    );
}

#[test]
fn trailing_and_own_line_comments_keep_their_place() {
    let source = "# head\nsystem { # opens\n  a = 1 # one\n\n  # before b\n  b = [1, # first\n    2]\n  # last\n} # closes\n";
    assert_eq!(
        fmt(source),
        "# head\nsystem { # opens\n  a = 1 # one\n\n  # before b\n  b = [\n    1, # first\n    2,\n  ]\n  # last\n} # closes\n"
    );
}

#[test]
fn comments_inside_expressions_keep_their_line_break() {
    assert_eq!(fmt("x = a + # why\n  b\n"), "x = a + # why\n  b\n");
    assert_eq!(
        fmt("fn f() -> Int = # why\n  1\n"),
        "fn f() -> Int = # why\n  1\n"
    );
    assert_eq!(
        fmt("x = [a + # one\n  b + # two\n  # three\n  c, d]\n"),
        "x = [\n  a + # one\n    b + # two\n    # three\n    c,\n  d,\n]\n"
    );
    assert_eq!(
        fmt("let t: Map<Str, # a\n Int, # b\n # c\n Str> = x\n"),
        "let t: Map<Str, # a\n  Int, # b\n  # c\n  Str> = x\n"
    );
}

#[test]
fn comments_before_a_brace_or_body_stay_before_it() {
    assert_eq!(
        fmt("a = if a { 1 } else # c\n{ 2 }\n"),
        "a = if a {\n  1\n} else # c\n{\n  2\n}\n"
    );
    assert_eq!(
        fmt("a = f(if # c\n a { 1 } else { 2 })\n"),
        "a = f(\n  if # c\n  a {\n    1\n  } else {\n    2\n  },\n)\n"
    );
    assert_eq!(
        fmt("a = f(if a # c\n{ 1 } # d\n else { 2 })\n"),
        "a = f(\n  if a # c\n  {\n    1\n  } # d\n  else {\n    2\n  },\n)\n"
    );
    assert_eq!(
        fmt("a = f(match x # c\n{ _ => 1 })\n"),
        "a = f(\n  match x # c\n    {\n      _ => 1\n    },\n)\n"
    );
    assert_eq!(
        fmt("a = f(let # c\n x = 1, y = 2 in x)\n"),
        "a = f(\n  let # c\n      x = 1,\n      y = 2\n  in x,\n)\n"
    );
}

#[test]
fn comments_on_their_own_line_stay_before_a_function_body() {
    assert_eq!(
        fmt("fn f() -> Int =\n  # why\n  1\n"),
        "fn f() -> Int =\n  # why\n  1\n"
    );
    assert_eq!(
        fmt("fn f() -> Int =\n  # why\n  match x { _ => 1 }\n"),
        "fn f() -> Int =\n  # why\n  match x {\n    _ => 1\n  }\n"
    );
    assert_eq!(
        fmt("fn f() -> Part<A> = # why\n  part { a = 1 }\n"),
        "fn f() -> Part<A> = # why\n  part { a = 1 }\n"
    );
}

#[test]
fn doc_comments_stay_before_what_they_document() {
    let source = "## A level.\ntype Level =\n  ## Verbose.\n  | debug\n  ## Quiet.\n  | quiet\n";
    assert_eq!(fmt(source), source);
}

#[test]
fn blank_lines_collapse_to_one_and_vanish_at_the_edges() {
    let source = "\n\n\na = 1\n\n\n\nb {\n\n  c = 2\n\n\n  d = 3\n\n}\n\n\n";
    assert_eq!(fmt(source), "a = 1\n\nb {\n  c = 2\n\n  d = 3\n}\n");
}

#[test]
fn removes_trailing_whitespace_and_ends_with_one_line_feed() {
    assert_eq!(fmt("a = 1   # note   \t\nb = 2"), "a = 1 # note\nb = 2\n");
    assert_eq!(fmt(""), "");
    assert_eq!(fmt("\n\n  \n"), "");
    assert_eq!(fmt("# only\n\n"), "# only\n");
}

#[test]
fn strings_are_kept_byte_for_byte() {
    let source = "system {\n      text = \"\"\"\n    two  \n    \t tab #{ a+b }\n    \"\"\"\n  raw = r\"\"\"\n x\\n\n \"\"\"\n  s = \"a  #{ f( x ) }  b\"\n}\n";
    let formatted = fmt(source);
    assert_eq!(
        formatted,
        "system {\n  text = \"\"\"\n    two  \n    \t tab #{ a+b }\n    \"\"\"\n  raw  = r\"\"\"\n x\\n\n \"\"\"\n  s    = \"a  #{ f( x ) }  b\"\n}\n"
    );
}

#[test]
fn multi_line_strings_as_arguments_stay_hugged() {
    let source = "file x {\n  text = default(\"\"\"\n    export A=1\n    \"\"\")\n}\n";
    assert_eq!(fmt(source), source);
}

#[test]
fn source_with_errors_is_returned_as_diagnostics() {
    let diagnostics = format("a = \nb = [1,\n").expect_err("the source has errors");
    assert!(!diagnostics.is_empty());
    assert!(format("a @ b").is_err());
}

#[test]
fn deep_nesting_formats_without_overflowing_the_stack() {
    let depth = MAX_DEPTH - 8;
    let source = format!("x = {}1{}\n", "[".repeat(depth), "]".repeat(depth));
    fmt(&source);
    let source = format!("x = 1{}\n", " + 1".repeat(depth));
    fmt(&source);
    let source = format!("x = {}1{}\n", "(".repeat(depth), ")".repeat(depth));
    fmt(&source);
}

/// Returns strategies for the parts of a generated program. Every program
/// they generate is meant to parse without errors.
mod program {
    use proptest::prelude::*;
    use proptest::sample::select;

    /// Text between tokens where the grammar ignores line feeds.
    fn gap() -> impl Strategy<Value = &'static str> {
        select(&["", " ", "  ", "\n", " # note\n", "\n\n  "][..])
    }

    /// Text between elements of a list, a map or a body.
    fn sep() -> impl Strategy<Value = &'static str> {
        select(
            &[
                ", ",
                ",",
                "\n",
                "\n\n",
                ", # note\n",
                " # note\n",
                ",\n",
                "\n# own line\n",
            ][..],
        )
    }

    fn name() -> impl Strategy<Value = &'static str> {
        select(
            &[
                "a",
                "b",
                "xdg-utils",
                "port",
                "a_rather_long_name_for_alignment",
            ][..],
        )
    }

    fn type_name() -> impl Strategy<Value = &'static str> {
        select(&["Str", "Int", "Upstream", "alias.Level"][..])
    }

    fn leaf() -> impl Strategy<Value = String> {
        select(
            &[
                "1",
                "0x1f",
                "1_000",
                "2.5",
                "true",
                "none",
                "self",
                "./files/a.conf",
                "llvm@19.1:static",
                "ripgrep:static",
                "\"text\"",
                "\"x #{a + 1} y\"",
                "r\"^raw$\"",
                "\"\"\"\n  line one\n    two\n  \"\"\"",
                "name",
                "Level.info",
                "\"a string that is long enough to push lists over the line width\"",
            ][..],
        )
        .prop_map(str::to_owned)
    }

    /// Joins elements with generated separators.
    fn joined(items: Vec<(String, &'static str)>) -> String {
        let mut out = String::new();
        for (index, (item, sep)) in items.iter().enumerate() {
            out.push_str(item);
            if index + 1 < items.len() {
                out.push_str(sep);
            }
        }
        out
    }

    /// Joins elements of a parenthesized list, which takes single commas.
    fn comma_joined(items: Vec<(String, &'static str)>) -> String {
        let mut out = String::new();
        for (index, (item, gap)) in items.iter().enumerate() {
            if index > 0 {
                out.push(',');
                out.push_str(gap);
            }
            out.push_str(item);
        }
        out
    }

    pub(super) fn ty() -> impl Strategy<Value = String> {
        let leaf = type_name().prop_map(str::to_owned);
        leaf.prop_recursive(3, 12, 3, |inner| {
            prop_oneof![
                (
                    select(&["List", "Map", "Option"][..]),
                    prop::collection::vec(inner.clone(), 1..3)
                )
                    .prop_map(|(name, args)| format!("{name}<{}>", args.join(", "))),
                (prop::collection::vec(inner.clone(), 0..3), inner.clone())
                    .prop_map(|(params, ret)| format!("Fn({}) -> {ret}", params.join(", "))),
                (
                    type_name(),
                    select(&["1..=100", "-5..", "..=0.5", "nonEmpty", "matches(r\"^x$\")"][..])
                )
                    .prop_map(|(name, predicate)| format!("{name} where {predicate}")),
            ]
        })
    }

    fn pattern() -> impl Strategy<Value = String> {
        let leaf = select(&["_", "none", "true", "-1", "\"s\"", "a", "Level.info"][..])
            .prop_map(str::to_owned);
        leaf.prop_recursive(2, 8, 3, |inner| {
            prop_oneof![
                (
                    name(),
                    prop::collection::vec((name(), prop::option::of(inner.clone())), 0..3),
                    any::<bool>()
                )
                    .prop_map(|(name, fields, rest)| {
                        let mut fields: Vec<String> = fields
                            .into_iter()
                            .map(|(field, pattern)| match pattern {
                                Some(pattern) => format!("{field} = {pattern}"),
                                None => field.to_owned(),
                            })
                            .collect();
                        if rest {
                            fields.push("..".to_owned());
                        }
                        format!("{name}({})", fields.join(", "))
                    }),
                prop::collection::vec(inner, 2..4)
                    .prop_map(|alternatives| alternatives.join(" | ")),
            ]
        })
    }

    pub(super) fn expr() -> impl Strategy<Value = String> {
        leaf().prop_recursive(4, 40, 5, |inner| {
            prop_oneof![
                prop::collection::vec((inner.clone(), sep()), 0..5)
                    .prop_map(|items| format!("[{}]", joined(items))),
                prop::collection::vec((name(), inner.clone(), sep()), 0..4).prop_map(|entries| {
                    let entries = entries
                        .into_iter()
                        .map(|(key, value, sep)| (format!("{key} = {value}"), sep))
                        .collect();
                    format!("{{{}}}", joined(entries))
                }),
                (
                    type_name(),
                    prop::collection::vec((name(), inner.clone(), sep()), 0..3)
                )
                    .prop_map(|(ty, entries)| {
                        let entries = entries
                            .into_iter()
                            .map(|(key, value, sep)| (format!("{key} = {value}"), sep))
                            .collect();
                        format!("{ty} {{ {} }}", joined(entries))
                    }),
                (
                    name(),
                    prop::collection::vec((inner.clone(), gap()), 0..3),
                    prop::sample::subsequence(&["a", "port", "xdg-utils"][..], 0..3),
                    inner.clone(),
                )
                    .prop_map(|(callee, positional, named, value)| {
                        let mut args = positional;
                        args.extend(
                            named
                                .into_iter()
                                .map(|name| (format!("{name} = {value}"), "\n")),
                        );
                        format!("{callee}({})", comma_joined(args))
                    }),
                (
                    inner.clone(),
                    select(&["+", "-", "*", "++", "&&", "||", "??", "=="][..]),
                    gap(),
                    inner.clone()
                )
                    .prop_map(|(lhs, op, gap, rhs)| format!("(({lhs}) {op}{gap}({rhs}))")),
                (select(&["!", "-"][..]), inner.clone())
                    .prop_map(|(op, operand)| format!("{op}({operand})")),
                (inner.clone(), select(&[".", "?."][..]), name())
                    .prop_map(|(base, op, field)| format!("({base}){op}{field}")),
                (inner.clone(), inner.clone())
                    .prop_map(|(base, index)| format!("({base})[{index}]")),
                (inner.clone(), inner.clone(), inner.clone()).prop_map(|(c, a, b)| {
                    format!("if ({c}) {{ {a} }} else if x {{\n{b}\n}} else {{ {a} }}")
                }),
                (
                    inner.clone(),
                    prop::collection::vec((pattern(), inner.clone(), sep()), 1..4)
                )
                    .prop_map(|(scrutinee, arms)| {
                        let arms = arms
                            .into_iter()
                            .map(|(pattern, value, sep)| (format!("{pattern} => {value}"), sep))
                            .collect();
                        format!("match ({scrutinee}) {{ {} }}", joined(arms))
                    }),
                (
                    prop::collection::vec(
                        (name(), prop::option::of(ty()), inner.clone(), gap()),
                        1..3
                    ),
                    inner.clone()
                )
                    .prop_map(|(bindings, body)| {
                        let bindings = bindings
                            .into_iter()
                            .map(|(name, ty, value, gap)| match ty {
                                Some(ty) => (format!("{name}: {ty} = {value}"), gap),
                                None => (format!("{name} = {value}"), gap),
                            })
                            .collect();
                        format!("(let {}\nin {body})", comma_joined(bindings))
                    }),
                (prop::collection::vec((name(), ty()), 0..3), inner.clone()).prop_map(
                    |(params, body)| {
                        let params: Vec<String> = params
                            .into_iter()
                            .map(|(name, ty)| format!("{name}: {ty}"))
                            .collect();
                        format!("fn({}) = {body}", params.join(", "))
                    }
                ),
                (inner.clone(), inner.clone()).prop_map(|(source, value)| {
                    format!("[for k, v in {source} if k ->\n {value}]")
                }),
                (inner.clone(), inner.clone()).prop_map(|(source, value)| {
                    format!("{{for x in {source} -> \"#{{x}}\" = {value}}}")
                }),
            ]
        })
    }

    fn stmt() -> impl Strategy<Value = String> {
        let attr = (prop::collection::vec(name(), 1..3), gap(), expr())
            .prop_map(|(path, gap, value)| format!("{} ={gap}{value}", path.join(".")));
        let simple = prop_oneof![
            4 => attr,
            1 => (name(), expr()).prop_map(|(name, value)| format!("let {name} = {value}")),
            1 => expr().prop_map(|value| format!("include ({value})")),
        ];
        simple.prop_recursive(3, 24, 5, |inner| {
            let body = prop::collection::vec((inner.clone(), sep()), 0..5)
                .prop_map(|stmts| format!("{{{}}}", joined(stmts)))
                .boxed();
            prop_oneof![
                (
                    name(),
                    prop::option::of(select(&["primary", "\"/etc/x\""][..])),
                    body.clone()
                )
                    .prop_map(|(kind, label, body)| match label {
                        Some(label) => format!("{kind} {label} {body}"),
                        None => format!("{kind} {body}"),
                    }),
                (expr(), body.clone(), prop::option::of(body.clone())).prop_map(
                    |(c, then, otherwise)| {
                        match otherwise {
                            Some(otherwise) => format!("if ({c}) {then} else {otherwise}"),
                            None => format!("if ({c}) {then}"),
                        }
                    }
                ),
                (expr(), body).prop_map(|(source, body)| format!("for k, v in ({source}) {body}")),
            ]
        })
    }

    fn decl() -> impl Strategy<Value = String> {
        let field = (name(), ty(), prop::option::of(expr()), any::<bool>()).prop_map(
            |(name, ty, default, block)| {
                let mut field = format!("{name}: {ty}");
                if let Some(default) = default {
                    field.push_str(&format!(" = {default}"));
                }
                if block {
                    field.push_str(" block item");
                }
                field
            },
        );
        let param = (name(), ty())
            .prop_map(|(name, ty)| format!("{name}: {ty}"))
            .boxed();
        prop_oneof![
            prop::collection::vec(
                (
                    field,
                    select(&["\n", "\n## Doc.\n", ", ", "\n\n", " # note\n"][..])
                ),
                0..5
            )
            .prop_map(|fields| format!("## A struct.\nstruct Gen {{{}}}", joined(fields))),
            (
                prop::collection::vec((name(), prop::collection::vec(param.clone(), 0..3)), 1..5),
                any::<bool>()
            )
                .prop_map(|(variants, vertical)| {
                    let variants: Vec<String> = variants
                        .into_iter()
                        .map(|(name, params)| match params.is_empty() {
                            true => name.to_owned(),
                            false => format!("{name}({})", params.join(", ")),
                        })
                        .collect();
                    match vertical {
                        true => format!("type Gen =\n  ## Doc.\n  | {}", variants.join("\n  | ")),
                        false => format!("type Gen = {}", variants.join(" | ")),
                    }
                }),
            (prop::collection::vec(param, 0..4), ty(), gap(), expr()).prop_map(
                |(params, ret, gap, body)| {
                    format!("fn gen({}) -> {ret} ={gap}{body}", params.join(", "))
                }
            ),
        ]
    }

    pub(super) fn file() -> impl Strategy<Value = String> {
        prop::collection::vec(
            (
                prop_oneof![1 => decl(), 3 => stmt()],
                select(&["\n", "\n\n", "\n\n\n", " # end\n", "\n# own\n"][..]),
            ),
            0..6,
        )
        .prop_map(|items| items.into_iter().map(|(item, sep)| item + sep).collect())
    }
}

/// Replaces the whitespace between tokens outside strings with other
/// horizontal whitespace, chosen by `choices`.
fn respace(source: &str, choices: &[u8]) -> String {
    let lexed = crate::lex(source);
    let mut out = String::new();
    let mut depth = 0usize;
    let mut choice = choices.iter().copied().cycle();
    for token in &lexed.tokens {
        let text = token.text(source);
        match token.kind {
            TokenKind::StrStart
            | TokenKind::MStrStart
            | TokenKind::RawStrStart
            | TokenKind::RawMStrStart => depth += 1,
            TokenKind::StrEnd | TokenKind::MStrEnd => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && token.kind == TokenKind::Whitespace {
            let spaces = match choice.next().unwrap_or(0) % 4 {
                0 => " ",
                1 => "  ",
                2 => "\t",
                _ => "   ",
            };
            out.push_str(spaces);
        } else {
            out.push_str(text);
        }
    }
    out
}

const CORPUS: &[&str] = &[
    include_str!("../tests/fixtures/format/formatted/application.stav"),
    include_str!("../tests/fixtures/format/formatted/system.stav"),
    include_str!("../tests/fixtures/format/formatted/recipe.stav"),
    include_str!("../tests/fixtures/format/formatted/library.stav"),
];

const FRAGMENTS: &[&str] = &[
    " ", "\n", "#c\n", "\"s\"", "{", "}", "[", "]", "(", ")", ",", "=", ".", "?.", "??", ":", "->",
    "=>", "..", "..=", "|", "+", "-", "*", "<", ">", "==", "&&", "!", "a", "B", "_", "x@1", "1",
    "./p", "let", "fn", "struct", "type", "match", "if", "else", "for", "in", "include", "part",
    "self", "none", "where", "block",
];

proptest! {
    #[test]
    fn generated_programs_format_stably_without_changing_meaning(source in program::file()) {
        prop_assume!(parse(&source).diagnostics.is_empty());
        let formatted = format(&source).expect("the source has no errors");
        assert_preserved(&source, &formatted);
    }

    #[test]
    fn horizontal_whitespace_does_not_change_the_result(
        index in 0..CORPUS.len(),
        choices in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let source = CORPUS[index];
        let respaced = respace(source, &choices);
        let formatted = format(&respaced).expect("respacing keeps the source valid");
        assert_eq!(&formatted, source);
    }

    #[test]
    fn arbitrary_valid_text_formats_stably_without_changing_meaning(
        parts in prop::collection::vec(prop::sample::select(FRAGMENTS), 0..64),
    ) {
        let source = parts.concat();
        if let Ok(formatted) = format(&source) {
            assert_preserved(&source, &formatted);
        }
    }
}

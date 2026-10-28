use crate::ast::{
    BinaryOp, ElseBranch, Expr, Item, Key, LiteralKind, Pattern, Predicate, Stmt, StrKind, Type,
};
use crate::{SyntaxToken, parse};

fn text(token: Option<SyntaxToken<'_>>) -> &str {
    token.map_or("?", SyntaxToken::text)
}

fn key(key: Option<Key<'_>>) -> String {
    match key {
        Some(Key::Name(token)) => token.text().to_owned(),
        Some(Key::Str(string)) => format!("{:?}", string.value().unwrap_or_default()),
        None => "?".to_owned(),
    }
}

/// Prints an expression as an S-expression through the typed view only.
fn sexp(expr: Option<Expr<'_>>) -> String {
    let Some(expr) = expr else {
        return "?".to_owned();
    };
    let list = |items: Vec<String>| items.join(" ");
    match expr {
        Expr::Literal(lit) => text(lit.token()).to_owned(),
        Expr::Str(string) => match string.value() {
            Some(value) => format!("{value:?}"),
            None => "(str ...)".to_owned(),
        },
        Expr::Name(name) => text(name.token()).to_owned(),
        Expr::Symbol(symbol) => format!(
            "(sym {} {} {})",
            symbol.name(),
            symbol.version().unwrap_or("-"),
            symbol.variant().unwrap_or("-")
        ),
        Expr::SelfExpr(_) => "self".to_owned(),
        Expr::List(l) => format!("[{}]", list(l.items().map(|e| sexp(Some(e))).collect())),
        Expr::ListComp(c) => format!(
            "[for {} in {} if {} -> {}]",
            list(c.names().map(|t| t.text().to_owned()).collect()),
            sexp(c.source()),
            sexp(c.guard()),
            sexp(c.value())
        ),
        Expr::Map(m) => format!(
            "{{{}}}",
            list(
                m.entries()
                    .map(|e| format!("{}={}", key(e.key()), sexp(e.value())))
                    .collect()
            )
        ),
        Expr::MapComp(c) => format!(
            "{{for {} in {} if {} -> {}={}}}",
            list(c.names().map(|t| t.text().to_owned()).collect()),
            sexp(c.source()),
            sexp(c.guard()),
            key(c.key()),
            sexp(c.value())
        ),
        Expr::StructLit(s) => format!(
            "{}{{{}}}",
            s.type_path()
                .map(|p| list(p.segments().map(|t| t.text().to_owned()).collect()))
                .unwrap_or_default(),
            list(
                s.entries()
                    .map(|e| format!("{}={}", key(e.key()), sexp(e.value())))
                    .collect()
            )
        ),
        Expr::Paren(p) => sexp(p.expr()),
        Expr::Call(call) => {
            let args = call
                .args()
                .into_iter()
                .flat_map(|a| a.args())
                .map(|arg| match arg.name() {
                    Some(name) => format!("{}={}", name.text(), sexp(arg.value())),
                    None => sexp(arg.value()),
                });
            format!("(call {} {})", sexp(call.callee()), list(args.collect()))
        }
        Expr::Field(f) => format!(
            "({} {} {})",
            if f.is_optional() { "?." } else { "." },
            sexp(f.base()),
            text(f.field())
        ),
        Expr::Index(i) => format!("([] {} {})", sexp(i.base()), sexp(i.index())),
        Expr::Binary(b) => format!(
            "({} {} {})",
            text(b.op_token()),
            sexp(b.lhs()),
            sexp(b.rhs())
        ),
        Expr::Unary(u) => format!(
            "({} {})",
            text(u.syntax().children_with_tokens().find_map(|e| match e {
                crate::SyntaxElement::Token(t) => Some(t),
                crate::SyntaxElement::Node(_) => None,
            })),
            sexp(u.operand())
        ),
        Expr::If(i) => format!(
            "(if {} {} {})",
            sexp(i.condition()),
            sexp(i.then_branch()),
            sexp(i.else_branch())
        ),
        Expr::Match(m) => format!(
            "(match {} {})",
            sexp(m.scrutinee()),
            list(
                m.arms()
                    .map(|arm| format!(
                        "({} if {} => {})",
                        pattern(arm.pattern()),
                        sexp(arm.guard()),
                        sexp(arm.value())
                    ))
                    .collect()
            )
        ),
        Expr::Let(l) => format!(
            "(let {} {})",
            list(
                l.bindings()
                    .map(|b| format!("({} {})", text(b.name()), sexp(b.value())))
                    .collect()
            ),
            sexp(l.body())
        ),
        Expr::Lambda(l) => format!(
            "(fn ({}) {})",
            list(
                l.params()
                    .into_iter()
                    .flat_map(|p| p.params())
                    .map(|p| text(p.name()).to_owned())
                    .collect()
            ),
            sexp(l.body())
        ),
        Expr::Part(p) => format!("(part {})", p.body().map_or(0, |b| b.statements().count())),
    }
}

fn pattern(pattern: Option<Pattern<'_>>) -> String {
    match pattern {
        Some(Pattern::Wildcard(_)) => "_".to_owned(),
        Some(Pattern::Literal(lit)) => sexp(lit.expr()),
        Some(Pattern::Name(name)) => name
            .segments()
            .map(|t| t.text())
            .collect::<Vec<_>>()
            .join("."),
        Some(Pattern::Variant(variant)) => {
            let mut fields: Vec<String> = variant
                .fields()
                .map(|f| match f.pattern() {
                    Some(p) => format!("{}={}", text(f.name()), self::pattern(Some(p))),
                    None => text(f.name()).to_owned(),
                })
                .collect();
            if variant.has_rest() {
                fields.push("..".to_owned());
            }
            let name: Vec<_> = variant.segments().map(|t| t.text()).collect();
            format!("{}({})", name.join("."), fields.join(" "))
        }
        Some(Pattern::Or(or)) => format!(
            "(| {})",
            or.alternatives()
                .map(|p| self::pattern(Some(p)))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        None => "?".to_owned(),
    }
}

/// Parses `value` as the right-hand side of an attribute and prints it.
fn expr(value: &str) -> String {
    let source = format!("x = {value}\n");
    let parsed = parse(&source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Some(Item::Stmt(Stmt::Attr(attr))) = parsed.file().items().next() else {
        panic!("expected an attribute");
    };
    sexp(attr.value())
}

fn ty(ty: Option<Type<'_>>) -> String {
    match ty {
        Some(Type::Named(named)) => {
            let name: Vec<_> = named
                .path()
                .into_iter()
                .flat_map(|p| p.segments())
                .map(|t| t.text())
                .collect();
            let args: Vec<_> = named.args().map(|a| self::ty(Some(a))).collect();
            if args.is_empty() {
                name.join(".")
            } else {
                format!("{}<{}>", name.join("."), args.join(" "))
            }
        }
        Some(Type::Fn(f)) => format!(
            "Fn({}) -> {}",
            f.params()
                .map(|p| self::ty(Some(p)))
                .collect::<Vec<_>>()
                .join(" "),
            self::ty(f.return_type())
        ),
        Some(Type::Refined(r)) => {
            let predicates: Vec<String> = r
                .predicates()
                .map(|p| match p {
                    Predicate::Range(range) => format!(
                        "{}{}{}",
                        sexp(range.start()),
                        if range.is_inclusive() { "..=" } else { ".." },
                        sexp(range.end())
                    ),
                    Predicate::Named(named) => format!(
                        "{}/{}",
                        named
                            .segments()
                            .map(|t| t.text())
                            .collect::<Vec<_>>()
                            .join("."),
                        named.args().map_or(0, |a| a.args().count())
                    ),
                })
                .collect();
            format!("({} where {})", self::ty(r.base()), predicates.join(" && "))
        }
        None => "?".to_owned(),
    }
}

#[test]
fn binary_operators_bind_by_precedence() {
    assert_eq!(expr("a || b && c"), "(|| a (&& b c))");
    assert_eq!(expr("a && b == c"), "(&& a (== b c))");
    assert_eq!(expr("a == b + c"), "(== a (+ b c))");
    assert_eq!(expr("a + b * c"), "(+ a (* b c))");
    assert_eq!(expr("a ?? b || c"), "(?? a (|| b c))");
    assert_eq!(expr("a * b ++ c % d"), "(++ (* a b) (% c d))");
    assert_eq!(expr("(a + b) * c"), "(* (+ a b) c)");
}

#[test]
fn coalesce_groups_right_and_other_operators_group_left() {
    assert_eq!(expr("a ?? b ?? c"), "(?? a (?? b c))");
    assert_eq!(expr("a - b - c"), "(- (- a b) c)");
    assert_eq!(expr("a / b * c"), "(* (/ a b) c)");
    assert_eq!(expr("a || b || c"), "(|| (|| a b) c)");
}

#[test]
fn unary_binds_between_binary_and_postfix() {
    assert_eq!(expr("-a.b"), "(- (. a b))");
    assert_eq!(expr("!f(x) && y"), "(&& (! (call f x)) y)");
    assert_eq!(expr("- -1 * 2"), "(* (- (- 1)) 2)");
    assert_eq!(expr("a?.b[0](c = 1)"), "(call ([] (?. a b) 0) c=1)");
}

#[test]
fn compound_expressions_expose_their_parts() {
    assert_eq!(
        expr("if a { 1 } else if b { 2 } else { 3 }"),
        "(if a 1 (if b 2 3))"
    );
    assert_eq!(
        expr("let a = 1, b = a in a + b"),
        "(let (a 1) (b a) (+ a b))"
    );
    assert_eq!(expr("fn(x: Int, y: Int) = x * y"), "(fn (x y) (* x y))");
    assert_eq!(expr("part { a = 1\nb = 2 }"), "(part 2)");
    assert_eq!(
        expr("[for x in xs if x > 0 -> x * 2]"),
        "[for x in xs if (> x 0) -> (* x 2)]"
    );
    assert_eq!(
        expr("{for k, v in m -> \"#{k}\" = v}"),
        "{for k v in m if ? -> \"\"=v}"
    );
    assert_eq!(
        expr("svc.Ready { fd = 3, \"a b\" = self.x }"),
        "svc Ready{fd=3 \"a b\"=(. self x)}"
    );
    assert_eq!(
        expr("[llvm@19:static, ripgrep:static, lib@1]"),
        "[(sym llvm 19 static) (sym ripgrep - static) (sym lib 1 -)]"
    );
}

#[test]
fn match_arms_expose_patterns_guards_and_values() {
    assert_eq!(
        expr(
            "match r {\n  0 | -1 => a\n  check(cmd, i = n, ..) if n > 1 => b\n  Level.info => c\n  _ => d\n}"
        ),
        "(match r ((| 0 (- 1)) if ? => a) (check(cmd i=n ..) if (> n 1) => b) (Level.info if ? => c) (_ if ? => d))"
    );
}

#[test]
fn string_values_decode_escapes_and_layout() {
    let escape = format!("\"a\\n\\t\\\"\\#{{\\u{{{}}}\"", "1F600");
    assert_eq!(expr(&escape), format!("{:?}", "a\n\t\"#{\u{1F600}"));
    assert_eq!(
        expr("\"\"\"\n  one\n    two\n  \"\"\""),
        format!("{:?}", "one\n  two\n")
    );
    assert_eq!(expr("r\"\\d+#{x}\""), format!("{:?}", "\\d+#{x}"));
    assert_eq!(expr("\"a #{b} c\""), "(str ...)");

    let parsed =
        parse("a = \"x\"\nb = \"\"\"\n  y\n  \"\"\"\nc = r\"z\"\nd = r\"\"\"\n  w\n  \"\"\"\n");
    let kinds: Vec<_> = parsed
        .file()
        .items()
        .filter_map(|item| match item {
            Item::Stmt(Stmt::Attr(attr)) => match attr.value() {
                Some(Expr::Str(s)) => s.kind(),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [
            StrKind::Plain,
            StrKind::Multi,
            StrKind::Raw,
            StrKind::RawMulti
        ]
    );
}

#[test]
fn invalid_escapes_have_no_value() {
    let parsed = parse("a = \"\\q\"\n");
    let Some(Item::Stmt(Stmt::Attr(attr))) = parsed.file().items().next() else {
        panic!("expected an attribute");
    };
    let Some(Expr::Str(string)) = attr.value() else {
        panic!("expected a string");
    };
    assert_eq!(string.value(), None);
}

#[test]
fn literals_report_their_kind() {
    let parsed = parse("a = [1, 1.5, ./p, true, false, none]\n");
    let Some(Item::Stmt(Stmt::Attr(attr))) = parsed.file().items().next() else {
        panic!("expected an attribute");
    };
    let Some(Expr::List(list)) = attr.value() else {
        panic!("expected a list");
    };
    let kinds: Vec<_> = list
        .items()
        .filter_map(|item| match item {
            Expr::Literal(lit) => lit.kind(),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [
            LiteralKind::Int,
            LiteralKind::Float,
            LiteralKind::Path,
            LiteralKind::True,
            LiteralKind::False,
            LiteralKind::None
        ]
    );
}

#[test]
fn declarations_expose_their_parts() {
    let source = r#"import "std/desktop" as desktop
struct Service {
  exec: List<Str> where nonEmpty && 1..=4
  units: Map<Str, Unit> = {} block unit
  run: Fn(Int, Str) -> Bool
}
type Ready = immediate | notify(fd: Int where -1.. = 3)
fn local(port: Int) -> desktop.Upstream = port
let n: Int = 1
"#;
    let parsed = parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let items: Vec<_> = parsed.file().items().collect();

    let Item::Import(import) = items[0] else {
        panic!("expected an import");
    };
    assert_eq!(
        import.path().and_then(|p| p.value()).as_deref(),
        Some("std/desktop")
    );
    assert_eq!(text(import.alias()), "desktop");

    let Item::Struct(service) = items[1] else {
        panic!("expected a struct");
    };
    assert_eq!(text(service.name()), "Service");
    let fields: Vec<String> = service
        .fields()
        .map(|f| {
            format!(
                "{}: {} = {} block {}",
                text(f.name()),
                ty(f.ty()),
                sexp(f.default()),
                text(f.block_kind())
            )
        })
        .collect();
    assert_eq!(
        fields,
        [
            "exec: (List<Str> where nonEmpty/0 && 1..=4) = ? block ?",
            "units: Map<Str Unit> = {} block unit",
            "run: Fn(Int Str) -> Bool = ? block ?",
        ]
    );

    let Item::Type(ready) = items[2] else {
        panic!("expected a type");
    };
    let variants: Vec<String> = ready
        .variants()
        .map(|v| {
            let params: Vec<String> = v
                .params()
                .into_iter()
                .flat_map(|p| p.params())
                .map(|p| format!("{}: {} = {}", text(p.name()), ty(p.ty()), sexp(p.default())))
                .collect();
            format!("{}({})", text(v.name()), params.join(", "))
        })
        .collect();
    assert_eq!(
        variants,
        ["immediate()", "notify(fd: (Int where (- 1)..?) = 3)"]
    );

    let Item::Fn(local) = items[3] else {
        panic!("expected a function");
    };
    assert_eq!(text(local.name()), "local");
    assert_eq!(local.params().map(|p| p.params().count()), Some(1));
    assert_eq!(ty(local.return_type()), "desktop.Upstream");
    assert_eq!(sexp(local.body()), "port");

    let Item::Stmt(Stmt::Let(n)) = items[4] else {
        panic!("expected a let");
    };
    assert_eq!(
        (text(n.name()), ty(n.ty()), sexp(n.value())),
        ("n", "Int".to_owned(), "1".to_owned())
    );
}

#[test]
fn statements_expose_their_parts() {
    let source = r#"a."b c".d = 1
service sshd { user = "root" }
fs "/" { }
include "./hw.stav"
if x { a = 1 } else if y { } else { }
for k, v in m { }
"#;
    let parsed = parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let stmts: Vec<_> = parsed
        .file()
        .items()
        .filter_map(|item| match item {
            Item::Stmt(stmt) => Some(stmt),
            _ => None,
        })
        .collect();

    let Stmt::Attr(attr) = stmts[0] else {
        panic!("expected an attribute");
    };
    let keys: Vec<String> = attr
        .path()
        .into_iter()
        .flat_map(|p| p.keys())
        .map(|k| key(Some(k)))
        .collect();
    assert_eq!(keys, ["a", "\"b c\"", "d"]);

    let Stmt::Block(service) = stmts[1] else {
        panic!("expected a block");
    };
    assert_eq!(
        (text(service.kind()), key(service.label())),
        ("service", "sshd".to_owned())
    );
    assert_eq!(service.body().map(|b| b.statements().count()), Some(1));

    let Stmt::Block(fs) = stmts[2] else {
        panic!("expected a block");
    };
    assert_eq!(
        (text(fs.kind()), key(fs.label())),
        ("fs", "\"/\"".to_owned())
    );

    let Stmt::Include(include) = stmts[3] else {
        panic!("expected an include");
    };
    assert_eq!(sexp(include.value()), "\"./hw.stav\"");

    let Stmt::If(branch) = stmts[4] else {
        panic!("expected an if");
    };
    assert_eq!(sexp(branch.condition()), "x");
    assert_eq!(branch.then_body().map(|b| b.statements().count()), Some(1));
    let Some(ElseBranch::If(nested)) = branch.else_branch() else {
        panic!("expected else if");
    };
    assert!(matches!(nested.else_branch(), Some(ElseBranch::Body(_))));

    let Stmt::For(each) = stmts[5] else {
        panic!("expected a for");
    };
    let names: Vec<_> = each.names().map(|t| t.text()).collect();
    assert_eq!(
        (names, sexp(each.collection())),
        (vec!["k", "v"], "m".to_owned())
    );
}

#[test]
fn blocks_without_label_have_none() {
    let parsed = parse("boot { }\n");
    let Some(Item::Stmt(Stmt::Block(block))) = parsed.file().items().next() else {
        panic!("expected a block");
    };
    assert!(block.label().is_none());
}

#[test]
fn missing_parts_are_none_in_broken_code() {
    let parsed = parse("x = (1 +)\nstruct {\n  : Int\n}\n");
    assert!(!parsed.diagnostics.is_empty());
    let items: Vec<_> = parsed.file().items().collect();
    let Item::Stmt(Stmt::Attr(attr)) = items[0] else {
        panic!("expected an attribute");
    };
    let Some(Expr::Paren(paren)) = attr.value() else {
        panic!("expected parentheses");
    };
    let Some(Expr::Binary(sum)) = paren.expr() else {
        panic!("expected a binary expression");
    };
    assert_eq!(sum.op(), Some(BinaryOp::Add));
    assert!(sum.lhs().is_some());
    assert!(sum.rhs().is_none());
    let Item::Struct(decl) = items[1] else {
        panic!("expected a struct");
    };
    assert!(decl.name().is_none());
    assert_eq!(decl.fields().count(), 0);
}

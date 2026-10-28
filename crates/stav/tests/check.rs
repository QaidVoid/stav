//! Checks the fixture programs and compares the diagnostics, or the
//! summary of what the checker inferred, with their snapshots.
//!
//! A fixture whose name starts with `err_` must fail to check, and every
//! other fixture must pass.
//!
//! A fixture is one `.stav` file. Without sections it is the entry file
//! `main.stav` of the root struct `Root`. Lines starting with `#-` split it
//! into several sources:
//!
//! - `#- file <path>`: a file of the import root, the first one being the
//!   entry file unless `#! entries` says otherwise,
//! - `#- std <name>`: the module `std/<name>`,
//! - `#- prelude <name>`: a prelude file,
//! - `#- part <name>`: an implicit part,
//! - `#- host <namespace>/<module>`: a host module.
//!
//! Lines starting with `#!` before the first section set options:
//! `#! root <Name>`, `#! entries <path> <path>` and
//! `#! symbol <Name> <constructor> [words] [strings]`.

use std::fs;
use std::path::{Path, PathBuf};

use stav::{Host, HostModule, ImportRoot, MemoryLoader, RootSpec, Source, SymbolType, check};

const ROOT_DIR: &str = "app";

struct Fixture {
    host: Host,
    root: RootSpec,
}

fn accept(text: &str) -> Result<String, String> {
    Ok(text.to_owned())
}

/// Builds the host and root of a fixture.
fn fixture(text: &str) -> Fixture {
    let mut loader = MemoryLoader::new();
    let mut prelude = Vec::new();
    let mut parts = Vec::new();
    let mut modules = Vec::new();
    let mut symbols = Vec::new();
    let mut root_struct = "Root".to_owned();
    let mut entries: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    let mut sections: Vec<(String, String, String)> = Vec::new();
    let mut current: Option<(String, String, String)> = None;
    for line in text.split_inclusive('\n') {
        if let Some(header) = line.strip_prefix("#- ") {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            let (kind, name) = header
                .trim()
                .split_once(' ')
                .expect("a section has a kind and a name");
            current = Some((kind.to_owned(), name.to_owned(), String::new()));
            continue;
        }
        match &mut current {
            Some((_, _, body)) => body.push_str(line),
            None => {
                let Some(option) = line.strip_prefix("#! ") else {
                    current = Some(("file".to_owned(), "main.stav".to_owned(), line.to_owned()));
                    continue;
                };
                let words: Vec<&str> = option.split_whitespace().collect();
                match words.as_slice() {
                    ["root", name] => root_struct = (*name).to_owned(),
                    ["entries", paths @ ..] => {
                        entries = paths.iter().map(|p| Path::new(ROOT_DIR).join(p)).collect();
                    }
                    ["symbol", name, constructor, flags @ ..] => symbols.push(SymbolType {
                        name: (*name).to_owned(),
                        words: flags.contains(&"words"),
                        strings: flags.contains(&"strings"),
                        constructor: (*constructor).to_owned(),
                        parse: accept,
                    }),
                    _ => panic!("unknown option {option:?}"),
                }
            }
        }
    }
    if let Some(section) = current.take() {
        sections.push(section);
    }
    for (kind, name, body) in sections {
        match kind.as_str() {
            "file" => {
                let path = Path::new(ROOT_DIR).join(&name);
                files.push(path.clone());
                loader = loader.with_file(path, body);
            }
            "std" => loader = loader.with_std(name, body),
            "prelude" => prelude.push(Source::new(name, body)),
            "part" => parts.push(Source::new(name, body)),
            "host" => modules.push(HostModule {
                path: name,
                source: body,
            }),
            _ => panic!("unknown section kind {kind:?}"),
        }
    }
    if entries.is_empty() {
        entries.extend(files.first().cloned());
    }
    let mut host = Host::new(loader);
    host.prelude = prelude;
    host.implicit_parts = parts;
    host.host_modules = modules;
    host.symbols = symbols;
    Fixture {
        host,
        root: RootSpec {
            root_struct,
            entries,
            import_roots: vec![ImportRoot::new(ROOT_DIR)],
        },
    }
}

/// Checks a fixture and renders the diagnostics, or the summary and the
/// warnings.
fn run(text: &str) -> String {
    let fixture = fixture(text);
    match check(&fixture.host, &fixture.root) {
        Ok(checked) => {
            let mut out = checked.summary();
            if !checked.warnings().is_empty() {
                out.push_str("--- warnings\n");
                out.push_str(&checked.warnings().to_string());
            }
            out
        }
        Err(diagnostics) => {
            assert!(diagnostics.has_errors(), "a failed check reports an error");
            format!("--- errors\n{diagnostics}")
        }
    }
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/check");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("the fixture directory exists")
        .map(|entry| entry.expect("the fixture directory is readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "stav"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "{} has no fixtures", dir.display());
    paths
}

#[test]
fn fixtures_check_to_their_snapshots() {
    let mut unexpected = Vec::new();
    for path in fixtures() {
        let text = fs::read_to_string(&path).expect("the fixture is readable");
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("the name is UTF-8");
        let output = run(&text);
        let failed = output.starts_with("--- errors");
        if failed != name.starts_with("err_") {
            unexpected.push(format!("{name}:\n{output}"));
        }
        insta::assert_snapshot!(name, output);
    }
    assert!(
        unexpected.is_empty(),
        "unexpected results:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn checking_twice_gives_the_same_result() {
    for path in fixtures() {
        let text = fs::read_to_string(&path).expect("the fixture is readable");
        assert_eq!(
            run(&text),
            run(&text),
            "{} is not deterministic",
            path.display()
        );
    }
}

/// A loader whose files fail with a given error.
struct Failing(stav::LoadError);

impl stav::Loader for Failing {
    fn std_module(&self, _: &str) -> Option<std::borrow::Cow<'static, str>> {
        None
    }

    fn read(&self, _: &ImportRoot, rel: &stav::RelPath) -> Result<String, stav::LoadError> {
        if rel.as_str() == "main.stav" {
            Ok("import \"./lib.stav\" as lib\nstruct Root { a: Int = 1 }\n".to_owned())
        } else {
            Err(self.0.clone())
        }
    }
}

fn codes(host: &Host, root: &RootSpec) -> Vec<&'static str> {
    match check(host, root) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics.iter().map(|d| d.diagnostic.code).collect(),
    }
}

fn app_root() -> RootSpec {
    RootSpec {
        root_struct: "Root".to_owned(),
        entries: vec![Path::new(ROOT_DIR).join("main.stav")],
        import_roots: vec![ImportRoot::new(ROOT_DIR)],
    }
}

#[test]
fn loader_errors_have_their_own_codes() {
    let cases = [
        (stav::LoadError::NotFound, "E0701"),
        (stav::LoadError::NotAFile, "E0705"),
        (stav::LoadError::OutsideRoot, "E0702"),
        (stav::LoadError::TooLarge, "E0706"),
        (stav::LoadError::Other("denied".to_owned()), "E0701"),
    ];
    for (error, code) in cases {
        let host = Host::new(Failing(error));
        assert_eq!(codes(&host, &app_root()), vec![code]);
    }
}

#[test]
fn sources_larger_than_four_mebibytes_are_refused() {
    let big = format!(
        "struct Root {{ a: Int = 1 }}\n{}",
        "#".repeat(4 * 1024 * 1024)
    );
    let host = Host::new(MemoryLoader::new().with_file("app/main.stav", big));
    assert_eq!(codes(&host, &app_root()), vec!["E0706"]);
}

#[test]
fn entries_belong_to_the_innermost_import_root() {
    let loader = MemoryLoader::new()
        .with_file(
            "app/sub/main.stav",
            "import \"./lib.stav\" as lib\nstruct Root { a: Int = lib.x }\n",
        )
        .with_file("app/sub/lib.stav", "let x = 1\n")
        .with_file("app/lib.stav", "let y = 1\n");
    let root = RootSpec {
        root_struct: "Root".to_owned(),
        entries: vec!["app/sub/./main.stav".into()],
        import_roots: vec![ImportRoot::new("app"), ImportRoot::new("app/sub")],
    };
    let checked = check(&Host::new(loader), &root).expect("the program checks");
    let names: Vec<&str> = checked.files().map(|(_, name)| name).collect();
    assert_eq!(names, ["app/sub/main.stav", "app/sub/lib.stav"]);

    let escaping = MemoryLoader::new()
        .with_file(
            "app/sub/main.stav",
            "import \"../lib.stav\" as lib\nstruct Root { a: Int = 1 }\n",
        )
        .with_file("app/lib.stav", "let y = 1\n");
    let only_sub = RootSpec {
        import_roots: vec![ImportRoot::new("app/sub")],
        ..root
    };
    assert_eq!(codes(&Host::new(escaping), &only_sub), vec!["E0702"]);
}

#[test]
fn the_schema_lists_declarations_with_their_docs() {
    let source = "## Logging levels.\ntype Level =\n  ## Everything.\n  | debug\n  | info\n\
                  ## The root.\nstruct Root {\n  ## How much to log.\n  level: Level = info\n  \
                  name: Str where nonEmpty\n  note: Option<Str>\n  ready: Ready = now\n}\n\
                  type Ready = now | later(after: Int, retries: Int = 3)\n";
    let host = Host::new(MemoryLoader::new().with_file("app/main.stav", source));
    let schema = check(&host, &app_root())
        .expect("the program checks")
        .schema();
    let root = &schema.structs[0];
    assert_eq!(
        (root.name.as_str(), root.doc.as_deref()),
        ("Root", Some("The root."))
    );
    let fields: Vec<(&str, &str, bool, Option<&str>)> = root
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.ty.as_str(), f.required, f.doc.as_deref()))
        .collect();
    assert_eq!(
        fields,
        [
            ("level", "Level", false, Some("How much to log.")),
            ("name", "Str where nonEmpty", true, None),
            ("note", "Option<Str>", false, None),
            ("ready", "Ready", false, None),
        ]
    );
    let level = &schema.sums[0];
    assert_eq!(level.doc.as_deref(), Some("Logging levels."));
    assert_eq!(level.variants[0].doc.as_deref(), Some("Everything."));
    let later = &schema.sums[1].variants[1];
    let payload: Vec<(&str, bool)> = later
        .fields
        .iter()
        .flatten()
        .map(|f| (f.name.as_str(), f.required))
        .collect();
    assert_eq!(payload, [("after", true), ("retries", false)]);
}

#[test]
fn deeply_nested_programs_check_without_overflowing_the_stack() {
    let depth = stav::MAX_DEPTH - 8;
    let nested = [
        format!("{}1{}", "(".repeat(depth), ")".repeat(depth)),
        format!("{}1", "1 + ".repeat(depth)),
        format!("{}1{}", "[".repeat(depth / 2), "]".repeat(depth / 2)),
        format!(
            "{}1{}",
            "if true { ".repeat(depth / 3),
            " } else { 0 }".repeat(depth / 3)
        ),
        format!("{}1", "let x = 1 in ".repeat(depth / 2)),
        format!("{}1", "-".repeat(depth)),
    ];
    for expression in nested {
        let source = format!("struct Root {{ a: Data = none }}\nlet v = {expression}\n");
        let text = source.clone();
        let result = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let host = Host::new(MemoryLoader::new().with_file("app/main.stav", text));
                check(&host, &app_root())
                    .map(|_| ())
                    .map_err(|d| d.to_string())
            })
            .expect("the thread starts")
            .join();
        assert!(result.is_ok(), "checking panicked for {source}");
    }
}

/// Checks the program of `loader` on a thread with a small stack and
/// returns the codes of its errors.
fn codes_on_small_stack(loader: MemoryLoader) -> Vec<&'static str> {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || codes(&Host::new(loader), &app_root()))
        .expect("the thread starts")
        .join()
        .expect("checking does not overflow the stack")
}

const CHAIN: usize = 3000;

#[test]
fn long_include_chains_check_without_overflowing_the_stack() {
    let mut loader = MemoryLoader::new().with_file(
        "app/main.stav",
        "struct Root { a: Int = 0 }\ninclude \"./p0.stav\"\n",
    );
    for i in 0..CHAIN {
        let text = if i + 1 < CHAIN {
            format!("include \"./p{}.stav\"\n", i + 1)
        } else {
            "a = \"x\"\n".to_owned()
        };
        loader = loader.with_file(format!("app/p{i}.stav"), text);
    }
    assert_eq!(codes_on_small_stack(loader), ["E0401"]);
}

#[test]
fn long_import_chains_check_without_overflowing_the_stack() {
    let mut loader = MemoryLoader::new().with_file(
        "app/main.stav",
        "import \"./m0.stav\" as m\nstruct Root { a: Int = m.x }\n",
    );
    for i in 0..CHAIN {
        let text = if i + 1 < CHAIN {
            format!("import \"./m{}.stav\" as n\nlet x = n.x\n", i + 1)
        } else {
            "let x = \"x\"\n".to_owned()
        };
        loader = loader.with_file(format!("app/m{i}.stav"), text);
    }
    assert_eq!(codes_on_small_stack(loader), ["E0401"]);
}

#[test]
fn long_chains_through_predicate_arguments_check_without_overflowing_the_stack() {
    let mut loader = MemoryLoader::new().with_file(
        "app/main.stav",
        "import \"./m0.stav\" as m\nstruct Root { a: Int = m.x }\n",
    );
    for i in 0..CHAIN {
        let text = if i + 1 < CHAIN {
            format!(
                "import \"./m{}.stav\" as n\nfn above(a: Int, b: Int) -> Bool = a > b\n\
                 let x: Int where above(n.x) = n.x\n",
                i + 1
            )
        } else {
            "let x = \"x\"\n".to_owned()
        };
        loader = loader.with_file(format!("app/m{i}.stav"), text);
    }
    assert!(codes_on_small_stack(loader).contains(&"E0401"));
}

#[test]
fn long_let_chains_check_without_overflowing_the_stack() {
    let mut source = format!("struct Root {{ a: Int = v00001 }}\nlet v{CHAIN:05} = \"x\"\n");
    for i in (1..CHAIN).rev() {
        source.push_str(&format!("let v{i:05} = v{:05}\n", i + 1));
    }
    let loader = MemoryLoader::new().with_file("app/main.stav", source);
    assert_eq!(codes_on_small_stack(loader), ["E0401"]);
}

#[test]
fn matches_with_thousands_of_arms_check_quickly() {
    let arms = 30_000;
    let ints: String = (0..arms).map(|i| format!("  {i} => {i},\n")).collect();
    let strs: String = (0..arms).map(|i| format!("  \"s{i}\" => {i},\n")).collect();
    let ors: String = (0..arms)
        .map(|i| format!("  {i} | {} => {i},\n", i + arms))
        .collect();
    let functions = [
        format!("fn f(x: Int) -> Int = match x {{\n{ints}  _ => 0,\n}}\n"),
        format!("fn f(x: Str) -> Int = match x {{\n{strs}  _ => 0,\n}}\n"),
        format!("fn f(x: Int) -> Int = match x {{\n{ors}  _ => 0,\n}}\n"),
        format!("fn f(x: Option<Int>) -> Int = match x {{\n  none => 0,\n{ints}  _ => 0,\n}}\n"),
    ];
    for function in functions {
        let source = format!("struct Root {{ a: Int = 0 }}\n{function}");
        let host = Host::new(MemoryLoader::new().with_file("app/main.stav", source));
        let start = std::time::Instant::now();
        assert_eq!(codes(&host, &app_root()), Vec::<&str>::new());
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }
}

mod random {
    use proptest::prelude::*;
    use proptest::sample::select;

    use super::*;

    fn name() -> impl Strategy<Value = String> {
        select(vec![
            "a",
            "b",
            "x",
            "f",
            "g",
            "lib.h",
            "lib.c",
            "info",
            "debug",
            "warn",
            "Level.info",
            "self.a",
            "self.b",
            "self.level",
            "neovim",
            "Root",
            "Point",
            "points",
            "p",
        ])
        .prop_map(str::to_owned)
    }

    fn ty() -> impl Strategy<Value = String> {
        select(vec![
            "Int",
            "Str",
            "Bool",
            "Float",
            "Data",
            "Path",
            "Option<Int>",
            "List<Str>",
            "Map<Str, Int>",
            "Set<Level>",
            "Level",
            "Point",
            "Option<Point>",
            "Fn(Int) -> Int",
            "Int where 1..=5",
            "Str where nonEmpty && matches(r\"^a\")",
            "Part<Root>",
            "Ref<Point>",
            "Pkg",
            "lib.Mode",
            "List<Point>",
        ])
        .prop_map(str::to_owned)
    }

    fn expr() -> impl Strategy<Value = String> {
        let leaf = prop_oneof![
            (0i64..50).prop_map(|i| i.to_string()),
            select(vec![
                "1.5",
                "true",
                "false",
                "none",
                "\"s\"",
                "\"#{a}\"",
                "./p.conf",
                "llvm@1:static",
                "[]",
                "{}",
                "self",
                "r\"raw\"",
            ])
            .prop_map(str::to_owned),
            name(),
        ];
        leaf.prop_recursive(4, 24, 3, |inner| {
            let op = select(vec!["+", "-", "*", "==", "<", "&&", "++", "??", "%", "/"]);
            prop_oneof![
                (inner.clone(), op, inner.clone()).prop_map(|(a, o, b)| format!("({a} {o} {b})")),
                prop::collection::vec(inner.clone(), 0..3)
                    .prop_map(|v| format!("[{}]", v.join(", "))),
                prop::collection::vec((name(), inner.clone()), 0..3).prop_map(|v| {
                    let entries: Vec<String> = v
                        .into_iter()
                        .map(|(k, e)| format!("{} = {e}", k.replace('.', "_")))
                        .collect();
                    format!("{{ {} }}", entries.join(", "))
                }),
                (
                    select(vec![
                        "f", "g", "len", "map", "fold", "join", "toStr", "force", "fail", "check",
                        "pkg", "lib.h"
                    ]),
                    prop::collection::vec(inner.clone(), 0..3)
                )
                    .prop_map(|(f, args)| format!("{f}({})", args.join(", "))),
                (inner.clone(), inner.clone(), inner.clone())
                    .prop_map(|(c, a, b)| format!("(if {c} {{ {a} }} else {{ {b} }})")),
                (inner.clone(), inner.clone(), inner.clone()).prop_map(|(s, a, b)| format!(
                    "(match {s} {{ debug => {a}, check(n, ..) => {b}, none => {a}, _ => {b} }})"
                )),
                (inner.clone(), inner.clone())
                    .prop_map(|(v, b)| format!("(let x = {v}, y = x in {b})")),
                (ty(), inner.clone()).prop_map(|(t, b)| format!("(fn(x: {t}) = {b})")),
                (inner.clone(), name()).prop_map(|(e, n)| format!("({e}).{}", n.replace('.', "_"))),
                (inner.clone(), inner.clone())
                    .prop_map(|(s, v)| format!("[for x in {s} if x -> {v}]")),
                (inner.clone(), inner.clone())
                    .prop_map(|(s, v)| format!("{{for k, v in {s} -> k = {v}}}")),
                inner.clone().prop_map(|e| format!("(part {{ a = {e} }})")),
                inner.clone().prop_map(|e| format!("Point {{ x = {e} }}")),
                (inner.clone(), inner).prop_map(|(a, b)| format!("{a}[{b}]")),
            ]
        })
    }

    fn declaration() -> impl Strategy<Value = String> {
        prop_oneof![
            (ty(), expr(), ty())
                .prop_map(|(t, e, u)| format!("struct Extra {{ x: {t} = {e}, y: {u} }}")),
            ty().prop_map(|t| format!("type Kind = one | two(n: {t})")),
            (ty(), ty(), expr(), ty(), expr())
                .prop_map(|(a, b, d, r, e)| format!("fn f(a: {a}, b: {b} = {d}) -> {r} = {e}")),
            (ty(), expr()).prop_map(|(t, e)| format!("fn g(x: {t}) -> Int = {e}")),
            expr().prop_map(|e| format!("let a = {e}")),
            (ty(), expr()).prop_map(|(t, e)| format!("let b: {t} = {e}")),
        ]
    }

    fn item() -> impl Strategy<Value = String> {
        prop_oneof![
            declaration(),
            (
                select(vec![
                    "a",
                    "b",
                    "level",
                    "p.x",
                    "points.k.y",
                    "data.k",
                    "a.b"
                ]),
                expr()
            )
                .prop_map(|(p, e)| format!("{p} = {e}")),
            expr().prop_map(|e| format!("a = force({e})")),
            (expr(), expr())
                .prop_map(|(l, e)| format!("points \"#{{{l}}}\" {{ x = {e}\n let z = self.x }}")),
            (expr(), expr())
                .prop_map(|(c, e)| format!("if {c} {{ b = {e} }} else {{ level = info }}")),
            (expr(), expr()).prop_map(|(c, e)| format!("for x, y in {c} {{ a = {e} }}")),
            expr().prop_map(|e| format!("include {e}")),
            Just("include \"./part.stav\"".to_owned()),
            Just("p { x = 1 }".to_owned()),
            Just("check n { }".to_owned()),
        ]
    }

    fn program() -> impl Strategy<Value = (String, String)> {
        (
            prop::collection::vec(item(), 0..8),
            prop::collection::vec(declaration(), 0..4),
        )
            .prop_map(|(main, lib)| {
                let main = format!(
                    "#! symbol Pkg pkg words\n#- file main.stav\nimport \"./lib.stav\" as lib\n\
                     struct Root {{ a: Int = 0, b: Str = \"\", level: Level = info, data: Data = none, \
                     points: Map<Str, Point> = {{}} block points, p: Option<Point>, check: Level = info }}\n\
                     struct Point {{ x: Int = 0, y: Str = \"\" }}\n\
                     type Level = debug | info | warn | check(n: Int = 0)\n{}\n\
                     #- file lib.stav\ntype Mode = on | off\nlet c = 1\nfn h(x: Int) -> Int = x\n{}\n\
                     #- file part.stav\na = 1\n",
                    main.join("\n"),
                    lib.join("\n")
                );
                (main, String::new())
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn checking_never_panics_and_is_deterministic((text, _) in program()) {
            let first = run(&text);
            let second = run(&text);
            prop_assert_eq!(first, second);
        }

        #[test]
        fn checking_arbitrary_text_never_panics(text in "[ -~\n]{0,200}") {
            let _ = run(&text.replace("#-", "# -"));
        }
    }
}

//! Evaluates the fixture programs and compares the rendered values, or the
//! diagnostics, with their snapshots.
//!
//! A fixture whose name starts with `err_` must check but fail to
//! evaluate, and every other fixture must evaluate. A fixture is one
//! `.stav` file. Without sections it is the entry file `main.stav` of the
//! root struct `Root`. Lines starting with `#-` split it into several
//! sources:
//!
//! - `#- file <path>`: a file of the import root, the first one being the
//!   entry file unless `#! entries` says otherwise,
//! - `#- std <name>`: the module `std/<name>`,
//! - `#- prelude <name>`: a prelude file,
//! - `#- part <name>`: an implicit part,
//! - `#- host <namespace>/<module>`: a host module.
//!
//! Lines starting with `#!` before the first section set options:
//! `#! root <Name>`, `#! entries <path> <path>`,
//! `#! symbol <Name> <constructor> [words] [strings]` for a symbol type
//! whose parser lowercases the text and refuses text with spaces,
//! `#! implicit <Struct>` for implicit reference targets named by the keys
//! of the root field `users`, `#! render <format>...` for the formats of
//! the snapshot, JSON by default, and `#! steps <n>` for the step limit.

use std::fs;
use std::path::{Path, PathBuf};

use stav::{
    Format, Host, HostModule, ImplicitRefs, ImportRoot, Limits, MemoryLoader, RootSpec, Source,
    Span, SymbolType, Value, check,
};

const ROOT_DIR: &str = "app";

struct Fixture {
    host: Host,
    root: RootSpec,
    formats: Vec<Format>,
    limits: Limits,
}

fn lowercase(text: &str) -> Result<String, String> {
    if text.contains(' ') {
        return Err("a symbol has no spaces".to_owned());
    }
    Ok(text.to_lowercase())
}

/// Returns the keys of the root field `users`.
fn user_groups(root: &Value) -> Vec<String> {
    match root.get("users") {
        Some(Value::Map(users)) => users.iter().map(|(name, _)| name.clone()).collect(),
        _ => Vec::new(),
    }
}

fn fixture(text: &str) -> Fixture {
    let mut loader = MemoryLoader::new();
    let mut host_parts = (Vec::new(), Vec::new(), Vec::new());
    let mut symbols = Vec::new();
    let mut implicit = Vec::new();
    let mut formats = Vec::new();
    let mut limits = Limits::default();
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
                        parse: lowercase,
                    }),
                    ["implicit", target] => implicit.push(ImplicitRefs {
                        target: (*target).to_owned(),
                        labels: user_groups,
                    }),
                    ["render", names @ ..] => {
                        formats = names
                            .iter()
                            .map(|n| match *n {
                                "json" => Format::Json,
                                "toml" => Format::Toml,
                                "yaml" => Format::Yaml,
                                _ => panic!("unknown format {n:?}"),
                            })
                            .collect();
                    }
                    ["steps", n] => limits.steps = n.parse().expect("a step limit"),
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
            "prelude" => host_parts.0.push(Source::new(name, body)),
            "part" => host_parts.1.push(Source::new(name, body)),
            "host" => host_parts.2.push(HostModule {
                path: name,
                source: body,
            }),
            _ => panic!("unknown section kind {kind:?}"),
        }
    }
    if entries.is_empty() {
        entries.extend(files.first().cloned());
    }
    if formats.is_empty() {
        formats.push(Format::Json);
    }
    let mut host = Host::new(loader);
    host.prelude = host_parts.0;
    host.implicit_parts = host_parts.1;
    host.host_modules = host_parts.2;
    host.symbols = symbols;
    host.implicit_refs = implicit;
    Fixture {
        host,
        root: RootSpec {
            root_struct,
            entries,
            import_roots: vec![ImportRoot::new(ROOT_DIR)],
        },
        formats,
        limits,
    }
}

/// Evaluates a fixture and renders the value in its formats, or the
/// diagnostics.
fn run(text: &str) -> (bool, String) {
    let fixture = fixture(text);
    let checked = match check(&fixture.host, &fixture.root) {
        Ok(checked) => checked,
        Err(diagnostics) => panic!("the fixture does not check:\n{diagnostics}"),
    };
    match checked.eval(&fixture.limits) {
        Ok(evaluated) => {
            let mut out = String::new();
            for format in fixture.formats {
                match evaluated.render(format) {
                    Ok(text) => out.push_str(&format!("--- {format}\n{text}")),
                    Err(diagnostics) => {
                        out.push_str(&format!("--- {format} errors\n{diagnostics}"));
                    }
                }
            }
            (true, out)
        }
        Err(diagnostics) => {
            assert!(
                diagnostics.has_errors(),
                "a failed evaluation reports an error"
            );
            (false, format!("--- errors\n{diagnostics}"))
        }
    }
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/eval");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("the fixture directory exists")
        .map(|entry| entry.expect("the fixture directory is readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "stav"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "{} has no fixtures", dir.display());
    paths
}

fn name(path: &Path) -> &str {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .expect("the name is UTF-8")
}

#[test]
fn fixtures_evaluate_to_their_snapshots() {
    let mut unexpected = Vec::new();
    for path in fixtures() {
        let text = fs::read_to_string(&path).expect("the fixture is readable");
        let name = name(&path);
        let (ok, output) = run(&text);
        if ok == name.starts_with("err_") {
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
fn evaluating_twice_gives_identical_canonical_json() {
    for path in fixtures() {
        let text = fs::read_to_string(&path).expect("the fixture is readable");
        let first = canonical(&text);
        assert_eq!(
            first,
            canonical(&text),
            "{} is not deterministic",
            path.display()
        );
    }
}

fn canonical(text: &str) -> Result<Vec<u8>, String> {
    let fixture = fixture(text);
    let checked = check(&fixture.host, &fixture.root).map_err(|d| d.to_string())?;
    checked
        .eval(&fixture.limits)
        .map(|e| e.to_canonical_json())
        .map_err(|d| d.to_string())
}

fn evaluate(text: &str) -> stav::Evaluated {
    let fixture = fixture(text);
    let checked = check(&fixture.host, &fixture.root).expect("the fixture checks");
    checked
        .eval(&fixture.limits)
        .expect("the fixture evaluates")
}

/// Returns JSON data with every number as a float, because canonical JSON
/// writes a float with an integral value as an integer.
fn numeric(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(n) => n
            .as_f64()
            .map_or(serde_json::Value::Null, serde_json::Value::from),
        serde_json::Value::Object(map) => {
            serde_json::Value::Object(map.into_iter().map(|(k, v)| (k, numeric(v))).collect())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(numeric).collect())
        }
        other => other,
    }
}

/// Returns JSON data without the object entries that are `null`, which
/// TOML leaves out.
fn without_nulls(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, without_nulls(v)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(without_nulls).collect())
        }
        other => other,
    }
}

fn yaml_to_json(yaml: &saphyr::Yaml<'_>) -> serde_json::Value {
    use saphyr::{Scalar, Yaml};
    match yaml {
        Yaml::Value(Scalar::Null) => serde_json::Value::Null,
        Yaml::Value(Scalar::Boolean(b)) => serde_json::Value::Bool(*b),
        Yaml::Value(Scalar::Integer(i)) => serde_json::Value::from(*i),
        Yaml::Value(Scalar::FloatingPoint(f)) => serde_json::Value::from(f.into_inner()),
        Yaml::Value(Scalar::String(s)) => serde_json::Value::String(s.to_string()),
        Yaml::Sequence(items) => serde_json::Value::Array(items.iter().map(yaml_to_json).collect()),
        Yaml::Mapping(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let key = match yaml_to_json(k) {
                        serde_json::Value::String(s) => s,
                        other => other.to_string(),
                    };
                    (key, yaml_to_json(v))
                })
                .collect(),
        ),
        other => panic!("unexpected YAML node {other:?}"),
    }
}

#[test]
fn renderings_parse_back_to_the_same_data() {
    use saphyr::LoadableYamlNode;
    for path in fixtures() {
        if name(&path).starts_with("err_") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("the fixture is readable");
        let evaluated = evaluate(&text);
        let expected = numeric(
            serde_json::from_slice(&evaluated.to_canonical_json()).expect("canonical JSON parses"),
        );
        let json = numeric(
            serde_json::from_str(&evaluated.render(Format::Json).expect("JSON renders"))
                .expect("JSON parses"),
        );
        assert_eq!(json, expected, "JSON of {}", path.display());
        let yaml = evaluated.render(Format::Yaml).expect("YAML renders");
        let documents = saphyr::Yaml::load_from_str(&yaml).expect("YAML parses");
        assert_eq!(
            numeric(yaml_to_json(&documents[0])),
            expected,
            "YAML of {}:\n{yaml}",
            path.display()
        );
        if let Ok(toml) = evaluated.render(Format::Toml) {
            let parsed = numeric(toml::from_str(&toml).expect("TOML parses"));
            assert_eq!(
                parsed,
                without_nulls(expected),
                "TOML of {}:\n{toml}",
                path.display()
            );
        }
    }
}

#[test]
fn values_convert_to_host_types_through_serde() {
    use std::collections::BTreeMap;

    use serde::Deserialize;

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Build {
        Cargo { features: Vec<String> },
        Make { target: String },
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Job {
        build: Build,
        timeout: u32,
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/eval/example_tagged_blocks.stav");
    let evaluated = evaluate(&fs::read_to_string(path).expect("the fixture is readable"));
    let jobs: BTreeMap<String, Job> = evaluated
        .get(&stav::AttrPath::new(["jobs"]))
        .expect("the jobs convert");
    assert_eq!(
        jobs["rg"],
        Job {
            build: Build::Cargo {
                features: vec!["pcre2".to_owned()]
            },
            timeout: 600
        }
    );
    assert_eq!(
        jobs["hello"].build,
        Build::Make {
            target: "all".to_owned()
        }
    );
    let error = evaluated
        .get::<u8>(&stav::AttrPath::new(["jobs", "rg", "timeout"]))
        .expect_err("600 does not fit a byte");
    assert_eq!(
        error.iter().next().map(|d| d.diagnostic.code),
        Some("E0805")
    );
    assert!(
        evaluated
            .get::<u32>(&stav::AttrPath::new(["jobs", "missing"]))
            .is_err()
    );
}

#[test]
fn spans_and_symbols_point_at_the_source() {
    let text = "#! symbol Pkg pkg words\n#! entries a.stav b.stav\n#- file a.stav\n\
                struct Root { port: Int = 80, packages: List<Pkg> = [] }\nport = default(8080)\n\
                packages = [Vim]\n#- file b.stav\nport = 443\n";
    let evaluated = evaluate(text);
    let spans = evaluated.spans(&stav::AttrPath::new(["port"]));
    let texts: Vec<(usize, Span)> = spans.iter().map(|s| (s.file.index(), s.span)).collect();
    assert_eq!(texts.len(), 3, "{texts:?}");
    let first = spans[0];
    assert_eq!(
        evaluated
            .diagnostics([])
            .source(first.file)
            .map(|s| &s[first.span.range()]),
        Some("443")
    );
    let symbols: Vec<&str> = evaluated.symbols("Pkg").map(|(text, _)| text).collect();
    assert_eq!(symbols, ["vim"]);
}

#[test]
fn deeply_nested_evaluation_stops_without_overflowing_the_stack() {
    let mut functions = String::from("fn f0(x: Int) -> Int = x + 1\n");
    for i in 1..2000 {
        functions.push_str(&format!("fn f{i}(x: Int) -> Int = f{}(x) + 1\n", i - 1));
    }
    let deep = format!("struct Root {{ n: Int = 0 }}\n{functions}n = f1999(0)\n");
    let data = "struct Root { d: Data = none }\n\
                d = fold(range(0, 1000), [], fn(acc: Data, i: Int) -> Data = [acc])\n"
        .to_owned();
    for text in [deep, data] {
        let result = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let fixture = fixture(&text);
                let checked = check(&fixture.host, &fixture.root).expect("the program checks");
                checked
                    .eval(&Limits::default())
                    .map(|_| ())
                    .map_err(|d| d.to_string())
            })
            .expect("the thread starts")
            .join()
            .expect("evaluation does not overflow the stack");
        let error = result.expect_err("evaluation stops");
        assert!(
            error.contains("E0611") || error.contains("E0612"),
            "{error}"
        );
    }
}

mod random {
    use proptest::prelude::*;
    use proptest::sample::select;

    use super::*;

    /// Integer expressions over the given names.
    fn int(depth: u32, names: &'static [&'static str]) -> BoxedStrategy<String> {
        let leaf = prop_oneof![
            (0i64..100).prop_map(|i| i.to_string()),
            select(names.to_vec()).prop_map(str::to_owned),
            Just("len(\"abc\")".to_owned()),
        ];
        if depth == 0 {
            return leaf.boxed();
        }
        let inner = || int(depth - 1, names);
        prop_oneof![
            leaf,
            (inner(), select(vec!["+", "-", "*", "/", "%"]), inner())
                .prop_map(|(a, op, b)| format!("({a} {op} {b})")),
            (inner(), inner(), inner())
                .prop_map(|(c, a, b)| format!("(if {c} > 10 {{ {a} }} else {{ {b} }})")),
            (inner(), inner()).prop_map(|(n, v)| format!(
                "fold(range(0, {n}), {v}, fn(acc: Int, i: Int) = acc + i)"
            )),
            inner().prop_map(|v| format!("(let y = {v} in y * y)")),
        ]
        .boxed()
    }

    /// Names an eager expression may use.
    const EAGER: &[&str] = &["x", "1"];
    /// Names a deferred expression may use as well.
    const LATE: &[&str] = &["x", "self.bounded"];

    fn text(depth: u32) -> BoxedStrategy<String> {
        prop_oneof![
            select(vec!["\"a\"", "\"b c\"", "\"\"", "self.s"]).prop_map(str::to_owned),
            int(depth, LATE).prop_map(|i| format!("\"n#{{{i}}}\"")),
            (int(depth, EAGER), int(depth, LATE))
                .prop_map(|(a, b)| format!("join([toStr({a}), toStr({b})], \"-\")")),
        ]
        .boxed()
    }

    fn statement() -> impl Strategy<Value = String> {
        let prio = select(vec!["", "default", "force"]);
        prop_oneof![
            (prio.clone(), int(2, LATE)).prop_map(|(p, v)| format!("n = {p}({v})")),
            (prio, int(1, EAGER)).prop_map(|(p, v)| format!("s = {p}(\"#{{{v}}}\")")),
            (int(1, EAGER), text(1)).prop_map(|(a, b)| format!("tags = [toStr({a}), {b}]")),
            int(1, LATE).prop_map(|v| format!("list = [{v}]")),
            (select(vec!["a", "b", "c"]), int(1, EAGER), text(1))
                .prop_map(|(k, v, w)| format!("item {k} {{ v = {v}\n w = {w} }}")),
            (select(vec!["a", "b"]), int(1, EAGER))
                .prop_map(|(k, v)| format!("items.{k}.v = force({v})")),
            (int(1, EAGER), int(1, LATE))
                .prop_map(|(c, v)| format!("if {c} > 5 {{ list = [{v}] }}")),
            (0u8..4).prop_map(|k| format!("for i in range(0, {k}) {{ tags = [\"#{{i}}\"] }}")),
            int(1, EAGER).prop_map(|v| format!("include part {{ list = [{v}] }}")),
            int(1, EAGER).prop_map(|v| format!("bounded = {v}")),
        ]
    }

    fn program() -> impl Strategy<Value = String> {
        (prop::collection::vec(statement(), 0..8), int(1, &["1"])).prop_map(|(statements, x)| {
            format!(
                "#! steps 20000\nstruct Item {{ v: Int = 0, w: Str = \"#{{self.v}}\" }}\n\
                 struct Root {{\n  n: Int = 0\n  s: Str = \"\"\n  tags: Set<Str> = []\n  \
                 list: List<Int> = []\n  items: Map<Str, Item> = {{}} block item\n  \
                 bounded: Int where 0..100 = 1\n}}\nlet x = {x}\n{}\n",
                statements.join("\n")
            )
        })
    }

    fn outcome(text: &str) -> Option<String> {
        let fixture = fixture(text);
        let checked = check(&fixture.host, &fixture.root).ok()?;
        Some(match checked.eval(&fixture.limits) {
            Ok(evaluated) => String::from_utf8_lossy(&evaluated.to_canonical_json()).into_owned(),
            Err(diagnostics) => diagnostics.to_string(),
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn evaluation_never_panics_and_is_deterministic(text in program()) {
            let first = outcome(&text);
            prop_assert_eq!(&first, &outcome(&text));
        }

        #[test]
        fn merging_does_not_depend_on_the_order_of_entry_files(
            entries in prop::collection::vec(
                (prop::collection::vec("[a-c]{1,2}", 0..3), prop::collection::btree_map("[d-f]", 0i64..9, 0..3)),
                1..4,
            ),
            order in Just(()).prop_perturb(|_, mut rng| rng.random::<u64>()),
        ) {
            let files: Vec<String> = entries
                .iter()
                .enumerate()
                .map(|(i, (tags, counts))| {
                    let tags: Vec<String> = tags.iter().map(|t| format!("\"{t}\"")).collect();
                    let counts: Vec<String> = counts
                        .iter()
                        .map(|(k, v)| format!("counts.f{i}{k} = {v}"))
                        .collect();
                    format!("tags = [{}]\n{}\n", tags.join(", "), counts.join("\n"))
                })
                .collect();
            let source = |permutation: &[usize]| {
                let names: Vec<String> = permutation.iter().map(|i| format!("e{i}.stav")).collect();
                let mut text = format!("#! entries root.stav {}\n#- file root.stav\n\
                    struct Root {{ tags: Set<Str> = [], counts: Map<Str, Int> = {{}} }}\n", names.join(" "));
                for &i in permutation {
                    text.push_str(&format!("#- file e{i}.stav\n{}", files[i]));
                }
                text
            };
            let forward: Vec<usize> = (0..files.len()).collect();
            let mut shuffled = forward.clone();
            let mut seed = order;
            for i in (1..shuffled.len()).rev() {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let j = usize::try_from(seed >> 33).unwrap_or(0) % (i + 1);
                shuffled.swap(i, j);
            }
            prop_assert_eq!(outcome(&source(&forward)), outcome(&source(&shuffled)));
        }
    }
}

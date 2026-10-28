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

# Embedding in Rust

The `stav` crate is the whole language as a library. A Rust program can load, check and evaluate Stav programs and read their values straight into its own types, without rendering them to text first.

The library performs no I/O of its own. The program that embeds it, the **host**, hands it source files through a loader. The host can also add a prelude of schemas, standard modules, domain types and validation of its own.

## Setup

Add `stav` to your dependencies, and `serde` with its `derive` feature:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
stav = "0.1"
```

## Check, evaluate, read

Embedding takes four steps:

1. Build a `Host` around a `Loader`. `MemoryLoader` serves files from memory.
2. Describe the program with a `RootSpec`: the name of the root struct, the entry files in order, and the import roots that files may reference.
3. Call `stav::check` to load and type check the program, then `eval` on the result to evaluate it within `Limits`.
4. Read values with `get`, which converts the attribute at a path into any type that implements `serde::Deserialize`.

This program layers a production file over a base file. Both are entry files of the root struct `Config`, so their contributions merge as if they were one file:

```rust
use std::collections::BTreeMap;
use std::process::ExitCode;

use serde::Deserialize;
use stav::{AttrPath, Diagnostics, Format, Host, ImportRoot, Limits, MemoryLoader, RootSpec};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Deserialize)]
struct Upstream {
    url: String,
    weight: u8,
}

#[derive(Deserialize)]
struct Config {
    listen: String,
    log_level: Level,
    upstreams: BTreeMap<String, Upstream>,
}

const BASE: &str = r#"
type Level = debug | info | warn | error

struct Upstream {
  url:    Str where matches(r"https?://.*")
  weight: Int where 1..=100 = 10
}

struct Config {
  listen:    Str = "127.0.0.1:8080"
  log_level: Level = info
  upstreams: Map<Str, Upstream> = {} block upstream
}

upstream primary { url = "https://a.example.org", weight = 90 }
"#;

const PRODUCTION: &str = r#"
listen    = "0.0.0.0:443"
log_level = warn

upstream backup { url = "https://b.example.org" }
"#;

fn load() -> Result<Config, Diagnostics> {
    let loader = MemoryLoader::new()
        .with_file("config/base.stav", BASE)
        .with_file("config/production.stav", PRODUCTION);
    let host = Host::new(loader);
    let root = RootSpec {
        root_struct: "Config".to_owned(),
        entries: vec!["config/base.stav".into(), "config/production.stav".into()],
        import_roots: vec![ImportRoot::new("config")],
    };

    let checked = stav::check(&host, &root)?;
    for warning in checked.warnings().iter() {
        eprintln!("warning[{}]: {}", warning.diagnostic.code, warning.diagnostic.message);
    }
    let evaluated = checked.eval(&Limits::default())?;

    let weight: u8 = evaluated.get(&AttrPath::new(["upstreams", "primary", "weight"]))?;
    assert_eq!(weight, 90);
    print!("{}", evaluated.render(Format::Toml)?);

    evaluated.get(&AttrPath::root())
}

fn main() -> ExitCode {
    match load() {
        Ok(config) => {
            println!("listening on {} at level {:?}", config.listen, config.log_level);
            for (name, upstream) in &config.upstreams {
                println!("{name}: {} (weight {})", upstream.url, upstream.weight);
            }
            ExitCode::SUCCESS
        }
        Err(diagnostics) => {
            eprint!("{diagnostics}");
            ExitCode::FAILURE
        }
    }
}
```

It prints the value as TOML and then the fields it read:

```text
listen = "0.0.0.0:443"
log_level = "warn"

[upstreams.backup]
url = "https://b.example.org"
weight = 10

[upstreams.primary]
url = "https://a.example.org"
weight = 90
listening on 0.0.0.0:443 at level Warn
backup: https://b.example.org (weight 10)
primary: https://a.example.org (weight 90)
```

`get` reads a value as if it were deserialized from its JSON rendering:

| Stav | Rust |
| --- | --- |
| `Bool`, `Int`, `Float`, `Str` | `bool`, `i64` or a smaller integer (range checked), `f64`, `String` |
| `List<T>`, `Set<T>` | `Vec<T>`, `BTreeSet<T>` |
| `Map<Str, V>` | `BTreeMap<String, V>` |
| struct | a struct with `#[derive(Deserialize)]` |
| sum type without payloads | an `enum` with unit variants |
| sum type with payloads | an externally tagged `enum` |
| `Option<T>` | `Option<T>` |
| `Ref<T>`, `Path`, symbol types | `String`, or a type of your own that deserializes from a string |
| `Data` | any type that matches its shape |

`AttrPath::root()` names the whole value, and `AttrPath::new(["upstreams", "primary"])` names an attribute inside it. An element of a list or set is named by its position, counted from zero, so `AttrPath::new(["servers", "0"])` is the first element of `servers`. A path that names no attribute, or a value that does not convert, is the error `E0805`.

`Evaluated` can also render the value with `render(Format::Json)`, `Format::Toml` or `Format::Yaml`, give its canonical JSON with `to_canonical_json`, or hand out the `Value` tree itself with `value()`.

## Diagnostics

`check` and `eval` return `Diagnostics` when they fail. It holds every error found, each with its code, message, location, notes and help, together with the names and texts of the files they refer to. Its `Display` form lists them with file, line and column. With `weight = 900` in the production file, the program above prints:

```text
error[E0803]: value violates a refinement
  --> config/production.stav:5:59
  label: config/production.stav:5:59: 900 is not in 1..=100
  label: config/base.stav:6:11: constraint declared here
```

Warnings do not stop a program. `Checked::warnings` returns them after a successful check. Evaluation can still fail after that, and its errors come without the warnings. `Checked::diagnostics` builds one `Diagnostics` from any list of diagnostics of the program, so a host can report every warning together with the errors of evaluation or of its own validation:

```rust
let checked = stav::check(&host, &root)?;
let evaluated = match checked.eval(&Limits::default()) {
    Ok(evaluated) => evaluated,
    Err(errors) => {
        let all = checked.warnings().iter().chain(errors.iter()).cloned();
        return Err(checked.diagnostics(all));
    }
};
```

A host can report problems of its own the same way. `Diagnostic::error` and `Diagnostic::warning` take any code, and `with_label`, `with_note` and `with_help` add the rest. A `FileDiagnostic` pairs a diagnostic with the file its span points into, as given by `Checked::files`, `Diagnostics::files` or the `file` of a `SourceSpan`. `Checked::diagnostics`, `Evaluated::diagnostics` and `Diagnostics::with_items` turn such lists into `Diagnostics` that render exactly like those of Stav.

`Diagnostics::with_items` also replaces diagnostics of Stav. A host that wants its own code and wording for an import that leaves its import root, [`E0702`](../reference/error-codes#e0702), maps the errors of a failed check:

```rust
let errors = stav::check(&host, &root).unwrap_err();
let ours = errors.with_items(errors.iter().map(|item| match item.diagnostic.code {
    "E0702" => FileDiagnostic {
        file: item.file,
        diagnostic: Diagnostic::error("H0002", item.diagnostic.span, "imports stay inside the project")
            .with_help("move the file into the project"),
    },
    _ => item.clone(),
}));
``` [Diagnostics](../reference/diagnostics) describes the parts of a diagnostic, and [Error codes](../reference/error-codes) lists every code.

## Reading files from disk

`MemoryLoader` suits tests and programs that embed their sources. To read from disk, implement `Loader`. It has two methods: `std_module` returns the source of `std/<name>`, and `read` returns a file inside an import root.

The library normalizes paths and refuses paths that leave their import root before it asks the loader. A loader that reads from disk should also resolve symbolic links and refuse files whose real path leaves the root:

```rust
use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

use stav::{Host, ImportRoot, Limits, LoadError, Loader, MAX_SOURCE_LEN, RelPath, RootSpec};

/// Reads source files from disk and serves no standard modules.
struct DiskLoader;

impl Loader for DiskLoader {
    fn std_module(&self, _name: &str) -> Option<Cow<'static, str>> {
        None
    }

    fn read(&self, root: &ImportRoot, rel: &RelPath) -> Result<String, LoadError> {
        let base = fs::canonicalize(root.path()).map_err(|e| LoadError::Other(e.to_string()))?;
        let path = fs::canonicalize(base.join(rel.to_path())).map_err(|_| LoadError::NotFound)?;
        if !path.starts_with(&base) {
            return Err(LoadError::OutsideRoot);
        }
        let metadata = fs::metadata(&path).map_err(|e| LoadError::Other(e.to_string()))?;
        if !metadata.is_file() {
            return Err(LoadError::NotAFile);
        }
        if metadata.len() > MAX_SOURCE_LEN as u64 {
            return Err(LoadError::TooLarge);
        }
        fs::read_to_string(&path).map_err(|e| LoadError::Other(e.to_string()))
    }
}

fn main() {
    let dir = PathBuf::from("config");
    let root = RootSpec {
        root_struct: "Root".to_owned(),
        entries: vec![dir.join("main.stav")],
        import_roots: vec![ImportRoot::new(&dir)],
    };
    let result = stav::check(&Host::new(DiskLoader), &root)
        .and_then(|checked| checked.eval(&Limits::default()));
    match result {
        Ok(evaluated) => println!("{}", evaluated.to_canonical_json()),
        Err(diagnostics) => eprint!("{diagnostics}"),
    }
}
```

## What a host can add

`Host::new` returns a host with nothing but a loader. Its public fields add the rest:

| Field | Purpose |
| --- | --- |
| `prelude` | Files whose declarations are in scope, unqualified, in every file of the program. A file that declares a name the prelude already declares is an error. |
| `implicit_parts` | Part files included at the start of the root body, before every entry file. They suit defaults that every program gets. |
| `symbols` | Domain types such as package names, whose values the host validates after evaluation. |
| `host_modules` | Modules whose source the host generates for one evaluation, imported as `"<namespace>/<module>"`. |
| `implicit_refs` | Labels that `Ref<T>` values may name although no block declares them, derived from the merged value. |
| `loader` | Reads every other source file, and serves `std/<name>`. |

The program below uses most of them. The prelude declares the schema, so the entry file only fills it in. An implicit part sets defaults. `Pkg` is a symbol type, so bare names and symbol words such as `compiler@19.1:static` are package literals where a `Pkg` is expected, and `pkg("2fa")` converts any string. The host module `build/target` tells the program which architecture it is evaluated for. After evaluation, the host checks every package against a list it knows and reports unknown ones as diagnostics that point at the source:

```rust
use std::process::ExitCode;

use stav::{
    Diagnostic, Diagnostics, FileDiagnostic, Format, Host, HostModule, ImportRoot, Limits,
    MemoryLoader, RootSpec, Source, SymbolType,
};

const PRELUDE: &str = r#"
type Arch = x86_64 | aarch64

struct Image {
  arch:     Arch
  packages: Set<Pkg> = []
  hostname: Str
}

struct Root { image: Image }
"#;

const DEFAULTS: &str = r#"
image {
  hostname = default("localhost")
  packages = [coreutils]
}
"#;

const MAIN: &str = r#"
import "build/target" as target

image {
  arch     = target.arch
  hostname = "builder"
  packages = [compiler@19.1:static, editor, pkg("2fa"), ripgrep]
}
"#;

const KNOWN: [&str; 4] = ["coreutils", "compiler@19.1:static", "editor", "2fa"];

/// Accepts names of letters, digits and `-`, with an optional version
/// after `@` and variant after `:`.
fn parse_pkg(text: &str) -> Result<String, String> {
    let name = text.split(['@', ':']).next().unwrap_or("");
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("`{name}` is not a package name"));
    }
    Ok(text.to_owned())
}

fn build(arch: &str) -> Result<String, Diagnostics> {
    let mut host = Host::new(MemoryLoader::new().with_file("image/main.stav", MAIN));
    host.prelude.push(Source::new("prelude/image.stav", PRELUDE));
    host.implicit_parts.push(Source::new("parts/defaults.stav", DEFAULTS));
    host.symbols.push(SymbolType {
        name: "Pkg".to_owned(),
        words: true,
        strings: false,
        constructor: "pkg".to_owned(),
        parse: parse_pkg,
    });
    host.host_modules.push(HostModule {
        path: "build/target".to_owned(),
        source: format!("let arch: Arch = {arch}\n"),
    });

    let root = RootSpec {
        root_struct: "Root".to_owned(),
        entries: vec!["image/main.stav".into()],
        import_roots: vec![ImportRoot::new("image")],
    };
    let evaluated = stav::check(&host, &root)?.eval(&Limits::default())?;

    let unknown: Vec<FileDiagnostic> = evaluated
        .symbol_paths("Pkg")
        .filter(|(name, _, _)| !KNOWN.contains(name))
        .map(|(name, path, at)| FileDiagnostic {
            file: Some(at.file),
            diagnostic: Diagnostic::error("H0001", at.span, format!("`{path}`: unknown package `{name}`")),
        })
        .collect();
    if !unknown.is_empty() {
        return Err(evaluated.diagnostics(unknown));
    }
    evaluated.render(Format::Json)
}

fn main() -> ExitCode {
    match build("aarch64") {
        Ok(json) => {
            print!("{json}");
            ExitCode::SUCCESS
        }
        Err(diagnostics) => {
            eprint!("{diagnostics}");
            ExitCode::FAILURE
        }
    }
}
```

It prints:

```text
error[H0001]: `image.packages.4`: unknown package `ripgrep`
  --> image/main.stav:7:57
```

Without `ripgrep`, it prints the image:

```json
{
  "image": {
    "arch": "aarch64",
    "packages": [
      "2fa",
      "compiler@19.1:static",
      "coreutils",
      "editor"
    ],
    "hostname": "builder"
  }
}
```

A few details are worth knowing:

- **Symbol types.** `words` allows symbol words such as `name@1.2:variant`, and `strings` allows string literals. The `parse` function checks the text of every literal when it is evaluated and returns its canonical text, or a message that becomes the error `E0613`. Stav never checks whether a symbol exists. `Evaluated::symbols` returns every symbol of a type with the place that wrote it, for the host to validate, and `Evaluated::symbol_paths` adds the path of the value that holds it, such as `image.packages.4`, for the host's messages.
- **Host modules** keep evaluation pure. Their source is an input of the evaluation exactly like a file, so the same files with the same host module sources always give the same value.
- **Host values.** A top-level `let` of a host module with a type annotation, such as `let arch: Arch = x86_64`, is an input of the program. The program is checked once against the annotation, and `Checked::eval_with` takes a `HostValues` that supplies a different `Value` for it in each evaluation. Where an evaluation supplies none, the value written in the module is used. A host that builds for several targets checks once and evaluates once per target. A value that names no such `let`, or does not fit its type, is [`E0614`](../reference/error-codes#e0614), and one that violates a refinement is [`E0803`](../reference/error-codes#e0803). The values are inputs of the evaluation like the source files, so a cache must key on them too.

```rust
let checked = stav::check(&host, &root)?;
for arch in ["x86_64", "aarch64"] {
    let values = HostValues::new().with("build/target", "arch", Value::Str(arch.to_owned()));
    let evaluated = checked.eval_with(&Limits::default(), &values)?;
    print!("{}", evaluated.render(Format::Json)?);
}
```
- **Host validation.** Rules that relate several fields, or that need outside knowledge such as a package index, belong to the host. `Evaluated::spans` returns where each attribute was written, down to the entry of a struct or map literal and the element of a list. `Evaluated::priority` says whether the winning value was set with `force(...)`, `default(...)`, a plain assignment or only the declared default, for rules that treat a forced value differently. `Evaluated::diagnostics` wraps the host's own diagnostics so that they render exactly like those of Stav. Choose codes that cannot be confused with the `E` and `W` codes of the language.
- **Caching.** `Checked::inputs` lists every source text the program was read from. A host can key a cache of evaluations by hashing them together with the host values it passes.
- **Limits.** `Limits::default()` allows 50 000 000 evaluation steps and 512 MiB of values. A host can raise or lower them for each evaluation. [Totality and limits](../reference/totality) explains how they are counted.

## The API documentation

The full API, with every type and method, is on [docs.rs](https://docs.rs/stav).

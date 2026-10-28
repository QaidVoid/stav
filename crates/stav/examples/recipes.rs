//! Generates a root of 2 000 recipe-like entry files of about 40 lines
//! each, then checks and evaluates it and prints how long each step took.
//!
//! Run it with `cargo run --release --example recipes`.

use std::fmt::Write as _;
use std::io::Write as _;
use std::time::Instant;

use stav::{Host, ImportRoot, Limits, MemoryLoader, RootSpec, Source, SymbolType};

const RECIPES: usize = 2_000;

const PRELUDE: &str = r#"
type Build =
  | native(features: List<Str> = [], bins: List<Str> = [])
  | configured(options: List<Str> = [])
  | custom(build: Str, install: Str)

struct Source {
  url:    Str where matches(r"https://.*")
  sha256: Str where 64..=64
}

struct Package {
  version:  Str where nonEmpty
  summary:  Str
  license:  Set<License> = []
  source:   Source
  build:    Build
  deps:     List<Ref<Package>> = []
  tools:    Set<Pkg> = []
  env:      Map<Str, Str> = {}
  install:  Str = ""
  jobs:     Int where 1..=64 = 4
  url:      Str = "https://example.org/#{self.version}"
}

struct Root { packages: Map<Str, Package> = {} block package }
"#;

fn accept(text: &str) -> Result<String, String> {
    Ok(text.to_owned())
}

fn recipe(i: usize) -> String {
    let mut out = String::new();
    let dep = |k: usize| format!("pkg{}", k % RECIPES);
    let _ = write!(
        out,
        r##"# Recipe {i}
let base = "pkg{i}"
let mirrors = {{ a = "https://a.example.org", b = "https://b.example.org" }}

package {base} {{
  version = "{major}.{minor}.{patch}"
  summary = "Package number {i}, generated for the benchmark"
  license = [MIT, Apache-2.0]
  source {{
    url    = "https://example.org/#{{base}}/#{{base}}-#{{self.version}}.tar.gz"
    sha256 = "{hash}"
  }}
  build {kind} {{
    {payload}
  }}
  deps  = [{d1}, {d2}, {d3}]
  tools = [compiler, builder@4.4, pkgtool:static]
  env {{
    CFLAGS  = "-O2 -pipe"
    LDFLAGS = "-Wl,--as-needed"
    NAME    = base
  }}
  for name, m in mirrors {{
    env = {{ "MIRROR_#{{upper(name)}}" = m }}
  }}
  jobs = default(if {i} % 2 == 0 {{ 8 }} else {{ 2 }})
  summary = default("no summary")
  if {i} % 5 == 0 {{
    deps = [{d4}]
    env {{ STATIC = "1" }}
  }}
  install = """
    install -Dm755 build/#{{base}} "$DESTDIR/usr/bin/#{{base}}"
    install -Dm644 README.md "$DESTDIR/usr/share/doc/#{{base}}/README"
    for f in docs/*.md; do
      install -Dm644 "$f" "$DESTDIR/usr/share/doc/#{{base}}/$(basename "$f")"
    done
    printf 'installed %s\n' "#{{self.version}}"
    """
}}
"##,
        base = format_args!("pkg{i}"),
        major = i % 7,
        minor = i % 13,
        patch = i % 5,
        hash = format_args!("{:064x}", i * 7919),
        kind = ["native", "configured", "custom"][i % 3],
        payload = [
            r#"features = ["a", "b"], bins = ["tool"]"#,
            r#"options = ["-Ddocs=false", "-Dtests=false"]"#,
            r#"build = "build -j#{4}", install = "build install""#,
        ][i % 3],
        d1 = dep(i + 1),
        d2 = dep(i + 7),
        d3 = dep(i * 3 + 11),
        d4 = dep(i + 100),
    );
    out
}

fn main() {
    let mut loader = MemoryLoader::new();
    let mut entries = Vec::with_capacity(RECIPES);
    let mut lines = 0;
    for i in 0..RECIPES {
        let text = recipe(i);
        lines += text.lines().count();
        let path = format!("repo/recipes/pkg{i}/package.stav");
        loader = loader.with_file(&path, text);
        entries.push(path.into());
    }
    let mut host = Host::new(loader);
    host.prelude = vec![Source::new("prelude/package.stav", PRELUDE)];
    host.symbols = ["Pkg", "License"]
        .iter()
        .map(|name| SymbolType {
            name: (*name).to_owned(),
            words: true,
            strings: true,
            constructor: name.to_lowercase(),
            parse: accept,
        })
        .collect();
    let root = RootSpec {
        root_struct: "Root".to_owned(),
        entries,
        import_roots: vec![ImportRoot::new("repo")],
    };
    let start = Instant::now();
    let checked = match stav::check(&host, &root) {
        Ok(checked) => checked,
        Err(diagnostics) => {
            eprintln!("{diagnostics}");
            std::process::exit(1);
        }
    };
    let checked_at = start.elapsed();
    let evaluated = match checked.eval(&Limits::default()) {
        Ok(evaluated) => evaluated,
        Err(diagnostics) => {
            eprintln!("{diagnostics}");
            std::process::exit(1);
        }
    };
    let total = start.elapsed();
    let json = evaluated.to_canonical_json();
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(
        stdout,
        "{RECIPES} recipes, {lines} lines: check {:.0} ms, eval {:.0} ms, total {:.0} ms, {} bytes of canonical JSON",
        checked_at.as_secs_f64() * 1000.0,
        (total - checked_at).as_secs_f64() * 1000.0,
        total.as_secs_f64() * 1000.0,
        json.len()
    );
}

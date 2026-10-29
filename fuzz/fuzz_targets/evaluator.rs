//! Checks arbitrary UTF-8 as a program and evaluates it when it checks,
//! with small limits. Evaluation must never panic and always terminate,
//! every diagnostic must point into its file, every value must render,
//! and evaluating twice must give the same result.
//!
//! The input is split at lines `#-` into the entry file, a file the entry
//! may import or include as `./lib.stav`, and the module `std/lib`. Unless
//! the entry file declares its own `struct Root`, a fixed schema is put
//! before it, so that bare statements evaluate against it.

#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use stav::{
    AttrPath, Checked, Format, Host, ImportRoot, Limits, MemoryLoader, RootSpec, SymbolType,
};

const SCHEMA: &str = "type Level = debug | info | warn\n\
    type Build = make(target: Str = \"all\") | cargo(features: List<Str> = [])\n\
    struct Item {\n  v: Int = 0\n  w: Str = \"#{self.v}\"\n  tags: Set<Str> = []\n  \
    level: Level = info\n  build: Build = make()\n  after: List<Ref<Item>> = []\n}\n\
    struct Root {\n  n: Int = 0\n  s: Str = \"\"\n  d: Data = none\n  list: List<Int> = []\n  \
    items: Map<Str, Item> = {} block item\n  opt: Option<Item>\n  pkgs: Set<Pkg> = []\n  \
    f: Float where 0.0..=1.0 = 0.5\n}\n";

fn parse(text: &str) -> Result<String, String> {
    if text.is_empty() || text.contains(' ') {
        return Err("a symbol is not empty and has no spaces".to_owned());
    }
    Ok(text.to_owned())
}

fn host(input: &str) -> Host {
    let mut parts = input.splitn(3, "\n#-\n");
    let main = parts.next().unwrap_or_default();
    let lib = parts.next().unwrap_or_default();
    let module = parts.next().unwrap_or_default();
    let main = if main.contains("struct Root") {
        main.to_owned()
    } else {
        format!("{SCHEMA}{main}")
    };
    let loader = MemoryLoader::new()
        .with_file("app/main.stav", main)
        .with_file("app/lib.stav", lib)
        .with_std("lib", module);
    let mut host = Host::new(loader);
    host.symbols = vec![SymbolType {
        name: "Pkg".to_owned(),
        words: true,
        strings: true,
        constructor: "pkg".to_owned(),
        parse,
    }];
    host
}

fn run(checked: &Checked, limits: &Limits) -> String {
    match checked.eval(limits) {
        Ok(evaluated) => {
            let mut out = evaluated.to_canonical_json();
            for format in [Format::Json, Format::Toml, Format::Yaml] {
                match evaluated.render(format) {
                    Ok(text) => out.push_str(&text),
                    Err(errors) => {
                        assert!(format == Format::Toml, "only TOML may fail to render");
                        out.push_str(&errors.to_string());
                    }
                }
            }
            for key in [
                "n",
                "items",
                "list",
                "opt",
                "list.0",
                "pkgs.1",
                "d.a.0",
                "items.a.tags.0",
            ] {
                let path: AttrPath = key.parse().expect("the path parses");
                let _ = evaluated.spans(&path);
                let _ = evaluated.priority(&path);
                let _ = evaluated.value().at(&path);
            }
            for (text, path, _) in evaluated.symbol_paths("Pkg") {
                assert!(evaluated.value().at(path).is_some());
                out.push_str(&format!("{path} {text}"));
            }
            out
        }
        Err(errors) => {
            assert!(errors.has_errors());
            for item in errors.iter() {
                if let Some(file) = item.file {
                    let source = errors.source(file).expect("the file of an error exists");
                    assert!(source.get(item.diagnostic.span.range()).is_some());
                    for label in &item.diagnostic.labels {
                        let file = label.file.unwrap_or(file);
                        let source = errors.source(file).expect("the file of a label exists");
                        assert!(source.get(label.span.range()).is_some());
                    }
                }
            }
            errors.to_string()
        }
    }
}

fuzz_target!(|input: &str| {
    let host = host(input);
    let root = RootSpec {
        root_struct: "Root".to_owned(),
        entries: vec!["app/main.stav".into()],
        import_roots: vec![ImportRoot::new("app")],
    };
    let Ok(checked) = stav::check(&host, &root) else {
        return;
    };
    let limits = Limits {
        steps: 200_000,
        value_bytes: 16 * 1024 * 1024,
        string_bytes: 1024 * 1024,
        collection_len: 100_000,
    };
    let first = run(&checked, &limits);
    assert_eq!(first, run(&checked, &limits));
});

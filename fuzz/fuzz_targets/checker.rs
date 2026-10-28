//! Checks arbitrary UTF-8 as a program of several files and checks that
//! checking never panics, that every diagnostic points into its file, and
//! that checking twice gives the same result.
//!
//! The input is split at lines `#-` into the entry file, a file that the
//! entry may import or include as `./lib.stav`, the module `std/lib` and
//! host module `ns/lib`, and a prelude file.

#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use stav::{Host, HostModule, ImportRoot, MemoryLoader, RootSpec, Source, SymbolType};

fn accept(text: &str) -> Result<String, String> {
    Ok(text.to_owned())
}

fn host(input: &str) -> Host {
    let mut parts = input.splitn(4, "\n#-\n");
    let main = parts.next().unwrap_or_default();
    let lib = parts.next().unwrap_or_default();
    let module = parts.next().unwrap_or_default();
    let prelude = parts.next();
    let loader = MemoryLoader::new()
        .with_file("app/main.stav", main)
        .with_file("app/lib.stav", lib)
        .with_std("lib", module);
    let mut host = Host::new(loader);
    host.prelude = prelude
        .map(|text| vec![Source::new("prelude.stav", text)])
        .unwrap_or_default();
    host.host_modules = vec![HostModule {
        path: "ns/lib".to_owned(),
        source: module.to_owned(),
    }];
    host.symbols = vec![SymbolType {
        name: "Pkg".to_owned(),
        words: true,
        strings: false,
        constructor: "pkg".to_owned(),
        parse: accept,
    }];
    host
}

fn run(host: &Host, root: &RootSpec) -> String {
    match stav::check(host, root) {
        Ok(checked) => {
            let _ = checked.schema();
            let warnings = checked.warnings();
            for item in warnings.iter() {
                if let Some(file) = item.file {
                    let source = warnings.source(file).expect("the file of a warning exists");
                    assert!(source.get(item.diagnostic.span.range()).is_some());
                }
            }
            format!("{}{warnings}", checked.summary())
        }
        Err(errors) => {
            assert!(errors.has_errors());
            for item in errors.iter() {
                if let Some(file) = item.file {
                    let source = errors.source(file).expect("the file of an error exists");
                    assert!(source.get(item.diagnostic.span.range()).is_some());
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
    let first = run(&host, &root);
    assert_eq!(first, run(&host, &root));
});

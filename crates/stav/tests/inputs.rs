//! Checks that a checked program lists the sources it was read from.

use std::path::PathBuf;

use stav::{Host, HostModule, ImportRoot, MemoryLoader, RootSpec, check};

fn inputs(variant: &str) -> Vec<(PathBuf, [u8; 32])> {
    let loader = MemoryLoader::new()
        .with_file(
            "app/main.stav",
            "import \"./lib.stav\" as lib\nimport \"ns/target\" as target\n\
             struct Root { a: Int = lib.x }\ninclude \"./part.stav\"\n",
        )
        .with_file("app/lib.stav", "let x = 1\n")
        .with_file("app/part.stav", "a = 2\n");
    let mut host = Host::new(loader);
    host.host_modules = vec![HostModule {
        path: "ns/target".to_owned(),
        source: format!("let variant = \"{variant}\"\n"),
    }];
    let root = RootSpec {
        root_struct: "Root".to_owned(),
        entries: vec!["app/main.stav".into()],
        import_roots: vec![ImportRoot::new("app")],
    };
    check(&host, &root)
        .expect("the program checks")
        .inputs()
        .to_vec()
}

#[test]
fn inputs_list_every_source_once_with_its_hash() {
    let found = inputs("static");
    let mut names: Vec<String> = found
        .iter()
        .map(|(path, _)| path.display().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "app/lib.stav",
            "app/main.stav",
            "app/part.stav",
            "ns/target"
        ]
    );
    assert_eq!(found, inputs("static"));
}

#[test]
fn host_module_sources_change_their_hash() {
    let hash = |found: &[(PathBuf, [u8; 32])]| {
        found
            .iter()
            .find(|(path, _)| path == &PathBuf::from("ns/target"))
            .map(|(_, hash)| *hash)
    };
    assert_ne!(hash(&inputs("static")), hash(&inputs("dynamic")));
}

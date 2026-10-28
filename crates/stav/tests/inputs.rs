//! Checks that a checked program lists the sources it was read from.

use stav::{Host, HostModule, ImportRoot, MemoryLoader, RootSpec, check};

fn inputs(variant: &str) -> Vec<(String, String)> {
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
        .map(|(path, text)| (path.display().to_string(), text.to_owned()))
        .collect()
}

#[test]
fn inputs_list_every_source_once_by_path_with_its_text() {
    let found = inputs("static");
    let names: Vec<&str> = found.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        names,
        [
            "app/lib.stav",
            "app/main.stav",
            "app/part.stav",
            "ns/target"
        ]
    );
    assert_eq!(found[0].1, "let x = 1\n");
    assert_eq!(found, inputs("static"));
}

#[test]
fn inputs_carry_the_exact_host_module_source() {
    let target = |found: Vec<(String, String)>| {
        found
            .into_iter()
            .find(|(path, _)| path == "ns/target")
            .map(|(_, text)| text)
    };
    assert_eq!(
        target(inputs("dynamic")).as_deref(),
        Some("let variant = \"dynamic\"\n")
    );
}

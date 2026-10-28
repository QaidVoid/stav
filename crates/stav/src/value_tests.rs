use super::{AttrPath, Value};

fn job() -> Value {
    Value::Struct {
        name: "Job".to_owned(),
        fields: vec![(
            "build".to_owned(),
            Value::Variant {
                ty: "Build".to_owned(),
                name: "cargo".to_owned(),
                fields: Some(vec![(
                    "features".to_owned(),
                    Value::List(vec![Value::Str("pcre2".to_owned())]),
                )]),
            },
        )],
    }
}

#[test]
fn paths_step_through_fields_keys_and_variants() {
    let root = Value::Map(vec![("rg".to_owned(), job())]);
    let features = root.at(&AttrPath::new(["rg", "build", "cargo", "features"]));
    assert_eq!(
        features,
        Some(&Value::List(vec![Value::Str("pcre2".to_owned())]))
    );
    assert!(root.at(&AttrPath::new(["rg", "build", "make"])).is_none());
    assert_eq!(root.at(&AttrPath::root()), Some(&root));
}

#[test]
fn paths_display_like_attribute_statements() {
    let path = AttrPath::new(["sysctl", "vm.swappiness"]).join("x");
    assert_eq!(path.to_string(), "sysctl.\"vm.swappiness\".x");
}

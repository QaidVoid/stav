use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Deserialize;

use crate::value::Value;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Build {
    Native { features: Vec<String> },
    Script { target: String },
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Level {
    Info,
    Warn,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Job {
    build: Build,
    level: Level,
    timeout: u32,
    note: Option<String>,
    tags: BTreeSet<String>,
    patch: PathBuf,
}

fn job(build: Value) -> Value {
    Value::Struct {
        name: "Job".to_owned(),
        fields: vec![
            ("build".to_owned(), build),
            (
                "level".to_owned(),
                Value::Variant {
                    ty: "Level".to_owned(),
                    name: "warn".to_owned(),
                    fields: None,
                },
            ),
            ("timeout".to_owned(), Value::Int(600)),
            ("note".to_owned(), Value::None),
            (
                "tags".to_owned(),
                Value::Set(vec![Value::Str("a".to_owned())]),
            ),
            (
                "patch".to_owned(),
                Value::Path(PathBuf::from("/etc/fix.patch")),
            ),
        ],
    }
}

fn variant(name: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Variant {
        ty: "Build".to_owned(),
        name: name.to_owned(),
        fields: Some(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()),
    }
}

#[test]
fn structs_maps_and_tagged_variants_deserialize() {
    let jobs = Value::Map(vec![
        (
            "hello".to_owned(),
            job(variant(
                "script",
                vec![("target", Value::Str("all".to_owned()))],
            )),
        ),
        (
            "finder".to_owned(),
            job(variant(
                "native",
                vec![("features", Value::List(vec![Value::Str("simd".to_owned())]))],
            )),
        ),
    ]);
    let found = BTreeMap::<String, Job>::deserialize(&jobs).unwrap();
    assert_eq!(
        found["finder"].build,
        Build::Native {
            features: vec!["simd".to_owned()]
        }
    );
    assert_eq!(
        found["hello"].build,
        Build::Script {
            target: "all".to_owned()
        }
    );
    assert_eq!(found["hello"].level, Level::Warn);
    assert_eq!(found["hello"].note, None);
    assert_eq!(found["hello"].patch, PathBuf::from("/etc/fix.patch"));
}

#[test]
fn integers_are_range_checked() {
    assert_eq!(u8::deserialize(&Value::Int(255)).unwrap(), 255);
    let error = u8::deserialize(&Value::Int(256)).unwrap_err();
    assert!(error.message().contains("256"), "{error}");
    assert!(u64::deserialize(&Value::Int(-1)).is_err());
}

#[test]
fn values_deserialize_like_their_json_rendering() {
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(untagged)]
    enum Any {
        Unit(()),
        Int(i64),
        Text(String),
        List(Vec<Any>),
        Map(BTreeMap<String, Any>),
    }
    let value = Value::List(vec![
        Value::None,
        Value::Int(1),
        Value::Ref("db".to_owned()),
        Value::Variant {
            ty: "Level".to_owned(),
            name: "info".to_owned(),
            fields: None,
        },
        variant("script", vec![("target", Value::Str("x".to_owned()))]),
    ]);
    let found = Any::deserialize(&value).unwrap();
    let script = BTreeMap::from([(
        "script".to_owned(),
        Any::Map(BTreeMap::from([(
            "target".to_owned(),
            Any::Text("x".to_owned()),
        )])),
    )]);
    assert_eq!(
        found,
        Any::List(vec![
            Any::Unit(()),
            Any::Int(1),
            Any::Text("db".to_owned()),
            Any::Text("info".to_owned()),
            Any::Map(script),
        ])
    );
}

use std::path::PathBuf;

use super::Format;
use super::json::ecmascript_number;
use crate::value::{AttrPath, Value};

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn entries(pairs: &[(&str, Value)]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn structure(name: &str, pairs: &[(&str, Value)]) -> Value {
    Value::Struct {
        name: name.to_owned(),
        fields: entries(pairs),
    }
}

fn upstream(url: &str, weight: i64) -> Value {
    structure(
        "Upstream",
        &[("url", s(url)), ("weight", Value::Int(weight))],
    )
}

/// The value of a small application configuration, with a map of
/// structs, a variant with payload, a nullary variant and an option that
/// is not set.
fn app() -> Value {
    structure(
        "Root",
        &[
            ("listen", s("127.0.0.1:8080")),
            (
                "log_level",
                Value::Variant {
                    ty: "Level".to_owned(),
                    name: "warn".to_owned(),
                    fields: None,
                },
            ),
            ("note", Value::None),
            (
                "upstreams",
                Value::Map(entries(&[
                    ("backup", upstream("https://b.example.org", 10)),
                    ("primary", upstream("https://a.example.org", 90)),
                ])),
            ),
            (
                "build",
                Value::Variant {
                    ty: "Build".to_owned(),
                    name: "native".to_owned(),
                    fields: Some(entries(&[("features", Value::List(vec![s("simd")]))])),
                },
            ),
        ],
    )
}

#[test]
fn json_keeps_field_order_and_tags_payload_variants() {
    let expected = r#"{
  "listen": "127.0.0.1:8080",
  "log_level": "warn",
  "note": null,
  "upstreams": {
    "backup": {
      "url": "https://b.example.org",
      "weight": 10
    },
    "primary": {
      "url": "https://a.example.org",
      "weight": 90
    }
  },
  "build": {
    "native": {
      "features": [
        "simd"
      ]
    }
  }
}
"#;
    assert_eq!(app().render(Format::Json).unwrap(), expected);
}

#[test]
fn canonical_json_sorts_keys_and_drops_whitespace() {
    let expected = r#"{"build":{"native":{"features":["simd"]}},"listen":"127.0.0.1:8080","log_level":"warn","note":null,"upstreams":{"backup":{"url":"https://b.example.org","weight":10},"primary":{"url":"https://a.example.org","weight":90}}}"#;
    assert_eq!(app().to_canonical_json(), expected);
}

#[test]
fn canonical_json_sorts_keys_by_utf16_code_units() {
    let value = Value::Map(entries(&[
        ("\u{e9}", Value::Int(1)),
        ("\u{1f600}", Value::Int(2)),
        ("\u{ff61}", Value::Int(3)),
    ]));
    let text = value.to_canonical_json();
    let keys: Vec<usize> = ["\u{e9}", "\u{1f600}", "\u{ff61}"]
        .iter()
        .map(|k| text.find(k).unwrap())
        .collect();
    assert!(keys[0] < keys[1] && keys[1] < keys[2], "{text}");
}

#[test]
fn json_escapes_control_characters() {
    let bell = char::from(7).to_string();
    let value = s(&format!("a\"b\\c\nd{bell}"));
    let escaped = format!("\"a\\\"b\\\\c\\nd{}u0007\"", '\\');
    assert_eq!(value.to_canonical_json(), escaped);
}

#[test]
fn canonical_numbers_follow_ecmascript() {
    let cases = [
        (0.0, "0"),
        (-0.0, "0"),
        (1.0, "1"),
        (0.1, "0.1"),
        (-2.5, "-2.5"),
        (123_456.789, "123456.789"),
        (1e21, "1e+21"),
        (1e20, "100000000000000000000"),
        (1e-6, "0.000001"),
        (1e-7, "1e-7"),
        (1.5e-9, "1.5e-9"),
        (f64::MAX, "1.7976931348623157e+308"),
        (5e-324, "5e-324"),
    ];
    for (number, expected) in cases {
        assert_eq!(ecmascript_number(number), expected, "{number:?}");
    }
}

#[test]
fn toml_writes_scalars_before_tables_and_omits_none() {
    let expected = r#"listen = "127.0.0.1:8080"
log_level = "warn"

[upstreams.backup]
url = "https://b.example.org"
weight = 10

[upstreams.primary]
url = "https://a.example.org"
weight = 90

[build.native]
features = ["simd"]
"#;
    assert_eq!(app().render(Format::Toml).unwrap(), expected);
}

#[test]
fn toml_writes_lists_of_tables_as_arrays_of_tables() {
    let value = structure(
        "Root",
        &[
            (
                "models",
                Value::List(vec![
                    structure("Model", &[("id", s("a")), ("env", Value::Map(Vec::new()))]),
                    structure("Model", &[("id", s("b")), ("tags", Value::List(vec![]))]),
                ]),
            ),
            ("empty", Value::Map(Vec::new())),
            (
                "matrix",
                Value::List(vec![Value::List(vec![Value::Int(1)])]),
            ),
            ("odd key", Value::Float(0.5)),
        ],
    );
    let expected = r#"matrix = [[1]]
"odd key" = 0.5

[[models]]
id = "a"

[models.env]

[[models]]
id = "b"
tags = []

[empty]
"#;
    assert_eq!(value.render(Format::Toml).unwrap(), expected);
}

#[test]
fn toml_rejects_what_it_cannot_express() {
    let mixed = structure(
        "Root",
        &[(
            "items",
            Value::List(vec![Value::Int(1), structure("A", &[])]),
        )],
    );
    let error = mixed.render(Format::Toml).unwrap_err();
    assert_eq!(error.path, AttrPath::new(["items"]));
    assert!(error.message.contains("mixes tables"), "{error}");

    let holes = structure(
        "Root",
        &[(
            "deep",
            Value::Map(entries(&[("xs", Value::List(vec![Value::None]))])),
        )],
    );
    let error = holes.render(Format::Toml).unwrap_err();
    assert_eq!(error.path, AttrPath::new(["deep", "xs"]));

    assert!(Value::Int(1).render(Format::Toml).is_err());
}

#[test]
fn yaml_uses_block_style() {
    let expected = r#"listen: 127.0.0.1:8080
log_level: warn
note: null
upstreams:
  backup:
    url: https://b.example.org
    weight: 10
  primary:
    url: https://a.example.org
    weight: 90
build:
  native:
    features:
      - simd
"#;
    let rendered = app().render(Format::Yaml).unwrap();
    assert_eq!(rendered.replace('"', ""), expected);
    assert!(
        rendered.contains("listen: \"127.0.0.1:8080\""),
        "{rendered}"
    );
}

#[test]
fn yaml_quotes_strings_that_read_as_other_types() {
    for text in [
        "yes",
        "no",
        "on",
        "Off",
        "null",
        "~",
        "1.0",
        "12",
        "0x1f",
        "true",
        "",
        " lead",
        "a: b",
        "#c",
        "-",
        ".inf",
        "2001-12-14",
    ] {
        let rendered = s(text).render(Format::Yaml).unwrap();
        assert!(rendered.starts_with('"'), "{text:?} rendered as {rendered}");
    }
    for text in ["forge", "kernel-lts", "usr/bin", "hello world"] {
        assert_eq!(s(text).render(Format::Yaml).unwrap(), format!("{text}\n"));
    }
}

#[test]
fn yaml_nests_sequences_and_mappings() {
    let value = Value::List(vec![
        Value::List(vec![Value::Int(1), Value::Int(2)]),
        Value::Map(entries(&[("a", Value::Int(1)), ("b", Value::List(vec![]))])),
        Value::Float(1e-7),
        Value::Path(PathBuf::from("/etc/fix.patch")),
    ]);
    let expected = "- - 1\n  - 2\n- a: 1\n  b: []\n- 1.0e-7\n- /etc/fix.patch\n";
    assert_eq!(value.render(Format::Yaml).unwrap(), expected);
}

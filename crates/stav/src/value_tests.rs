use super::{AttrPath, Value};

fn job() -> Value {
    Value::Struct {
        name: "Job".to_owned(),
        fields: vec![(
            "build".to_owned(),
            Value::Variant {
                ty: "Build".to_owned(),
                name: "native".to_owned(),
                fields: Some(vec![(
                    "features".to_owned(),
                    Value::List(vec![Value::Str("simd".to_owned())]),
                )]),
            },
        )],
    }
}

#[test]
fn paths_step_through_fields_keys_and_variants() {
    let root = Value::Map(vec![("finder".to_owned(), job())]);
    let features = root.at(&AttrPath::new(["finder", "build", "native", "features"]));
    assert_eq!(
        features,
        Some(&Value::List(vec![Value::Str("simd".to_owned())]))
    );
    assert!(
        root.at(&AttrPath::new(["finder", "build", "script"]))
            .is_none()
    );
    assert_eq!(root.at(&AttrPath::root()), Some(&root));
}

#[test]
fn paths_step_into_list_elements_by_position() {
    let root = Value::Map(vec![("finder".to_owned(), job())]);
    let path = |keys: &[&str]| {
        let mut all = vec!["finder", "build", "native", "features"];
        all.extend(keys);
        AttrPath::new(all)
    };
    assert_eq!(root.at(&path(&["0"])), Some(&Value::Str("simd".to_owned())));
    for missing in ["1", "00", "+0", "-1", "x"] {
        assert!(root.at(&path(&[missing])).is_none(), "{missing}");
    }
}

#[test]
fn paths_display_like_attribute_statements() {
    let path = AttrPath::new(["tunables", "mem.reclaim"]).join("x");
    assert_eq!(path.to_string(), "tunables.\"mem.reclaim\".x");
}

#[test]
fn quoted_keys_use_string_literal_escapes() {
    let path = AttrPath::new(["a\"b\\c", "line\n\ttab\r", "#{x} #y", "\u{1b}", ""]);
    assert_eq!(
        path.to_string(),
        r#""a\"b\\c"."line\n\ttab\r"."\#{x} #y"."\u{1b}"."""#
    );
}

#[test]
fn displayed_paths_parse_back() {
    let paths = [
        AttrPath::root(),
        AttrPath::new(["server", "port"]),
        AttrPath::new(["tunables", "mem.reclaim", "x-y_z"]),
        AttrPath::new(["a\"b\\c", "line\n\ttab\r\0", "#{x} #y", "\u{1b}\u{7f}", ""]),
        AttrPath::new(["caf\u{e9}", "with space", "\u{1F600}"]),
    ];
    for path in paths {
        let text = path.to_string();
        assert_eq!(text.parse::<AttrPath>(), Ok(path), "{text}");
    }
}

#[test]
fn quoted_keys_accept_unicode_escapes() {
    let path: AttrPath = r#""\u{48}i""#.parse().unwrap();
    assert_eq!(path.keys(), ["Hi"]);
}

#[test]
fn malformed_paths_are_rejected_with_a_reason() {
    let cases = [
        ("a..b", "empty key"),
        ("a.", "empty key"),
        (".a", "empty key"),
        ("a b", "`a b` must be quoted"),
        ("caf\u{e9}", "`caf\u{e9}` must be quoted"),
        (r#""open"#, "unterminated quoted key"),
        (r#""a"b"#, "expected `.` after a quoted key, found `b`"),
        (r#""a\q""#, r"unknown escape `\q`"),
        (r#""\u{d800}""#, r"invalid unicode escape `\u{d800}`"),
        (r#""\u{48""#, r"invalid unicode escape `\u{48`"),
    ];
    for (text, message) in cases {
        let error = text.parse::<AttrPath>().unwrap_err();
        assert_eq!(error.to_string(), message, "{text}");
    }
}

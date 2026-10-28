use super::parse_path;

fn keys(text: &str) -> Vec<String> {
    parse_path(text).unwrap().keys().to_vec()
}

#[test]
fn dotted_path_splits_into_keys() {
    assert_eq!(keys("server.port"), ["server", "port"]);
}

#[test]
fn quoted_key_keeps_its_dots() {
    assert_eq!(
        keys(r#"sysctl."vm.swappiness""#),
        ["sysctl", "vm.swappiness"]
    );
}

#[test]
fn backslash_in_quoted_key_takes_next_character() {
    assert_eq!(keys(r#""a\"b\\c""#), [r#"a"b\c"#]);
}

#[test]
fn displayed_path_parses_back() {
    let path = stav::AttrPath::new(["sysctl", "vm.swappiness", "x-y"]);
    assert_eq!(parse_path(&path.to_string()).unwrap(), path);
}

#[test]
fn malformed_paths_are_rejected() {
    for text in ["", "a..b", "a.", ".a", r#""open"#, r#""a"b"#] {
        assert!(parse_path(text).is_err(), "{text:?} was accepted");
    }
}

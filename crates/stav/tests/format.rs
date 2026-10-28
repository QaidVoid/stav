//! Formats the fixture programs and checks the layout, stability and
//! meaning of the result.

use std::fs;
use std::path::{Path, PathBuf};

use stav::{SyntaxElement, TokenKind, format, parse};

use self::support::meaning;

mod support;

fn fixtures(dir: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/format")
        .join(dir);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("the fixture directory exists")
        .map(|entry| entry.expect("the fixture directory is readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "stav"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "{} has no fixtures", dir.display());
    paths
}

fn format_checked(path: &Path) -> (String, String) {
    let source = fs::read_to_string(path).expect("the fixture is readable");
    let formatted = format(&source)
        .unwrap_or_else(|diagnostics| panic!("{} has errors: {diagnostics:?}", path.display()));
    assert_eq!(
        meaning(&source),
        meaning(&formatted),
        "formatting changed the meaning of {}",
        path.display()
    );
    let again = format(&formatted).expect("the formatted text has no errors");
    assert_eq!(
        again,
        formatted,
        "formatting {} is not idempotent",
        path.display()
    );
    (source, formatted)
}

#[test]
fn unformatted_fixtures_format_to_their_snapshots() {
    for path in fixtures("unformatted") {
        let (_, formatted) = format_checked(&path);
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("the name is UTF-8");
        insta::assert_snapshot!(name, formatted);
    }
}

#[test]
fn formatted_fixtures_are_left_unchanged() {
    for path in fixtures("formatted") {
        let (source, formatted) = format_checked(&path);
        assert_eq!(formatted, source, "{} changed", path.display());
    }
}

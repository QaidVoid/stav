//! Runs the `stav` binary on fixture files in temporary directories and
//! compares its exit status and output with snapshots.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use insta::assert_snapshot;

/// The application program of the language overview, with a refinement
/// that every upstream satisfies.
const APP: &str = r#"type Level = debug | info | warn | error

struct Upstream {
  url:    Str where matches(r"https?://.*")
  weight: Int where 1..=100 = 10
}

struct Root {
  listen:    Str = "127.0.0.1:8080"
  log_level: Level = info
  upstreams: Map<Str, Upstream> = {} block upstream
}

fn local(port: Int where 1..=65535) -> Upstream =
  Upstream { url = "http://127.0.0.1:#{port}" }

log_level = warn

upstream primary { url = "https://a.example.org", weight = 90 }
upstream backup { url = "https://b.example.org" }

upstreams.dev = local(9000)
"#;

/// A program with two unknown names, each with a suggestion.
const UNKNOWN_NAMES: &str = "type Init = runit | s6
struct Root {
  init:  Init = runit
  count: Int = cont
}
let cuont = 1
init = runti
";

/// Creates an empty directory named after the test and writes `files`
/// into it.
fn fixture(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("cli")
        .join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("the fixture directory can be removed");
    }
    fs::create_dir_all(&dir).expect("the fixture directory can be created");
    for (path, text) in files {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the fixture directory can be created");
        }
        fs::write(path, text).expect("the fixture file can be written");
    }
    dir
}

/// Runs `stav` with `args` in `dir`, feeding `stdin`, and returns its exit
/// status, standard output and standard error.
fn stav(dir: &Path, args: &[&str], stdin: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_stav"))
        .args(args)
        .current_dir(dir)
        .env_remove("NO_COLOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the stav binary starts");
    if let Some(mut input) = child.stdin.take() {
        input
            .write_all(stdin.as_bytes())
            .expect("the standard input of stav can be written");
    }
    let output = child.wait_with_output().expect("stav finishes");
    format!(
        "status: {}\n--- stdout\n{}--- stderr\n{}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn run(name: &str, files: &[(&str, &str)], args: &[&str]) -> String {
    stav(&fixture(name, files), args, "")
}

#[test]
fn fmt_rewrites_files_in_place() {
    let dir = fixture(
        "fmt_rewrites_files_in_place",
        &[
            ("a.stav", "struct Root { a: Int }\na=1\n"),
            ("lib/b.stav", "let x=[1,2]\n"),
            ("lib/notes.txt", "x=1\n"),
        ],
    );
    assert_snapshot!(stav(&dir, &["fmt", "."], ""), @r"
    status: 0
    --- stdout
    --- stderr
    ");
    assert_eq!(
        fs::read_to_string(dir.join("a.stav")).unwrap(),
        "struct Root { a: Int }\na = 1\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("lib/b.stav")).unwrap(),
        "let x = [1, 2]\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("lib/notes.txt")).unwrap(),
        "x=1\n"
    );
}

#[test]
fn fmt_check_lists_files_that_would_change() {
    let dir = fixture(
        "fmt_check_lists_files_that_would_change",
        &[("clean.stav", "a = 1\n"), ("messy.stav", "a=1\n")],
    );
    assert_snapshot!(stav(&dir, &["fmt", "--check", "clean.stav", "messy.stav"], ""), @r"
    status: 1
    --- stdout
    messy.stav
    --- stderr
    ");
    assert_eq!(fs::read_to_string(dir.join("messy.stav")).unwrap(), "a=1\n");
}

#[test]
fn fmt_check_passes_formatted_files() {
    let output = run(
        "fmt_check_passes_formatted_files",
        &[("clean.stav", "a = 1\n")],
        &["fmt", "--check", "clean.stav"],
    );
    assert_snapshot!(output, @r"
    status: 0
    --- stdout
    --- stderr
    ");
}

#[test]
fn fmt_formats_standard_input_to_standard_output() {
    let dir = fixture("fmt_formats_standard_input_to_standard_output", &[]);
    assert_snapshot!(stav(&dir, &["fmt", "-"], "a=1,b=2\n"), @r"
    status: 0
    --- stdout
    a = 1
    b = 2
    --- stderr
    ");
    assert_snapshot!(stav(&dir, &["fmt", "--check", "-"], "a=1\n"), @r"
    status: 1
    --- stdout
    <stdin>
    --- stderr
    ");
}

#[test]
fn fmt_reports_syntax_errors_and_keeps_the_file() {
    let dir = fixture(
        "fmt_reports_syntax_errors_and_keeps_the_file",
        &[("bad.stav", "a = (1\n")],
    );
    assert_snapshot!(stav(&dir, &["fmt", "bad.stav"], ""));
    assert_eq!(
        fs::read_to_string(dir.join("bad.stav")).unwrap(),
        "a = (1\n"
    );
}

#[test]
fn fmt_missing_file_is_an_io_error() {
    let output = run(
        "fmt_missing_file_is_an_io_error",
        &[],
        &["fmt", "nope.stav"],
    );
    assert_snapshot!(output, @r"
    status: 3
    --- stdout
    --- stderr
    error: cannot read `nope.stav`: no such file or directory
    ");
}

#[test]
fn check_accepts_a_valid_program_silently() {
    let output = run(
        "check_accepts_a_valid_program_silently",
        &[("app.stav", APP)],
        &["check", "app.stav"],
    );
    assert_snapshot!(output, @r"
    status: 0
    --- stdout
    --- stderr
    ");
}

#[test]
fn check_shows_snippets_and_help() {
    let output = run(
        "check_shows_snippets_and_help",
        &[("app.stav", UNKNOWN_NAMES)],
        &["check", "app.stav"],
    );
    assert_snapshot!(output);
}

#[test]
fn check_shows_notes() {
    let source = "struct Root { a: Int = fact(3) }
fn fact(n: Int) -> Int = if n == 0 { 1 } else { n * fact(n - 1) }
";
    let output = run(
        "check_shows_notes",
        &[("app.stav", source)],
        &["check", "app.stav"],
    );
    assert_snapshot!(output);
}

#[test]
fn check_writes_json_diagnostics_one_per_line() {
    let output = run(
        "check_writes_json_diagnostics_one_per_line",
        &[("app.stav", UNKNOWN_NAMES)],
        &["check", "--message-format", "json", "app.stav"],
    );
    assert_snapshot!(output);
}

#[test]
fn color_is_used_only_when_asked() {
    let files = [("app.stav", UNKNOWN_NAMES)];
    let always = run(
        "color_always",
        &files,
        &["check", "--color", "always", "app.stav"],
    );
    let never = run(
        "color_never",
        &files,
        &["check", "--color", "never", "app.stav"],
    );
    let auto = run("color_auto", &files, &["check", "app.stav"]);
    assert!(always.contains('\x1b'));
    assert!(!never.contains('\x1b'));
    assert!(!auto.contains('\x1b'), "standard error is not a terminal");
}

#[test]
fn check_without_root_struct_fails() {
    let output = run(
        "check_without_root_struct_fails",
        &[("app.stav", "struct Config { a: Int = 1 }\n")],
        &["check", "app.stav"],
    );
    assert_snapshot!(output);
}

#[test]
fn check_uses_the_named_root_struct() {
    let output = run(
        "check_uses_the_named_root_struct",
        &[("app.stav", "struct Config { a: Int = 1 }\n")],
        &["check", "--root", "Config", "app.stav"],
    );
    assert_snapshot!(output, @r"
    status: 0
    --- stdout
    --- stderr
    ");
}

#[test]
fn check_missing_entry_is_an_io_error() {
    let output = run(
        "check_missing_entry_is_an_io_error",
        &[],
        &["check", "app.stav"],
    );
    assert_snapshot!(output, @r"
    status: 3
    --- stdout
    --- stderr
    error: cannot read `app.stav`: no such file or directory
    ");
}

#[test]
fn usage_errors_exit_with_two() {
    let output = run("usage_errors_exit_with_two", &[], &["eval"]);
    assert!(output.starts_with("status: 2\n"), "{output}");
    let output = run(
        "usage_errors_exit_with_two",
        &[],
        &["eval", "-o", "xml", "a.stav"],
    );
    assert!(output.starts_with("status: 2\n"), "{output}");
}

#[test]
fn eval_prints_json_by_default() {
    let output = run(
        "eval_prints_json_by_default",
        &[("app.stav", APP)],
        &["eval", "app.stav"],
    );
    assert_snapshot!(output);
}

#[test]
fn eval_prints_toml_and_yaml() {
    let files = [("app.stav", APP)];
    let toml = run(
        "eval_prints_toml",
        &files,
        &["eval", "-o", "toml", "app.stav"],
    );
    let yaml = run(
        "eval_prints_yaml",
        &files,
        &["eval", "--output", "yaml", "app.stav"],
    );
    assert_snapshot!("eval_prints_toml", toml);
    assert_snapshot!("eval_prints_yaml", yaml);
}

#[test]
fn eval_prints_canonical_json() {
    let output = run(
        "eval_prints_canonical_json",
        &[("app.stav", "struct Root { b: Int = 2, a: Str = \"x\" }\n")],
        &["eval", "--canonical", "app.stav"],
    );
    assert_snapshot!(output, @r#"
    status: 0
    --- stdout
    {"a":"x","b":2}
    --- stderr
    "#);
}

#[test]
fn eval_prints_one_attribute() {
    let files = [("app.stav", APP)];
    let table = run(
        "eval_prints_one_attribute",
        &files,
        &["eval", "-o", "toml", "--path", "upstreams.dev", "app.stav"],
    );
    assert_snapshot!(table, @r#"
    status: 0
    --- stdout
    url = "http://127.0.0.1:9000"
    weight = 10
    --- stderr
    "#);
    let scalar = run(
        "eval_prints_one_attribute",
        &files,
        &["eval", "--path", "upstreams.\"primary\".weight", "app.stav"],
    );
    assert_snapshot!(scalar, @r"
    status: 0
    --- stdout
    90
    --- stderr
    ");
}

#[test]
fn eval_reports_attributes_that_cannot_be_printed() {
    let files = [("app.stav", APP)];
    let missing = run(
        "eval_reports_missing_attribute",
        &files,
        &["eval", "--path", "upstreams.nope", "app.stav"],
    );
    assert_snapshot!("eval_reports_missing_attribute", missing);
    let scalar = run(
        "eval_reports_scalar_as_toml",
        &files,
        &[
            "eval",
            "-o",
            "toml",
            "--path",
            "upstreams.dev.weight",
            "app.stav",
        ],
    );
    assert_snapshot!("eval_reports_scalar_as_toml", scalar);
}

#[test]
fn eval_reports_errors_and_prints_nothing() {
    let output = run(
        "eval_reports_errors_and_prints_nothing",
        &[(
            "app.stav",
            "struct Root { a: Int = 1 / zero }\nlet zero = 0\n",
        )],
        &["eval", "app.stav"],
    );
    assert_snapshot!(output);
}

#[test]
fn eval_stops_when_fuel_runs_out() {
    let output = run(
        "eval_stops_when_fuel_runs_out",
        &[("app.stav", APP)],
        &["eval", "--fuel", "10", "app.stav"],
    );
    assert!(
        output.starts_with("status: 1\n--- stdout\n--- stderr\nerror[E0611]"),
        "{output}"
    );
}

#[test]
fn eval_follows_imports_and_std_modules() {
    let files = [
        (
            "app/main.stav",
            "import \"./lib/net.stav\" as net\nimport \"std/level\" as level\n\
             struct Root { server: net.Server, level: level.Level = level.Level.info }\n\
             server = net.server(8080)\n",
        ),
        (
            "app/lib/net.stav",
            "struct Server { port: Int, host: Str = \"localhost\" }\n\
             fn server(port: Int) -> Server = Server { port = port }\n",
        ),
        ("std/level.stav", "type Level = debug | info\n"),
    ];
    let output = run(
        "eval_follows_imports_and_std_modules",
        &files,
        &["eval", "-o", "yaml", "--std", "std", "app/main.stav"],
    );
    assert_snapshot!(output, @r"
    status: 0
    --- stdout
    server:
      port: 8080
      host: localhost
    level: info
    --- stderr
    ");
}

#[test]
fn imports_cannot_leave_the_import_root() {
    let files = [
        (
            "app/main.stav",
            "import \"../secret.stav\" as s\nstruct Root { a: Int = 1 }\n",
        ),
        ("secret.stav", "let a = 1\n"),
    ];
    let output = run(
        "imports_cannot_leave_the_import_root",
        &files,
        &["check", "app/main.stav"],
    );
    assert_snapshot!(output);
}

#[test]
fn root_dir_widens_the_import_root() {
    let files = [
        (
            "app/main.stav",
            "import \"../lib.stav\" as lib\nstruct Root { a: Int = lib.a }\n",
        ),
        ("lib.stav", "let a = 1\n"),
    ];
    let output = run(
        "root_dir_widens_the_import_root",
        &files,
        &["eval", "--root-dir", ".", "app/main.stav"],
    );
    assert_snapshot!(output, @r#"
    status: 0
    --- stdout
    {
      "a": 1
    }
    --- stderr
    "#);
}

#[cfg(unix)]
#[test]
fn symbolic_links_cannot_leave_the_import_root() {
    let files = [
        (
            "app/main.stav",
            "import \"./lib.stav\" as lib\nstruct Root { a: Int = lib.a }\n",
        ),
        ("secret.stav", "let a = 1\n"),
    ];
    let dir = fixture("symbolic_links_cannot_leave_the_import_root", &files);
    std::os::unix::fs::symlink(dir.join("secret.stav"), dir.join("app/lib.stav")).unwrap();
    assert_snapshot!(stav(&dir, &["check", "app/main.stav"], ""));
}

#[test]
fn directories_are_not_source_files() {
    let files = [
        (
            "app/main.stav",
            "import \"./lib.stav\" as lib\nstruct Root { a: Int = 1 }\n",
        ),
        ("app/lib.stav/x", ""),
    ];
    let output = run(
        "directories_are_not_source_files",
        &files,
        &["check", "app/main.stav"],
    );
    assert_snapshot!(output);
}

#[test]
fn relative_root_dir_may_name_a_parent_directory() {
    let files = [
        (
            "app/main.stav",
            "import \"../lib.stav\" as lib\nstruct Root { a: Int = lib.a }\n",
        ),
        ("lib.stav", "let a = 1\n"),
    ];
    let dir = fixture("relative_root_dir_may_name_a_parent_directory", &files);
    let output = stav(
        &dir.join("app"),
        &["eval", "--root-dir", "..", "main.stav"],
        "",
    );
    assert_snapshot!(output, @r#"
    status: 0
    --- stdout
    {
      "a": 1
    }
    --- stderr
    "#);
}

#[test]
fn missing_root_dir_is_reported_as_the_root_dir() {
    let files = [("app.stav", "struct Root { a: Int = 1 }\n")];
    let missing = run(
        "missing_root_dir_is_reported_as_the_root_dir",
        &files,
        &["check", "--root-dir", "nope", "app.stav"],
    );
    assert_snapshot!(missing, @r"
    status: 3
    --- stdout
    --- stderr
    error: cannot read `nope`: no such file or directory
    ");
    let file = run(
        "root_dir_must_be_a_directory",
        &files,
        &["check", "--root-dir", "app.stav", "app.stav"],
    );
    assert_snapshot!(file, @r"
    status: 3
    --- stdout
    --- stderr
    error: `app.stav` is not a directory
    ");
}

#[test]
fn fmt_skips_hidden_entries_unless_named() {
    let dir = fixture(
        "fmt_skips_hidden_entries_unless_named",
        &[
            ("a.stav", "a=1\n"),
            (".hidden.stav", "a=1\n"),
            (".git/x.stav", "a=1\n"),
            ("lib/.cache/y.stav", "a=1\n"),
        ],
    );
    assert_snapshot!(stav(&dir, &["fmt", "--check", "."], ""), @r"
    status: 1
    --- stdout
    ./a.stav
    --- stderr
    ");
    assert_snapshot!(stav(&dir, &["fmt", "--check", ".hidden.stav", ".git"], ""), @r"
    status: 1
    --- stdout
    .hidden.stav
    .git/x.stav
    --- stderr
    ");
}

#[test]
fn eval_path_takes_keys_as_diagnostics_print_them() {
    let files = [(
        "app.stav",
        "struct Root { labels: Map<Str, Int> = {} }\n\
         labels.\"say \\\"hi\\\"\" = 1\n\
         labels.\"two\\nlines\" = 2\n",
    )];
    let quoted = run(
        "eval_path_takes_quoted_keys",
        &files,
        &["eval", "--path", r#"labels."say \"hi\"""#, "app.stav"],
    );
    assert_snapshot!(quoted, @r"
    status: 0
    --- stdout
    1
    --- stderr
    ");
    let newline = run(
        "eval_path_takes_escaped_newlines",
        &files,
        &["eval", "--path", r#"labels."two\nlines""#, "app.stav"],
    );
    assert_snapshot!(newline, @r"
    status: 0
    --- stdout
    2
    --- stderr
    ");
    let malformed = run(
        "eval_path_rejects_malformed_paths",
        &files,
        &["eval", "--path", r#"labels."two\qlines""#, "app.stav"],
    );
    assert!(malformed.starts_with("status: 2\n"), "{malformed}");
    assert!(malformed.contains(r"unknown escape `\q`"), "{malformed}");
}

#[test]
fn fmt_refuses_files_larger_than_the_checker_accepts() {
    let big = "\n".repeat(stav::MAX_SOURCE_LEN + 1);
    let dir = fixture(
        "fmt_refuses_files_larger_than_the_checker_accepts",
        &[("big.stav", &big)],
    );
    assert_snapshot!(stav(&dir, &["fmt", "big.stav"], ""), @r"
    status: 3
    --- stdout
    --- stderr
    error: cannot read `big.stav`: it is larger than 4 MiB
    ");
    assert_eq!(fs::read_to_string(dir.join("big.stav")).unwrap(), big);
}

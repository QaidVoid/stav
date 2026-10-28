//! The `check` and `eval` commands: type checking and evaluating a program
//! read from disk.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use stav::{
    AttrPath, Checked, Diagnostic, Diagnostics, Evaluated, FileDiagnostic, Format, Host,
    ImportRoot, Limits, RootSpec, Span,
};

use crate::loader::DiskLoader;
use crate::report::Emitter;
use crate::{Status, read_error};

/// The arguments that select a program: its entry file, root struct,
/// import root and standard modules.
#[derive(Debug, clap::Args)]
pub(crate) struct RootArgs {
    /// The entry file, a body of the root struct.
    pub(crate) entry: PathBuf,
    /// The name of the root struct, declared in the entry file.
    #[arg(long, value_name = "STRUCT", default_value = "Root")]
    pub(crate) root: String,
    /// The directory that imports, part files and path literals cannot
    /// leave. Defaults to the directory of the entry file.
    #[arg(long, value_name = "DIR")]
    pub(crate) root_dir: Option<PathBuf>,
    /// Serves the module `std/<name>` from the file `<DIR>/<name>.stav`.
    #[arg(long, value_name = "DIR")]
    pub(crate) std: Option<PathBuf>,
}

/// The format `eval` prints the value in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Output {
    /// Pretty printed JSON.
    Json,
    /// TOML, which needs a table.
    Toml,
    /// YAML in block style.
    Yaml,
}

/// The arguments of `eval`.
#[derive(Debug, clap::Args)]
pub(crate) struct EvalArgs {
    #[command(flatten)]
    pub(crate) root: RootArgs,
    /// The format the value is printed in.
    #[arg(short, long, value_enum, default_value_t = Output::Json)]
    pub(crate) output: Output,
    /// Prints canonical JSON (RFC 8785) on one line instead.
    #[arg(long, conflicts_with = "output")]
    pub(crate) canonical: bool,
    /// Prints only the attribute at this path, for example
    /// `server.port` or `sysctl."vm.swappiness"`.
    #[arg(long, value_name = "ATTR", value_parser = parse_path)]
    pub(crate) path: Option<AttrPath>,
    /// The number of evaluation steps after which evaluation stops.
    #[arg(long, value_name = "STEPS")]
    pub(crate) fuel: Option<u64>,
}

/// Type checks the program and reports its diagnostics.
///
/// # Errors
///
/// Returns an error when the entry file cannot be read or a diagnostic
/// cannot be written.
pub(crate) fn check(args: &RootArgs, emitter: &Emitter) -> io::Result<Status> {
    match checked(args, emitter)? {
        Some(checked) => {
            emitter.diagnostics(checked.warnings())?;
            Ok(Status::Success)
        }
        None => Ok(Status::Failure),
    }
}

/// Type checks and evaluates the program, and prints its value or the
/// value of one attribute on standard output.
///
/// # Errors
///
/// Returns an error when the entry file cannot be read, a diagnostic
/// cannot be written, or standard output cannot be written.
pub(crate) fn eval(args: &EvalArgs, emitter: &Emitter) -> io::Result<Status> {
    let Some(checked) = checked(&args.root, emitter)? else {
        return Ok(Status::Failure);
    };
    emitter.diagnostics(checked.warnings())?;
    let defaults = Limits::default();
    let limits = Limits {
        steps: args.fuel.unwrap_or(defaults.steps),
        ..defaults
    };
    let rendered = checked
        .eval(&limits)
        .and_then(|evaluated| render(&evaluated, args));
    match rendered {
        Ok(text) => {
            io::stdout().lock().write_all(text.as_bytes())?;
            Ok(Status::Success)
        }
        Err(diagnostics) => {
            emitter.diagnostics(&diagnostics)?;
            Ok(Status::Failure)
        }
    }
}

/// Loads and type checks the program, or reports why it does not check.
fn checked(args: &RootArgs, emitter: &Emitter) -> io::Result<Option<Checked>> {
    fs::metadata(&args.entry).map_err(|e| read_error(&args.entry, &e))?;
    let (import_root, entry) = match &args.root_dir {
        Some(dir) => (dir.clone(), entry_in(dir, &args.entry)?),
        None => (
            args.entry.parent().unwrap_or(Path::new("")).to_owned(),
            args.entry.clone(),
        ),
    };
    let host = Host::new(DiskLoader {
        std_dir: args.std.clone(),
    });
    let root = RootSpec {
        root_struct: args.root.clone(),
        entries: vec![entry],
        import_roots: vec![ImportRoot::new(import_root)],
    };
    match stav::check(&host, &root) {
        Ok(checked) => Ok(Some(checked)),
        Err(diagnostics) => {
            emitter.diagnostics(&diagnostics)?;
            Ok(None)
        }
    }
}

/// Returns the entry file as a path below the root directory `dir`, so
/// that the two compare by their real locations rather than by how they
/// were written. The file name of the entry is kept, so that the loader
/// still confines an entry that is a symbolic link. An entry outside `dir`
/// is returned unchanged, for the checker to report.
///
/// # Errors
///
/// Returns an error when `dir` does not exist or is not a directory.
fn entry_in(dir: &Path, entry: &Path) -> io::Result<PathBuf> {
    let real_dir = fs::canonicalize(dir).map_err(|e| read_error(dir, &e))?;
    if !real_dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("`{}` is not a directory", dir.display()),
        ));
    }
    let (Some(parent), Some(name)) = (entry.parent(), entry.file_name()) else {
        return Ok(entry.to_owned());
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let real_parent = fs::canonicalize(parent).map_err(|e| read_error(parent, &e))?;
    Ok(match real_parent.strip_prefix(&real_dir) {
        Ok(rest) => dir.join(rest).join(name),
        Err(_) => entry.to_owned(),
    })
}

fn render(evaluated: &Evaluated, args: &EvalArgs) -> Result<String, Diagnostics> {
    let path = args.path.clone().unwrap_or_default();
    let Some(value) = evaluated.value().at(&path) else {
        return Err(error(
            evaluated,
            &path,
            "E0805",
            format!("there is no attribute `{path}`"),
        ));
    };
    if args.canonical {
        let mut text = String::from_utf8_lossy(&value.to_canonical_json()).into_owned();
        text.push('\n');
        return Ok(text);
    }
    let format = match args.output {
        Output::Json => Format::Json,
        Output::Toml => Format::Toml,
        Output::Yaml => Format::Yaml,
    };
    if path.keys().is_empty() {
        return evaluated.render(format);
    }
    value.render(format).map_err(|e| {
        let full = AttrPath::new(path.keys().iter().chain(e.path.keys()).cloned());
        error(
            evaluated,
            &full,
            "E0804",
            format!("cannot render `{full}` as {format}: {}", e.message),
        )
    })
}

/// Returns an error about the attribute at `path`, pointing at its first
/// source when it has one.
fn error(
    evaluated: &Evaluated,
    path: &AttrPath,
    code: &'static str,
    message: String,
) -> Diagnostics {
    let source = evaluated.spans(path).first().copied();
    let span = source.map_or(Span { start: 0, end: 0 }, |s| s.span);
    evaluated.diagnostics([FileDiagnostic {
        file: source.map(|s| s.file),
        diagnostic: Diagnostic::error(code, span, message),
    }])
}

/// Parses an attribute path such as `server.port` or
/// `sysctl."vm.swappiness"`.
///
/// Keys are separated by dots. A key in double quotes may contain any
/// character, and a backslash in it takes the next character literally.
///
/// # Errors
///
/// Returns a message when the path is empty, has an empty key, or has an
/// unterminated quoted key.
pub(crate) fn parse_path(text: &str) -> Result<AttrPath, String> {
    let mut keys = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        let mut key = String::new();
        if chars.next_if_eq(&'"').is_some() {
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some('\\') => key.extend(chars.next()),
                    Some(c) => key.push(c),
                    None => return Err("unterminated quoted key".to_owned()),
                }
            }
        } else {
            while let Some(c) = chars.next_if(|&c| c != '.') {
                key.push(c);
            }
            if key.is_empty() {
                return Err("empty key".to_owned());
            }
        }
        keys.push(key);
        match chars.next() {
            None => return Ok(AttrPath::new(keys)),
            Some('.') => {}
            Some(c) => return Err(format!("expected `.` after a quoted key, found `{c}`")),
        }
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;

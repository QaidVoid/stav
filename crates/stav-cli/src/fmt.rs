//! The `fmt` command: formatting files in place or checking their layout.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::report::Emitter;
use crate::{Status, io_error, read_error};

/// The name standard input has in diagnostics and in the list of files
/// that would change.
const STDIN_NAME: &str = "<stdin>";

/// Formats every file in `paths`, or with `check` lists the files whose
/// layout would change on standard output and changes nothing.
///
/// A directory stands for every `.stav` file below it, in path order. The
/// path `-` reads standard input and, unless `check` is set, writes the
/// formatted text to standard output.
///
/// # Errors
///
/// Returns an error when a file cannot be read or written.
pub(crate) fn run(paths: &[PathBuf], check: bool, emitter: &Emitter) -> io::Result<Status> {
    let mut status = Status::Success;
    for path in paths {
        if path.as_os_str() == "-" {
            status = status.max(stdin(check, emitter)?);
            continue;
        }
        let mut files = Vec::new();
        collect(path, &mut files)?;
        for file in files {
            status = status.max(file_in_place(&file, check, emitter)?);
        }
    }
    Ok(status)
}

/// Adds `path` to `files`, or every `.stav` file below it when it is a
/// directory. Symbolic links inside a directory are not followed.
fn collect(path: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if !fs::metadata(path)
        .map_err(|e| read_error(path, &e))?
        .is_dir()
    {
        files.push(path.to_owned());
        return Ok(());
    }
    let mut entries = fs::read_dir(path)
        .and_then(|entries| entries.collect::<io::Result<Vec<_>>>())
        .map_err(|e| read_error(path, &e))?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| read_error(&path, &e))?;
        if kind.is_dir() {
            collect(&path, files)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "stav") {
            files.push(path);
        }
    }
    Ok(())
}

fn file_in_place(path: &Path, check: bool, emitter: &Emitter) -> io::Result<Status> {
    let text = fs::read_to_string(path).map_err(|e| read_error(path, &e))?;
    let name = path.display().to_string();
    let Some(formatted) = format(&name, &text, emitter)? else {
        return Ok(Status::Failure);
    };
    if formatted == text {
        return Ok(Status::Success);
    }
    if check {
        writeln!(io::stdout().lock(), "{name}")?;
        return Ok(Status::Failure);
    }
    fs::write(path, formatted).map_err(|e| io_error("write", path, &e))?;
    Ok(Status::Success)
}

fn stdin(check: bool, emitter: &Emitter) -> io::Result<Status> {
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| read_error(Path::new(STDIN_NAME), &e))?;
    let Some(formatted) = format(STDIN_NAME, &text, emitter)? else {
        return Ok(Status::Failure);
    };
    let mut out = io::stdout().lock();
    if !check {
        out.write_all(formatted.as_bytes())?;
        return Ok(Status::Success);
    }
    if formatted == text {
        return Ok(Status::Success);
    }
    writeln!(out, "{STDIN_NAME}")?;
    Ok(Status::Failure)
}

/// Returns the formatted text, or `None` after reporting the syntax errors
/// that keep `text` from being formatted.
fn format(name: &str, text: &str, emitter: &Emitter) -> io::Result<Option<String>> {
    match stav::format(text) {
        Ok(formatted) => Ok(Some(formatted)),
        Err(diagnostics) => {
            emitter.source(name, text, &diagnostics)?;
            Ok(None)
        }
    }
}

//! The `stav` command: formats, type checks and evaluates Stav programs.
//!
//! The exit status is 0 on success, 1 when a diagnostic is an error or
//! `fmt --check` finds a file that would change, 2 on a usage error and 3
//! when a file or a stream cannot be read or written outside of
//! diagnostics.

#![forbid(unsafe_code)]

mod fmt;
mod loader;
mod report;
mod run;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::report::{Color, Emitter, MessageFormat};

/// Formats, type checks and evaluates Stav programs.
#[derive(Debug, Parser)]
#[command(name = "stav", version)]
struct Cli {
    /// How diagnostics are written to standard error.
    #[arg(long, global = true, value_enum, default_value_t = MessageFormat::Human)]
    message_format: MessageFormat,
    /// When diagnostics are colored.
    #[arg(long, global = true, value_enum, default_value_t = Color::Auto)]
    color: Color,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Formats files in place.
    Fmt {
        /// Lists the files that would change instead of changing them.
        #[arg(long)]
        check: bool,
        /// The files to format. A directory stands for every `.stav` file
        /// below it, skipping entries whose names start with a dot and
        /// symbolic links, and `-` for standard input.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,
    },
    /// Type checks a program.
    Check(run::RootArgs),
    /// Type checks and evaluates a program, and prints its value.
    Eval(run::EvalArgs),
}

/// How a command ended, ordered from best to worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    /// Everything succeeded.
    Success,
    /// A diagnostic is an error, or a file would change.
    Failure,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let emitter = Emitter::new(cli.message_format, cli.color);
    let result = match &cli.command {
        Command::Fmt { check, paths } => fmt::run(paths, *check, &emitter),
        Command::Check(args) => run::check(args, &emitter),
        Command::Eval(args) => run::eval(args, &emitter),
    };
    match result {
        Ok(Status::Success) => ExitCode::SUCCESS,
        Ok(Status::Failure) => ExitCode::from(1),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::from(3),
        Err(error) => {
            // Standard error may be closed as well, and nothing else is
            // left to report the failure on.
            let _ = writeln!(io::stderr().lock(), "error: {error}");
            ExitCode::from(3)
        }
    }
}

/// Returns an error that says `path` cannot be read and why.
fn read_error(path: &Path, error: &io::Error) -> io::Error {
    io_error("read", path, error)
}

/// Returns an error that says `verb` failed on `path` and why, in words
/// that do not depend on the operating system.
fn io_error(verb: &str, path: &Path, error: &io::Error) -> io::Error {
    let reason = match error.kind() {
        io::ErrorKind::NotFound => "no such file or directory".to_owned(),
        kind => kind.to_string(),
    };
    io::Error::new(
        error.kind(),
        format!("cannot {verb} `{}`: {reason}", path.display()),
    )
}

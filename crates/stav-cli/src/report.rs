//! Writing diagnostics to standard error, for people or for programs.
//!
//! People get each diagnostic with its code and message, the source lines
//! it points at, its labels, notes and help. Programs get one JSON object
//! per diagnostic and line.

use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::ops::Range;

use codespan_reporting::diagnostic as cs;
use codespan_reporting::files::{self, Files, SimpleFile};
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use codespan_reporting::term::{self, Config};
use serde::Serialize;
use stav::{Diagnostic, Diagnostics, FileId, Severity, Span};

/// The form diagnostics are written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum MessageFormat {
    /// Source excerpts with the spans underlined, for people.
    Human,
    /// One JSON object per diagnostic and line, for programs.
    Json,
}

/// When diagnostics for people are colored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Color {
    /// When standard error is a terminal and `NO_COLOR` is unset or empty.
    Auto,
    /// Always, even when `NO_COLOR` is set.
    Always,
    /// Never.
    Never,
}

/// Writes diagnostics to standard error in one [`MessageFormat`].
pub(crate) struct Emitter {
    format: MessageFormat,
    color: ColorChoice,
}

impl Emitter {
    /// Returns an emitter that writes in `format`, colored as `color` asks.
    pub(crate) fn new(format: MessageFormat, color: Color) -> Self {
        let colored = match color {
            Color::Always => true,
            Color::Never => false,
            Color::Auto => {
                io::stderr().is_terminal()
                    && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
            }
        };
        let color = if colored {
            ColorChoice::Always
        } else {
            ColorChoice::Never
        };
        Self { format, color }
    }

    /// Writes every diagnostic of a program, in order.
    ///
    /// # Errors
    ///
    /// Returns an error when standard error cannot be written.
    pub(crate) fn diagnostics(&self, diagnostics: &Diagnostics) -> io::Result<()> {
        let mut sources = Sources::default();
        for item in diagnostics.iter() {
            let labels = item.diagnostic.labels.iter().filter_map(|label| label.file);
            for file in item.file.into_iter().chain(labels) {
                if let (Some(name), Some(text)) =
                    (diagnostics.file_name(file), diagnostics.source(file))
                {
                    sources.0.insert(file.index(), SimpleFile::new(name, text));
                }
            }
        }
        for item in diagnostics.iter() {
            self.emit(&sources, item.file.map(FileId::index), &item.diagnostic)?;
        }
        Ok(())
    }

    /// Writes diagnostics about one source text that stands alone, such as
    /// the syntax errors of a file that is formatted.
    ///
    /// # Errors
    ///
    /// Returns an error when standard error cannot be written.
    pub(crate) fn source(
        &self,
        name: &str,
        text: &str,
        diagnostics: &[Diagnostic],
    ) -> io::Result<()> {
        let sources = Sources(BTreeMap::from([(0, SimpleFile::new(name, text))]));
        for diagnostic in diagnostics {
            self.emit(&sources, Some(0), diagnostic)?;
        }
        Ok(())
    }

    fn emit(
        &self,
        sources: &Sources<'_>,
        file: Option<usize>,
        diagnostic: &Diagnostic,
    ) -> io::Result<()> {
        let file = file.filter(|file| sources.0.contains_key(file));
        match self.format {
            MessageFormat::Human => {
                let stream = StandardStream::stderr(self.color);
                let mut out = stream.lock();
                term::emit_to_write_style(
                    &mut out,
                    &Config::default(),
                    sources,
                    &human(sources, file, diagnostic),
                )
                .map_err(|error| match error {
                    files::Error::Io(error) => error,
                    error => io::Error::other(error.to_string()),
                })
            }
            MessageFormat::Json => {
                let line = serde_json::to_string(&json(sources, file, diagnostic))
                    .map_err(io::Error::other)?;
                writeln!(io::stderr().lock(), "{line}")
            }
        }
    }
}

/// The files that diagnostics point into, keyed by the index of their
/// [`FileId`].
#[derive(Default)]
struct Sources<'a>(BTreeMap<usize, SimpleFile<&'a str, &'a str>>);

impl Sources<'_> {
    fn get(&self, id: usize) -> Result<&SimpleFile<&str, &str>, files::Error> {
        self.0.get(&id).ok_or(files::Error::FileMissing)
    }

    /// Returns the one-based line and column of `offset`, counting columns
    /// in characters.
    fn position(&self, id: usize, offset: usize) -> Position {
        self.location(id, offset)
            .map_or(Position { line: 0, col: 0 }, |location| Position {
                line: location.line_number,
                col: location.column_number,
            })
    }
}

impl<'a> Files<'a> for Sources<'a> {
    type FileId = usize;
    type Name = &'a str;
    type Source = &'a str;

    fn name(&'a self, id: usize) -> Result<&'a str, files::Error> {
        Ok(self.get(id)?.name())
    }

    fn source(&'a self, id: usize) -> Result<&'a str, files::Error> {
        Ok(self.get(id)?.source())
    }

    fn line_index(&'a self, id: usize, byte_index: usize) -> Result<usize, files::Error> {
        self.get(id)?.line_index((), byte_index)
    }

    fn line_range(&'a self, id: usize, line_index: usize) -> Result<Range<usize>, files::Error> {
        self.get(id)?.line_range((), line_index)
    }
}

/// Returns the file of a label: its own, or the file of its diagnostic.
fn label_file(sources: &Sources<'_>, file: Option<usize>, label: &stav::Label) -> Option<usize> {
    label
        .file
        .map(FileId::index)
        .or(file)
        .filter(|file| sources.0.contains_key(file))
}

/// Returns the notes of a diagnostic, followed by the messages of labels
/// that point into no known file.
fn notes<'d>(
    sources: &Sources<'_>,
    file: Option<usize>,
    diagnostic: &'d Diagnostic,
) -> Vec<&'d str> {
    let orphans = diagnostic
        .labels
        .iter()
        .filter(|label| label_file(sources, file, label).is_none())
        .map(|label| label.message.as_str());
    diagnostic
        .notes
        .iter()
        .map(String::as_str)
        .chain(orphans)
        .collect()
}

fn human(
    sources: &Sources<'_>,
    file: Option<usize>,
    diagnostic: &Diagnostic,
) -> cs::Diagnostic<usize> {
    let severity = match diagnostic.severity {
        Severity::Error => cs::Severity::Error,
        Severity::Warning => cs::Severity::Warning,
    };
    let primary = file.map(|file| cs::Label::primary(file, diagnostic.span.range()));
    let secondary = diagnostic.labels.iter().filter_map(|label| {
        let file = label_file(sources, file, label)?;
        Some(cs::Label::secondary(file, label.span.range()).with_message(&label.message))
    });
    let notes = notes(sources, file, diagnostic)
        .into_iter()
        .map(|note| format!("note: {note}"))
        .chain(diagnostic.help.iter().map(|help| format!("help: {help}")));
    cs::Diagnostic::new(severity)
        .with_code(diagnostic.code)
        .with_message(&diagnostic.message)
        .with_labels(primary.into_iter().chain(secondary).collect())
        .with_notes(notes.collect())
}

/// A diagnostic as one line of JSON.
#[derive(Serialize)]
struct JsonDiagnostic<'a> {
    code: &'a str,
    severity: &'a str,
    message: &'a str,
    labels: Vec<JsonLabel<'a>>,
    notes: Vec<&'a str>,
    help: Option<&'a str>,
}

/// A location of a [`JsonDiagnostic`]. The primary label marks the span
/// of the diagnostic itself and has an empty message.
#[derive(Serialize)]
struct JsonLabel<'a> {
    file: &'a str,
    start: Position,
    end: Position,
    message: &'a str,
    primary: bool,
}

/// A one-based line and column, the column counted in characters.
#[derive(Serialize)]
struct Position {
    line: usize,
    col: usize,
}

fn json<'a>(
    sources: &'a Sources<'a>,
    file: Option<usize>,
    diagnostic: &'a Diagnostic,
) -> JsonDiagnostic<'a> {
    let label = |file: usize, span: Span, message: &'a str, primary: bool| JsonLabel {
        file: sources.get(file).map_or("", |f| f.name()),
        start: sources.position(file, span.start),
        end: sources.position(file, span.end),
        message,
        primary,
    };
    let primary = file.map(|file| label(file, diagnostic.span, "", true));
    let secondary = diagnostic.labels.iter().filter_map(|l| {
        Some(label(
            label_file(sources, file, l)?,
            l.span,
            &l.message,
            false,
        ))
    });
    JsonDiagnostic {
        code: diagnostic.code,
        severity: match diagnostic.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        },
        message: &diagnostic.message,
        labels: primary.into_iter().chain(secondary).collect(),
        notes: notes(sources, file, diagnostic),
        help: diagnostic.help.as_deref(),
    }
}

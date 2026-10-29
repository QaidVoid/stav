//! Errors and warnings reported about source text.

use std::fmt;

use crate::check::FileId;
use crate::span::Span;

/// How serious a [`Diagnostic`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The program is invalid and cannot be evaluated.
    Error,
    /// The program is valid, but probably not what its author meant.
    Warning,
}

/// A secondary location that a [`Diagnostic`] points at, for example the
/// other end of a dependency cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The file of the location, or `None` for the file of the diagnostic.
    pub file: Option<FileId>,
    /// The source text the label is about.
    pub span: Span,
    /// What the location has to do with the diagnostic.
    pub message: String,
}

/// An error or a warning found in source text.
///
/// Every diagnostic has a stable code such as `E0105` that keeps its meaning
/// across releases, the span of the offending text and a message that says
/// what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The stable code, for example `E0105` for an error or `W0302` for a
    /// warning.
    pub code: &'static str,
    /// Whether the diagnostic is an error or a warning.
    pub severity: Severity,
    /// What is wrong, starting lowercase and without a trailing period.
    pub message: String,
    /// The source text the diagnostic is about.
    pub span: Span,
    /// A suggested fix, when one can be given.
    pub help: Option<String>,
    /// Further locations that explain the diagnostic.
    pub labels: Vec<Label>,
    /// Further explanations that belong to no location.
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// Returns an error with the given code, location and message.
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            span,
            help: None,
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Returns a warning with the given code, location and message.
    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, span, message)
        }
    }

    /// Returns the diagnostic with a suggested fix.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Returns the diagnostic with a suggested fix, if there is one.
    pub fn with_optional_help(mut self, help: Option<String>) -> Self {
        self.help = help;
        self
    }

    /// Returns the diagnostic with one more secondary location.
    pub fn with_label(
        mut self,
        file: Option<FileId>,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        self.labels.push(Label {
            file,
            span,
            message: message.into(),
        });
        self
    }

    /// Returns the diagnostic with one more note.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

/// A [`Diagnostic`] together with the file it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiagnostic {
    /// The file the diagnostic's span refers to, or `None` for a problem
    /// with the program as a whole, such as a missing root struct.
    pub file: Option<FileId>,
    /// The diagnostic.
    pub diagnostic: Diagnostic,
}

/// The name and text of a file that diagnostics refer to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamedSource {
    pub(crate) name: String,
    pub(crate) text: String,
}

/// The diagnostics of a program, in the order they were found, with the
/// names and texts of the files they refer to.
///
/// The [`Display`](fmt::Display) implementation renders every diagnostic
/// with its code, message and the line and column of each location.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub(crate) files: Vec<NamedSource>,
    pub(crate) items: Vec<FileDiagnostic>,
}

impl Diagnostics {
    /// Returns the diagnostics in the order they were found.
    pub fn iter(&self) -> impl Iterator<Item = &FileDiagnostic> {
        self.items.iter()
    }

    /// Returns the number of diagnostics.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether there are no diagnostics.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns whether at least one diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.items
            .iter()
            .any(|item| item.diagnostic.severity == Severity::Error)
    }

    /// Returns diagnostics with the files of these, and `items` in place
    /// of their diagnostics.
    ///
    /// A host uses this to add diagnostics of its own, with codes of its
    /// own, or to replace a diagnostic of Stav with its own version. The
    /// result renders exactly like the diagnostics of Stav. The file of
    /// each item must be one of [`Diagnostics::files`].
    ///
    /// # Examples
    ///
    /// A host that reports leaving an import root under a code of its own:
    ///
    /// ```
    /// use stav::{Diagnostic, FileDiagnostic, Host, ImportRoot, MemoryLoader, RootSpec};
    ///
    /// let loader = MemoryLoader::new()
    ///     .with_file("app/main.stav", "import \"../secret.stav\" as s\nstruct Root {}\n");
    /// let root = RootSpec {
    ///     root_struct: "Root".to_owned(),
    ///     entries: vec!["app/main.stav".into()],
    ///     import_roots: vec![ImportRoot::new("app")],
    /// };
    /// let errors = stav::check(&Host::new(loader), &root).unwrap_err();
    /// let ours = errors.with_items(errors.iter().map(|item| match item.diagnostic.code {
    ///     "E0702" => FileDiagnostic {
    ///         file: item.file,
    ///         diagnostic: Diagnostic::error("H0002", item.diagnostic.span, "imports stay inside the project")
    ///             .with_help("move the file into the project"),
    ///     },
    ///     _ => item.clone(),
    /// }));
    /// assert_eq!(
    ///     ours.to_string(),
    ///     "error[H0002]: imports stay inside the project\n  --> app/main.stav:1:8\n  \
    ///      help: move the file into the project\n"
    /// );
    /// ```
    pub fn with_items(&self, items: impl IntoIterator<Item = FileDiagnostic>) -> Diagnostics {
        Diagnostics {
            files: self.files.clone(),
            items: items.into_iter().collect(),
        }
    }

    /// Returns every file that diagnostics can refer to, with its display
    /// name, so that a host can point its own diagnostics at them.
    pub fn files(&self) -> impl Iterator<Item = (FileId, &str)> {
        self.files
            .iter()
            .enumerate()
            .map(|(i, f)| (FileId::new(i), f.name.as_str()))
    }

    /// Returns the display name of a file, for example `lib/net.stav` or
    /// `std/desktop`.
    pub fn file_name(&self, file: FileId) -> Option<&str> {
        self.files.get(file.index()).map(|f| f.name.as_str())
    }

    /// Returns the source text of a file.
    pub fn source(&self, file: FileId) -> Option<&str> {
        self.files.get(file.index()).map(|f| f.text.as_str())
    }

    fn location(&self, file: Option<FileId>, span: Span) -> String {
        let Some(source) = file.and_then(|file| self.files.get(file.index())) else {
            return format!("{}..{}", span.start, span.end);
        };
        let (line, column) = line_column(&source.text, span.start);
        format!("{}:{line}:{column}", source.name)
    }
}

/// Returns the one-based line and column of `offset`, counting columns in
/// characters.
fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = before[start..].chars().count() + 1;
    (line, column)
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for item in &self.items {
            let d = &item.diagnostic;
            let kind = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            writeln!(f, "{kind}[{}]: {}", d.code, d.message)?;
            if item.file.is_some() {
                writeln!(f, "  --> {}", self.location(item.file, d.span))?;
            }
            for label in &d.labels {
                let file = label.file.or(item.file);
                writeln!(
                    f,
                    "  label: {}: {}",
                    self.location(file, label.span),
                    label.message
                )?;
            }
            for note in &d.notes {
                writeln!(f, "  note: {note}")?;
            }
            if let Some(help) = &d.help {
                writeln!(f, "  help: {help}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "diagnostic_tests.rs"]
mod tests;

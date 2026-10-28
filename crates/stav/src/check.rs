//! Loading and checking whole programs.
//!
//! [`check`] loads every file of a root through the host's [`Loader`],
//! resolves imports and part files, and reports files that cannot be
//! loaded, unsupported import paths, syntax errors and cycles of imports and
//! part files.

mod graph;
mod host;
mod load;
mod summary;

pub use host::{
    Host, HostModule, ImportRoot, LoadError, Loader, MemoryLoader, RelPath, RootSpec, Source,
    SymbolType,
};
pub use load::FileId;

use self::load::{Program, file_kind};
use crate::diagnostic::{Diagnostics, FileDiagnostic, NamedSource, Severity};

/// A program that passed every check.
///
/// It owns every source file of the root and the warnings found on the
/// way.
#[derive(Debug)]
pub struct Checked {
    program: Program,
    warnings: Diagnostics,
}

impl Checked {
    /// Returns the warnings found while checking.
    pub fn warnings(&self) -> &Diagnostics {
        &self.warnings
    }

    /// Returns every file of the program with its display name, in the order
    /// the files were loaded.
    pub fn files(&self) -> impl Iterator<Item = (FileId, &str)> {
        self.program
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (FileId::new(i), f.name.as_str()))
    }

    /// Returns the source text of a file.
    pub fn source(&self, file: FileId) -> Option<&str> {
        self.program
            .files
            .get(file.index())
            .map(|f| f.parsed.tree.root().text())
    }
}

/// Loads and checks the root `root` of `host`.
///
/// Every file of the root is read through the host's loader and parsed. No
/// part of the program is evaluated.
///
/// # Errors
///
/// Returns every error found, together with the warnings, when a file
/// cannot be loaded, an import path is not supported, a file has syntax
/// errors or items its kind of file may not contain, or imports and part
/// files form a cycle.
///
/// # Examples
///
/// ```
/// use stav::{Host, ImportRoot, MemoryLoader, RootSpec};
///
/// let loader = MemoryLoader::new().with_file(
///     "app/main.stav",
///     "import \"./missing.stav\" as missing\nstruct Root { a: Int = 1 }\n",
/// );
/// let root = RootSpec {
///     root_struct: "Root".to_owned(),
///     entries: vec!["app/main.stav".into()],
///     import_roots: vec![ImportRoot::new("app")],
/// };
/// let errors = stav::check(&Host::new(loader), &root).unwrap_err();
/// let error = &errors.iter().next().unwrap().diagnostic;
/// assert_eq!(error.code, "E0701");
/// ```
pub fn check(host: &Host, root: &RootSpec) -> Result<Checked, Diagnostics> {
    let (program, mut found) = load::load(host, root);
    for (i, file) in program.files.iter().enumerate() {
        let id = Some(FileId::new(i));
        let syntax = file
            .parsed
            .diagnostics
            .iter()
            .cloned()
            .chain(file.parsed.file().validate(file_kind(file.role)));
        found.extend(syntax.map(|diagnostic| FileDiagnostic {
            file: id,
            diagnostic,
        }));
    }
    if found
        .iter()
        .any(|d| d.diagnostic.severity == Severity::Error)
    {
        return Err(diagnostics(&program, found));
    }
    let warnings = diagnostics(&program, found);
    Ok(Checked { program, warnings })
}

fn diagnostics(program: &Program, items: Vec<FileDiagnostic>) -> Diagnostics {
    Diagnostics {
        files: program
            .files
            .iter()
            .map(|f| NamedSource {
                name: f.name.clone(),
                text: f.parsed.tree.root().text().to_owned(),
            })
            .collect(),
        items,
    }
}

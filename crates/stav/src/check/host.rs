//! The interface between the checker and the program that embeds it.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::value::Value;

/// Reads source files for the checker.
///
/// The library never touches the filesystem itself. Every source file it
/// checks, other than the sources a [`Host`] passes in directly, comes from
/// a loader. A loader that reads from disk is expected to resolve symbolic
/// links and to refuse files whose real path leaves the import root.
pub trait Loader {
    /// Returns the source of the module `std/<name>`, or `None` if the host
    /// has no such module.
    fn std_module(&self, name: &str) -> Option<Cow<'static, str>>;

    /// Reads the file `rel` inside the import root `root`.
    ///
    /// # Errors
    ///
    /// Returns a [`LoadError`] when the file does not exist, is not a
    /// regular file, leaves the import root once symbolic links are
    /// resolved, or cannot be read.
    fn read(&self, root: &ImportRoot, rel: &RelPath) -> Result<String, LoadError>;
}

/// Why a [`Loader`] could not read a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// No file exists at the path.
    NotFound,
    /// The path names something other than a regular file.
    NotAFile,
    /// The real path of the file, with symbolic links resolved, leaves the
    /// import root.
    OutsideRoot,
    /// The file is larger than the checker accepts.
    TooLarge,
    /// The file could not be read for another reason, described here.
    Other(String),
}

/// A directory that the files of a root may reference.
///
/// Imports, part files and path literals of a file cannot leave the import
/// root the file belongs to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImportRoot {
    path: PathBuf,
}

impl ImportRoot {
    /// Returns an import root for the directory `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the directory of the import root.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// A normalized path inside an import root, such as `lib/net.stav`.
///
/// It consists of one or more `/`-separated components, none of which is
/// empty, `.` or `..`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(String);

impl RelPath {
    /// Returns the path for the components of `path`, normalizing `.` and
    /// `..` lexically. Returns `None` if the path is absolute, not UTF-8,
    /// empty, or leaves its starting directory.
    pub fn new(path: impl AsRef<Path>) -> Option<Self> {
        let mut parts: Vec<&str> = Vec::new();
        for component in path.as_ref().components() {
            match component {
                Component::Normal(part) => parts.push(part.to_str()?),
                Component::CurDir => {}
                Component::ParentDir => {
                    parts.pop()?;
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Self::from_parts(parts)
    }

    pub(crate) fn from_parts<'a>(parts: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let text = parts.into_iter().collect::<Vec<_>>().join("/");
        (!text.is_empty()).then_some(Self(text))
    }

    /// Returns the path as text, with `/` between components.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the path as a relative [`Path`].
    pub fn to_path(&self) -> PathBuf {
        self.0.split('/').collect()
    }

    /// Returns the components of the directory that contains the file.
    pub(crate) fn dir(&self) -> Vec<&str> {
        let mut parts: Vec<&str> = self.0.split('/').collect();
        parts.pop();
        parts
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A source text the host passes in directly, such as a prelude file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The name the file has in diagnostics, for example `prelude/system.stav`.
    pub name: String,
    /// The source text.
    pub text: String,
}

impl Source {
    /// Returns a source with the given name and text.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }
}

/// A domain type whose values the host validates after evaluation, such as
/// a package reference.
///
/// Where a symbol type is expected, an identifier is a literal of that type.
/// Symbol words and string literals are literals of it when the host allows
/// them. The constructor converts any string value.
#[derive(Debug, Clone)]
pub struct SymbolType {
    /// The type name, which starts with an uppercase letter.
    pub name: String,
    /// Whether symbol words such as `name@1.2:static` are literals of the
    /// type.
    pub words: bool,
    /// Whether string literals are literals of the type.
    pub strings: bool,
    /// The name of the built-in function that converts a `Str` into a value
    /// of the type, for example `pkg`.
    pub constructor: String,
    /// Checks the text of a literal when it is evaluated, and returns its
    /// canonical text or a message that says what is wrong.
    pub parse: fn(&str) -> Result<String, String>,
}

/// A module whose source the host supplies for one evaluation, imported as
/// `"<namespace>/<module>"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostModule {
    /// The import path, a namespace and a module name such as `ns/target`.
    pub path: String,
    /// The source text, which may contain declarations only.
    pub source: String,
}

/// Labels that a host lets references to a struct name, in addition to
/// the labels of the struct's target map.
///
/// The host derives them from the merged value of the root, for example a
/// group for every user that has no explicit group. The function must be
/// deterministic and total. It runs after merging and before references
/// are checked, and cannot change the value.
#[derive(Debug, Clone)]
pub struct ImplicitRefs {
    /// The name of the struct the labels are targets for, for example
    /// `Group`.
    pub target: String,
    /// Returns the labels, given the merged value of the root.
    pub labels: fn(&Value) -> Vec<String>,
}

/// Everything a host program contributes to the checking of a root.
pub struct Host {
    /// Files whose declarations are in scope, unqualified, in every file.
    pub prelude: Vec<Source>,
    /// Part files included at the start of the root body, before every entry
    /// file.
    pub implicit_parts: Vec<Source>,
    /// The domain types the host validates after evaluation.
    pub symbols: Vec<SymbolType>,
    /// The modules the host supplies for this evaluation.
    pub host_modules: Vec<HostModule>,
    /// Labels that references may name although no block declares them.
    pub implicit_refs: Vec<ImplicitRefs>,
    /// Reads every other source file.
    pub loader: Box<dyn Loader>,
}

impl Host {
    /// Returns a host without prelude, implicit parts, symbol types, host
    /// modules or implicit reference targets, which reads files through
    /// `loader`.
    pub fn new(loader: impl Loader + 'static) -> Self {
        Self {
            prelude: Vec::new(),
            implicit_parts: Vec::new(),
            symbols: Vec::new(),
            host_modules: Vec::new(),
            implicit_refs: Vec::new(),
            loader: Box::new(loader),
        }
    }
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("prelude", &self.prelude)
            .field("implicit_parts", &self.implicit_parts)
            .field("symbols", &self.symbols)
            .field("host_modules", &self.host_modules)
            .field("implicit_refs", &self.implicit_refs)
            .finish_non_exhaustive()
    }
}

/// What to check: the root struct, the entry files and the import roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSpec {
    /// The name of the root struct, declared in the prelude or in an entry
    /// file.
    pub root_struct: String,
    /// The entry files, each a body of the root struct, in evaluation
    /// order. Each must lie inside one of the import roots.
    pub entries: Vec<PathBuf>,
    /// The directories that files of the root may reference.
    pub import_roots: Vec<ImportRoot>,
}

/// A [`Loader`] that serves files and standard modules from memory.
///
/// It suits tests and hosts that embed their sources. Files are keyed by
/// the path of their import root joined with their relative path.
///
/// # Examples
///
/// ```
/// use stav::{Host, ImportRoot, MemoryLoader, RootSpec};
///
/// let loader = MemoryLoader::new()
///     .with_file("app/main.stav", "struct Root { port: Int = 80 }\nport = 8080\n");
/// let root = RootSpec {
///     root_struct: "Root".to_owned(),
///     entries: vec!["app/main.stav".into()],
///     import_roots: vec![ImportRoot::new("app")],
/// };
/// let checked = stav::check(&Host::new(loader), &root);
/// assert!(checked.is_ok());
/// ```
#[derive(Debug, Clone, Default)]
pub struct MemoryLoader {
    files: BTreeMap<PathBuf, String>,
    std: BTreeMap<String, String>,
}

impl MemoryLoader {
    /// Returns a loader without files.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the loader with a file at `path`, replacing any file at the
    /// same path.
    pub fn with_file(mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        self.files.insert(path.into(), text.into());
        self
    }

    /// Returns the loader with the module `std/<name>`.
    pub fn with_std(mut self, name: impl Into<String>, text: impl Into<String>) -> Self {
        self.std.insert(name.into(), text.into());
        self
    }
}

impl Loader for MemoryLoader {
    fn std_module(&self, name: &str) -> Option<Cow<'static, str>> {
        self.std.get(name).map(|text| Cow::Owned(text.clone()))
    }

    fn read(&self, root: &ImportRoot, rel: &RelPath) -> Result<String, LoadError> {
        self.files
            .get(&root.path().join(rel.to_path()))
            .cloned()
            .ok_or(LoadError::NotFound)
    }
}

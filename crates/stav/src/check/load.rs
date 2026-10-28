//! Loading the files of a root: entry files, the prelude, implicit parts,
//! imported modules and part files.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use super::graph;
use super::host::{Host, ImportRoot, LoadError, RelPath, RootSpec};
use crate::ast::{Expr, FileKind, Item};
use crate::diagnostic::{Diagnostic, FileDiagnostic};
use crate::parser::{Parsed, parse};
use crate::span::Span;
use crate::syntax::{NodeKind, SyntaxNode};

/// The largest source file the checker accepts, in bytes.
pub(crate) const MAX_SOURCE_LEN: usize = 4 * 1024 * 1024;

/// Identifies one file of a checked program.
///
/// A file loaded in two roles, for example imported as a module and
/// included as a part file, has one identifier per role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(u32);

impl FileId {
    pub(crate) fn new(index: usize) -> Self {
        Self(u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// Returns the position of the file in the program's file list.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Where the text of a file comes from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Origin {
    /// A file inside the import root with the given index.
    Path { root: usize, rel: RelPath },
    /// The module `std/<name>` served by the loader.
    Std(String),
    /// The host module with the given import path.
    HostModule(String),
    /// A source the host passed in directly, by its position among the
    /// prelude files or the implicit parts.
    Host(usize),
}

/// What a file is loaded as, which decides the items it may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Role {
    /// A prelude file: declarations in scope everywhere.
    Prelude,
    /// An entry file: a body of the root struct.
    Entry,
    /// A module, loaded through `import`.
    Module,
    /// A part file, loaded through `include "path"` or as an implicit part.
    Part,
}

/// An `import` of a file, resolved.
#[derive(Debug, Clone)]
pub(crate) struct ImportLink {
    pub(crate) alias: String,
    pub(crate) alias_span: Span,
    /// The imported module, or `None` when it could not be loaded.
    pub(crate) target: Option<FileId>,
}

/// A loaded and parsed file.
#[derive(Debug)]
pub(crate) struct File {
    pub(crate) name: String,
    pub(crate) origin: Origin,
    pub(crate) role: Role,
    pub(crate) parsed: Parsed,
    pub(crate) imports: Vec<ImportLink>,
    /// The part file of every `include "path"` node, by node index.
    pub(crate) includes: BTreeMap<usize, FileId>,
}

/// Every file of a root and how they reference each other.
#[derive(Debug)]
pub(crate) struct Program {
    pub(crate) files: Vec<File>,
    pub(crate) prelude: Vec<FileId>,
    pub(crate) implicit_parts: Vec<FileId>,
    pub(crate) entries: Vec<FileId>,
}

impl Program {
    pub(crate) fn file(&self, id: FileId) -> &File {
        &self.files[id.index()]
    }
}

struct Loader<'h> {
    host: &'h Host,
    files: Vec<File>,
    keys: BTreeMap<(Origin, Role), FileId>,
    queue: Vec<FileId>,
    /// Import and include edges with the span of the referencing node.
    edges: Vec<Vec<(usize, Span)>>,
    diagnostics: Vec<FileDiagnostic>,
}

/// Loads and parses every file of the root, and reports files that cannot
/// be loaded, unsupported import paths, syntax errors, items that do not
/// belong in their file and cycles of imports and part files.
pub(crate) fn load(host: &Host, spec: &RootSpec) -> (Program, Vec<FileDiagnostic>) {
    let mut loader = Loader {
        host,
        files: Vec::new(),
        keys: BTreeMap::new(),
        queue: Vec::new(),
        edges: Vec::new(),
        diagnostics: Vec::new(),
    };
    let prelude = (0..host.prelude.len())
        .filter_map(|i| loader.open(Origin::Host(i), Role::Prelude, None))
        .collect();
    let implicit_parts = (0..host.implicit_parts.len())
        .filter_map(|i| loader.open(Origin::Host(i), Role::Part, None))
        .collect();
    let mut entries = Vec::new();
    for entry in &spec.entries {
        match locate(&spec.import_roots, entry) {
            Some((root, rel)) => {
                let origin = Origin::Path { root, rel };
                entries.extend(loader.open_root_file(
                    origin,
                    Role::Entry,
                    &spec.import_roots,
                    None,
                ));
            }
            None => loader.diagnostics.push(FileDiagnostic {
                file: None,
                diagnostic: Diagnostic::error(
                    "E0702",
                    Span { start: 0, end: 0 },
                    format!(
                        "entry file `{}` is not inside an import root",
                        entry.display()
                    ),
                )
                .with_help("add the directory that contains it to the import roots"),
            }),
        }
    }
    while let Some(file) = loader.queue.pop() {
        loader.scan(file, &spec.import_roots);
    }
    loader.report_cycles();
    let program = Program {
        files: loader.files,
        prelude,
        implicit_parts,
        entries,
    };
    (program, loader.diagnostics)
}

/// Returns the import root that contains `path`, the innermost one when
/// several do, and the path relative to it.
fn locate(roots: &[ImportRoot], path: &Path) -> Option<(usize, RelPath)> {
    let path = normalize(path)?;
    roots
        .iter()
        .enumerate()
        .filter_map(|(i, root)| {
            let root = normalize(root.path())?;
            let rest = path.strip_prefix(root.as_slice())?;
            Some((
                root.len(),
                i,
                RelPath::from_parts(rest.iter().map(String::as_str))?,
            ))
        })
        .max_by_key(|&(len, i, _)| (len, std::cmp::Reverse(i)))
        .map(|(_, i, rel)| (i, rel))
}

/// Returns the components of `path` with `.` and `..` resolved lexically.
/// A leading root or prefix is kept as a component of its own.
fn normalize(path: &Path) -> Option<Vec<String>> {
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.last().is_none_or(|last| last == "/" || last == "..") {
                    parts.push("..".to_owned());
                } else {
                    parts.pop();
                }
            }
            Component::RootDir => parts.push("/".to_owned()),
            Component::Prefix(prefix) => parts.push(prefix.as_os_str().to_str()?.to_owned()),
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
        }
    }
    Some(parts)
}

/// Joins a relative path such as `./a.stav` or `../lib/b.stav` with the
/// directory of `from`, and returns the components of the result. Returns
/// `None` when the result leaves the import root.
pub(crate) fn join_relative(from: &RelPath, path: &str) -> Option<Vec<String>> {
    let mut parts: Vec<String> = from.dir().into_iter().map(str::to_owned).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part.to_owned()),
        }
    }
    Some(parts)
}

/// Returns whether `path` is relative to the file that contains it.
pub(crate) fn is_relative(path: &str) -> bool {
    path.starts_with("./") || path.starts_with("../")
}

impl Loader<'_> {
    fn host_source(&self, role: Role, index: usize) -> Option<(String, String)> {
        let sources = match role {
            Role::Prelude => &self.host.prelude,
            _ => &self.host.implicit_parts,
        };
        sources.get(index).map(|s| (s.name.clone(), s.text.clone()))
    }

    /// Returns the file for `origin` in `role`, loading it if needed.
    /// `site` is the file and span of the reference, for diagnostics.
    fn open(&mut self, origin: Origin, role: Role, site: Option<(FileId, Span)>) -> Option<FileId> {
        self.open_root_file(origin, role, &[], site)
    }

    fn open_root_file(
        &mut self,
        origin: Origin,
        role: Role,
        roots: &[ImportRoot],
        site: Option<(FileId, Span)>,
    ) -> Option<FileId> {
        if let Some(&id) = self.keys.get(&(origin.clone(), role)) {
            return Some(id);
        }
        let (name, text) = match self.read(&origin, role, roots) {
            Ok(found) => found,
            Err(diagnostic) => {
                let (file, span) =
                    site.map_or((None, Span { start: 0, end: 0 }), |(f, s)| (Some(f), s));
                self.diagnostics.push(FileDiagnostic {
                    file,
                    diagnostic: Diagnostic {
                        span,
                        ..*diagnostic
                    },
                });
                return None;
            }
        };
        let id = FileId::new(self.files.len());
        if text.len() > MAX_SOURCE_LEN {
            let (file, span) =
                site.map_or((None, Span { start: 0, end: 0 }), |(f, s)| (Some(f), s));
            self.diagnostics.push(FileDiagnostic {
                file,
                diagnostic: Diagnostic {
                    span,
                    ..too_large(&name)
                },
            });
            return None;
        }
        let parsed = parse(&text);
        self.files.push(File {
            name,
            origin: origin.clone(),
            role,
            parsed,
            imports: Vec::new(),
            includes: BTreeMap::new(),
        });
        self.edges.push(Vec::new());
        self.keys.insert((origin, role), id);
        self.queue.push(id);
        Some(id)
    }

    fn read(
        &self,
        origin: &Origin,
        role: Role,
        roots: &[ImportRoot],
    ) -> Result<(String, String), Box<Diagnostic>> {
        let span = Span { start: 0, end: 0 };
        match origin {
            Origin::Host(index) => self
                .host_source(role, *index)
                .ok_or_else(|| Box::new(Diagnostic::error("E0701", span, "missing host source"))),
            Origin::Std(name) => {
                let full = format!("std/{name}");
                match self.host.loader.std_module(name) {
                    Some(text) => Ok((full, text.into_owned())),
                    None => Err(Box::new(Diagnostic::error(
                        "E0701",
                        span,
                        format!("there is no module `{full}`"),
                    ))),
                }
            }
            Origin::HostModule(path) => self
                .host
                .host_modules
                .iter()
                .find(|module| &module.path == path)
                .map(|module| (path.clone(), module.source.clone()))
                .ok_or_else(|| {
                    Box::new(Diagnostic::error(
                        "E0701",
                        span,
                        format!("there is no host module `{path}`"),
                    ))
                }),
            Origin::Path { root, rel } => {
                let Some(import_root) = roots.get(*root) else {
                    return Err(Box::new(Diagnostic::error(
                        "E0701",
                        span,
                        "missing import root",
                    )));
                };
                let name = import_root.path().join(rel.to_path()).display().to_string();
                match self.host.loader.read(import_root, rel) {
                    Ok(text) => Ok((name, text)),
                    Err(error) => Err(Box::new(load_error(&name, error))),
                }
            }
        }
    }

    /// Resolves the imports and part file includes of `file` and queues the
    /// files they name.
    fn scan(&mut self, file: FileId, roots: &[ImportRoot]) {
        let mut imports = Vec::new();
        let mut includes = Vec::new();
        {
            let f = &self.files[file.index()];
            for item in f.parsed.file().items() {
                if let Item::Import(import) = item {
                    let path = import
                        .path()
                        .and_then(|p| p.value().map(|v| (v, p.syntax().span())));
                    let alias = import.alias().map(|a| (a.text().to_owned(), a.span()));
                    imports.push((path, alias, import.syntax().span()));
                }
            }
            let mut stack = vec![f.parsed.tree.root()];
            while let Some(node) = stack.pop() {
                if node.kind() == NodeKind::Include
                    && let Some(path) = include_path(node)
                {
                    includes.push((node.index(), path));
                }
                stack.extend(node.children());
            }
        }
        includes.sort_by_key(|(index, _)| *index);
        let role = self.files[file.index()].role;
        if role == Role::Prelude && !imports.is_empty() {
            for (_, _, span) in &imports {
                self.error(
                    file,
                    Diagnostic::error("E0707", *span, "a prelude file cannot import modules")
                        .with_help("declare what the prelude needs in the prelude itself"),
                );
            }
            imports.clear();
        }
        let mut links = Vec::new();
        for (path, alias, span) in imports {
            let Some((alias, alias_span)) = alias else {
                continue;
            };
            let target = path.and_then(|(path, path_span)| {
                self.follow(file, &path, path_span, Role::Module, roots)
                    .inspect(|&target| self.edges[file.index()].push((target.index(), span)))
            });
            links.push(ImportLink {
                alias,
                alias_span,
                target,
            });
        }
        let mut resolved = BTreeMap::new();
        for (node, (path, path_span, span)) in includes {
            if let Some(target) = self.follow(file, &path, path_span, Role::Part, roots) {
                self.edges[file.index()].push((target.index(), span));
                resolved.insert(node, target);
            }
        }
        let f = &mut self.files[file.index()];
        f.imports = links;
        f.includes = resolved;
    }

    /// Resolves the import or part file path `path` written in `file`, and
    /// returns the file it names, loading it if needed.
    fn follow(
        &mut self,
        file: FileId,
        path: &str,
        span: Span,
        role: Role,
        roots: &[ImportRoot],
    ) -> Option<FileId> {
        let from = self.files[file.index()].origin.clone();
        match self.resolve(&from, path, role, span) {
            Ok(origin) => self.open_root_file(origin, role, roots, Some((file, span))),
            Err(diagnostic) => {
                self.error(file, *diagnostic);
                None
            }
        }
    }

    fn resolve(
        &self,
        from: &Origin,
        path: &str,
        role: Role,
        span: Span,
    ) -> Result<Origin, Box<Diagnostic>> {
        let what = if role == Role::Part {
            "part file"
        } else {
            "import"
        };
        if is_relative(path) {
            let Origin::Path { root, rel } = from else {
                return Err(Box::new(
                    Diagnostic::error(
                        "E0704",
                        span,
                        format!("a host source cannot use the relative {what} path `{path}`"),
                    )
                    .with_help("use a `std/...` path or a host module path"),
                ));
            };
            if !path.ends_with(".stav") {
                return Err(Box::new(
                    Diagnostic::error(
                        "E0704",
                        span,
                        format!("the relative {what} path `{path}` does not end in `.stav`"),
                    )
                    .with_help(format!("write `{path}.stav`")),
                ));
            }
            let parts = join_relative(rel, path)
                .and_then(|parts| RelPath::from_parts(parts.iter().map(String::as_str)));
            return match parts {
                Some(rel) => Ok(Origin::Path { root: *root, rel }),
                None => Err(Box::new(Diagnostic::error(
                    "E0702",
                    span,
                    format!("the {what} path `{path}` leaves its import root"),
                ))),
            };
        }
        let (namespace, module) = path.split_once('/').unwrap_or((path, ""));
        let valid_module = !module.is_empty()
            && module
                .split('/')
                .all(|part| !matches!(part, "" | "." | ".."));
        if namespace == "std" && valid_module && role == Role::Module {
            return Ok(Origin::Std(module.to_owned()));
        }
        let registered = namespace != "std"
            && self.host.host_modules.iter().any(|m| {
                m.path
                    .split_once('/')
                    .is_some_and(|(ns, _)| ns == namespace)
            });
        if registered && valid_module && role == Role::Module {
            return Ok(Origin::HostModule(path.to_owned()));
        }
        let help = if role == Role::Part {
            "part files are named by relative paths such as `./part.stav`"
        } else {
            "write a relative path such as `./lib.stav`, a `std/...` path or a host module path"
        };
        Err(Box::new(
            Diagnostic::error("E0704", span, format!("unsupported {what} path `{path}`"))
                .with_help(help),
        ))
    }

    fn error(&mut self, file: FileId, diagnostic: Diagnostic) {
        self.diagnostics.push(FileDiagnostic {
            file: Some(file),
            diagnostic,
        });
    }

    fn report_cycles(&mut self) {
        let adj: Vec<Vec<usize>> = self
            .edges
            .iter()
            .map(|edges| {
                let mut targets: Vec<usize> = edges.iter().map(|(t, _)| *t).collect();
                targets.sort_unstable();
                targets.dedup();
                targets
            })
            .collect();
        for cycle in graph::cycles(&adj) {
            let last = cycle[cycle.len() - 1];
            let first = cycle[0];
            let Some(&(_, span)) = self.edges[last].iter().find(|(t, _)| *t == first) else {
                continue;
            };
            let mut chain: Vec<&str> = cycle.iter().map(|&f| self.files[f].name.as_str()).collect();
            chain.push(&self.files[first].name);
            let note = format!("cycle: {}", chain.join(" -> "));
            let message = if cycle.len() == 1 {
                format!("`{}` imports or includes itself", self.files[first].name)
            } else {
                "imports and part files form a cycle".to_owned()
            };
            self.error(
                FileId::new(last),
                Diagnostic::error("E0703", span, message).with_note(note),
            );
        }
    }
}

/// Returns the plain string an `include` names, with the span of the
/// string and of the whole statement.
fn include_path(node: SyntaxNode<'_>) -> Option<(String, Span, Span)> {
    let include = crate::ast::Include::cast(node)?;
    let Some(Expr::Str(string)) = include.value() else {
        return None;
    };
    let value = string.value()?;
    Some((value, string.syntax().span(), node.span()))
}

fn too_large(name: &str) -> Diagnostic {
    Diagnostic::error(
        "E0706",
        Span { start: 0, end: 0 },
        format!("`{name}` is larger than 4 MiB"),
    )
}

fn load_error(name: &str, error: LoadError) -> Diagnostic {
    let span = Span { start: 0, end: 0 };
    match error {
        LoadError::NotFound => Diagnostic::error("E0701", span, format!("`{name}` does not exist")),
        LoadError::NotAFile => {
            Diagnostic::error("E0705", span, format!("`{name}` is not a regular file"))
        }
        LoadError::OutsideRoot => Diagnostic::error(
            "E0702",
            span,
            format!("`{name}` leaves its import root once links are resolved"),
        ),
        LoadError::TooLarge => too_large(name),
        LoadError::Other(reason) => {
            Diagnostic::error("E0701", span, format!("cannot read `{name}`: {reason}"))
        }
    }
}

/// Returns the item rules of a file loaded in `role`.
pub(crate) fn file_kind(role: Role) -> FileKind {
    match role {
        Role::Entry => FileKind::Entry,
        Role::Prelude | Role::Module => FileKind::Library,
        Role::Part => FileKind::Part,
    }
}

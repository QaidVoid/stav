//! Loading and type checking whole programs.
//!
//! [`check`] loads every file of a root through the host's [`Loader`],
//! resolves imports and part files, resolves every name and type checks
//! every declaration, expression and statement, and enforces the totality
//! rules: no recursive functions, bindings or types, and no cycles of
//! imports, part files or `self` dependencies. Nothing is evaluated.

mod body;
mod builtins;
mod call;
mod decls;
mod expr;
mod graph;
mod host;
mod load;
mod model;
mod pattern;
mod schema;
mod suggest;
mod summary;
mod totality;
mod types;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub use host::{
    Host, HostModule, ImportRoot, LoadError, Loader, MemoryLoader, RelPath, RootSpec, Source,
    SymbolType,
};
pub use load::FileId;
pub use schema::{SchemaField, SchemaIndex, SchemaStruct, SchemaSum, SchemaVariant};

use self::decls::{FileScope, Global};
use self::load::{Origin, Program, Role, file_kind};
use self::model::{Fact, LocalId, SelfRead, Target, Unit, UnitId};
use self::totality::{CallGraph, Node};
use self::types::{StructId, Types};
use crate::diagnostic::{Diagnostic, Diagnostics, FileDiagnostic, NamedSource, Severity};
use crate::span::Span;
use crate::syntax::SyntaxNode;

/// A program that passed every check, ready to be evaluated.
///
/// It owns every source file of the root, the declarations with their
/// resolved types, the type and meaning of every expression, and the
/// warnings found on the way.
#[derive(Debug)]
pub struct Checked {
    program: Program,
    types: Types,
    globals: Vec<Global>,
    units: Vec<Unit>,
    root: StructId,
    entries: Vec<UnitId>,
    implicit_parts: Vec<UnitId>,
    let_order: Vec<types::GlobalId>,
    calls: CallGraph,
    ref_targets: BTreeMap<StructId, Vec<model::Step>>,
    warnings: Diagnostics,
    inputs: Vec<(PathBuf, [u8; 32])>,
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

    /// Returns every source file the program was read from, with the
    /// BLAKE3 hash of its text, in the order the files were loaded.
    ///
    /// A file inside an import root is named by the import root joined with
    /// its relative path, a standard module by `std/<name>`, a host module
    /// by its import path such as `ns/target`, and a prelude file or
    /// implicit part by the name the host gave it. A cache keyed by these
    /// inputs tells apart every evaluation whose sources differ.
    pub fn inputs(&self) -> &[(PathBuf, [u8; 32])] {
        &self.inputs
    }
}

/// Returns every distinct source file of the program with the hash of its
/// text.
fn inputs(program: &Program, spec: &RootSpec) -> Vec<(PathBuf, [u8; 32])> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for file in &program.files {
        let path = match &file.origin {
            Origin::Path { root, rel } => spec
                .import_roots
                .get(*root)
                .map_or_else(|| rel.to_path(), |r| r.path().join(rel.to_path())),
            _ => PathBuf::from(&file.name),
        };
        if seen.insert(path.clone()) {
            let text = file.parsed.tree.root().text();
            out.push((path, crate::hash::blake3(text.as_bytes())));
        }
    }
    out
}

/// Loads and type checks the root `root` of `host`.
///
/// Every file of the root is read through the host's loader, parsed and
/// checked. No part of the program is evaluated.
///
/// # Errors
///
/// Returns every error found, together with the warnings, when a file
/// cannot be loaded, has syntax errors, or does not type check. Once files
/// load and parse, as many independent errors as possible are reported in
/// one run.
///
/// # Examples
///
/// ```
/// use stav::{Host, ImportRoot, MemoryLoader, RootSpec};
///
/// let loader = MemoryLoader::new().with_file(
///     "app/main.stav",
///     "type Level = debug | info\nstruct Root { level: Level = info }\nlevel = debgu\n",
/// );
/// let root = RootSpec {
///     root_struct: "Root".to_owned(),
///     entries: vec!["app/main.stav".into()],
///     import_roots: vec![ImportRoot::new("app")],
/// };
/// let errors = stav::check(&Host::new(loader), &root).unwrap_err();
/// let error = &errors.iter().next().unwrap().diagnostic;
/// assert_eq!(error.code, "E0302");
/// assert_eq!(error.help.as_deref(), Some("did you mean `debug`?"));
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
    let (found, parts) = {
        let mut checker = Checker::new(&program, host);
        checker.run(root);
        checker.finish()
    };
    match parts {
        Some(parts) => {
            let warnings = diagnostics(&program, found);
            let inputs = inputs(&program, root);
            Ok(Checked {
                program,
                types: parts.types,
                globals: parts.globals,
                units: parts.units,
                root: parts.root,
                entries: parts.entries,
                implicit_parts: parts.implicit_parts,
                let_order: parts.let_order,
                calls: parts.calls,
                ref_targets: parts.ref_targets,
                warnings,
                inputs,
            })
        }
        None => Err(diagnostics(&program, found)),
    }
}

/// What a successful run of the checker produced.
struct Parts {
    types: Types,
    globals: Vec<Global>,
    units: Vec<Unit>,
    root: StructId,
    entries: Vec<UnitId>,
    implicit_parts: Vec<UnitId>,
    let_order: Vec<types::GlobalId>,
    calls: CallGraph,
    ref_targets: BTreeMap<StructId, Vec<model::Step>>,
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

/// A binding in a lexical scope.
#[derive(Debug, Clone, Copy)]
enum Binding {
    Local(LocalId),
    /// A binding of a `let ... in` expression that is not visible yet,
    /// because the reference is in its own value or an earlier one.
    Pending,
}

/// Where `self` may be used, and what it refers to.
#[derive(Debug, Clone)]
enum SelfMode {
    /// `self` is not allowed. The text says where the expression is, for
    /// example "a function body".
    Forbidden(&'static str),
    /// In the default of a field of the given struct.
    Default(StructId),
    /// In a body.
    Body(BodyCx),
}

/// The blocks around a statement of a body, outermost first.
#[derive(Debug, Clone)]
struct BodyCx {
    /// Identifies the body: an entry file, a part file instance or a
    /// `part { }` expression.
    top: u32,
    frames: Vec<Frame>,
    graph: body::SelfGraph,
}

/// One block around a statement.
#[derive(Debug, Clone)]
struct Frame {
    target: Target,
    /// The keys from the top of the body to the block, which identify the
    /// block in the `self` graph.
    path: Vec<String>,
}

/// The lexical bindings in scope, innermost last.
#[derive(Debug, Clone, Default)]
struct Scope {
    bindings: BTreeMap<String, Vec<Binding>>,
    order: Vec<String>,
}

impl Scope {
    /// Returns a mark that [`Scope::truncate`] returns to.
    fn len(&self) -> usize {
        self.order.len()
    }

    fn push(&mut self, (name, binding): (String, Binding)) {
        self.bindings.entry(name.clone()).or_default().push(binding);
        self.order.push(name);
    }

    /// Removes the bindings pushed since `mark`.
    fn truncate(&mut self, mark: usize) {
        while self.order.len() > mark {
            let Some(name) = self.order.pop() else { break };
            if let Some(stack) = self.bindings.get_mut(&name) {
                stack.pop();
                if stack.is_empty() {
                    self.bindings.remove(&name);
                }
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        self.bindings
            .get(name)
            .and_then(|stack| stack.last().copied())
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        self.bindings.keys().map(String::as_str)
    }
}

/// The context an expression is checked in.
#[derive(Debug, Clone)]
struct Cx {
    unit: UnitId,
    file: FileId,
    scope: Scope,
    self_mode: SelfMode,
    /// The declaration whose body is being checked, for the call graph.
    owner: Option<Node>,
    /// The `self` reads of the expression being checked.
    reads: Vec<SelfRead>,
    /// References to top-level `let` declarations of this file that start
    /// at or after this offset are forward references.
    limit: Option<usize>,
    /// Whether a map or struct literal checked next contributes per field,
    /// so that it may leave required fields out.
    partial: bool,
}

impl Cx {
    fn new(unit: UnitId, file: FileId, self_mode: SelfMode, owner: Option<Node>) -> Self {
        Self {
            unit,
            file,
            scope: Scope::default(),
            self_mode,
            owner,
            reads: Vec::new(),
            limit: None,
            partial: false,
        }
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        self.scope.lookup(name)
    }
}

/// The state of a checking run.
struct Checker<'p> {
    program: &'p Program,
    host: &'p Host,
    diagnostics: Vec<FileDiagnostic>,
    seen: BTreeSet<(Option<FileId>, &'static str, Span, String)>,
    types: Types,
    scopes: Vec<FileScope>,
    prelude: FileScope,
    globals: Vec<Global>,
    units: Vec<Unit>,
    unit_keys: BTreeMap<(FileId, Option<StructId>), UnitId>,
    root: Option<StructId>,
    entries: Vec<UnitId>,
    implicit_parts: Vec<UnitId>,
    entry_tops: BTreeMap<FileId, u32>,
    next_top: u32,
    calls: CallGraph,
    let_order: Vec<types::GlobalId>,
    ref_targets: BTreeMap<StructId, Vec<model::Step>>,
    ref_uses: Vec<(FileId, Span, StructId)>,
    pending_predicates: Vec<decls::PendingPredicate<'p>>,
}

impl<'p> Checker<'p> {
    fn new(program: &'p Program, host: &'p Host) -> Self {
        Self {
            program,
            host,
            diagnostics: Vec::new(),
            seen: BTreeSet::new(),
            types: Types::new(),
            scopes: (0..program.files.len())
                .map(|_| FileScope::default())
                .collect(),
            prelude: FileScope::default(),
            globals: Vec::new(),
            units: Vec::new(),
            unit_keys: BTreeMap::new(),
            root: None,
            entries: Vec::new(),
            implicit_parts: Vec::new(),
            entry_tops: BTreeMap::new(),
            next_top: 0,
            calls: CallGraph::default(),
            let_order: Vec::new(),
            ref_targets: BTreeMap::new(),
            ref_uses: Vec::new(),
            pending_predicates: Vec::new(),
        }
    }

    fn run(&mut self, spec: &RootSpec) {
        self.declare();
        self.resolve_declarations();
        self.find_root(&spec.root_struct);
        self.check_structure();
        self.check_declaration_bodies();
        if let Some(root) = self.root {
            for &file in &self.program.implicit_parts {
                let unit = self.part_unit(file, root);
                self.implicit_parts.push(unit);
            }
            for &file in &self.program.entries {
                let unit = self.entry_body(file, root);
                self.entries.push(unit);
            }
        }
        self.check_totality();
    }

    /// Returns the diagnostics, and what the evaluator needs when none of
    /// them is an error.
    fn finish(mut self) -> (Vec<FileDiagnostic>, Option<Parts>) {
        self.diagnostics
            .sort_by_key(|d| (d.file, d.diagnostic.span.start, d.diagnostic.span.end));
        let errors = self
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.severity == Severity::Error);
        let parts = match self.root {
            Some(root) if !errors => Some(Parts {
                types: self.types,
                globals: self.globals,
                units: self.units,
                root,
                entries: self.entries,
                implicit_parts: self.implicit_parts,
                let_order: self.let_order,
                calls: self.calls,
                ref_targets: self.ref_targets,
            }),
            _ => None,
        };
        (self.diagnostics, parts)
    }

    fn report(&mut self, file: Option<FileId>, diagnostic: Diagnostic) {
        let key = (
            file,
            diagnostic.code,
            diagnostic.span,
            diagnostic.message.clone(),
        );
        if self.seen.insert(key) {
            self.diagnostics.push(FileDiagnostic { file, diagnostic });
        }
    }

    fn error(&mut self, file: FileId, diagnostic: Diagnostic) {
        self.report(Some(file), diagnostic);
    }

    fn fact(&mut self, cx: &Cx, node: SyntaxNode<'_>, fact: Fact) {
        if let Some(slot) = self
            .units
            .get_mut(cx.unit.index())
            .and_then(|unit| unit.facts.get_mut(node.index()))
        {
            *slot = fact;
        }
    }

    /// Returns the unit of a file that is not a part file, creating it on
    /// first use.
    fn file_unit(&mut self, file: FileId) -> UnitId {
        self.unit(file, None).0
    }

    /// Returns the unit of `file` in the context `target`, and whether it
    /// was just created.
    fn unit(&mut self, file: FileId, target: Option<StructId>) -> (UnitId, bool) {
        if let Some(&unit) = self.unit_keys.get(&(file, target)) {
            return (unit, false);
        }
        let unit = UnitId::new(self.units.len());
        let count = self.program.file(file).parsed.tree.node_count();
        self.units.push(Unit {
            file,
            target,
            facts: vec![Fact::None; count],
            locals: Vec::new(),
        });
        self.unit_keys.insert((file, target), unit);
        (unit, true)
    }

    fn new_top(&mut self) -> u32 {
        self.next_top += 1;
        self.next_top
    }

    fn role(&self, file: FileId) -> Role {
        self.program.file(file).role
    }
}

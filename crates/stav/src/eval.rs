//! Evaluating checked programs.
//!
//! [`Checked::eval`] evaluates every statement of a root eagerly, in order,
//! into contributions: a path, a priority and a value, or a deferred value
//! for a right side that reads `self`. The contributions are merged by the
//! types of their paths and their priorities. Once the merge has selected
//! its winners, the `self` dependencies of every block instance are checked
//! for cycles, and only then are deferred values evaluated, each after the
//! attributes it reads. Refinements, required fields and references are
//! checked on the merged value, which is then converted to a [`Value`].
//!
//! Evaluation is total: every loop runs over a finite collection, no
//! function can call itself, and [`Limits`] turn evaluations that are
//! finite but too large into errors.

mod body;
mod builtins;
mod call;
mod env;
mod expr;
mod finish;
mod merge;
mod pattern;
mod val;

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde::de::DeserializeOwned;

use self::env::Env;
use self::merge::{NodeId, SpanTree};
use self::val::{FnVal, MapVal, Seq, StructVal, Val, VariantVal, canonical, depth_of};
use crate::check::model::{Fact, UnitId};
use crate::check::types::{GlobalId, StructId, SumId};
use crate::check::{Checked, FileId};
use crate::diagnostic::{Diagnostic, Diagnostics, FileDiagnostic};
use crate::render::Format;
use crate::span::{SourceSpan, Span};
use crate::syntax::SyntaxTree;
use crate::value::{AttrPath, Value};

/// Limits on the work and the memory one evaluation may use.
///
/// Evaluation always terminates, but a terminating program can still take
/// very long or build very large values, for example by doubling a string
/// in a loop. Crossing a limit stops evaluation with an error instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The number of evaluation steps. Evaluation counts one per
    /// expression and per statement evaluated, one per element of every
    /// collection it builds and per 64 bytes that `value_bytes` counts, one
    /// per key it checks for duplicates in a map literal or comprehension,
    /// one per entry of the `Data` maps it merges, one per byte of every
    /// regular expression it compiles and one per 64 bytes of every string
    /// matched against one. Crossing it is `E0611`.
    pub steps: u64,
    /// The total size in bytes of the strings and collections evaluation
    /// builds, counting every intermediate value. This bounds the memory
    /// that live values can take. Crossing it is `E0612`.
    pub value_bytes: u64,
    /// The length in bytes of one string. Crossing it is `E0612`.
    pub string_bytes: u64,
    /// The number of elements of one list, set or map. Crossing it is
    /// `E0612`.
    pub collection_len: u64,
}

impl Default for Limits {
    /// Returns 50 000 000 steps, 512 MiB of values, strings of up to
    /// 16 MiB and collections of up to 10 000 000 elements.
    fn default() -> Self {
        Self {
            steps: 50_000_000,
            value_bytes: 512 * 1024 * 1024,
            string_bytes: 16 * 1024 * 1024,
            collection_len: 10_000_000,
        }
    }
}

/// How deeply values may nest. Every typed value nests far less, so this
/// only stops `Data` values built in loops, and keeps every recursive walk
/// over values shallow.
pub(crate) const MAX_VALUE_DEPTH: u32 = 128;

/// How deeply expression evaluation, calls and the finalizing of merged
/// attributes may nest before evaluation stops with `E0611`, so that a
/// long chain of calls cannot exhaust the stack.
pub(crate) const MAX_NESTING: u32 = 384;

/// An evaluation error that stops evaluation.
pub(crate) struct Stop(pub(crate) Box<FileDiagnostic>);

pub(crate) type R<T> = Result<T, Stop>;

impl Stop {
    pub(crate) fn with_help(mut self, help: &str) -> Self {
        self.0.diagnostic.help = Some(help.to_owned());
        self
    }
}

/// The result of evaluating a checked program: the merged value of the
/// root struct, and where each of its attributes came from.
#[derive(Debug, Clone)]
pub struct Evaluated {
    value: Value,
    diagnostics: Diagnostics,
    spans: SpanTree,
    symbols: Vec<(String, String, SourceSpan)>,
}

impl Evaluated {
    /// Returns the value of the root struct.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Returns the value of the attribute at `path`.
    ///
    /// # Errors
    ///
    /// Returns `E0805` when `path` names no attribute of the value.
    pub fn value_at(&self, path: &AttrPath) -> Result<&Value, Diagnostics> {
        self.value
            .at(path)
            .ok_or_else(|| self.error(path, "E0805", format!("there is no attribute `{path}`")))
    }

    /// Converts the value at `path` to `T`, as if `T` were deserialized
    /// from the JSON rendering of that value.
    ///
    /// # Errors
    ///
    /// Returns `E0805` when `path` names no attribute of the value or the
    /// value does not convert to `T`. The error points at the source of
    /// the attribute when it has one.
    ///
    /// # Examples
    ///
    /// ```
    /// use stav::{AttrPath, Host, ImportRoot, Limits, MemoryLoader, RootSpec};
    ///
    /// let source = "struct Root { ports: List<Int> = [] }\nports = [80, 443]\n";
    /// let host = Host::new(MemoryLoader::new().with_file("app/main.stav", source));
    /// let root = RootSpec {
    ///     root_struct: "Root".to_owned(),
    ///     entries: vec!["app/main.stav".into()],
    ///     import_roots: vec![ImportRoot::new("app")],
    /// };
    /// let evaluated = stav::check(&host, &root)?.eval(&Limits::default())?;
    /// let ports: Vec<u16> = evaluated.get(&AttrPath::new(["ports"]))?;
    /// assert_eq!(ports, [80, 443]);
    /// # Ok::<(), stav::Diagnostics>(())
    /// ```
    pub fn get<T: DeserializeOwned>(&self, path: &AttrPath) -> Result<T, Diagnostics> {
        T::deserialize(self.value_at(path)?).map_err(|e| {
            self.error(
                path,
                "E0805",
                format!("`{path}` does not convert to the requested type: {e}"),
            )
        })
    }

    /// Returns the source locations of every contribution to `path`, the
    /// winners of the merge first, each group in the order of the
    /// contributions. A block contributes the span of its statement, an
    /// entry of a struct or map literal the span of the entry, and a
    /// declared default the span of its expression. An attribute without
    /// contributions of its own returns those of the attributes inside it.
    ///
    /// An element of a list or set, named by its position as in
    /// `packages.3`, returns the span of the element of the list literal
    /// that wrote it, or else of the contribution it came from. An element
    /// of a set returns the spans of every element equal to it. A path
    /// further inside a value, such as a field of a struct in a list or a
    /// key of a `Data` map, returns the spans of the deepest attribute or
    /// element on the path. A path that names nothing in the value returns
    /// no spans.
    ///
    /// # Examples
    ///
    /// ```
    /// use stav::{AttrPath, Host, ImportRoot, Limits, MemoryLoader, RootSpec};
    ///
    /// let source = "struct Root { ports: List<Int> = [], env: Map<Str, Str> = {} }\n\
    ///               ports = [80, 443]\nenv = { LANG = \"C\", TZ = \"UTC\" }\n";
    /// let host = Host::new(MemoryLoader::new().with_file("app/main.stav", source));
    /// let root = RootSpec {
    ///     root_struct: "Root".to_owned(),
    ///     entries: vec!["app/main.stav".into()],
    ///     import_roots: vec![ImportRoot::new("app")],
    /// };
    /// let evaluated = stav::check(&host, &root)?.eval(&Limits::default())?;
    /// let text = |path: &str| -> Vec<&str> {
    ///     let path: AttrPath = path.parse().unwrap();
    ///     evaluated.spans(&path).iter().map(|s| &source[s.span.range()]).collect()
    /// };
    /// assert_eq!(text("ports.1"), ["443"]);
    /// assert_eq!(text("env.TZ"), ["TZ = \"UTC\""]);
    /// # Ok::<(), stav::Diagnostics>(())
    /// ```
    pub fn spans(&self, path: &AttrPath) -> Vec<SourceSpan> {
        self.spans.spans(path, self.value.at(path).is_some())
    }

    /// Returns every value of the symbol type named `ty` in the final
    /// value, with the place that wrote it, in the order of the value.
    /// Hosts validate these, for example against a package index.
    pub fn symbols(&self, ty: &str) -> impl Iterator<Item = (&str, SourceSpan)> {
        self.symbols
            .iter()
            .filter(move |(t, ..)| t == ty)
            .map(|(_, text, span)| (text.as_str(), *span))
    }

    /// Renders the value in `format`, as [`Value::render`] describes.
    ///
    /// # Errors
    ///
    /// Returns `E0804` when the value cannot be expressed in `format`,
    /// pointing at the source of the attribute that cannot be rendered.
    pub fn render(&self, format: Format) -> Result<String, Diagnostics> {
        self.render_at(&AttrPath::root(), format)
    }

    /// Renders the value of the attribute at `path` in `format`, as
    /// [`Value::render`] describes.
    ///
    /// # Errors
    ///
    /// Returns `E0805` when `path` names no attribute of the value, and
    /// `E0804` when the attribute cannot be expressed in `format`, pointing
    /// at the source of the attribute that cannot be rendered. Its path is
    /// written from the root of the value.
    pub fn render_at(&self, path: &AttrPath, format: Format) -> Result<String, Diagnostics> {
        self.value_at(path)?.render(format).map_err(|e| {
            let full = AttrPath::new(path.keys().iter().chain(e.path.keys()).cloned());
            self.error(
                &full,
                "E0804",
                format!("cannot render `{full}` as {format}: {}", e.message),
            )
        })
    }

    /// Returns the value as canonical JSON, as
    /// [`Value::to_canonical_json`] describes.
    pub fn to_canonical_json(&self) -> String {
        self.value.to_canonical_json()
    }

    /// Returns diagnostics about this program, such as the errors of host
    /// validation, that render like the diagnostics of the checker and the
    /// evaluator.
    pub fn diagnostics(&self, items: impl IntoIterator<Item = FileDiagnostic>) -> Diagnostics {
        Diagnostics {
            files: self.diagnostics.files.clone(),
            items: items.into_iter().collect(),
        }
    }

    fn error(&self, path: &AttrPath, code: &'static str, message: String) -> Diagnostics {
        let span = self.spans(path).first().copied();
        let diagnostic = Diagnostic::error(
            code,
            span.map_or(Span { start: 0, end: 0 }, |s| s.span),
            message,
        );
        self.diagnostics([FileDiagnostic {
            file: span.map(|s| s.file),
            diagnostic,
        }])
    }
}

impl Checked {
    /// Evaluates the program within `limits`.
    ///
    /// Statements are evaluated in order: the host's implicit parts first,
    /// then the entry files. Their contributions are merged by priority,
    /// `self` is resolved on the merged value, and the final value is
    /// checked for missing required fields, refinements and references.
    ///
    /// # Errors
    ///
    /// Returns the diagnostics of the evaluation when it fails: the first
    /// error of evaluating an expression, such as a division by zero or a
    /// crossed limit, or every conflict of the merge, every `self` cycle,
    /// every missing required field, every violated refinement and every
    /// reference to an undeclared block.
    ///
    /// # Examples
    ///
    /// ```
    /// use stav::{Host, ImportRoot, Limits, MemoryLoader, RootSpec};
    ///
    /// let base = "struct Root { port: Int = 80 }\nport = default(8080)\n";
    /// let prod = "port = 443\n";
    /// let loader = MemoryLoader::new()
    ///     .with_file("app/base.stav", base)
    ///     .with_file("app/prod.stav", prod);
    /// let root = RootSpec {
    ///     root_struct: "Root".to_owned(),
    ///     entries: vec!["app/base.stav".into(), "app/prod.stav".into()],
    ///     import_roots: vec![ImportRoot::new("app")],
    /// };
    /// let evaluated = stav::check(&Host::new(loader), &root)?.eval(&Limits::default())?;
    /// assert_eq!(evaluated.to_canonical_json(), r#"{"port":443}"#);
    /// # Ok::<(), stav::Diagnostics>(())
    /// ```
    pub fn eval(&self, limits: &Limits) -> Result<Evaluated, Diagnostics> {
        let mut eval = Eval::new(self, *limits);
        let result = eval.run();
        let files = self.named_sources();
        match result {
            Ok((value, spans, symbols)) if eval.errors.is_empty() => Ok(Evaluated {
                value,
                diagnostics: Diagnostics {
                    files,
                    items: Vec::new(),
                },
                spans,
                symbols,
            }),
            Ok(_) => Err(Diagnostics {
                files,
                items: sorted(eval.errors),
            }),
            Err(Stop(stop)) => {
                let mut items = eval.errors;
                items.push(*stop);
                Err(Diagnostics {
                    files,
                    items: sorted(items),
                })
            }
        }
    }
}

/// Returns diagnostics in the order of their places in the source.
fn sorted(mut items: Vec<FileDiagnostic>) -> Vec<FileDiagnostic> {
    items.sort_by_key(|d| (d.file, d.diagnostic.span.start, d.diagnostic.span.end));
    items
}

/// The state of one evaluation.
pub(crate) struct Eval<'c> {
    pub(crate) c: &'c Checked,
    limits: Limits,
    steps: u64,
    bytes: u64,
    pub(crate) nesting: u32,
    /// The calls being evaluated, innermost last, with the name of the
    /// function and the place of the call.
    pub(crate) calls: Vec<(String, SourceSpan)>,
    file_units: BTreeMap<FileId, UnitId>,
    pub(crate) globals: Vec<GlobalSlot>,
    pub(crate) caches: merge::Caches,
    pub(crate) regexes: BTreeMap<String, Option<Rc<regex::Regex>>>,
    pub(crate) merge: merge::Merge,
    /// The `part { }` bodies being evaluated, innermost last.
    pub(crate) parts: Vec<body::PartFrame>,
    /// The parts included into the blocks of the root so far, by the path
    /// of the block.
    pub(crate) included: BTreeSet<(Vec<merge::Seg>, val::PartId)>,
    /// The identity of every part value evaluated so far.
    pub(crate) part_ids: BTreeMap<body::PartKey, u32>,
    /// The steps taken comparing the keys of `part_ids`, not counted yet.
    pub(crate) work: Rc<std::cell::Cell<u64>>,
    /// Errors that do not stop evaluation, such as merge conflicts.
    pub(crate) errors: Vec<FileDiagnostic>,
}

/// The state of a top-level `let` or function.
#[derive(Debug, Clone)]
pub(crate) enum GlobalSlot {
    Unset,
    Ready(Val),
    /// A top-level `let` of an entry file that reads `self` of the root,
    /// evaluated when a deferred contribution first uses it.
    Late,
}

impl<'c> Eval<'c> {
    fn new(c: &'c Checked, limits: Limits) -> Self {
        let mut file_units = BTreeMap::new();
        for (i, unit) in c.units.iter().enumerate() {
            if unit.target.is_none() {
                file_units.entry(unit.file).or_insert(UnitId::new(i));
            }
        }
        Self {
            c,
            limits,
            steps: 0,
            bytes: 0,
            nesting: 0,
            calls: Vec::new(),
            file_units,
            globals: vec![GlobalSlot::Unset; c.globals.len()],
            caches: merge::Caches::default(),
            regexes: BTreeMap::new(),
            merge: merge::Merge::default(),
            parts: Vec::new(),
            included: BTreeSet::new(),
            part_ids: BTreeMap::new(),
            work: Rc::default(),
            errors: Vec::new(),
        }
    }

    #[allow(clippy::type_complexity, reason = "the parts of an evaluation result")]
    fn run(&mut self) -> R<(Value, SpanTree, Vec<(String, String, SourceSpan)>)> {
        self.globals_in_order()?;
        let pending = self.bodies()?;
        let root = self.merge_all(pending)?;
        if !self.errors.is_empty() {
            return Ok((Value::None, SpanTree::default(), Vec::new()));
        }
        self.finish(&root)
    }

    /// Returns the unit of a file that is not a part file.
    pub(crate) fn file_unit(&self, file: FileId) -> Option<UnitId> {
        self.file_units.get(&file).copied()
    }

    pub(crate) fn unit_file(&self, unit: UnitId) -> FileId {
        self.c
            .units
            .get(unit.index())
            .map_or(FileId::new(0), |u| u.file)
    }

    pub(crate) fn tree(&self, file: FileId) -> &'c SyntaxTree {
        &self.c.program.file(file).parsed.tree
    }

    pub(crate) fn fact(&self, unit: UnitId, node: usize) -> &'c Fact {
        self.c
            .units
            .get(unit.index())
            .and_then(|u| u.facts.get(node))
            .unwrap_or(&Fact::None)
    }

    /// Returns the expression with the given node index in the file of
    /// `unit`.
    pub(crate) fn expr_at(&self, unit: UnitId, node: usize) -> R<crate::ast::Expr<'c>> {
        let file = self.unit_file(unit);
        self.tree(file)
            .node(node)
            .and_then(crate::ast::Expr::cast)
            .ok_or_else(|| self.bug(file, Span { start: 0, end: 0 }, "an expression is missing"))
    }

    /// Returns an error for a state that the checker rules out.
    pub(crate) fn bug(&self, file: FileId, span: Span, what: &str) -> Stop {
        Stop(Box::new(FileDiagnostic {
            file: Some(file),
            diagnostic: Diagnostic::error("E0600", span, format!("internal error: {what}"))
                .with_note("the checker accepted a program that cannot be evaluated"),
        }))
    }

    /// Returns an error that stops evaluation, with the calls that led to
    /// it as labels.
    pub(crate) fn stop(&self, at: SourceSpan, diagnostic: Diagnostic) -> Stop {
        let mut diagnostic = diagnostic;
        for (name, site) in self.calls.iter().rev().take(8) {
            diagnostic = diagnostic.with_label(
                Some(site.file),
                site.span,
                format!("in this call of `{name}`"),
            );
        }
        Stop(Box::new(FileDiagnostic {
            file: Some(at.file),
            diagnostic,
        }))
    }

    pub(crate) fn fail(
        &self,
        at: SourceSpan,
        code: &'static str,
        message: impl Into<String>,
    ) -> Stop {
        self.stop(at, Diagnostic::error(code, at.span, message))
    }

    /// Counts one evaluation step.
    pub(crate) fn step(&mut self, at: SourceSpan) -> R<()> {
        self.charge(1, at)
    }

    /// Counts `n` evaluation steps.
    pub(crate) fn charge(&mut self, n: u64, at: SourceSpan) -> R<()> {
        self.steps = self.steps.saturating_add(n);
        if self.steps > self.limits.steps {
            return Err(self.stop(
                at,
                Diagnostic::error(
                    "E0611",
                    at.span,
                    format!("evaluation needs more than {} steps", self.limits.steps),
                )
                .with_help("make the computation smaller, or raise the step limit"),
            ));
        }
        Ok(())
    }

    /// Enters one level of nesting.
    pub(crate) fn enter(&mut self, at: SourceSpan) -> R<()> {
        self.nesting += 1;
        if self.nesting > MAX_NESTING {
            return Err(self.stop(
                at,
                Diagnostic::error(
                    "E0611",
                    at.span,
                    format!("evaluation nests more than {MAX_NESTING} levels deep"),
                )
                .with_help("flatten the chain of calls or of `self` dependencies"),
            ));
        }
        Ok(())
    }

    pub(crate) fn leave(&mut self) {
        self.nesting = self.nesting.saturating_sub(1);
    }

    fn too_large(&self, at: SourceSpan, what: String) -> Stop {
        self.stop(
            at,
            Diagnostic::error("E0612", at.span, what)
                .with_help("make the value smaller, or raise the size limits"),
        )
    }

    /// Counts `bytes` towards the size of the values built.
    fn allocate(&mut self, bytes: u64, at: SourceSpan) -> R<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > self.limits.value_bytes {
            return Err(self.too_large(
                at,
                format!(
                    "evaluation builds more than {} bytes of values",
                    self.limits.value_bytes
                ),
            ));
        }
        self.charge(bytes / 64, at)
    }

    fn collection(&mut self, len: usize, depth: u32, at: SourceSpan) -> R<()> {
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        if len > self.limits.collection_len {
            return Err(self.too_large(
                at,
                format!(
                    "a collection has more than {} elements",
                    self.limits.collection_len
                ),
            ));
        }
        if depth > MAX_VALUE_DEPTH {
            return Err(self.too_large(
                at,
                format!("a value nests more than {MAX_VALUE_DEPTH} levels deep"),
            ));
        }
        self.charge(len, at)?;
        self.allocate(len.saturating_mul(16), at)
    }

    /// Checks that a collection of `len` elements may be built, before
    /// building it.
    pub(crate) fn reserve(&mut self, len: u64, at: SourceSpan) -> R<()> {
        if len > self.limits.collection_len {
            return Err(self.too_large(
                at,
                format!(
                    "a collection has more than {} elements",
                    self.limits.collection_len
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn new_str(&mut self, text: String, at: SourceSpan) -> R<Val> {
        let len = u64::try_from(text.len()).unwrap_or(u64::MAX);
        if len > self.limits.string_bytes {
            return Err(self.too_large(
                at,
                format!("a string is longer than {} bytes", self.limits.string_bytes),
            ));
        }
        self.allocate(len, at)?;
        Ok(Val::Str(text.into()))
    }

    /// Checks that a string of `len` bytes may be built, before building
    /// it.
    pub(crate) fn reserve_str(&mut self, len: usize, at: SourceSpan) -> R<()> {
        if u64::try_from(len).unwrap_or(u64::MAX) > self.limits.string_bytes {
            return Err(self.too_large(
                at,
                format!("a string is longer than {} bytes", self.limits.string_bytes),
            ));
        }
        Ok(())
    }

    pub(crate) fn new_list(&mut self, items: Vec<Val>, at: SourceSpan) -> R<Val> {
        let depth = depth_of(&items);
        self.collection(items.len(), depth, at)?;
        Ok(Val::List(Rc::new(Seq { items, depth })))
    }

    pub(crate) fn new_set(&mut self, items: Vec<Val>, at: SourceSpan) -> R<Val> {
        let items = canonical(items);
        let depth = depth_of(&items);
        self.collection(items.len(), depth, at)?;
        Ok(Val::Set(Rc::new(Seq { items, depth })))
    }

    /// Builds a map from entries whose keys are distinct.
    pub(crate) fn new_map(&mut self, mut entries: Vec<(Val, Val)>, at: SourceSpan) -> R<Val> {
        entries.sort_by(|a, b| a.0.order(&b.0));
        let depth = depth_of(entries.iter().map(|(_, v)| v));
        self.collection(entries.len(), depth, at)?;
        Ok(Val::Map(Rc::new(MapVal { entries, depth })))
    }

    pub(crate) fn new_struct(&mut self, id: StructId, fields: Vec<Val>, at: SourceSpan) -> R<Val> {
        let set = vec![true; fields.len()];
        self.new_struct_set(id, fields, set, at)
    }

    /// Builds a struct value, with which fields were set rather than
    /// defaulted.
    pub(crate) fn new_struct_set(
        &mut self,
        id: StructId,
        fields: Vec<Val>,
        set: Vec<bool>,
        at: SourceSpan,
    ) -> R<Val> {
        let depth = depth_of(&fields);
        self.collection(fields.len(), depth, at)?;
        Ok(Val::Struct(Rc::new(StructVal {
            id,
            fields,
            set,
            depth,
        })))
    }

    pub(crate) fn new_variant(
        &mut self,
        sum: SumId,
        index: usize,
        fields: Option<Vec<Val>>,
        at: SourceSpan,
    ) -> R<Val> {
        let set = fields.as_ref().map(|f| vec![true; f.len()]);
        self.new_variant_set(sum, index, fields.zip(set), at)
    }

    /// Builds a variant, with its payload fields and which of them were
    /// given rather than defaulted.
    pub(crate) fn new_variant_set(
        &mut self,
        sum: SumId,
        index: usize,
        payload: Option<(Vec<Val>, Vec<bool>)>,
        at: SourceSpan,
    ) -> R<Val> {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let Some((fields, set)) = payload else {
            return Ok(Val::Tag(sum, index));
        };
        let depth = depth_of(&fields);
        self.collection(fields.len(), depth, at)?;
        Ok(Val::Variant(Rc::new(VariantVal {
            sum,
            index,
            fields,
            set,
            depth,
        })))
    }

    /// Evaluates every top-level `let` in an order where each comes after
    /// the declarations it uses.
    fn globals_in_order(&mut self) -> R<()> {
        for g in self.c.let_order.clone() {
            if self.c.let_reads(g) {
                self.globals[g.index()] = GlobalSlot::Late;
                continue;
            }
            let value = self.global_let(g, &[])?;
            self.globals[g.index()] = GlobalSlot::Ready(value);
        }
        Ok(())
    }

    /// Evaluates the value of a top-level `let`, checking it against its
    /// annotation.
    pub(crate) fn global_let(&mut self, g: GlobalId, frames: &[merge::Frame]) -> R<Val> {
        let global = &self.c.globals[g.index()];
        let file = global.file;
        let Some(unit) = self.file_unit(file) else {
            return Err(self.bug(file, global.span, "a declaration has no unit"));
        };
        let decl = self
            .tree(file)
            .node(global.node)
            .and_then(crate::ast::Let::cast);
        let Some(value) = decl.and_then(crate::ast::Let::value) else {
            return Err(self.bug(file, global.span, "a `let` has no value"));
        };
        let cx = expr::Cx {
            unit,
            file,
            env: Env::default(),
            frames,
        };
        let val = self.expr(&cx, value)?;
        if let Some(annotation) = self.c.let_annotation(g) {
            self.refine(
                annotation,
                &val,
                SourceSpan {
                    file,
                    span: value.syntax().span(),
                },
            )?;
        }
        Ok(val)
    }

    /// Returns the value of a top-level `let` or function.
    pub(crate) fn global(
        &mut self,
        g: GlobalId,
        at: SourceSpan,
        frames: &[merge::Frame],
    ) -> R<Val> {
        match self.globals.get(g.index()).cloned() {
            Some(GlobalSlot::Ready(v)) => Ok(v),
            Some(GlobalSlot::Late) => {
                let root = [merge::Frame::Node(NodeId::ROOT)];
                let frames = if frames.is_empty() { &root[..] } else { frames };
                let value = self.global_let(g, &frames[..1])?;
                self.globals[g.index()] = GlobalSlot::Ready(value.clone());
                Ok(value)
            }
            _ if self.c.is_fn(g) => Ok(Val::Fn(Rc::new(FnVal::Global(g)))),
            _ => Err(self.bug(
                at.file,
                at.span,
                "a `let` is used before its value is known",
            )),
        }
    }
}

//! Values of host module declarations supplied for one evaluation.

use std::collections::BTreeMap;
use std::path::Component;
use std::rc::Rc;

use super::val::{PathVal, Val};
use super::{Eval, MAX_VALUE_DEPTH, R};
use crate::check::types::{GlobalId, Type, TypeId};
use crate::check::{FileId, decls};
use crate::diagnostic::{Diagnostic, FileDiagnostic};
use crate::span::{SourceSpan, Span};
use crate::value::Value;

/// The values of host module declarations for one evaluation.
///
/// A top-level `let` of a host module that has a type annotation is an
/// input of the program. The program is checked once against the
/// annotation, and every evaluation may supply a different value for it.
/// Where an evaluation supplies none, the value written in the module is
/// used. The values are inputs of the evaluation exactly like the source
/// files, so the same files and the same values always give the same
/// result.
///
/// A value is given in the shape of the annotated type. A struct gives
/// every field, including those that have a default. Values for a host
/// module that the program does not import are ignored, so one set of
/// values serves every program of a host. A value for a path that is no
/// host module, or for a name that is no annotated `let` of an imported
/// host module, is `E0614`.
///
/// # Examples
///
/// ```
/// use stav::{Host, HostModule, HostValues, ImportRoot, Limits, MemoryLoader, RootSpec, Value};
///
/// let main = "import \"build/target\" as target\n\
///             struct Root { triple: Str }\n\
///             triple = \"#{target.arch}-linux\"\n";
/// let mut host = Host::new(MemoryLoader::new().with_file("app/main.stav", main));
/// host.host_modules.push(HostModule {
///     path: "build/target".to_owned(),
///     source: "let arch: Str = \"x86_64\"\n".to_owned(),
/// });
/// let root = RootSpec {
///     root_struct: "Root".to_owned(),
///     entries: vec!["app/main.stav".into()],
///     import_roots: vec![ImportRoot::new("app")],
/// };
/// let checked = stav::check(&host, &root)?;
/// for arch in ["aarch64", "riscv64"] {
///     let values = HostValues::new().with("build/target", "arch", Value::Str(arch.to_owned()));
///     let evaluated = checked.eval_with(&Limits::default(), &values)?;
///     assert_eq!(evaluated.to_canonical_json(), format!(r#"{{"triple":"{arch}-linux"}}"#));
/// }
/// # Ok::<(), stav::Diagnostics>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostValues {
    values: BTreeMap<(String, String), Value>,
}

impl HostValues {
    /// Returns an empty set of values, which leaves every declaration at
    /// the value written in its module.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns these values with `value` for the `let` named `name` of the
    /// host module `module`, such as `build/target`.
    pub fn with(
        mut self,
        module: impl Into<String>,
        name: impl Into<String>,
        value: Value,
    ) -> Self {
        self.insert(module, name, value);
        self
    }

    /// Sets the value of the `let` named `name` of the host module
    /// `module`, in place of any value set before.
    pub fn insert(&mut self, module: impl Into<String>, name: impl Into<String>, value: Value) {
        self.values.insert((module.into(), name.into()), value);
    }
}

impl Eval<'_> {
    /// Finds the declaration of every supplied value, and reports the
    /// values that name no annotated `let` of a host module.
    pub(crate) fn host_inputs(&mut self, values: &HostValues) -> BTreeMap<GlobalId, Value> {
        let mut found = BTreeMap::new();
        for ((module, name), value) in &values.values {
            let files = self.c.host_module_files(module);
            if files.is_empty() {
                if !self.c.host_modules.contains(module) {
                    self.input_error(
                        None,
                        Span { start: 0, end: 0 },
                        format!(
                            "a value is given for `{name}` of `{module}`, which is no host module"
                        ),
                    );
                }
                continue;
            }
            let declared = self.c.globals.iter().enumerate().find(|(_, g)| {
                g.name == *name
                    && files.contains(&g.file)
                    && matches!(&g.kind, decls::GlobalKind::Let(l) if l.annotation.is_some())
            });
            match declared {
                Some((i, _)) => {
                    found.insert(GlobalId::new(i), value.clone());
                }
                None => self.input_error(
                    files.first().copied(),
                    Span { start: 0, end: 0 },
                    format!(
                        "a value is given for `{name}`, which is no annotated `let` of `{module}`"
                    ),
                ),
            }
        }
        found
    }

    fn input_error(&mut self, file: Option<FileId>, span: Span, message: String) {
        self.errors.push(FileDiagnostic {
            file,
            diagnostic: Diagnostic::error("E0614", span, message)
                .with_help("give values only for top-level `let` declarations of host modules that have a type"),
        });
    }

    /// Converts the value supplied for the top-level `let` `g` to its
    /// annotated type and checks its refinements.
    pub(crate) fn host_input(&mut self, g: GlobalId, value: &Value) -> R<Val> {
        let global = &self.c.globals[g.index()];
        let at = SourceSpan {
            file: global.file,
            span: global.span,
        };
        let name = global.name.clone();
        let Some(ty) = self.c.let_annotation(g) else {
            return Err(self.bug(at.file, at.span, "a host input has no annotation"));
        };
        let converted = match self.convert(ty, value, at, 0) {
            Ok(v) => v,
            Err(Mismatch(detail)) => {
                let shown = self.c.types.render_plain(ty);
                return Err(self.fail(
                    at,
                    "E0614",
                    format!("the value given for `{name}` is not a `{shown}`: {detail}"),
                ));
            }
            Err(Stopped(stop)) => return Err(stop),
        };
        self.refine(ty, &converted, at)?;
        Ok(converted)
    }

    fn convert(
        &mut self,
        ty: TypeId,
        value: &Value,
        at: SourceSpan,
        depth: u32,
    ) -> Result<Val, Failure> {
        if depth > MAX_VALUE_DEPTH {
            return Err(Mismatch(format!(
                "it nests more than {MAX_VALUE_DEPTH} levels deep"
            )));
        }
        let types = &self.c.types;
        let shown = || types.render_plain(ty);
        let wrong = |what: &str| Mismatch(format!("expected `{}`, found {what}", shown()));
        let found = kind(value);
        let val = match (types.get(ty).clone(), value) {
            (Type::Refined(base, _), _) => return self.convert(base, value, at, depth),
            (Type::Option(_), Value::None) => Val::None,
            (Type::Option(inner), _) => return self.convert(inner, value, at, depth),
            (Type::Bool, Value::Bool(b)) => Val::Bool(*b),
            (Type::Int, Value::Int(i)) => Val::Int(*i),
            (Type::Float, Value::Float(f)) if f.is_finite() => Val::Float(*f),
            (Type::Str, Value::Str(s)) => self.new_str(s.clone(), at).map_err(Stopped)?,
            (Type::Path, Value::Path(p)) => self.path_input(p).ok_or_else(|| {
                Mismatch(format!("`{}` is not inside an import root", p.display()))
            })?,
            (Type::Symbol(s), Value::Str(text) | Value::Symbol { text, .. }) => {
                self.symbol(s, text, at).map_err(Stopped)?
            }
            (Type::Ref(t), Value::Str(label) | Value::Ref(label)) => {
                self.reference(t, label.clone(), at)
            }
            (Type::List(t), Value::List(items)) => {
                let items = self.convert_all(t, items, at, depth)?;
                self.new_list(items, at).map_err(Stopped)?
            }
            (Type::Set(t), Value::Set(items) | Value::List(items)) => {
                let items = self.convert_all(t, items, at, depth)?;
                self.new_set(items, at).map_err(Stopped)?
            }
            (Type::Map(k, v), Value::Map(entries)) => {
                let mut out = Vec::with_capacity(entries.len());
                for (key, item) in entries {
                    let key = self.input_key(k, key)?;
                    out.push((key, self.convert(v, item, at, depth + 1)?));
                }
                out.sort_by(|a, b| a.0.order(&b.0));
                if out.windows(2).any(|w| w[0].0.equals(&w[1].0)) {
                    return Err(Mismatch("a key appears twice".to_owned()));
                }
                self.new_map(out, at).map_err(Stopped)?
            }
            (Type::Struct(s), Value::Struct { fields, .. } | Value::Map(fields)) => {
                let def = self.c.types.structure(s).clone();
                if let Some((extra, _)) = fields.iter().find(|(k, _)| def.field(k).is_none()) {
                    return Err(Mismatch(format!("`{}` has no field `{extra}`", def.name)));
                }
                let mut out = Vec::with_capacity(def.fields.len());
                for field in &def.fields {
                    let Some((_, item)) = fields.iter().find(|(k, _)| *k == field.name) else {
                        return Err(Mismatch(format!("the field `{}` is missing", field.name)));
                    };
                    out.push(self.convert(field.ty, item, at, depth + 1)?);
                }
                self.new_struct(s, out, at).map_err(Stopped)?
            }
            (Type::Sum(s), Value::Str(name)) => {
                let sum = self.c.types.sum(s);
                match sum.variant(name) {
                    Some((index, v)) if v.fields.is_none() => {
                        self.new_variant(s, index, None, at).map_err(Stopped)?
                    }
                    _ => {
                        return Err(Mismatch(format!(
                            "`{name}` is no variant without fields of `{}`",
                            sum.name
                        )));
                    }
                }
            }
            (Type::Sum(s), Value::Variant { name, fields, .. }) => {
                let sum = self.c.types.sum(s).clone();
                let Some((index, variant)) = sum.variant(name) else {
                    return Err(Mismatch(format!(
                        "`{name}` is no variant of `{}`",
                        sum.name
                    )));
                };
                let payload = match (&variant.fields, fields) {
                    (None, None) => None,
                    (Some(params), Some(given)) => {
                        if let Some((extra, _)) = given
                            .iter()
                            .find(|(k, _)| !params.iter().any(|p| p.name == *k))
                        {
                            return Err(Mismatch(format!("`{name}` has no field `{extra}`")));
                        }
                        let mut out = Vec::with_capacity(params.len());
                        for param in params {
                            let Some((_, item)) = given.iter().find(|(k, _)| *k == param.name)
                            else {
                                return Err(Mismatch(format!(
                                    "the field `{}` of `{name}` is missing",
                                    param.name
                                )));
                            };
                            out.push(self.convert(param.ty, item, at, depth + 1)?);
                        }
                        Some(out)
                    }
                    (None, Some(_)) => return Err(Mismatch(format!("`{name}` has no fields"))),
                    (Some(_), None) => return Err(Mismatch(format!("`{name}` needs its fields"))),
                };
                self.new_variant(s, index, payload, at).map_err(Stopped)?
            }
            (Type::Data, _) => self.data(value, at, depth)?,
            _ => return Err(wrong(found)),
        };
        Ok(val)
    }

    fn convert_all(
        &mut self,
        ty: TypeId,
        items: &[Value],
        at: SourceSpan,
        depth: u32,
    ) -> Result<Vec<Val>, Failure> {
        self.reserve(u64::try_from(items.len()).unwrap_or(u64::MAX), at)
            .map_err(Stopped)?;
        items
            .iter()
            .map(|item| self.convert(ty, item, at, depth + 1))
            .collect()
    }

    /// Converts a map key: a string, or the name of a variant without
    /// fields of a sum key type.
    fn input_key(&mut self, ty: TypeId, key: &str) -> Result<Val, Failure> {
        let types = &self.c.types;
        match types.get(types.peel(ty)) {
            Type::Sum(s) => {
                let s = *s;
                let sum = types.sum(s);
                match sum.variant(key) {
                    Some((index, v)) if v.fields.is_none() => {
                        Ok(Val::Tag(s, u32::try_from(index).unwrap_or(u32::MAX)))
                    }
                    _ => Err(Mismatch(format!(
                        "the key `{key}` is no variant of `{}`",
                        sum.name
                    ))),
                }
            }
            _ => Ok(Val::Str(key.into())),
        }
    }

    /// Converts a value of type `Data`: maps, structs and variants become
    /// maps, and symbols, references and paths become strings.
    fn data(&mut self, value: &Value, at: SourceSpan, depth: u32) -> Result<Val, Failure> {
        if depth > MAX_VALUE_DEPTH {
            return Err(Mismatch(format!(
                "it nests more than {MAX_VALUE_DEPTH} levels deep"
            )));
        }
        let entries = |this: &mut Self, entries: &[(String, Value)]| -> Result<Val, Failure> {
            let mut out = Vec::with_capacity(entries.len());
            for (key, item) in entries {
                out.push((
                    Val::Str(key.as_str().into()),
                    this.data(item, at, depth + 1)?,
                ));
            }
            out.sort_by(|a, b| a.0.order(&b.0));
            if out.windows(2).any(|w| w[0].0.equals(&w[1].0)) {
                return Err(Mismatch("a key appears twice".to_owned()));
            }
            this.new_map(out, at).map_err(Stopped)
        };
        Ok(match value {
            Value::None => Val::None,
            Value::Bool(b) => Val::Bool(*b),
            Value::Int(i) => Val::Int(*i),
            Value::Float(f) if f.is_finite() => Val::Float(*f),
            Value::Float(_) => return Err(Mismatch("a float is not finite".to_owned())),
            Value::Str(s) | Value::Symbol { text: s, .. } | Value::Ref(s) => {
                self.new_str(s.clone(), at).map_err(Stopped)?
            }
            Value::Path(p) => self.new_str(p.display().to_string(), at).map_err(Stopped)?,
            Value::List(items) | Value::Set(items) => {
                self.reserve(u64::try_from(items.len()).unwrap_or(u64::MAX), at)
                    .map_err(Stopped)?;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.data(item, at, depth + 1)?);
                }
                if matches!(value, Value::Set(_)) {
                    self.new_set(out, at).map_err(Stopped)?
                } else {
                    self.new_list(out, at).map_err(Stopped)?
                }
            }
            Value::Map(fields) | Value::Struct { fields, .. } => entries(self, fields)?,
            Value::Variant {
                name, fields: None, ..
            } => self.new_str(name.clone(), at).map_err(Stopped)?,
            Value::Variant {
                name,
                fields: Some(fields),
                ..
            } => {
                let payload = entries(self, fields)?;
                self.new_map(vec![(Val::Str(name.as_str().into()), payload)], at)
                    .map_err(Stopped)?
            }
        })
    }

    /// Returns the path value of an absolute or relative path inside one
    /// of the import roots.
    fn path_input(&self, path: &std::path::Path) -> Option<Val> {
        self.c
            .import_roots
            .iter()
            .enumerate()
            .find_map(|(root, r)| {
                let inside = path.strip_prefix(r.path()).ok()?;
                let mut parts = Vec::new();
                for component in inside.components() {
                    match component {
                        Component::Normal(part) => parts.push(part.to_str()?),
                        _ => return None,
                    }
                }
                Some(Val::Path(Rc::new(PathVal {
                    root,
                    rel: parts.join("/"),
                })))
            })
    }
}

/// Why a supplied value could not be converted.
enum Failure {
    /// The value does not fit the type, for the reason given.
    Mismatch(String),
    /// Building the value crossed a limit.
    Stopped(super::Stop),
}

use Failure::{Mismatch, Stopped};

/// Describes the kind of a value for a diagnostic.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::None => "`none`",
        Value::Bool(_) => "a `Bool`",
        Value::Int(_) => "an `Int`",
        Value::Float(_) => "a `Float`",
        Value::Str(_) => "a `Str`",
        Value::Path(_) => "a `Path`",
        Value::Symbol { .. } => "a symbol",
        Value::Ref(_) => "a reference",
        Value::List(_) => "a list",
        Value::Set(_) => "a set",
        Value::Map(_) => "a map",
        Value::Struct { .. } => "a struct",
        Value::Variant { .. } => "a variant",
    }
}

//! Checks on the final value: refinements, references, and the symbols the
//! host validates.

use std::collections::{BTreeMap, BTreeSet};

use super::builtins::length;
use super::env::Env;
use super::expr::Cx;
use super::merge::{NodeId, SpanTree};
use super::val::{RefVal, Val};
use super::{Eval, R, Stop};
use crate::check::suggest::did_you_mean;
use crate::check::types::{Number, Predicate, RefinementId, StructId, Type, TypeId};
use crate::diagnostic::{Diagnostic, FileDiagnostic};
use crate::span::SourceSpan;
use crate::value::Value;

impl<'c> Eval<'c> {
    /// Checks the merged value of the root, and returns it in its public
    /// form with the span index and the symbols it holds.
    #[allow(clippy::type_complexity, reason = "the parts of an evaluation result")]
    pub(crate) fn finish(
        &mut self,
        root: &Val,
    ) -> R<(Value, SpanTree, Vec<(String, String, SourceSpan)>)> {
        self.final_refinements(NodeId::ROOT)?;
        let value = root.to_value(&self.c.types, &self.c.import_roots);
        self.check_refs(root, &value);
        let mut symbols = Vec::new();
        self.collect_symbols(root, &mut symbols);
        Ok((value, self.walk_spans(), symbols))
    }

    /// Checks the refinements of a value against `ty` and stops at the
    /// first violation. Struct values inside the value are checked when
    /// they reach the final value.
    pub(crate) fn refine(&mut self, ty: TypeId, value: &Val, at: SourceSpan) -> R<()> {
        let mut found = Vec::new();
        self.refinements(ty, value, at, Depth::Values, &mut found)?;
        match found.into_iter().next() {
            Some(first) => Err(Stop(Box::new(first))),
            None => Ok(()),
        }
    }

    /// Checks every refinement of the final value, attribute by attribute,
    /// so that each violation points at the contribution that set it.
    fn final_refinements(&mut self, root: NodeId) -> R<()> {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            let Some(value) = self.final_value(n) else {
                continue;
            };
            let ty = self.node_type(n);
            let at = self.node_at(n);
            let mut found = Vec::new();
            let inside = self.inside_nodes(n);
            let depth = if inside.is_empty() {
                Depth::Fields
            } else {
                Depth::Top
            };
            self.refinements(ty, &value, at, depth, &mut found)?;
            self.errors.extend(found);
            stack.extend(inside.into_iter().rev());
        }
        Ok(())
    }

    /// Checks the refinements of `ty` on `value` to the given depth.
    fn refinements(
        &mut self,
        ty: TypeId,
        value: &Val,
        at: SourceSpan,
        depth: Depth,
        found: &mut Vec<FileDiagnostic>,
    ) -> R<()> {
        if value.is_none() {
            return Ok(());
        }
        let mut current = ty;
        loop {
            match self.c.types.get(current).clone() {
                Type::Refined(base, refinement) => {
                    if let Some(diagnostic) = self.violation(refinement, value, at)? {
                        found.push(diagnostic);
                        return Ok(());
                    }
                    current = base;
                }
                Type::Option(inner) => current = inner,
                _ => break,
            }
        }
        if depth == Depth::Top {
            return Ok(());
        }
        let deep = depth == Depth::Fields;
        match (self.c.types.get(current).clone(), value) {
            (Type::List(t) | Type::Set(t), Val::List(s) | Val::Set(s)) => {
                for item in &s.items {
                    self.refinements(t, item, at, depth, found)?;
                }
            }
            (Type::Map(_, v), Val::Map(m)) => {
                for (_, item) in &m.entries {
                    self.refinements(v, item, at, depth, found)?;
                }
            }
            (Type::Struct(s), Val::Struct(v)) if deep => {
                let fields: Vec<TypeId> = self
                    .c
                    .types
                    .structure(s)
                    .fields
                    .iter()
                    .map(|f| f.ty)
                    .collect();
                for (field, item) in fields.into_iter().zip(&v.fields) {
                    self.refinements(field, item, at, depth, found)?;
                }
            }
            (Type::Sum(s), Val::Variant(v)) if deep => {
                let params: Vec<TypeId> = self
                    .c
                    .types
                    .sum(s)
                    .variants
                    .get(v.index as usize)
                    .and_then(|v| v.fields.as_ref())
                    .map(|f| f.iter().map(|p| p.ty).collect())
                    .unwrap_or_default();
                for (param, item) in params.into_iter().zip(&v.fields) {
                    self.refinements(param, item, at, depth, found)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Returns the diagnostic of the first predicate of a refinement that
    /// `value` violates.
    fn violation(
        &mut self,
        refinement: RefinementId,
        value: &Val,
        at: SourceSpan,
    ) -> R<Option<FileDiagnostic>> {
        let def = &self.c.types.refinements[refinement.index()];
        let (file, span) = (def.file, def.span);
        for predicate in &def.predicates {
            let Some(detail) = self.predicate(predicate, value, file, at)? else {
                continue;
            };
            let diagnostic = Diagnostic::error("E0803", at.span, "value violates a refinement")
                .with_label(Some(at.file), at.span, detail)
                .with_label(Some(file), span, "constraint declared here");
            return Ok(Some(FileDiagnostic {
                file: Some(at.file),
                diagnostic,
            }));
        }
        Ok(None)
    }

    /// Returns what is wrong when `value` violates `predicate`.
    fn predicate(
        &mut self,
        predicate: &'c Predicate,
        value: &Val,
        declared_in: crate::check::FileId,
        at: SourceSpan,
    ) -> R<Option<String>> {
        let shown = || {
            value
                .to_value(&self.c.types, &self.c.import_roots)
                .describe()
        };
        match predicate {
            Predicate::Range {
                start,
                end,
                inclusive,
                length: measured,
            } => {
                let x = if *measured {
                    Number::Int(length(value))
                } else {
                    match value {
                        Val::Int(i) => Number::Int(*i),
                        Val::Float(f) => Number::Float(*f),
                        _ => return Ok(None),
                    }
                };
                let below = |a: Number, b: Number| match (a, b) {
                    (Number::Int(a), Number::Int(b)) => a < b,
                    (a, b) => float(a) < float(b),
                };
                let outside = start.is_some_and(|s| below(x, s))
                    || end.is_some_and(|e| {
                        if *inclusive {
                            below(e, x)
                        } else {
                            !below(x, e)
                        }
                    });
                if !outside {
                    return Ok(None);
                }
                let range = format!(
                    "{}{}{}",
                    start.map(number).unwrap_or_default(),
                    if *inclusive { "..=" } else { ".." },
                    end.map(number).unwrap_or_default()
                );
                Ok(Some(if *measured {
                    format!("the length {} is not in {range}", number(x))
                } else {
                    format!("{} is not in {range}", shown())
                }))
            }
            Predicate::NonEmpty => {
                Ok((length(value) == 0).then(|| format!("{} is empty", shown())))
            }
            Predicate::Matches(pattern) => {
                let Val::Str(text) = value else {
                    return Ok(None);
                };
                match self.regex(pattern, at)? {
                    Some(regex) if regex.is_match(text) => Ok(None),
                    _ => Ok(Some(format!("{} does not match `{pattern}`", shown()))),
                }
            }
            Predicate::Call { function, args } => {
                let Some(unit) = self.file_unit(declared_in) else {
                    return Err(self.bug(at.file, at.span, "a refinement has no unit"));
                };
                let mut values = vec![value.clone()];
                let cx = Cx {
                    unit,
                    file: declared_in,
                    env: Env::default(),
                    frames: &[],
                };
                for &node in args {
                    let e = self.expr_at(unit, node)?;
                    values.push(self.expr(&cx, e)?);
                }
                let name = self.c.globals[function.index()].name.clone();
                match self.call_global(*function, values, at)? {
                    Val::Bool(false) => Ok(Some(format!("`{name}` is false for {}", shown()))),
                    _ => Ok(None),
                }
            }
        }
    }

    /// Reports every reference whose label is neither a key of the target
    /// map of its struct nor an implicit target of the host.
    fn check_refs(&mut self, root: &Val, public: &Value) {
        let mut targets: BTreeMap<StructId, BTreeSet<String>> = BTreeMap::new();
        let mut refs = Vec::new();
        self.collect_refs(root, "", &mut refs);
        for (field, r) in refs {
            let labels = targets
                .entry(r.target)
                .or_insert_with(|| self.target_labels(root, public, r.target));
            if labels.contains(&r.label) {
                continue;
            }
            let (kind, name) = self.target_kind(r.target);
            let help = did_you_mean(&r.label, labels.iter().map(String::as_str));
            let subject = if field.is_empty() {
                "a reference".to_owned()
            } else {
                format!("`{field}`")
            };
            self.errors.push(FileDiagnostic {
                file: Some(r.file),
                diagnostic: Diagnostic::error(
                    "E0802",
                    r.span,
                    format!(
                        "{subject} refers to `{}`, which is not a declared `{kind}`",
                        r.label
                    ),
                )
                .with_optional_help(help)
                .with_note(format!(
                    "references to `{name}` name the blocks of `{kind}`"
                )),
            });
        }
    }

    /// Returns the labels a reference to struct `target` may name.
    fn target_labels(&self, root: &Val, public: &Value, target: StructId) -> BTreeSet<String> {
        let mut labels = BTreeSet::new();
        if let Some(steps) = self.c.ref_targets.get(&target) {
            let mut current = Some(root.clone());
            for step in steps {
                current = match (current, step) {
                    (Some(Val::Struct(s)), crate::check::model::Step::Field(i)) => {
                        s.fields.get(*i).cloned()
                    }
                    _ => None,
                };
            }
            if let Some(Val::Map(map)) = current {
                labels.extend(map.entries.iter().map(|(k, _)| k.text(&self.c.types)));
            }
        }
        let name = &self.c.types.structure(target).name;
        for implicit in &self.c.implicit_refs {
            if &implicit.target == name {
                labels.extend((implicit.labels)(public));
            }
        }
        labels
    }

    /// Returns the block kind of the target map of a struct and the name of
    /// the struct.
    fn target_kind(&self, target: StructId) -> (String, String) {
        let name = self.c.types.structure(target).name.clone();
        let mut kind = name.clone();
        if let Some(steps) = self.c.ref_targets.get(&target) {
            let mut current = Some(self.c.root);
            for step in steps {
                let (Some(s), crate::check::model::Step::Field(i)) = (current, step) else {
                    break;
                };
                let Some(field) = self.c.types.structure(s).fields.get(*i) else {
                    break;
                };
                if let Some(block) = &field.block {
                    kind = block.clone();
                }
                let inner = self.c.types.unwrap_option(field.ty).unwrap_or(field.ty);
                current = self.c.types.as_struct(inner);
            }
        }
        (kind, name)
    }

    fn collect_refs<'v>(&self, value: &'v Val, field: &str, out: &mut Vec<(String, &'v RefVal)>) {
        match value {
            Val::Ref(r) => out.push((field.to_owned(), r)),
            Val::List(s) | Val::Set(s) => {
                for item in &s.items {
                    self.collect_refs(item, field, out);
                }
            }
            Val::Map(m) => {
                for (_, item) in &m.entries {
                    self.collect_refs(item, field, out);
                }
            }
            Val::Struct(s) => {
                let def = self.c.types.structure(s.id);
                for (f, item) in def.fields.iter().zip(&s.fields) {
                    self.collect_refs(item, &f.name, out);
                }
            }
            Val::Variant(v) => {
                let params = self
                    .c
                    .types
                    .sum(v.sum)
                    .variants
                    .get(v.index as usize)
                    .and_then(|v| v.fields.as_ref());
                for (p, item) in params.into_iter().flatten().zip(&v.fields) {
                    self.collect_refs(item, &p.name, out);
                }
            }
            _ => {}
        }
    }

    fn collect_symbols(&self, value: &Val, out: &mut Vec<(String, String, SourceSpan)>) {
        match value {
            Val::Symbol(s) => {
                let ty = self
                    .c
                    .types
                    .symbols
                    .get(s.ty.index())
                    .map_or_else(String::new, |d| d.name.clone());
                out.push((
                    ty,
                    s.text.clone(),
                    SourceSpan {
                        file: s.file,
                        span: s.span,
                    },
                ));
            }
            Val::List(s) | Val::Set(s) => {
                for item in &s.items {
                    self.collect_symbols(item, out);
                }
            }
            Val::Map(m) => {
                for (_, item) in &m.entries {
                    self.collect_symbols(item, out);
                }
            }
            Val::Struct(s) => {
                for item in &s.fields {
                    self.collect_symbols(item, out);
                }
            }
            Val::Variant(v) => {
                for item in &v.fields {
                    self.collect_symbols(item, out);
                }
            }
            _ => {}
        }
    }
}

/// How far into a value refinements are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    /// Only the refinements of the type itself.
    Top,
    /// Also those of the elements of collections and options, but not the
    /// fields of structs and variants, which their own checks cover.
    Values,
    /// Also those of the fields of every struct and variant inside.
    Fields,
}

fn float(n: Number) -> f64 {
    match n {
        #[allow(clippy::cast_precision_loss, reason = "bounds are compared as floats")]
        Number::Int(i) => i as f64,
        Number::Float(f) => f,
    }
}

fn number(n: Number) -> String {
    match n {
        Number::Int(i) => i.to_string(),
        Number::Float(f) => format!("{f:?}"),
    }
}

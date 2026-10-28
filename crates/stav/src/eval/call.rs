//! Calls of functions, variants, constructors and function values, and
//! complete struct values with their defaults.

use super::env::{Slot, bind};
use super::expr::Cx;
use super::merge::Frame;
use super::val::{FnVal, Val};
use super::{Eval, R};
use crate::ast;
use crate::check::decls::GlobalKind;
use crate::check::model::{CallFact, Callee, Fact, Meaning};
use crate::check::types::{GlobalId, ParamDef, StructId, SumId, Type, TypeId};
use crate::diagnostic::{Diagnostic, Label};
use crate::span::SourceSpan;

/// A struct value being built, whose defaults may read its other fields
/// through `self`.
#[derive(Debug)]
pub(crate) struct Building {
    id: StructId,
    fields: Vec<Field>,
}

#[derive(Debug, Clone)]
enum Field {
    Ready(Val),
    Pending,
    Busy,
}

impl<'c> Eval<'c> {
    pub(crate) fn call(
        &mut self,
        cx: &Cx<'_>,
        call: ast::Call<'c>,
        fact: &CallFact,
        at: SourceSpan,
    ) -> R<Val> {
        match fact.callee {
            Callee::Global(g) => {
                let params = match &self.c.globals[g.index()].kind {
                    GlobalKind::Fn(sig) => &sig.params,
                    GlobalKind::Let(_) => {
                        return Err(self.bug(
                            cx.file,
                            at.span,
                            "a call of a `let` is not a function",
                        ));
                    }
                };
                let file = self.c.globals[g.index()].file;
                let (args, _) = self.arguments(cx, params, &fact.args, file, at)?;
                self.call_global(g, args, at)
            }
            Callee::Variant { sum, variant } => {
                let def = &self.c.types.sum(sum).variants[variant];
                let Some(params) = &def.fields else {
                    return self.new_variant(sum, variant, None, at);
                };
                let file = self.c.types.sum(sum).file;
                let payload = self.arguments(cx, params, &fact.args, file, at)?;
                self.new_variant_set(sum, variant, Some(payload), at)
            }
            Callee::Builtin(b) => self.builtin(cx, b, &fact.args, at),
            Callee::Constructor(s) => {
                let Some(Some(node)) = fact.args.first() else {
                    return Err(self.bug(cx.file, at.span, "a constructor has no argument"));
                };
                let e = self.expr_at(cx.unit, *node)?;
                match self.expr(cx, e)? {
                    Val::Str(text) => self.symbol(s, &text, cx.at(e.syntax().span())),
                    _ => Err(self.bug(cx.file, at.span, "a constructor argument is not a string")),
                }
            }
            Callee::Value => {
                let Some(callee) = call.callee() else {
                    return Err(self.bug(cx.file, at.span, "a call has no callee"));
                };
                let Val::Fn(f) = self.expr(cx, callee)? else {
                    return Err(self.bug(cx.file, at.span, "a called value is not a function"));
                };
                let mut args = Vec::new();
                for node in fact.args.iter().flatten() {
                    let e = self.expr_at(cx.unit, *node)?;
                    args.push(self.expr(cx, e)?);
                }
                self.apply(&f, args, at)
            }
        }
    }

    /// Evaluates the arguments of a call in parameter order, filling in the
    /// defaults of the parameters without arguments, and checks each
    /// against the refinements of its parameter. Returns the values and
    /// whether each was given.
    fn arguments(
        &mut self,
        cx: &Cx<'_>,
        params: &'c [ParamDef],
        args: &[Option<usize>],
        declared_in: crate::check::FileId,
        at: SourceSpan,
    ) -> R<(Vec<Val>, Vec<bool>)> {
        let mut values = Vec::with_capacity(params.len());
        let mut given = Vec::with_capacity(params.len());
        for (i, param) in params.iter().enumerate() {
            given.push(args.get(i).copied().flatten().is_some());
            let (value, value_at) = match args.get(i).copied().flatten() {
                Some(node) => {
                    let e = self.expr_at(cx.unit, node)?;
                    (self.expr(cx, e)?, cx.at(e.syntax().span()))
                }
                None => match param.default {
                    Some(node) => {
                        let Some(unit) = self.file_unit(declared_in) else {
                            return Err(self.bug(cx.file, at.span, "a declaration has no unit"));
                        };
                        let e = self.expr_at(unit, node)?;
                        let inner = Cx {
                            unit,
                            file: declared_in,
                            env: None,
                            frames: &[],
                        };
                        (self.expr(&inner, e)?, at)
                    }
                    None => (Val::None, at),
                },
            };
            self.refine(param.ty, &value, value_at)?;
            values.push(value);
        }
        Ok((values, given))
    }

    /// Calls a top-level function with its arguments in parameter order.
    pub(crate) fn call_global(&mut self, g: GlobalId, args: Vec<Val>, at: SourceSpan) -> R<Val> {
        let global = &self.c.globals[g.index()];
        let GlobalKind::Fn(sig) = &global.kind else {
            return Err(self.bug(at.file, at.span, "a called declaration is not a function"));
        };
        let file = global.file;
        let Some(unit) = self.file_unit(file) else {
            return Err(self.bug(at.file, at.span, "a function has no unit"));
        };
        let body = self
            .tree(file)
            .node(global.node)
            .and_then(ast::FnDecl::cast)
            .and_then(ast::FnDecl::body);
        let Some(body) = body else {
            return Err(self.bug(at.file, at.span, "a function has no body"));
        };
        let mut env = None;
        for (&id, value) in sig.binds.iter().zip(args) {
            env = bind(&env, id, Slot::Ready(value));
        }
        self.calls.push((global.name.clone(), at));
        let cx = Cx {
            unit,
            file,
            env,
            frames: &[],
        };
        let result = self.expr(&cx, body);
        self.calls.pop();
        let value = result?;
        self.refine_result(sig.ret, &value, cx.at(body.syntax().span()), at)?;
        Ok(value)
    }

    /// Checks the result of a call against the refinements of the return
    /// type, pointing at the body that produced it and at the call.
    fn refine_result(
        &mut self,
        ret: TypeId,
        value: &Val,
        body: SourceSpan,
        call: SourceSpan,
    ) -> R<()> {
        self.refine(ret, value, body).map_err(|mut stop| {
            stop.0.diagnostic.labels.push(Label {
                file: Some(call.file),
                span: call.span,
                message: "the result of this call".to_owned(),
            });
            stop
        })
    }

    /// Calls a function value with every argument positional.
    pub(crate) fn apply(&mut self, f: &FnVal, args: Vec<Val>, at: SourceSpan) -> R<Val> {
        match f {
            FnVal::Global(g) => {
                if let GlobalKind::Fn(sig) = &self.c.globals[g.index()].kind {
                    for (param, value) in sig.params.iter().zip(&args) {
                        self.refine(param.ty, value, at)?;
                    }
                }
                self.call_global(*g, args, at)
            }
            FnVal::Lambda { unit, node, env } => {
                let file = self.unit_file(*unit);
                let lambda = self.tree(file).node(*node).and_then(ast::Lambda::cast);
                let Some(body) = lambda.and_then(ast::Lambda::body) else {
                    return Err(self.bug(at.file, at.span, "a function value has no body"));
                };
                let Fact::Expr(fact) = self.fact(*unit, *node) else {
                    return Err(self.bug(at.file, at.span, "a function value has no facts"));
                };
                let Meaning::Binds(binds) = &fact.meaning else {
                    return Err(self.bug(at.file, at.span, "a function value has no parameters"));
                };
                let mut inner = env.clone();
                for (&id, value) in binds.iter().zip(args) {
                    self.refine(self.local_type(*unit, id), &value, at)?;
                    inner = bind(&inner, id, Slot::Ready(value));
                }
                self.calls.push(("fn".to_owned(), at));
                let cx = Cx {
                    unit: *unit,
                    file,
                    env: inner,
                    frames: &[],
                };
                let result = self.expr(&cx, body);
                self.calls.pop();
                let value = result?;
                if let Type::Fn(_, ret) = self.c.types.get(fact.ty) {
                    self.refine_result(*ret, &value, cx.at(body.syntax().span()), at)?;
                }
                Ok(value)
            }
        }
    }

    /// Builds a complete struct value: the fields a literal sets, and the
    /// declared defaults of the others, which may read the value through
    /// `self`.
    pub(crate) fn struct_value(
        &mut self,
        cx: &Cx<'_>,
        id: StructId,
        entries: Vec<ast::MapEntry<'c>>,
        at: SourceSpan,
    ) -> R<Val> {
        let count = self.c.types.structure(id).fields.len();
        let mut fields = vec![Field::Pending; count];
        for entry in entries {
            let (Some(key), Some(value)) = (entry.key(), entry.value()) else {
                continue;
            };
            let name = self.key_text(cx, key)?;
            let Some((index, _)) = self.c.types.structure(id).field(&name) else {
                return Err(self.bug(
                    cx.file,
                    key.span(),
                    "a struct literal names an unknown field",
                ));
            };
            let ty = self.c.types.structure(id).fields[index].ty;
            let value_at = cx.at(value.syntax().span());
            let value = self.expr(cx, value)?;
            self.refine(ty, &value, value_at)?;
            fields[index] = Field::Ready(value);
        }
        self.build(id, fields, at)
    }

    /// Builds a struct value from the fields a literal sets, evaluating
    /// the defaults of the others and checking them against the
    /// refinements of their fields.
    fn build(&mut self, id: StructId, fields: Vec<Field>, at: SourceSpan) -> R<Val> {
        let count = fields.len();
        let set: Vec<bool> = fields
            .iter()
            .map(|f| matches!(f, Field::Ready(_)))
            .collect();
        let index = self.caches.building.len();
        self.caches.building.push(Building { id, fields });
        let mut values = Vec::with_capacity(count);
        for (i, &given) in set.iter().enumerate() {
            let value = self.building_field(index, i, at).and_then(|v| {
                if !given {
                    let ty = self.c.types.structure(id).fields[i].ty;
                    self.refine(ty, &v, at)?;
                }
                Ok(v)
            });
            match value {
                Ok(v) => values.push(v),
                Err(e) => {
                    self.caches.building.truncate(index);
                    return Err(e);
                }
            }
        }
        self.caches.building.truncate(index);
        self.new_struct_set(id, values, set, at)
    }

    /// Returns a field of a struct value being built, evaluating its
    /// default on first use.
    pub(crate) fn building_field(&mut self, index: usize, field: usize, at: SourceSpan) -> R<Val> {
        let Some(building) = self.caches.building.get(index) else {
            return Err(self.bug(at.file, at.span, "a struct under construction is gone"));
        };
        let id = building.id;
        let def = self.c.types.structure(id);
        let Some(field_def) = def.fields.get(field) else {
            return Err(self.bug(at.file, at.span, "a field is missing"));
        };
        match building.fields.get(field).cloned() {
            Some(Field::Ready(v)) => return Ok(v),
            Some(Field::Busy) => {
                return Err(self.fail(
                    at,
                    "E0607",
                    format!(
                        "the defaults of `{}` read each other through `self` in a cycle",
                        def.name
                    ),
                ));
            }
            _ => {}
        }
        self.caches.building[index].fields[field] = Field::Busy;
        let value = match field_def.default {
            Some(node) => {
                if self.default_is_late(id, field)? {
                    let file = def.file;
                    let Some(unit) = self.file_unit(file) else {
                        return Err(self.bug(at.file, at.span, "a declaration has no unit"));
                    };
                    let e = self.expr_at(unit, node)?;
                    let frames = [Frame::Building(index)];
                    let cx = Cx {
                        unit,
                        file,
                        env: None,
                        frames: &frames,
                    };
                    self.expr(&cx, e)?
                } else {
                    self.complete_default(id, field)?
                }
            }
            None if self.c.types.unwrap_option(field_def.ty).is_some() => Val::None,
            None => {
                return Err(self.stop(
                    at,
                    Diagnostic::error(
                        "E0801",
                        at.span,
                        format!(
                            "the `{}` value is missing the required field `{}`",
                            def.name, field_def.name
                        ),
                    )
                    .with_help("set the field, or give it a default"),
                ));
            }
        };
        if let Some(building) = self.caches.building.get_mut(index)
            && let Some(slot) = building.fields.get_mut(field)
        {
            *slot = Field::Ready(value.clone());
        }
        Ok(value)
    }

    /// Returns whether the default of a field reads `self`.
    pub(crate) fn default_is_late(&mut self, id: StructId, field: usize) -> R<bool> {
        Ok(!self.default_reads(id, field).is_empty())
    }

    /// Returns the complete value of a field default that does not read
    /// `self`, evaluated once.
    pub(crate) fn complete_default(&mut self, id: StructId, field: usize) -> R<Val> {
        if let Some(v) = self.caches.complete.get(&(id, field)) {
            return Ok(v.clone());
        }
        let def = self.c.types.structure(id);
        let file = def.file;
        let span = def.span;
        let (Some(node), Some(unit)) = (
            def.fields.get(field).and_then(|f| f.default),
            self.file_unit(file),
        ) else {
            return Err(self.bug(file, span, "a default is missing"));
        };
        let e = self.expr_at(unit, node)?;
        let cx = Cx {
            unit,
            file,
            env: None,
            frames: &[],
        };
        let value = self.expr(&cx, e)?;
        self.caches.complete.insert((id, field), value.clone());
        Ok(value)
    }

    /// Returns the value of a payload default, evaluated once.
    pub(crate) fn payload_default(
        &mut self,
        sum: SumId,
        variant: usize,
        field: usize,
    ) -> R<Option<Val>> {
        if let Some(v) = self.caches.payload.get(&(sum, variant, field)) {
            return Ok(Some(v.clone()));
        }
        let def = self.c.types.sum(sum);
        let file = def.file;
        let param = def
            .variants
            .get(variant)
            .and_then(|v| v.fields.as_ref())
            .and_then(|f| f.get(field));
        let Some(param) = param else {
            return Ok(None);
        };
        let Some(node) = param.default else {
            return Ok(None);
        };
        let Some(unit) = self.file_unit(file) else {
            return Err(self.bug(file, def.span, "a declaration has no unit"));
        };
        let e = self.expr_at(unit, node)?;
        let cx = Cx {
            unit,
            file,
            env: None,
            frames: &[],
        };
        let value = self.expr(&cx, e)?;
        self.caches
            .payload
            .insert((sum, variant, field), value.clone());
        Ok(Some(value))
    }
}

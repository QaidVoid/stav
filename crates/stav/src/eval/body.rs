//! Evaluating bodies into contributions.
//!
//! A body does not build a value. Each attribute statement contributes its
//! value at a path, with a priority, and each block marks the attribute it
//! maps to as present. A struct or map literal on the right side of an
//! attribute contributes each field or key it sets, so that it merges with
//! contributions from elsewhere. A right side that reads `self` becomes a
//! deferred contribution, evaluated once the merge knows the values it
//! reads.

use std::rc::Rc;

use super::env::{Env, LateLet, Slot, bind};
use super::expr::Cx;
use super::merge::{Frame, Seg};
use super::val::{PartVal, Val};
use super::{Eval, R};
use crate::ast::{self, Expr, Key, Stmt};
use crate::check::FileId;
use crate::check::model::{
    BlockRule, Fact, IncludeFact, LabelFact, LetFact, Meaning, Priority, SelfRead, Step, Target,
    UnitId,
};
use crate::check::types::{Type, TypeId};
use crate::span::{SourceSpan, Span};

/// The priority of a contribution, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Prio {
    /// The declared default of a field.
    Declared,
    /// `default(...)`.
    Default,
    /// A plain assignment or a block.
    Plain,
    /// `force(...)`.
    Force,
}

impl Prio {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Prio::Declared => "declared default",
            Prio::Default => "default",
            Prio::Plain => "plain",
            Prio::Force => "force",
        }
    }
}

impl From<Priority> for Prio {
    fn from(p: Priority) -> Self {
        match p {
            Priority::Default => Prio::Default,
            Priority::Plain => Prio::Plain,
            Priority::Force => Prio::Force,
        }
    }
}

/// What a contribution contributes.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    Value(Val),
    /// A value that reads `self`, evaluated after merging.
    Deferred(Rc<Deferred>),
    /// The attribute is present: a block, or a struct or map value, sets
    /// it, even when it sets nothing inside it.
    Present,
    /// A tagged block selects the variant with this index.
    Select(u32),
    /// A deferred contribution whose value has been evaluated and merged
    /// in as contributions of its own.
    Spent,
}

/// A right side that reads `self`.
#[derive(Debug)]
pub(crate) struct Deferred {
    pub(crate) unit: UnitId,
    /// The node index of the expression.
    pub(crate) node: usize,
    pub(crate) env: super::env::Env,
    /// The paths of the blocks around the statement, outermost first,
    /// which the depths of `self` reads index.
    pub(crate) frames: Vec<Rc<[Seg]>>,
    /// The type of the attribute.
    pub(crate) ty: TypeId,
    pub(crate) reads: Vec<SelfRead>,
}

/// One contribution to an attribute.
#[derive(Debug, Clone)]
pub(crate) struct Contrib {
    pub(crate) prio: Prio,
    pub(crate) kind: Kind,
    pub(crate) file: FileId,
    pub(crate) span: Span,
    /// The `include` statements the contribution came through, innermost
    /// first.
    pub(crate) via: Option<Rc<Vec<SourceSpan>>>,
    /// The order of the contribution: contributions to one attribute
    /// concatenate and are reported in this order.
    pub(crate) seq: u32,
}

impl Contrib {
    pub(crate) fn with(&self, kind: Kind) -> Self {
        Self {
            kind,
            ..self.clone()
        }
    }

    pub(crate) fn at(&self) -> SourceSpan {
        SourceSpan {
            file: self.file,
            span: self.span,
        }
    }
}

/// A contribution with its path, relative to the body that made it.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    pub(crate) path: Vec<Seg>,
    pub(crate) contrib: Contrib,
}

/// The place a body is evaluated in.
pub(crate) struct Body {
    pub(crate) unit: UnitId,
    pub(crate) file: FileId,
    /// The path of the innermost block.
    pub(crate) base: Vec<Seg>,
    /// The paths of the blocks around the statement, outermost first.
    pub(crate) frames: Vec<Vec<Seg>>,
    pub(crate) env: super::env::Env,
    pub(crate) via: Option<Rc<Vec<SourceSpan>>>,
}

impl Body {
    fn cx(&self) -> Cx<'static> {
        Cx {
            unit: self.unit,
            file: self.file,
            env: self.env.clone(),
            frames: &[],
        }
    }

    fn contrib(&self, prio: Prio, kind: Kind, span: Span) -> Contrib {
        Contrib {
            prio,
            kind,
            file: self.file,
            span,
            via: self.via.clone(),
            seq: 0,
        }
    }
}

impl<'c> Eval<'c> {
    /// Evaluates the implicit parts and the entry files, in order, and
    /// returns their contributions with paths from the root.
    pub(crate) fn bodies(&mut self) -> R<Vec<Pending>> {
        let mut out = Vec::new();
        for &unit in &self.c.implicit_parts {
            self.part_file(unit, Vec::new(), None, &mut out)?;
        }
        for &unit in &self.c.entries {
            let file = self.unit_file(unit);
            let mut body = Body {
                unit,
                file,
                base: Vec::new(),
                frames: vec![Vec::new()],
                env: Env::default(),
                via: None,
            };
            let statements = self
                .c
                .program
                .file(file)
                .parsed
                .file()
                .items()
                .filter_map(|item| match item {
                    ast::Item::Stmt(Stmt::Let(_)) => None,
                    ast::Item::Stmt(s) => Some(s),
                    _ => None,
                });
            for statement in statements {
                self.statement(&mut body, statement, &mut out)?;
            }
        }
        for (seq, pending) in out.iter_mut().enumerate() {
            pending.contrib.seq = u32::try_from(seq).unwrap_or(u32::MAX);
        }
        Ok(out)
    }

    /// Evaluates the statements of a part file included at `base`.
    fn part_file(
        &mut self,
        unit: UnitId,
        base: Vec<Seg>,
        via: Option<Rc<Vec<SourceSpan>>>,
        out: &mut Vec<Pending>,
    ) -> R<()> {
        let file = self.unit_file(unit);
        let mut body = Body {
            unit,
            file,
            frames: vec![base.clone()],
            base,
            env: Env::default(),
            via,
        };
        let statements = self
            .c
            .program
            .file(file)
            .parsed
            .file()
            .items()
            .filter_map(|item| match item {
                ast::Item::Stmt(s) => Some(s),
                _ => None,
            });
        for statement in statements {
            self.statement(&mut body, statement, out)?;
        }
        Ok(())
    }

    /// Evaluates the statements of a nested body. Its `let` bindings end
    /// with it.
    fn statements(
        &mut self,
        body: &mut Body,
        statements: impl Iterator<Item = Stmt<'c>>,
        out: &mut Vec<Pending>,
    ) -> R<()> {
        let env = body.env.clone();
        for statement in statements {
            self.statement(body, statement, out)?;
        }
        body.env = env;
        Ok(())
    }

    fn statement(&mut self, body: &mut Body, statement: Stmt<'c>, out: &mut Vec<Pending>) -> R<()> {
        let node = statement.syntax();
        let at = SourceSpan {
            file: body.file,
            span: node.span(),
        };
        self.step(at)?;
        match statement {
            Stmt::Let(l) => self.let_statement(body, l),
            Stmt::Attr(attr) => self.attr(body, attr, out),
            Stmt::Block(block) => self.block(body, block, out),
            Stmt::Include(include) => self.include(body, include, out),
            Stmt::If(branch) => self.if_statement(body, branch, out),
            Stmt::For(each) => {
                let Fact::For(fact) = self.fact(body.unit, node.index()) else {
                    return Err(self.bug(body.file, node.span(), "a `for` statement has no facts"));
                };
                let Some(collection) = each.collection() else {
                    return Ok(());
                };
                let source = self.expr(&body.cx(), collection)?;
                let env = body.env.clone();
                for (key, value) in elements(&source) {
                    body.env = env.clone();
                    let mut binds = fact.binds.iter();
                    if let (Some(&id), Some(value)) = (binds.next(), key) {
                        body.env = bind(&body.env, id, Slot::Ready(value));
                    }
                    if let (Some(&id), Some(value)) = (binds.next(), value) {
                        body.env = bind(&body.env, id, Slot::Ready(value));
                    }
                    if let Some(inner) = each.body() {
                        self.statements(body, inner.statements(), out)?;
                    }
                }
                body.env = env;
                Ok(())
            }
        }
    }

    fn let_statement(&mut self, body: &mut Body, l: ast::Let<'c>) -> R<()> {
        let node = l.syntax();
        let Fact::Let(LetFact::Local(id)) = self.fact(body.unit, node.index()) else {
            return Ok(());
        };
        let Some(value) = l.value() else {
            return Ok(());
        };
        let local = &self.c.units[body.unit.index()].locals[id.index()];
        let annotation = l.ty().map(|_| local.ty);
        let slot = if local.reads.is_empty() {
            let val = self.expr(&body.cx(), value)?;
            if let Some(ty) = annotation {
                self.refine(
                    ty,
                    &val,
                    SourceSpan {
                        file: body.file,
                        span: value.syntax().span(),
                    },
                )?;
            }
            Slot::Ready(val)
        } else {
            Slot::Late(Rc::new(LateLet {
                unit: body.unit,
                node: value.syntax().index(),
                env: body.env.clone(),
                annotation,
            }))
        };
        body.env = bind(&body.env, *id, slot);
        Ok(())
    }

    fn attr(&mut self, body: &mut Body, attr: ast::Attr<'c>, out: &mut Vec<Pending>) -> R<()> {
        let node = attr.syntax();
        let Fact::Attr(fact) = self.fact(body.unit, node.index()) else {
            return Err(self.bug(body.file, node.span(), "an attribute has no facts"));
        };
        let mut path = body.base.clone();
        path.extend(fact.steps.iter().map(|step| match step {
            Step::Field(i) => Seg::Field(u32::try_from(*i).unwrap_or(u32::MAX)),
            Step::Key(k) => Seg::Key(k.as_str().into()),
        }));
        let value = self.expr_at(body.unit, fact.value)?;
        let span = value.syntax().span();
        let prio = Prio::from(fact.priority);
        if fact.reads.is_empty() {
            let template = body.contrib(prio, Kind::Present, span);
            return self.contribute(&body.cx(), path, fact.ty, value, &template, out);
        }
        let deferred = Deferred {
            unit: body.unit,
            node: fact.value,
            env: body.env.clone(),
            frames: body.frames.iter().map(|f| f.as_slice().into()).collect(),
            ty: fact.ty,
            reads: fact.reads.clone(),
        };
        out.push(Pending {
            path,
            contrib: body.contrib(prio, Kind::Deferred(Rc::new(deferred)), span),
        });
        Ok(())
    }

    /// Contributes the value of `e` at `path`: a struct or map literal one
    /// contribution per field or key it sets, a struct or map value one
    /// per field or key it has, and any other value as a whole. The
    /// contributions copy `template` with their own kinds.
    pub(crate) fn contribute(
        &mut self,
        cx: &Cx<'_>,
        path: Vec<Seg>,
        ty: TypeId,
        e: Expr<'c>,
        template: &Contrib,
        out: &mut Vec<Pending>,
    ) -> R<()> {
        let types = &self.c.types;
        let inner = types.unwrap_option(ty).unwrap_or_else(|| types.peel(ty));
        let meaning = match self.fact(cx.unit, e.syntax().index()) {
            Fact::Expr(f) => Some(&f.meaning),
            _ => None,
        };
        match (e, types.get(inner).clone()) {
            (Expr::Paren(paren), _) => {
                if let Some(e) = paren.expr() {
                    return self.contribute(cx, path, ty, e, template, out);
                }
                Ok(())
            }
            (Expr::StructLit(_) | Expr::Map(_), Type::Struct(s))
                if meaning == Some(&Meaning::Struct(s)) =>
            {
                out.push(Pending {
                    path: path.clone(),
                    contrib: template.with(Kind::Present),
                });
                let entries: Vec<ast::MapEntry<'c>> = match e {
                    Expr::StructLit(lit) => lit.entries().collect(),
                    Expr::Map(map) => map.entries().collect(),
                    _ => Vec::new(),
                };
                for entry in entries {
                    let (Some(key), Some(value)) = (entry.key(), entry.value()) else {
                        continue;
                    };
                    let name = self.key_text(cx, key)?;
                    let def = self.c.types.structure(s);
                    let Some((index, field)) = def.field(&name) else {
                        return Err(self.bug(
                            cx.file,
                            key.span(),
                            "a struct literal names an unknown field",
                        ));
                    };
                    let field_ty = field.ty;
                    let mut sub = path.clone();
                    sub.push(Seg::Field(u32::try_from(index).unwrap_or(u32::MAX)));
                    self.contribute(cx, sub, field_ty, value, template, out)?;
                }
                Ok(())
            }
            (Expr::Map(map), Type::Map(_, value_ty)) => {
                out.push(Pending {
                    path: path.clone(),
                    contrib: template.with(Kind::Present),
                });
                for entry in map.entries() {
                    let (Some(key), Some(value)) = (entry.key(), entry.value()) else {
                        continue;
                    };
                    let name = self.key_text(cx, key)?;
                    let mut sub = path.clone();
                    sub.push(Seg::Key(name.into()));
                    self.contribute(cx, sub, value_ty, value, template, out)?;
                }
                Ok(())
            }
            _ => {
                let value = self.expr(cx, e)?;
                self.decompose(path, ty, value, template, out);
                Ok(())
            }
        }
    }

    /// Contributes a value at `path`: a struct or map one contribution per
    /// field or key, anything else as a whole.
    pub(crate) fn decompose(
        &self,
        path: Vec<Seg>,
        ty: TypeId,
        value: Val,
        template: &Contrib,
        out: &mut Vec<Pending>,
    ) {
        let types = &self.c.types;
        let inner = types.unwrap_option(ty).unwrap_or_else(|| types.peel(ty));
        match (&value, types.get(inner)) {
            (Val::Struct(v), Type::Struct(s)) if v.id == *s => {
                out.push(Pending {
                    path: path.clone(),
                    contrib: template.with(Kind::Present),
                });
                let def = types.structure(*s);
                for (i, (field, val)) in def.fields.iter().zip(&v.fields).enumerate() {
                    if !v.set.get(i).copied().unwrap_or(true) {
                        continue;
                    }
                    let mut sub = path.clone();
                    sub.push(Seg::Field(u32::try_from(i).unwrap_or(u32::MAX)));
                    self.decompose(sub, field.ty, val.clone(), template, out);
                }
            }
            (Val::Map(m), Type::Map(_, value_ty)) => {
                out.push(Pending {
                    path: path.clone(),
                    contrib: template.with(Kind::Present),
                });
                for (key, val) in &m.entries {
                    let mut sub = path.clone();
                    sub.push(Seg::Key(key.text(types).into()));
                    self.decompose(sub, *value_ty, val.clone(), template, out);
                }
            }
            _ => out.push(Pending {
                path,
                contrib: template.with(Kind::Value(value)),
            }),
        }
    }

    /// Returns the text of a key: an identifier, or the value of a string,
    /// interpolated if it must be.
    pub(crate) fn key_text(&mut self, cx: &Cx<'_>, key: Key<'c>) -> R<String> {
        match key {
            Key::Name(token) => Ok(token.text().to_owned()),
            Key::Str(string) => self.string_text(cx, string),
        }
    }

    fn block(&mut self, body: &mut Body, block: ast::Block<'c>, out: &mut Vec<Pending>) -> R<()> {
        let node = block.syntax();
        let Fact::Block(fact) = self.fact(body.unit, node.index()) else {
            return Err(self.bug(body.file, node.span(), "a block has no facts"));
        };
        let field = Seg::Field(u32::try_from(fact.field).unwrap_or(u32::MAX));
        let span = node.span();
        let mut path = body.base.clone();
        path.push(field);
        match fact.rule {
            BlockRule::Clause => {
                let label = match (&fact.label, block.label()) {
                    (Some(LabelFact::Static(text)), _) => text.clone(),
                    (_, Some(key)) => self.key_text(&body.cx(), key)?,
                    _ => return Err(self.bug(body.file, span, "a block has no label")),
                };
                path.push(Seg::Key(label.into()));
                out.push(Pending {
                    path: path.clone(),
                    contrib: body.contrib(Prio::Plain, Kind::Present, span),
                });
            }
            BlockRule::Field | BlockRule::MapField => out.push(Pending {
                path: path.clone(),
                contrib: body.contrib(Prio::Plain, Kind::Present, span),
            }),
            BlockRule::Tagged => {
                let Target::Variant { variant, .. } = fact.target else {
                    return Err(self.bug(body.file, span, "a tagged block has no variant"));
                };
                let variant = u32::try_from(variant).unwrap_or(u32::MAX);
                out.push(Pending {
                    path: path.clone(),
                    contrib: body.contrib(Prio::Plain, Kind::Select(variant), span),
                });
                path.push(Seg::Variant(variant));
            }
        }
        let Some(inner) = block.body() else {
            return Ok(());
        };
        let base = std::mem::replace(&mut body.base, path.clone());
        body.frames.push(path);
        let result = self.statements(body, inner.statements(), out);
        body.frames.pop();
        body.base = base;
        result
    }

    fn include(
        &mut self,
        body: &mut Body,
        include: ast::Include<'c>,
        out: &mut Vec<Pending>,
    ) -> R<()> {
        let node = include.syntax();
        let site = SourceSpan {
            file: body.file,
            span: node.span(),
        };
        let mut chain = body.via.as_deref().cloned().unwrap_or_default();
        match self.fact(body.unit, node.index()) {
            Fact::Include(IncludeFact::File(unit)) => {
                chain.insert(0, site);
                self.part_file(*unit, body.base.clone(), Some(Rc::new(chain)), out)
            }
            Fact::Include(IncludeFact::Value) => {
                let Some(value) = include.value() else {
                    return Ok(());
                };
                let Val::Part(part) = self.expr(&body.cx(), value)? else {
                    return Err(self.bug(
                        body.file,
                        node.span(),
                        "an included value is not a part",
                    ));
                };
                for pending in &part.contributions {
                    let mut path = body.base.clone();
                    path.extend(pending.path.iter().cloned());
                    let mut contrib = pending.contrib.clone();
                    let mut inner = contrib.via.as_deref().cloned().unwrap_or_default();
                    inner.push(site);
                    inner.extend(chain.iter().copied());
                    contrib.via = Some(Rc::new(inner));
                    if let Kind::Deferred(d) = &contrib.kind {
                        let frames = d
                            .frames
                            .iter()
                            .map(|f| {
                                let mut full = body.base.clone();
                                full.extend(f.iter().cloned());
                                full.into()
                            })
                            .collect();
                        contrib.kind = Kind::Deferred(Rc::new(Deferred {
                            unit: d.unit,
                            node: d.node,
                            env: d.env.clone(),
                            frames,
                            ty: d.ty,
                            reads: d.reads.clone(),
                        }));
                    }
                    out.push(Pending { path, contrib });
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn if_statement(
        &mut self,
        body: &mut Body,
        branch: ast::IfStmt<'c>,
        out: &mut Vec<Pending>,
    ) -> R<()> {
        let Some(condition) = branch.condition() else {
            return Ok(());
        };
        let taken = matches!(self.expr(&body.cx(), condition)?, Val::Bool(true));
        if taken {
            if let Some(inner) = branch.then_body() {
                self.statements(body, inner.statements(), out)?;
            }
            return Ok(());
        }
        match branch.else_branch() {
            Some(ast::ElseBranch::Body(inner)) => self.statements(body, inner.statements(), out),
            Some(ast::ElseBranch::If(nested)) => self.if_statement(body, nested, out),
            None => Ok(()),
        }
    }

    /// Evaluates a `part { }` body into a part value.
    pub(crate) fn part(&mut self, cx: &Cx<'_>, part: ast::PartExpr<'c>) -> R<Val> {
        let mut body = Body {
            unit: cx.unit,
            file: cx.file,
            base: Vec::new(),
            frames: vec![Vec::new()],
            env: cx.env.clone(),
            via: None,
        };
        let mut out = Vec::new();
        if let Some(inner) = part.body() {
            self.statements(&mut body, inner.statements(), &mut out)?;
        }
        Ok(Val::Part(Rc::new(PartVal { contributions: out })))
    }

    /// Evaluates a deferred contribution with the blocks it reads, and
    /// returns its contributions relative to its attribute.
    pub(crate) fn evaluate_deferred(
        &mut self,
        deferred: &Deferred,
        frames: &[Frame],
        template: &Contrib,
    ) -> R<Vec<Pending>> {
        let file = self.unit_file(deferred.unit);
        let e = self.expr_at(deferred.unit, deferred.node)?;
        let cx = Cx {
            unit: deferred.unit,
            file,
            env: deferred.env.clone(),
            frames,
        };
        let mut out = Vec::new();
        self.contribute(&cx, Vec::new(), deferred.ty, e, template, &mut out)?;
        Ok(out)
    }
}

/// Returns the elements of a collection: the values of a list or set, or
/// the keys and values of a map. The elements are shared with the
/// collection, not copied.
pub(crate) fn elements(source: &Val) -> impl Iterator<Item = (Option<Val>, Option<Val>)> + '_ {
    let (items, entries): (&[Val], &[(Val, Val)]) = match source {
        Val::List(s) | Val::Set(s) => (&s.items, &[]),
        Val::Map(m) => (&[], &m.entries),
        _ => (&[], &[]),
    };
    let items = items.iter().map(|v| (Some(v.clone()), None));
    let entries = entries
        .iter()
        .map(|(k, v)| (Some(k.clone()), Some(v.clone())));
    items.chain(entries)
}

//! Bodies: attribute, block, `let`, `include`, `if` and `for` statements,
//! priority markers, and the `self` dependencies of each body.

use std::collections::BTreeMap;

use super::decls::{GlobalKind, ValueItem};
use super::load::FileId;
use super::model::{
    AttrFact, BlockFact, BlockRule, Fact, ForFact, IncludeFact, LabelFact, LetFact, Priority, Step,
    Target, UnitId,
};
use super::suggest::did_you_mean;
use super::types::{StructId, Type, TypeId};
use super::{Binding, BodyCx, Checker, Cx, Frame, SelfMode};
use crate::ast::{self, Expr, Item, Key, Stmt};
use crate::diagnostic::Diagnostic;
use crate::span::Span;

/// Where an attribute path stands: at a body target, or at a value of
/// some type.
#[derive(Clone, Copy)]
enum Place {
    Target(Target),
    Type(TypeId),
}

/// A field a body may contain: a struct field or a payload field.
struct BodyField {
    index: usize,
    name: String,
    ty: TypeId,
    block: Option<String>,
}

impl<'p> Checker<'p> {
    fn body_fields(&self, target: Target) -> Vec<BodyField> {
        match target {
            Target::Struct(s) => self
                .types
                .structure(s)
                .fields
                .iter()
                .enumerate()
                .map(|(index, f)| BodyField {
                    index,
                    name: f.name.clone(),
                    ty: f.ty,
                    block: f.block.clone(),
                })
                .collect(),
            Target::Variant { sum, variant } => self.types.sum(sum).variants[variant]
                .fields
                .iter()
                .flatten()
                .enumerate()
                .map(|(index, f)| BodyField {
                    index,
                    name: f.name.clone(),
                    ty: f.ty,
                    block: None,
                })
                .collect(),
            Target::Map { .. } => Vec::new(),
        }
    }

    fn target_name(&self, target: Target) -> String {
        match target {
            Target::Struct(s) => self.types.structure(s).name.clone(),
            Target::Variant { sum, variant } => {
                let sum = self.types.sum(sum);
                format!("{}.{}", sum.name, sum.variants[variant].name)
            }
            Target::Map { key, value } => match self.types.get(self.types.peel(key)) {
                Type::Error => "a map".to_owned(),
                _ => format!(
                    "Map<{}, {}>",
                    self.types.render_plain(key),
                    self.types.render_plain(value)
                ),
            },
        }
    }

    /// Checks the statements of a body of `target`. The bindings of `let`
    /// statements stay in scope for the caller to remove.
    pub(super) fn statements(
        &mut self,
        cx: &mut Cx,
        statements: impl Iterator<Item = Stmt<'p>>,
        target: Target,
    ) {
        let mut included = BTreeMap::new();
        for statement in statements {
            self.statement(cx, statement, target, &mut included);
        }
    }

    fn statement(
        &mut self,
        cx: &mut Cx,
        statement: Stmt<'p>,
        target: Target,
        included: &mut BTreeMap<FileId, Span>,
    ) {
        match statement {
            Stmt::Let(l) => self.let_statement(cx, l),
            Stmt::Attr(attr) => self.attr(cx, attr, target),
            Stmt::Block(block) => self.block(cx, block, target),
            Stmt::Include(include) => self.include(cx, include, target, included),
            Stmt::If(branch) => self.if_statement(cx, branch, target),
            Stmt::For(each) => {
                let span = each.syntax().span();
                let source = match each.collection() {
                    Some(e) => self.structural(cx, e, None, "a `for` collection"),
                    None => TypeId::ERROR,
                };
                let mark = cx.scope.len();
                let names: Vec<_> = each.names().collect();
                let binds = self.iterate(cx, source, &names, span);
                self.fact(cx, each.syntax(), Fact::For(ForFact { binds }));
                if let Some(body) = each.body() {
                    self.statements(cx, body.statements(), target);
                }
                cx.scope.truncate(mark);
            }
        }
    }

    fn if_statement(&mut self, cx: &mut Cx, branch: ast::IfStmt<'p>, target: Target) {
        if let Some(condition) = branch.condition() {
            self.structural(cx, condition, Some(TypeId::BOOL), "an `if` condition");
        }
        if let Some(body) = branch.then_body() {
            let mark = cx.scope.len();
            self.statements(cx, body.statements(), target);
            cx.scope.truncate(mark);
        }
        match branch.else_branch() {
            Some(ast::ElseBranch::Body(body)) => {
                let mark = cx.scope.len();
                self.statements(cx, body.statements(), target);
                cx.scope.truncate(mark);
            }
            Some(ast::ElseBranch::If(nested)) => self.if_statement(cx, nested, target),
            None => {}
        }
    }

    /// Checks an expression that decides which contributions a body makes,
    /// where `self` is not allowed.
    pub(super) fn structural(
        &mut self,
        cx: &mut Cx,
        e: Expr<'p>,
        expected: Option<TypeId>,
        place: &'static str,
    ) -> TypeId {
        let saved = std::mem::replace(&mut cx.self_mode, SelfMode::Forbidden(place));
        let reads = std::mem::take(&mut cx.reads);
        let ty = self.expr(cx, e, expected);
        cx.self_mode = saved;
        cx.reads = reads;
        ty
    }

    fn let_statement(&mut self, cx: &mut Cx, l: ast::Let<'p>) {
        let annotation = l.ty().map(|t| self.resolve_type(cx.file, t, cx.owner));
        let saved = std::mem::take(&mut cx.reads);
        let ty = match (l.value(), annotation) {
            (Some(v), Some(a)) => {
                self.expr(cx, v, Some(a));
                a
            }
            (Some(v), None) => self.expr(cx, v, None),
            (None, a) => a.unwrap_or(TypeId::ERROR),
        };
        let reads = std::mem::replace(&mut cx.reads, saved);
        let Some(name) = l.name() else { return };
        let id = self.local(cx, name.text(), ty, reads);
        cx.scope.push((name.text().to_owned(), Binding::Local(id)));
        self.fact(cx, l.syntax(), Fact::Let(LetFact::Local(id)));
    }

    fn attr(&mut self, cx: &mut Cx, attr: ast::Attr<'p>, target: Target) {
        let Some(path) = attr.path() else { return };
        let keys: Vec<Key<'p>> = path.keys().collect();
        let mut place = Place::Target(target);
        let mut steps = Vec::new();
        let mut names = Vec::new();
        let mut i = 0;
        let ty = loop {
            let Some(key) = keys.get(i) else {
                break match place {
                    Place::Type(ty) => Some(ty),
                    Place::Target(_) => None,
                };
            };
            let Some(text) = key_text(*key) else {
                break None;
            };
            match place {
                Place::Target(Target::Map { key: k, value }) => {
                    if let Some(s) = self.types.as_sum(k)
                        && self.types.sum(s).variant(&text).is_none()
                    {
                        let sum = self.types.sum(s);
                        let help =
                            did_you_mean(&text, sum.variants.iter().map(|v| v.name.as_str()));
                        let name = sum.name.clone();
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0302",
                                key.span(),
                                format!("`{text}` is not a variant of `{name}`"),
                            )
                            .with_optional_help(help),
                        );
                        break None;
                    }
                    steps.push(Step::Key(text.clone()));
                    names.push(text);
                    place = Place::Type(value);
                    i += 1;
                }
                Place::Target(t) => {
                    let fields = self.body_fields(t);
                    let Some(field) = fields.iter().find(|f| f.name == text) else {
                        let help = did_you_mean(&text, fields.iter().map(|f| f.name.as_str()));
                        let name = self.target_name(t);
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0302",
                                key.span(),
                                format!("`{name}` has no field `{text}`"),
                            )
                            .with_optional_help(help),
                        );
                        break None;
                    };
                    steps.push(Step::Field(field.index));
                    names.push(text);
                    place = Place::Type(field.ty);
                    i += 1;
                }
                Place::Type(ty) => {
                    let inner = self
                        .types
                        .unwrap_option(ty)
                        .unwrap_or_else(|| self.types.peel(ty));
                    match self.types.get(inner).clone() {
                        Type::Struct(s) => place = Place::Target(Target::Struct(s)),
                        Type::Map(k, v) => place = Place::Target(Target::Map { key: k, value: v }),
                        Type::Error => break None,
                        _ => {
                            let prefix: Vec<String> = names.clone();
                            let rendered = self.types.render_plain(ty);
                            self.error(
                                cx.file,
                                Diagnostic::error(
                                    "E0436",
                                    key.span(),
                                    format!("`{}` is a value of type `{rendered}`, which has no attribute `{text}`", prefix.join(".")),
                                )
                                .with_help("attribute paths go through struct fields and map keys"),
                            );
                            break None;
                        }
                    }
                }
            }
        };
        let Some(value) = attr.value() else { return };
        let (priority, value) = self.marker(cx, value);
        let saved = std::mem::take(&mut cx.reads);
        cx.partial = true;
        self.expr(cx, value, Some(ty.unwrap_or(TypeId::ERROR)));
        cx.partial = false;
        let reads = std::mem::replace(&mut cx.reads, saved);
        let Some(ty) = ty else { return };
        self.fact(
            cx,
            attr.syntax(),
            Fact::Attr(AttrFact {
                steps,
                ty,
                priority,
                value: value.syntax().index(),
                reads,
            }),
        );
    }

    /// Returns the priority of a right-hand side and the contributed value:
    /// the argument of `default(...)` or `force(...)`, or the whole side.
    fn marker(&mut self, cx: &mut Cx, value: Expr<'p>) -> (Priority, Expr<'p>) {
        let Expr::Call(call) = value else {
            return (Priority::Plain, value);
        };
        let Some(Expr::Name(name)) = call.callee() else {
            return (Priority::Plain, value);
        };
        let Some(token) = name.token() else {
            return (Priority::Plain, value);
        };
        let priority = match token.text() {
            "default" => Priority::Default,
            "force" => Priority::Force,
            _ => return (Priority::Plain, value),
        };
        if cx.lookup(token.text()).is_some() || self.lookup_value(cx.file, token.text()).is_some() {
            return (Priority::Plain, value);
        }
        let args: Vec<ast::Arg<'p>> = call.args().map(|a| a.args().collect()).unwrap_or_default();
        match args.as_slice() {
            [arg] if arg.name().is_none() => match arg.value() {
                Some(inner) => (priority, inner),
                None => (Priority::Plain, value),
            },
            _ => {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0444",
                        call.syntax().span(),
                        format!("`{}` takes one positional argument", token.text()),
                    ),
                );
                (Priority::Plain, value)
            }
        }
    }

    fn block(&mut self, cx: &mut Cx, block: ast::Block<'p>, target: Target) {
        let Some(kind_token) = block.kind() else {
            return;
        };
        let kind = kind_token.text();
        let kind_span = kind_token.span();
        let label = block.label();
        let label_span = label.map_or(kind_span, Key::span);
        let label_fact = label.map(|l| match key_text(l) {
            Some(text) => LabelFact::Static(text),
            None => LabelFact::Dynamic,
        });
        if let Some(Key::Str(string)) = label {
            let saved = std::mem::replace(&mut cx.self_mode, SelfMode::Forbidden("a block label"));
            let reads = std::mem::take(&mut cx.reads);
            self.interpolations(cx, string);
            cx.self_mode = saved;
            cx.reads = reads;
        }
        let fields = self.body_fields(target);
        let clause = match target {
            Target::Struct(_) => fields.iter().find(|f| f.block.as_deref() == Some(kind)),
            _ => None,
        };
        let named = fields.iter().find(|f| f.name == kind);
        let mapped = match (clause, named) {
            (Some(field), _) => {
                let ty = self.types.peel(field.ty);
                match self.types.get(ty).clone() {
                    Type::Map(k, v) => {
                        let target = match self.types.as_struct(v) {
                            Some(s) => Target::Struct(s),
                            None => Target::Map {
                                key: TypeId::ERROR,
                                value: TypeId::ERROR,
                            },
                        };
                        if label.is_none() {
                            self.needs_label(cx, kind, kind_span);
                        } else if let Some(s) = self.types.as_sum(k) {
                            self.label_variant(cx, s, label_fact.as_ref(), label_span);
                        }
                        Some((BlockRule::Clause, field.index, target))
                    }
                    _ => self.tagged(cx, field, kind, label_fact.as_ref(), label_span),
                }
            }
            (None, Some(field)) => {
                let ty = self
                    .types
                    .unwrap_option(field.ty)
                    .unwrap_or_else(|| self.types.peel(field.ty));
                match self.types.get(ty).clone() {
                    Type::Struct(s) => {
                        self.no_label(cx, kind, label, label_span);
                        Some((BlockRule::Field, field.index, Target::Struct(s)))
                    }
                    Type::Map(k, v) => {
                        self.no_label(cx, kind, label, label_span);
                        Some((
                            BlockRule::MapField,
                            field.index,
                            Target::Map { key: k, value: v },
                        ))
                    }
                    Type::Sum(_) => self.tagged(cx, field, kind, label_fact.as_ref(), label_span),
                    Type::Error => None,
                    _ => {
                        self.unknown_block(cx, target, &fields, kind, kind_span);
                        None
                    }
                }
            }
            (None, None) => {
                self.unknown_block(cx, target, &fields, kind, kind_span);
                None
            }
        };
        let Some((rule, field, body_target)) = mapped else {
            if let Some(body) = block.body() {
                self.ignored_body(cx, body);
            }
            return;
        };
        self.fact(
            cx,
            block.syntax(),
            Fact::Block(BlockFact {
                rule,
                field,
                label: label_fact,
                target: body_target,
            }),
        );
        let Some(body) = block.body() else { return };
        if let SelfMode::Body(b) = &mut cx.self_mode {
            b.frames.push(Frame {
                target: body_target,
            });
        }
        let mark = cx.scope.len();
        self.statements(cx, body.statements(), body_target);
        cx.scope.truncate(mark);
        if let SelfMode::Body(b) = &mut cx.self_mode {
            b.frames.pop();
        }
    }

    /// Checks the statements of a block that maps to nothing, without a
    /// target, so that their own errors are still found.
    fn ignored_body(&mut self, cx: &mut Cx, body: ast::Body<'p>) {
        let error = Target::Map {
            key: TypeId::ERROR,
            value: TypeId::ERROR,
        };
        let mark = cx.scope.len();
        if let SelfMode::Body(b) = &mut cx.self_mode {
            b.frames.push(Frame { target: error });
        }
        self.statements(cx, body.statements(), error);
        if let SelfMode::Body(b) = &mut cx.self_mode {
            b.frames.pop();
        }
        cx.scope.truncate(mark);
    }

    fn tagged(
        &mut self,
        cx: &Cx,
        field: &BodyField,
        kind: &str,
        label: Option<&LabelFact>,
        span: Span,
    ) -> Option<(BlockRule, usize, Target)> {
        let ty = self
            .types
            .unwrap_option(field.ty)
            .unwrap_or_else(|| self.types.peel(field.ty));
        let s = self.types.as_sum(ty)?;
        if label.is_none() {
            self.needs_label(cx, kind, span);
            return None;
        }
        let variant = self.label_variant(cx, s, label, span)?;
        Some((
            BlockRule::Tagged,
            field.index,
            Target::Variant { sum: s, variant },
        ))
    }

    fn label_variant(
        &mut self,
        cx: &Cx,
        s: super::types::SumId,
        label: Option<&LabelFact>,
        span: Span,
    ) -> Option<usize> {
        let sum = self.types.sum(s);
        let text = match label {
            Some(LabelFact::Static(text)) => text.clone(),
            _ => {
                let name = sum.name.clone();
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0439",
                        span,
                        format!("the label must name a variant of `{name}`"),
                    )
                    .with_help("write the variant name without interpolation"),
                );
                return None;
            }
        };
        match sum.variant(&text) {
            Some((index, _)) => Some(index),
            None => {
                let help = did_you_mean(&text, sum.variants.iter().map(|v| v.name.as_str()));
                let name = sum.name.clone();
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0439",
                        span,
                        format!("`{text}` is not a variant of `{name}`"),
                    )
                    .with_optional_help(help),
                );
                None
            }
        }
    }

    fn needs_label(&mut self, cx: &Cx, kind: &str, span: Span) {
        self.error(
            cx.file,
            Diagnostic::error("E0437", span, format!("the `{kind}` block needs a label"))
                .with_help(format!("write `{kind} <label> {{ ... }}`")),
        );
    }

    fn no_label(&mut self, cx: &Cx, kind: &str, label: Option<Key<'p>>, span: Span) {
        if label.is_some() {
            self.error(
                cx.file,
                Diagnostic::error("E0438", span, format!("the `{kind}` block takes no label"))
                    .with_help(format!("write `{kind} {{ ... }}`")),
            );
        }
    }

    fn unknown_block(
        &mut self,
        cx: &Cx,
        target: Target,
        fields: &[BodyField],
        kind: &str,
        span: Span,
    ) {
        let candidates: Vec<&str> = fields
            .iter()
            .flat_map(|f| [Some(f.name.as_str()), f.block.as_deref()])
            .flatten()
            .collect();
        let help = did_you_mean(kind, candidates);
        let name = self.target_name(target);
        self.error(
            cx.file,
            Diagnostic::error("E0440", span, format!("`{name}` has no block `{kind}`"))
                .with_optional_help(help),
        );
    }

    fn include(
        &mut self,
        cx: &mut Cx,
        include: ast::Include<'p>,
        target: Target,
        included: &mut BTreeMap<FileId, Span>,
    ) {
        let span = include.syntax().span();
        let part_file = self
            .program
            .file(cx.file)
            .includes
            .get(&include.syntax().index())
            .copied();
        let Target::Struct(s) = target else {
            let name = self.target_name(target);
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0442",
                    span,
                    format!("a part cannot be included into a body of `{name}`"),
                ),
            );
            return;
        };
        if let Some(file) = part_file {
            if let Some(&first) = included.get(&file) {
                let name = self.program.file(file).name.clone();
                self.report(
                    Some(cx.file),
                    Diagnostic::warning(
                        "W0703",
                        span,
                        format!("`{name}` is already included into this body"),
                    )
                    .with_label(None, first, "first included here")
                    .with_help("remove one of the includes"),
                );
            }
            included.insert(file, span);
            let unit = self.part_unit(file, s);
            self.fact(cx, include.syntax(), Fact::Include(IncludeFact::File(unit)));
            return;
        }
        let Some(value) = include.value() else { return };
        if let Expr::Str(string) = value
            && string.value().is_some()
        {
            return;
        }
        let expected = self.types.intern(Type::Part(s));
        let found = if let Expr::Part(_) = value {
            self.structural(cx, value, Some(expected), "an `include` expression")
        } else {
            self.structural(cx, value, None, "an `include` expression")
        };
        match self.types.get(self.types.peel(found)).clone() {
            Type::Part(a) if a == s => {}
            Type::Part(a) => {
                let (a, b) = (
                    self.types.structure(a).name.clone(),
                    self.types.structure(s).name.clone(),
                );
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0442",
                        value.syntax().span(),
                        format!("cannot include a `Part<{a}>` into a body of `{b}`"),
                    )
                    .with_help(format!(
                        "include it into a `{a}` block, or make it a `Part<{b}>`"
                    )),
                );
            }
            _ => self.mismatch(cx.file, value.syntax().span(), expected, found),
        }
        self.fact(cx, include.syntax(), Fact::Include(IncludeFact::Value));
    }

    /// Returns the unit of the part file `file` included into a body of
    /// `s`, checking it on first use.
    pub(super) fn part_unit(&mut self, file: FileId, s: StructId) -> UnitId {
        let (unit, new) = self.unit(file, Some(s));
        if new {
            let mut cx = Cx::new(unit, file, SelfMode::Forbidden("a part file"), None);
            let statements = self
                .program
                .file(file)
                .parsed
                .file()
                .items()
                .filter_map(|item| match item {
                    Item::Stmt(stmt) => Some(stmt),
                    _ => None,
                });
            self.top_body(&mut cx, statements, s);
        }
        unit
    }

    /// Checks the statements of an entry file as a body of the root struct.
    pub(super) fn entry_body(&mut self, file: FileId, root: StructId) -> UnitId {
        let unit = self.file_unit(file);
        let top = self.entry_top(file);
        let body = BodyCx {
            top,
            frames: vec![Frame {
                target: Target::Struct(root),
            }],
        };
        let mut cx = Cx::new(unit, file, SelfMode::Body(body), None);
        let mut included = BTreeMap::new();
        for item in self.program.file(file).parsed.file().items() {
            let Item::Stmt(statement) = item else {
                continue;
            };
            if let Stmt::Let(_) = statement {
                continue;
            }
            cx.limit = Some(statement.syntax().span().start);
            self.statement(&mut cx, statement, Target::Struct(root), &mut included);
        }
        unit
    }

    /// Checks the defaults, function bodies and top-level `let` values of
    /// every file that may declare them.
    pub(super) fn check_declaration_bodies(&mut self) {
        let program = self.program;
        for (i, file) in program.files.iter().enumerate() {
            if file.role == super::Role::Part {
                continue;
            }
            let id = FileId::new(i);
            let unit = self.file_unit(id);
            for s in 0..self.types.structs.len() {
                if self.types.structs[s].file != id {
                    continue;
                }
                let sid = StructId::new(s);
                let fields = self.types.structs[s].fields.clone();
                let mut cx = Cx::new(
                    unit,
                    id,
                    SelfMode::Default(sid),
                    Some(super::totality::Node::StructDefaults(sid)),
                );
                for field in fields {
                    let Some(e) = field
                        .default
                        .and_then(|n| file.parsed.tree.node(n))
                        .and_then(Expr::cast)
                    else {
                        continue;
                    };
                    cx.partial = true;
                    self.expr(&mut cx, e, Some(field.ty));
                    cx.reads.clear();
                }
            }
            for s in 0..self.types.sums.len() {
                if self.types.sums[s].file != id {
                    continue;
                }
                let sid = super::types::SumId::new(s);
                let variants = self.types.sums[s].variants.clone();
                let mut cx = Cx::new(
                    unit,
                    id,
                    SelfMode::Forbidden("a payload default"),
                    Some(super::totality::Node::SumDefaults(sid)),
                );
                for param in variants.iter().flat_map(|v| v.fields.iter().flatten()) {
                    if let Some(e) = param
                        .default
                        .and_then(|n| file.parsed.tree.node(n))
                        .and_then(Expr::cast)
                    {
                        self.expr(&mut cx, e, Some(param.ty));
                    }
                }
            }
            let scope = self.scopes[i].clone();
            for item in scope.values.values() {
                let ValueItem::Global(g) = *item else {
                    continue;
                };
                if self.globals[g.index()].file != id {
                    continue;
                }
                match &self.globals[g.index()].kind {
                    GlobalKind::Fn(_) => self.fn_body(g),
                    GlobalKind::Let(_) => {
                        self.let_type(g);
                    }
                }
            }
        }
    }

    fn fn_body(&mut self, g: super::types::GlobalId) {
        let global = &self.globals[g.index()];
        let GlobalKind::Fn(sig) = &global.kind else {
            return;
        };
        let (file, node, params, ret) = (global.file, global.node, sig.params.clone(), sig.ret);
        let unit = self.file_unit(file);
        let owner = Some(super::totality::Node::Global(g));
        let tree = &self.program.file(file).parsed.tree;
        let mut defaults = Cx::new(
            unit,
            file,
            SelfMode::Forbidden("a parameter default"),
            owner,
        );
        for param in &params {
            if let Some(e) = param
                .default
                .and_then(|n| tree.node(n))
                .and_then(Expr::cast)
            {
                self.expr(&mut defaults, e, Some(param.ty));
            }
        }
        let mut cx = Cx::new(unit, file, SelfMode::Forbidden("a function body"), owner);
        let mut binds = Vec::new();
        for param in &params {
            let id = self.local(&cx, &param.name, param.ty, Vec::new());
            cx.scope.push((param.name.clone(), Binding::Local(id)));
            binds.push(id);
        }
        if let GlobalKind::Fn(sig) = &mut self.globals[g.index()].kind {
            sig.binds = binds;
        }
        let body = tree
            .node(node)
            .and_then(ast::FnDecl::cast)
            .and_then(ast::FnDecl::body);
        if let Some(body) = body {
            self.expr(&mut cx, body, Some(ret));
        }
    }
}

/// Returns the text of a key: the identifier, or the value of a string
/// without interpolation.
fn key_text(key: Key<'_>) -> Option<String> {
    match key {
        Key::Name(token) => Some(token.text().to_owned()),
        Key::Str(string) => string.value(),
    }
}

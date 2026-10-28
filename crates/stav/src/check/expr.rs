//! Typing expressions.
//!
//! Checking is bidirectional. [`Checker::expr`] takes the type the
//! position of an expression requires, when there is one, and the
//! expression either uses it, for example to resolve a variant name, or
//! synthesizes its own type, which is then coerced to the expected one.

use std::collections::BTreeMap;

use super::decls::{GlobalKind, LetState, TypeItem, ValueItem};
use super::load::{Origin, join_relative};
use super::model::{ExprFact, Fact, LetFact, LocalId, Meaning, SelfRead, Target};
use super::suggest::did_you_mean;
use super::totality::Node;
use super::types::{GlobalId, StructId, SumId, Type, TypeId};
use super::{Binding, BodyCx, Checker, Cx, Frame, SelfMode};
use crate::ast::{self, BinaryOp, Expr, Key, LiteralKind, StrPart, UnaryOp};
use crate::diagnostic::Diagnostic;
use crate::span::Span;
use crate::syntax::SyntaxToken;

/// What a static path such as `alias.Type.variant` names.
#[derive(Debug, Clone, Copy)]
pub(super) enum PathItem {
    Module(Option<super::FileId>),
    Type(TypeItem),
    Value(GlobalId),
    Variant(SumId, usize),
    /// A path whose error has been reported.
    Error,
}

impl<'p> Checker<'p> {
    /// Checks `e` against `expected`, or synthesizes its type when nothing
    /// is expected, and records what the evaluator needs to know about it.
    /// Returns the type of the expression in its position.
    pub(super) fn expr(&mut self, cx: &mut Cx, e: Expr<'p>, expected: Option<TypeId>) -> TypeId {
        let (ty, meaning) = self.expr_kind(cx, e, expected);
        self.coerce(cx, e, ty, meaning, expected)
    }

    fn coerce(
        &mut self,
        cx: &Cx,
        e: Expr<'p>,
        ty: TypeId,
        meaning: Meaning,
        expected: Option<TypeId>,
    ) -> TypeId {
        let result = match expected {
            None => ty,
            Some(exp) => {
                let inner = self.types.unwrap_option(exp);
                let data_target = inner.unwrap_or(self.types.peel(exp));
                let fits = self.types.fits(ty, exp)
                    || inner.is_some_and(|inner| self.types.fits(ty, inner))
                    || (data_target == TypeId::DATA && !self.types.has_code(ty));
                if !fits {
                    self.mismatch(cx.file, e.syntax().span(), exp, ty);
                }
                exp
            }
        };
        self.fact(cx, e.syntax(), Fact::Expr(ExprFact { ty, meaning }));
        result
    }

    /// Reports that a value of type `found` is not accepted where
    /// `expected` is.
    pub(super) fn mismatch(
        &mut self,
        file: super::FileId,
        span: Span,
        expected: TypeId,
        found: TypeId,
    ) {
        if self.types.has_error(expected) || self.types.has_error(found) {
            return;
        }
        let (e, f) = (
            self.types.render_plain(expected),
            self.types.render_plain(found),
        );
        let mut diagnostic =
            Diagnostic::error("E0401", span, format!("expected `{e}`, found `{f}`"));
        if e == f {
            let names: Vec<String> = [expected, found]
                .iter()
                .filter_map(|t| self.types.origin(*t))
                .map(|file| self.program.file(file).name.clone())
                .collect();
            if let [a, b] = names.as_slice() {
                diagnostic = diagnostic.with_note(format!(
                    "the expected `{e}` is declared in `{a}`, the found one in `{b}`"
                ));
            }
        } else if found == TypeId::INT && self.types.peel(expected) == TypeId::FLOAT {
            diagnostic = diagnostic.with_help("convert it with `toFloat(...)`");
        }
        self.error(file, diagnostic);
    }

    /// Returns the type that literals and names are resolved against: the
    /// expected type without refinements and without `Option`.
    fn look_through(&self, expected: Option<TypeId>) -> Option<TypeId> {
        let expected = expected?;
        Some(
            self.types
                .unwrap_option(expected)
                .unwrap_or_else(|| self.types.peel(expected)),
        )
    }

    pub(super) fn expr_kind(
        &mut self,
        cx: &mut Cx,
        e: Expr<'p>,
        expected: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let partial = std::mem::take(&mut cx.partial);
        let want = self.look_through(expected);
        match e {
            Expr::Literal(literal) => self.literal(cx, literal, expected, want),
            Expr::Str(string) => self.string(cx, string, want),
            Expr::Name(name) => match name.token() {
                Some(token) => self.name(cx, token, want),
                None => (TypeId::ERROR, Meaning::Plain),
            },
            Expr::Symbol(symbol) => self.symbol_word(cx, symbol, want),
            Expr::SelfExpr(node) => {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0303",
                        node.syntax().span(),
                        "`self` must be followed by a field",
                    )
                    .with_help("write `self.<field>`"),
                );
                (TypeId::ERROR, Meaning::Plain)
            }
            Expr::List(list) => self.list(cx, list, expected, want),
            Expr::ListComp(comp) => self.list_comp(cx, comp, want),
            Expr::Map(map) => self.map_literal(cx, map, expected, want, partial),
            Expr::MapComp(comp) => self.map_comp(cx, comp, want),
            Expr::StructLit(lit) => self.struct_lit(cx, lit, partial),
            Expr::Paren(paren) => match paren.expr() {
                Some(inner) => {
                    cx.partial = partial;
                    (self.expr(cx, inner, expected), Meaning::Plain)
                }
                None => (TypeId::ERROR, Meaning::Plain),
            },
            Expr::Call(call) => self.call(cx, call, expected, want),
            Expr::Field(field) => self.field(cx, field),
            Expr::Index(index) => self.index(cx, index),
            Expr::Binary(binary) => self.binary(cx, binary, expected),
            Expr::Unary(unary) => self.unary(cx, unary, want),
            Expr::If(branch) => self.if_expr(cx, branch, expected),
            Expr::Match(m) => self.match_expr(cx, m, expected),
            Expr::Let(l) => self.let_expr(cx, l, expected),
            Expr::Lambda(lambda) => self.lambda(cx, lambda, want),
            Expr::Part(part) => self.part_expr(cx, part, want),
        }
    }

    fn literal(
        &mut self,
        cx: &Cx,
        literal: ast::Literal<'p>,
        expected: Option<TypeId>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let span = literal.syntax().span();
        let Some(kind) = literal.kind() else {
            return (TypeId::ERROR, Meaning::Plain);
        };
        match kind {
            LiteralKind::Int if want == Some(TypeId::FLOAT) => (TypeId::FLOAT, Meaning::Plain),
            LiteralKind::Int => (TypeId::INT, Meaning::Plain),
            LiteralKind::Float => (TypeId::FLOAT, Meaning::Plain),
            LiteralKind::True | LiteralKind::False => (TypeId::BOOL, Meaning::Plain),
            LiteralKind::Path => {
                let text = literal.token().map_or("", |t| t.text());
                (TypeId::PATH, self.path_literal(cx, text, span))
            }
            LiteralKind::None => match expected {
                Some(exp) if self.types.unwrap_option(exp).is_some() => {
                    (self.types.peel(exp), Meaning::Plain)
                }
                Some(_) if want == Some(TypeId::DATA) => (TypeId::DATA, Meaning::Plain),
                Some(exp) => {
                    if !self.types.has_error(exp) {
                        let ty = self.types.render_plain(exp);
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0401",
                                span,
                                format!("expected `{ty}`, found `none`"),
                            )
                            .with_help(format!("make the type `Option<{ty}>`")),
                        );
                    }
                    (exp, Meaning::Plain)
                }
                None => {
                    self.cannot_infer(cx, span, "none");
                    (TypeId::ERROR, Meaning::Plain)
                }
            },
        }
    }

    fn cannot_infer(&mut self, cx: &Cx, span: Span, what: &str) {
        let help = match what {
            "[]" => "add an annotation: `let xs: List<Str> = []`",
            "{}" => "add an annotation: `let m: Map<Str, Int> = {}`",
            _ => "add a type annotation",
        };
        self.error(
            cx.file,
            Diagnostic::error("E0430", span, format!("cannot infer the type of `{what}`"))
                .with_help(help),
        );
    }

    fn path_literal(&mut self, cx: &Cx, text: &str, span: Span) -> Meaning {
        let file = self.program.file(cx.file);
        let Origin::Path { root, rel } = &file.origin else {
            let name = file.name.clone();
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0702",
                    span,
                    format!("the path `{text}` in `{name}` has no import root"),
                ),
            );
            return Meaning::Plain;
        };
        match join_relative(rel, text) {
            Some(parts) => Meaning::Path {
                root: *root,
                rel: super::RelPath::from_parts(parts.iter().map(String::as_str)),
            },
            None => {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0702",
                        span,
                        format!("the path `{text}` leaves its import root"),
                    ),
                );
                Meaning::Plain
            }
        }
    }

    /// Checks the interpolations of a string.
    pub(super) fn interpolations(&mut self, cx: &mut Cx, string: ast::Str<'p>) {
        for part in string.parts() {
            let StrPart::Interp(interp) = part else {
                continue;
            };
            let Some(e) = interp.expr() else { continue };
            let ty = self.expr(cx, e, None);
            if !self.types.is_text(ty) {
                let rendered = self.types.render_plain(ty);
                let help = match self.types.get(self.types.peel(ty)) {
                    Type::List(_) => "join the elements with `join(...)`",
                    Type::Option(_) => "unwrap the value with `??` or a `match`",
                    _ => "convert the value to a string first",
                };
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0421",
                        e.syntax().span(),
                        format!("cannot interpolate a value of type `{rendered}`"),
                    )
                    .with_help(help),
                );
            }
        }
    }

    fn string(
        &mut self,
        cx: &mut Cx,
        string: ast::Str<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        self.interpolations(cx, string);
        let Some(want) = want else {
            return (TypeId::STR, Meaning::Plain);
        };
        match self.types.get(want).clone() {
            Type::Symbol(s) => {
                let def = &self.types.symbols[s.index()];
                if def.strings {
                    return (want, Meaning::Symbol(s));
                }
                let (name, ctor) = (def.name.clone(), def.constructor.clone());
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0401",
                        string.syntax().span(),
                        format!("`{name}` does not accept string literals"),
                    )
                    .with_help(format!("convert the string with `{ctor}(...)`")),
                );
                (want, Meaning::Plain)
            }
            Type::Ref(target) => (want, Meaning::RefLabel(target)),
            _ => (TypeId::STR, Meaning::Plain),
        }
    }

    fn symbol_word(
        &mut self,
        cx: &Cx,
        symbol: ast::Symbol<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let span = symbol.syntax().span();
        if let Some(want) = want {
            if let Type::Symbol(s) = self.types.get(want) {
                let s = *s;
                if self.types.symbols[s.index()].words {
                    return (want, Meaning::Symbol(s));
                }
                let name = self.types.symbols[s.index()].name.clone();
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0420",
                        span,
                        format!("`{name}` does not accept symbol words"),
                    ),
                );
                return (want, Meaning::Plain);
            }
            if want == TypeId::ERROR {
                return (TypeId::ERROR, Meaning::Plain);
            }
        }
        self.error(
            cx.file,
            Diagnostic::error(
                "E0420",
                span,
                format!(
                    "the symbol word `{}` is only valid where a symbol type is expected",
                    symbol.syntax().text()
                ),
            ),
        );
        (TypeId::ERROR, Meaning::Plain)
    }

    /// Resolves an identifier in expression position.
    fn name(
        &mut self,
        cx: &mut Cx,
        token: SyntaxToken<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let name = token.text();
        let span = token.span();
        if let Some(found) = self.lexical(cx, name, span) {
            self.shadowed_variant(cx, name, span, want);
            return found;
        }
        if let Some(want) = want {
            match self.types.get(want).clone() {
                Type::Sum(s) => {
                    if let Some((index, variant)) = self.types.sum(s).variant(name) {
                        if variant.fields.is_some() {
                            self.error(
                                cx.file,
                                Diagnostic::error(
                                    "E0444",
                                    span,
                                    format!("the variant `{name}` needs its payload fields"),
                                )
                                .with_help(format!("write `{name}(...)`")),
                            );
                        }
                        return (
                            want,
                            Meaning::Variant {
                                sum: s,
                                variant: index,
                            },
                        );
                    }
                }
                Type::Symbol(s) => return (want, Meaning::Symbol(s)),
                Type::Ref(target) => return (want, Meaning::RefLabel(target)),
                Type::Error => return (TypeId::ERROR, Meaning::Plain),
                _ => {}
            }
        }
        if self.lookup_type(cx.file, name).is_some() {
            self.error(
                cx.file,
                Diagnostic::error("E0401", span, format!("`{name}` is a type, not a value")),
            );
            return (TypeId::ERROR, Meaning::Plain);
        }
        if !self.used_too_early(cx, name, span) {
            self.unknown_name(cx, name, span, want);
        }
        (TypeId::ERROR, Meaning::Plain)
    }

    /// Reports a name that resolves to nothing but is a binding of an
    /// enclosing `let ... in` expression that is not visible yet, and
    /// returns whether it is one.
    pub(super) fn used_too_early(&mut self, cx: &Cx, name: &str, span: Span) -> bool {
        if !cx.scope.is_pending(name) {
            return false;
        }
        self.error(
            cx.file,
            Diagnostic::error(
                "E0305",
                span,
                format!("`{name}` is used in its own definition or before it"),
            )
            .with_help("a binding can only use the bindings before it"),
        );
        true
    }

    fn unknown_name(&mut self, cx: &Cx, name: &str, span: Span, want: Option<TypeId>) {
        let mut candidates: Vec<String> = cx.scope.names().map(str::to_owned).collect();
        candidates.extend(self.value_names(cx.file));
        let mut message = format!("unknown name `{name}`");
        if let Some(s) = want.and_then(|w| self.types.as_sum(w)) {
            let sum = self.types.sum(s);
            candidates.extend(sum.variants.iter().map(|v| v.name.clone()));
            message = format!(
                "unknown name `{name}`, which is not a variant of `{}`",
                sum.name
            );
        }
        let help = did_you_mean(name, candidates.iter().map(String::as_str));
        self.error(
            cx.file,
            Diagnostic::error("E0302", span, message).with_optional_help(help),
        );
    }

    /// Warns when a binding hides a variant of the expected sum type.
    pub(super) fn shadowed_variant(
        &mut self,
        cx: &Cx,
        name: &str,
        span: Span,
        want: Option<TypeId>,
    ) {
        let Some(s) = want.and_then(|w| self.types.as_sum(w)) else {
            return;
        };
        if self.types.sum(s).variant(name).is_none() {
            return;
        }
        let sum = self.types.sum(s).name.clone();
        self.report(
            Some(cx.file),
            Diagnostic::warning(
                "W0302",
                span,
                format!("`{name}` refers to a binding, not to the variant of `{sum}`"),
            )
            .with_help(format!(
                "rename the binding, or write `{sum}.{name}` for the variant"
            )),
        );
    }

    /// Resolves a name to a lexical binding: a local, a top-level function
    /// or `let`, or a prelude declaration. Returns `None` when no binding
    /// has the name.
    pub(super) fn lexical(
        &mut self,
        cx: &mut Cx,
        name: &str,
        span: Span,
    ) -> Option<(TypeId, Meaning)> {
        if let Some(id) = cx.lookup(name) {
            let local = &self.units[cx.unit.index()].locals[id.index()];
            let (ty, reads, top) = (local.ty, local.reads.clone(), local.top);
            self.use_late(cx, name, span, &reads, top);
            return Some((ty, Meaning::Local(id)));
        }
        match self.lookup_value(cx.file, name)? {
            ValueItem::Global(g) => Some(self.global_value(cx, g, span)),
            ValueItem::Module(_) => {
                self.error(
                    cx.file,
                    Diagnostic::error("E0401", span, format!("`{name}` is a module, not a value"))
                        .with_help(format!(
                            "write `{name}.<name>` to use one of its declarations"
                        )),
                );
                Some((TypeId::ERROR, Meaning::Plain))
            }
        }
    }

    /// Returns the type of a top-level function or `let` used as a value,
    /// and records the dependency.
    pub(super) fn global_value(
        &mut self,
        cx: &mut Cx,
        g: GlobalId,
        span: Span,
    ) -> (TypeId, Meaning) {
        let global = &self.globals[g.index()];
        let ty = match &global.kind {
            GlobalKind::Fn(sig) => {
                let params = sig.params.iter().map(|p| p.ty).collect();
                let ret = sig.ret;
                self.types.intern(Type::Fn(params, ret))
            }
            GlobalKind::Let(l) => {
                let name = global.name.clone();
                if global.file == cx.file && cx.limit.is_some_and(|limit| l.start >= limit) {
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0305",
                            span,
                            format!("`{name}` is used in its own definition or before it"),
                        )
                        .with_help("a `let` can only use the declarations before it"),
                    );
                    return (TypeId::ERROR, Meaning::Plain);
                }
                let file = global.file;
                let (ty, reads) = self.let_type(g);
                let top = self.entry_tops.get(&file).copied();
                self.use_late(cx, &name, span, &reads, top);
                ty
            }
        };
        if let Some(owner) = cx.owner {
            self.calls.depend(owner, Node::Global(g), cx.file, span);
        }
        (ty, Meaning::Global(g))
    }

    /// Records the `self` reads of a late binding used in `cx`, or reports
    /// that `self` is not available there.
    fn use_late(
        &mut self,
        cx: &mut Cx,
        name: &str,
        span: Span,
        reads: &[SelfRead],
        top: Option<u32>,
    ) {
        if reads.is_empty() {
            return;
        }
        match &cx.self_mode {
            SelfMode::Body(body) if Some(body.top) == top => cx.reads.extend_from_slice(reads),
            SelfMode::Forbidden(place) => {
                let place = *place;
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0606",
                        span,
                        format!("`{name}` reads `self`, which cannot be used in {place}"),
                    ),
                );
            }
            _ => self.error(
                cx.file,
                Diagnostic::error(
                    "E0606",
                    span,
                    format!("`{name}` reads `self` of another body"),
                ),
            ),
        }
    }

    /// Returns the type of a top-level `let` and the `self` fields it
    /// reads, checking its value on first use.
    pub(super) fn let_type(&mut self, g: GlobalId) -> (TypeId, Vec<SelfRead>) {
        let global = &self.globals[g.index()];
        let GlobalKind::Let(l) = &global.kind else {
            return (TypeId::ERROR, Vec::new());
        };
        match &l.state {
            LetState::Done { ty, reads } => return (*ty, reads.clone()),
            LetState::InProgress => {
                let name = global.name.clone();
                let (file, span) = (global.file, global.span);
                let annotation = l.annotation;
                self.error(
                    file,
                    Diagnostic::error("E0305", span, format!("`{name}` depends on itself")),
                );
                return (annotation.unwrap_or(TypeId::ERROR), Vec::new());
            }
            LetState::Pending => {}
        }
        let (file, node, annotation, start) = (global.file, global.node, l.annotation, l.start);
        self.set_let_state(g, LetState::InProgress);
        let unit = self.file_unit(file);
        let self_mode = self.top_level_self_mode(file);
        let mut cx = Cx::new(unit, file, self_mode, Some(Node::Global(g)));
        cx.limit = Some(start);
        let decl = self
            .program
            .file(file)
            .parsed
            .tree
            .node(node)
            .and_then(ast::Let::cast);
        let value = decl.and_then(ast::Let::value);
        let ty = match (value, annotation) {
            (Some(value), Some(annotation)) => {
                self.expr(&mut cx, value, Some(annotation));
                annotation
            }
            (Some(value), None) => self.expr(&mut cx, value, None),
            (None, annotation) => annotation.unwrap_or(TypeId::ERROR),
        };
        if let Some(decl) = decl {
            self.fact(&cx, decl.syntax(), Fact::Let(LetFact::Global(g)));
        }
        let reads = cx.reads;
        self.set_let_state(
            g,
            LetState::Done {
                ty,
                reads: reads.clone(),
            },
        );
        (ty, reads)
    }

    fn set_let_state(&mut self, g: GlobalId, state: LetState) {
        if let GlobalKind::Let(l) = &mut self.globals[g.index()].kind {
            l.state = state;
        }
    }

    /// Returns where `self` refers to in a top-level `let` of `file`.
    pub(super) fn top_level_self_mode(&mut self, file: super::FileId) -> SelfMode {
        match (self.role(file), self.root) {
            (super::Role::Entry, Some(root)) => SelfMode::Body(BodyCx {
                top: self.entry_top(file),
                frames: vec![Frame {
                    target: Target::Struct(root),
                    path: Vec::new(),
                }],
                graph: super::body::SelfGraph::default(),
            }),
            _ => SelfMode::Forbidden("a module"),
        }
    }

    pub(super) fn entry_top(&mut self, file: super::FileId) -> u32 {
        if let Some(&top) = self.entry_tops.get(&file) {
            return top;
        }
        let top = self.new_top();
        self.entry_tops.insert(file, top);
        top
    }

    fn list(
        &mut self,
        cx: &mut Cx,
        list: ast::List<'p>,
        expected: Option<TypeId>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let items: Vec<Expr<'p>> = list.items().collect();
        let elem = want.and_then(|w| match self.types.get(w) {
            Type::List(t) | Type::Set(t) => Some(*t),
            Type::Data => Some(TypeId::DATA),
            _ => None,
        });
        if let (Some(elem), Some(want)) = (elem, want) {
            for item in &items {
                self.expr(cx, *item, Some(elem));
            }
            return (want, Meaning::Plain);
        }
        let Some((first, rest)) = items.split_first() else {
            if expected.is_none() {
                self.cannot_infer(cx, list.syntax().span(), "[]");
                return (TypeId::ERROR, Meaning::Plain);
            }
            return (self.types.list(TypeId::ERROR), Meaning::Plain);
        };
        let elem = self.expr(cx, *first, None);
        for item in rest {
            self.expr(cx, *item, Some(elem));
        }
        (self.types.list(elem), Meaning::Plain)
    }

    /// Binds the loop names of a `for` statement or a comprehension to the
    /// elements of a collection of type `source`.
    pub(super) fn iterate(
        &mut self,
        cx: &mut Cx,
        source: TypeId,
        names: &[SyntaxToken<'p>],
        span: Span,
    ) -> Vec<LocalId> {
        let (map, types) = match self.types.get(self.types.peel(source)).clone() {
            Type::List(t) | Type::Set(t) => (false, vec![t]),
            Type::Map(k, v) => (true, vec![k, v]),
            Type::Error => (false, vec![TypeId::ERROR; names.len()]),
            _ => {
                let ty = self.types.render_plain(source);
                self.error(
                    cx.file,
                    Diagnostic::error("E0401", span, format!("cannot iterate over `{ty}`"))
                        .with_help("iterate over a `List`, a `Set` or a `Map`"),
                );
                (false, vec![TypeId::ERROR; names.len()])
            }
        };
        if types.len() != names.len() {
            let message = if map {
                "iterating over a map binds a key and a value"
            } else {
                "iterating over a list or a set binds one value"
            };
            let help = if map {
                "write `for key, value in ...`"
            } else {
                "write `for x in ...`"
            };
            self.error(
                cx.file,
                Diagnostic::error("E0401", span, message).with_help(help),
            );
        }
        let mut binds = Vec::new();
        for (i, token) in names.iter().enumerate() {
            let ty = types.get(i).copied().unwrap_or(TypeId::ERROR);
            let id = self.local(cx, token.text(), ty, Vec::new());
            cx.scope.push((token.text().to_owned(), Binding::Local(id)));
            binds.push(id);
        }
        binds
    }

    fn list_comp(
        &mut self,
        cx: &mut Cx,
        comp: ast::ListComp<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let mark = cx.scope.len();
        let source = match comp.source() {
            Some(source) => self.expr(cx, source, None),
            None => TypeId::ERROR,
        };
        let names: Vec<SyntaxToken<'p>> = comp.names().collect();
        let binds = self.iterate(cx, source, &names, comp.syntax().span());
        if let Some(guard) = comp.guard() {
            self.expr(cx, guard, Some(TypeId::BOOL));
        }
        let elem = want.and_then(|w| match self.types.get(w) {
            Type::List(t) | Type::Set(t) => Some((*t, w)),
            Type::Data => Some((TypeId::DATA, TypeId::DATA)),
            _ => None,
        });
        let ty = match (comp.value(), elem) {
            (Some(value), Some((elem, ty))) => {
                self.expr(cx, value, Some(elem));
                ty
            }
            (Some(value), None) => {
                let elem = self.expr(cx, value, None);
                self.types.list(elem)
            }
            (None, _) => TypeId::ERROR,
        };
        cx.scope.truncate(mark);
        (ty, Meaning::Binds(binds))
    }

    fn map_comp(
        &mut self,
        cx: &mut Cx,
        comp: ast::MapComp<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let mark = cx.scope.len();
        let source = match comp.source() {
            Some(source) => self.expr(cx, source, None),
            None => TypeId::ERROR,
        };
        let names: Vec<SyntaxToken<'p>> = comp.names().collect();
        let binds = self.iterate(cx, source, &names, comp.syntax().span());
        if let Some(guard) = comp.guard() {
            self.expr(cx, guard, Some(TypeId::BOOL));
        }
        let (want_key, want_value) = match want.map(|w| self.types.get(w).clone()) {
            Some(Type::Map(k, v)) => (Some(k), Some(v)),
            Some(Type::Data) => (Some(TypeId::STR), Some(TypeId::DATA)),
            _ => (None, None),
        };
        let (key_ty, key_meaning) = match comp.key() {
            Some(Key::Str(string)) => {
                self.interpolations(cx, string);
                (TypeId::STR, None)
            }
            Some(Key::Name(token)) => {
                let want_key = want_key.map(|k| self.types.peel(k));
                let (ty, meaning) = self.name(cx, token, want_key);
                (ty, Some(meaning))
            }
            None => (TypeId::ERROR, None),
        };
        let key_span = comp.key().map_or(comp.syntax().span(), Key::span);
        if !self.types.is_map_key(key_ty) {
            let ty = self.types.render_plain(key_ty);
            self.error(
                cx.file,
                Diagnostic::error("E0411", key_span, format!("`{ty}` cannot be a map key"))
                    .with_help("map keys are `Str` or a sum type whose variants have no fields"),
            );
        } else if let Some(k) = want_key
            && !self.types.fits(key_ty, k)
        {
            self.mismatch(cx.file, key_span, k, key_ty);
        }
        let value_ty = match comp.value() {
            Some(value) => self.expr(cx, value, want_value),
            None => TypeId::ERROR,
        };
        cx.scope.truncate(mark);
        let ty = match want {
            Some(w) if want_key.is_some() => w,
            _ => self.types.map(key_ty, value_ty),
        };
        let meaning = match key_meaning {
            Some(key) => Meaning::MapCompKey {
                binds,
                key: Box::new(key),
            },
            None => Meaning::Binds(binds),
        };
        (ty, meaning)
    }

    fn map_literal(
        &mut self,
        cx: &mut Cx,
        map: ast::Map<'p>,
        expected: Option<TypeId>,
        want: Option<TypeId>,
        partial: bool,
    ) -> (TypeId, Meaning) {
        let entries: Vec<ast::MapEntry<'p>> = map.entries().collect();
        let span = map.syntax().span();
        match want.map(|w| self.types.get(w).clone()) {
            Some(Type::Struct(s)) => {
                self.struct_entries(cx, s, &entries, span, partial);
                return (want.unwrap_or(TypeId::ERROR), Meaning::Struct(s));
            }
            Some(Type::Map(k, v)) => {
                self.map_entries(cx, k, v, &entries, partial);
                return (want.unwrap_or(TypeId::ERROR), Meaning::Plain);
            }
            Some(Type::Data) => {
                // A struct value inside `Data` becomes data when it is built
                // and cannot merge with other contributions, so it must be
                // complete even where a contribution may be partial.
                self.map_entries(cx, TypeId::STR, TypeId::DATA, &entries, false);
                return (TypeId::DATA, Meaning::Plain);
            }
            Some(Type::Error) => {
                for entry in &entries {
                    if let Some(value) = entry.value() {
                        self.expr(cx, value, Some(TypeId::ERROR));
                    }
                }
                return (TypeId::ERROR, Meaning::Plain);
            }
            _ => {}
        }
        let Some(first) = entries.first().and_then(|e| e.value()) else {
            if expected.is_none() {
                self.cannot_infer(cx, span, "{}");
                return (TypeId::ERROR, Meaning::Plain);
            }
            return (self.types.map(TypeId::STR, TypeId::ERROR), Meaning::Plain);
        };
        let value = self.expr(cx, first, None);
        self.check_map_keys(cx, TypeId::STR, &entries);
        for entry in &entries[1..] {
            if let Some(v) = entry.value() {
                self.expr(cx, v, Some(value));
            }
        }
        (self.types.map(TypeId::STR, value), Meaning::Plain)
    }

    fn map_entries(
        &mut self,
        cx: &mut Cx,
        key: TypeId,
        value: TypeId,
        entries: &[ast::MapEntry<'p>],
        partial: bool,
    ) {
        self.check_map_keys(cx, key, entries);
        for entry in entries {
            if let Some(v) = entry.value() {
                cx.partial = partial;
                self.expr(cx, v, Some(value));
            }
        }
    }

    /// Checks the keys of map literal entries against the key type and
    /// reports duplicates.
    fn check_map_keys(&mut self, cx: &mut Cx, key: TypeId, entries: &[ast::MapEntry<'p>]) {
        let sum = self.types.as_sum(key);
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for entry in entries {
            let Some(k) = entry.key() else { continue };
            let text = match k {
                Key::Name(token) => Some(token.text().to_owned()),
                Key::Str(string) => {
                    self.interpolations(cx, string);
                    string.value()
                }
            };
            let Some(text) = text else { continue };
            if let Some(s) = sum
                && self.types.sum(s).variant(&text).is_none()
            {
                let def = self.types.sum(s);
                let help = did_you_mean(&text, def.variants.iter().map(|v| v.name.as_str()));
                let name = def.name.clone();
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0302",
                        k.span(),
                        format!("`{text}` is not a variant of `{name}`"),
                    )
                    .with_optional_help(help),
                );
            }
            if let Some(&first) = seen.get(&text) {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0306",
                        k.span(),
                        format!("the key `{text}` is given more than once"),
                    )
                    .with_label(None, first, "first given here"),
                );
            } else {
                seen.insert(text, k.span());
            }
        }
    }

    /// Checks the entries of a struct literal of `s`. A complete literal
    /// must set every required field.
    fn struct_entries(
        &mut self,
        cx: &mut Cx,
        s: StructId,
        entries: &[ast::MapEntry<'p>],
        span: Span,
        partial: bool,
    ) {
        let mut set: BTreeMap<String, Span> = BTreeMap::new();
        for entry in entries {
            let Some(key) = entry.key() else { continue };
            let name = match key {
                Key::Name(token) => Some(token.text().to_owned()),
                Key::Str(string) => string.value(),
            };
            let value = entry.value();
            let Some(name) = name else {
                self.error(
                    cx.file,
                    Diagnostic::error("E0401", key.span(), "a field name cannot be interpolated"),
                );
                if let Some(v) = value {
                    self.expr(cx, v, None);
                }
                continue;
            };
            if let Some(&first) = set.get(&name) {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0306",
                        key.span(),
                        format!("the field `{name}` is given more than once"),
                    )
                    .with_label(None, first, "first given here"),
                );
            }
            set.entry(name.clone()).or_insert(key.span());
            let def = self.types.structure(s);
            let field_ty = match def.field(&name) {
                Some((_, field)) => Some(field.ty),
                None => {
                    let help = did_you_mean(&name, def.fields.iter().map(|f| f.name.as_str()));
                    let struct_name = def.name.clone();
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0302",
                            key.span(),
                            format!("`{struct_name}` has no field `{name}`"),
                        )
                        .with_optional_help(help),
                    );
                    None
                }
            };
            if let Some(v) = value {
                cx.partial = partial;
                self.expr(cx, v, Some(field_ty.unwrap_or(TypeId::ERROR)));
            }
        }
        if let Some(owner) = cx.owner {
            self.calls
                .depend(owner, Node::StructDefaults(s), cx.file, span);
        }
        if partial {
            return;
        }
        let def = self.types.structure(s);
        let missing: Vec<String> = def
            .fields
            .iter()
            .filter(|f| f.is_required(&self.types) && !set.contains_key(&f.name))
            .map(|f| format!("`{}`", f.name))
            .collect();
        if !missing.is_empty() {
            let name = def.name.clone();
            let fields = missing.join(", ");
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0801",
                    span,
                    format!("the `{name}` value is missing the required fields {fields}"),
                )
                .with_help("set them, or give the fields defaults"),
            );
        }
    }

    fn struct_lit(
        &mut self,
        cx: &mut Cx,
        lit: ast::StructLit<'p>,
        partial: bool,
    ) -> (TypeId, Meaning) {
        let entries: Vec<ast::MapEntry<'p>> = lit.entries().collect();
        let span = lit.syntax().span();
        let item = lit.type_path().and_then(|path| {
            let segments: Vec<SyntaxToken<'p>> = path.segments().collect();
            self.type_path(cx, &segments, path.syntax().span())
        });
        let Some(item) = item else {
            for entry in &entries {
                if let Some(v) = entry.value() {
                    self.expr(cx, v, Some(TypeId::ERROR));
                }
            }
            return (TypeId::ERROR, Meaning::Plain);
        };
        let TypeItem::Struct(s) = item else {
            let path = lit
                .type_path()
                .map_or(String::new(), |p| p.syntax().text().to_owned());
            self.error(
                cx.file,
                Diagnostic::error("E0401", span, format!("`{path}` is not a struct")),
            );
            return (TypeId::ERROR, Meaning::Plain);
        };
        self.struct_entries(cx, s, &entries, span, partial);
        (self.types.intern(Type::Struct(s)), Meaning::Struct(s))
    }

    /// Resolves a written type name such as `Name` or `alias.Name`.
    fn type_path(&mut self, cx: &Cx, segments: &[SyntaxToken<'p>], span: Span) -> Option<TypeItem> {
        let found = match segments {
            [name] => self.lookup_type(cx.file, name.text()),
            [alias, name] => match self.lookup_value(cx.file, alias.text()) {
                Some(ValueItem::Module(Some(m))) => {
                    self.scopes[m.index()].types.get(name.text()).copied()
                }
                Some(ValueItem::Module(None)) => return None,
                _ => None,
            },
            _ => None,
        };
        if found.is_none() {
            let name = segments.last().map_or("", |t| t.text());
            let candidates: Vec<String> = self.scopes[cx.file.index()]
                .types
                .keys()
                .chain(self.prelude.types.keys())
                .cloned()
                .collect();
            let help = did_you_mean(name, candidates.iter().map(String::as_str));
            self.error(
                cx.file,
                Diagnostic::error("E0302", span, format!("unknown type `{name}`"))
                    .with_optional_help(help),
            );
        }
        found
    }

    /// Resolves a static path: a module alias, a type, a member of a module
    /// or a qualified variant. Returns `None` for an expression that is not
    /// a static path, such as a local binding or a field of a value.
    pub(super) fn static_path(&mut self, cx: &Cx, e: Expr<'p>) -> Option<PathItem> {
        match e {
            Expr::Name(name) => {
                let token = name.token()?;
                let text = token.text();
                if cx.lookup(text).is_some() {
                    return None;
                }
                match self.lookup_value(cx.file, text) {
                    Some(ValueItem::Module(m)) => Some(PathItem::Module(m)),
                    Some(ValueItem::Global(_)) => None,
                    None => self.lookup_type(cx.file, text).map(PathItem::Type),
                }
            }
            Expr::Field(field) if !field.is_optional() => {
                let base = field.base()?;
                let member = field.field()?;
                let text = member.text();
                match self.static_path(cx, base)? {
                    PathItem::Module(None) | PathItem::Error => Some(PathItem::Error),
                    PathItem::Module(Some(m)) => {
                        let scope = &self.scopes[m.index()];
                        if let Some(ValueItem::Global(g)) = scope.values.get(text) {
                            return Some(PathItem::Value(*g));
                        }
                        if let Some(t) = scope.types.get(text) {
                            return Some(PathItem::Type(*t));
                        }
                        let candidates: Vec<String> = scope
                            .values
                            .iter()
                            .filter(|(_, v)| matches!(v, ValueItem::Global(_)))
                            .map(|(n, _)| n.clone())
                            .chain(scope.types.keys().cloned())
                            .collect();
                        let help = did_you_mean(text, candidates.iter().map(String::as_str));
                        let module = base.syntax().text().to_owned();
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0302",
                                member.span(),
                                format!("module `{module}` has no declaration `{text}`"),
                            )
                            .with_optional_help(help),
                        );
                        Some(PathItem::Error)
                    }
                    PathItem::Type(TypeItem::Sum(s)) => {
                        let sum = self.types.sum(s);
                        if let Some((index, _)) = sum.variant(text) {
                            return Some(PathItem::Variant(s, index));
                        }
                        let help = did_you_mean(text, sum.variants.iter().map(|v| v.name.as_str()));
                        let name = sum.name.clone();
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0302",
                                member.span(),
                                format!("`{name}` has no variant `{text}`"),
                            )
                            .with_optional_help(help),
                        );
                        Some(PathItem::Error)
                    }
                    PathItem::Type(_) => {
                        let name = base.syntax().text().to_owned();
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0401",
                                member.span(),
                                format!("the type `{name}` has no members"),
                            ),
                        );
                        Some(PathItem::Error)
                    }
                    PathItem::Value(_) | PathItem::Variant(..) => None,
                }
            }
            _ => None,
        }
    }

    fn field(&mut self, cx: &mut Cx, field: ast::FieldExpr<'p>) -> (TypeId, Meaning) {
        let span = field.syntax().span();
        let (Some(base), Some(member)) = (field.base(), field.field()) else {
            return (TypeId::ERROR, Meaning::Plain);
        };
        if let Expr::SelfExpr(_) = base {
            return self.self_field(cx, field, member);
        }
        if let Some(item) = self.static_path(cx, Expr::Field(field)) {
            return self.path_value(cx, item, span);
        }
        let base_ty = self.expr(cx, base, None);
        let name = member.text();
        let optional = field.is_optional();
        let peeled = self.types.peel(base_ty);
        let (struct_ty, wrap) = match self.types.get(peeled).clone() {
            Type::Error => return (TypeId::ERROR, Meaning::Plain),
            Type::Option(inner) if optional => (inner, true),
            Type::Option(_) => {
                let ty = self.types.render_plain(base_ty);
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0401",
                        member.span(),
                        format!("a value of type `{ty}` may be `none`"),
                    )
                    .with_help(format!(
                        "write `?.{name}` to read the field of a present value"
                    )),
                );
                return (TypeId::ERROR, Meaning::Plain);
            }
            _ if optional => {
                let ty = self.types.render_plain(base_ty);
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0401",
                        member.span(),
                        format!("`?.` needs an `Option`, found `{ty}`"),
                    )
                    .with_help(format!("write `.{name}`")),
                );
                (peeled, false)
            }
            _ => (peeled, false),
        };
        let Some(s) = self.types.as_struct(struct_ty) else {
            if !self.types.has_error(struct_ty) {
                let ty = self.types.render_plain(struct_ty);
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0401",
                        member.span(),
                        format!("a value of type `{ty}` has no fields"),
                    ),
                );
            }
            return (TypeId::ERROR, Meaning::Plain);
        };
        let def = self.types.structure(s);
        let Some((index, f)) = def.field(name) else {
            let help = did_you_mean(name, def.fields.iter().map(|f| f.name.as_str()));
            let struct_name = def.name.clone();
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0302",
                    member.span(),
                    format!("`{struct_name}` has no field `{name}`"),
                )
                .with_optional_help(help),
            );
            return (TypeId::ERROR, Meaning::Plain);
        };
        let mut ty = f.ty;
        if wrap {
            ty = self.types.option(ty);
        }
        (ty, Meaning::Field { index, optional })
    }

    /// Returns the value a static path names in expression position.
    fn path_value(&mut self, cx: &mut Cx, item: PathItem, span: Span) -> (TypeId, Meaning) {
        match item {
            PathItem::Value(g) => self.global_value(cx, g, span),
            PathItem::Variant(s, index) => {
                let variant = &self.types.sum(s).variants[index];
                if variant.fields.is_some() {
                    let name = variant.name.clone();
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0444",
                            span,
                            format!("the variant `{name}` needs its payload fields"),
                        )
                        .with_help(format!("write `{name}(...)`")),
                    );
                }
                (
                    self.types.intern(Type::Sum(s)),
                    Meaning::Variant {
                        sum: s,
                        variant: index,
                    },
                )
            }
            PathItem::Type(_) => {
                self.error(
                    cx.file,
                    Diagnostic::error("E0401", span, "a type is not a value"),
                );
                (TypeId::ERROR, Meaning::Plain)
            }
            PathItem::Module(_) => {
                self.error(
                    cx.file,
                    Diagnostic::error("E0401", span, "a module is not a value"),
                );
                (TypeId::ERROR, Meaning::Plain)
            }
            PathItem::Error => (TypeId::ERROR, Meaning::Plain),
        }
    }

    fn self_field(
        &mut self,
        cx: &mut Cx,
        field: ast::FieldExpr<'p>,
        member: SyntaxToken<'p>,
    ) -> (TypeId, Meaning) {
        let name = member.text();
        let span = field.syntax().span();
        if field.is_optional() {
            self.error(
                cx.file,
                Diagnostic::error("E0401", span, "`self` is never `none`")
                    .with_help(format!("write `self.{name}`")),
            );
        }
        let found = match &cx.self_mode {
            SelfMode::Forbidden(place) => {
                let place = *place;
                self.error(
                    cx.file,
                    Diagnostic::error("E0606", span, format!("`self` cannot be used in {place}"))
                        .with_help("pass the value as a parameter instead"),
                );
                return (TypeId::ERROR, Meaning::Plain);
            }
            SelfMode::Default(s) => self
                .types
                .structure(*s)
                .field(name)
                .map(|(index, f)| (0, *s, index, f.ty)),
            SelfMode::Body(body) => {
                body.frames
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(depth, frame)| {
                        let Target::Struct(s) = frame.target else {
                            return None;
                        };
                        self.types
                            .structure(s)
                            .field(name)
                            .map(|(index, f)| (depth, s, index, f.ty))
                    })
            }
        };
        let Some((depth, structure, index, ty)) = found else {
            let candidates: Vec<String> = match &cx.self_mode {
                SelfMode::Default(s) => self
                    .types
                    .structure(*s)
                    .fields
                    .iter()
                    .map(|f| f.name.clone())
                    .collect(),
                SelfMode::Body(body) => body
                    .frames
                    .iter()
                    .filter_map(|f| match f.target {
                        Target::Struct(s) => Some(s),
                        _ => None,
                    })
                    .flat_map(|s| {
                        self.types
                            .structure(s)
                            .fields
                            .iter()
                            .map(|f| f.name.clone())
                    })
                    .collect(),
                SelfMode::Forbidden(_) => Vec::new(),
            };
            let help = did_you_mean(name, candidates.iter().map(String::as_str));
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0303",
                    member.span(),
                    format!("no enclosing block has a field `{name}`"),
                )
                .with_optional_help(help),
            );
            return (TypeId::ERROR, Meaning::Plain);
        };
        let read = SelfRead {
            depth,
            structure,
            field: index,
        };
        cx.reads.push(read);
        (ty, Meaning::SelfField(read))
    }

    fn index(&mut self, cx: &mut Cx, index: ast::IndexExpr<'p>) -> (TypeId, Meaning) {
        let base = match index.base() {
            Some(b) => self.expr(cx, b, None),
            None => TypeId::ERROR,
        };
        let (key, value) = match self.types.get(self.types.peel(base)).clone() {
            Type::List(t) => (TypeId::INT, t),
            Type::Map(k, v) => (k, v),
            Type::Error => (TypeId::ERROR, TypeId::ERROR),
            _ => {
                let ty = self.types.render_plain(base);
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0401",
                        index.syntax().span(),
                        format!("cannot index a value of type `{ty}`"),
                    ),
                );
                (TypeId::ERROR, TypeId::ERROR)
            }
        };
        if let Some(i) = index.index() {
            self.expr(cx, i, Some(key));
        }
        (value, Meaning::Plain)
    }

    fn binary(
        &mut self,
        cx: &mut Cx,
        binary: ast::BinaryExpr<'p>,
        expected: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let span = binary
            .op_token()
            .map_or(binary.syntax().span(), |t| t.span());
        let (Some(op), Some(lhs), Some(rhs)) = (binary.op(), binary.lhs(), binary.rhs()) else {
            return (TypeId::ERROR, Meaning::Plain);
        };
        let ty = match op {
            BinaryOp::And | BinaryOp::Or => {
                self.expr(cx, lhs, Some(TypeId::BOOL));
                self.expr(cx, rhs, Some(TypeId::BOOL));
                TypeId::BOOL
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                let l = self.expr(cx, lhs, None);
                if self.types.has_code(l) {
                    self.error(
                        cx.file,
                        Diagnostic::error("E0401", span, "functions and parts cannot be compared"),
                    );
                    self.expr(cx, rhs, None);
                } else {
                    self.expr(cx, rhs, Some(l));
                }
                TypeId::BOOL
            }
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                let l = self.expr(cx, lhs, None);
                self.expr(cx, rhs, Some(l));
                if !self.types.is_ordered(l) {
                    let ty = self.types.render_plain(l);
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0401",
                            span,
                            format!("values of type `{ty}` cannot be ordered"),
                        )
                        .with_help("order numbers, strings or sum types without payloads"),
                    );
                }
                TypeId::BOOL
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                let l = self.expr(cx, lhs, None);
                let r = self.expr(cx, rhs, None);
                self.arithmetic(cx, op, l, r, span)
            }
            BinaryOp::Concat => {
                let l = self.expr(cx, lhs, None);
                match self.types.get(self.types.peel(l)) {
                    Type::Str => {
                        self.expr(cx, rhs, Some(TypeId::STR));
                        TypeId::STR
                    }
                    Type::List(_) => {
                        let l = self.types.peel(l);
                        self.expr(cx, rhs, Some(l));
                        l
                    }
                    Type::Error => {
                        self.expr(cx, rhs, None);
                        TypeId::ERROR
                    }
                    _ => {
                        self.expr(cx, rhs, None);
                        let ty = self.types.render_plain(l);
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0401",
                                span,
                                format!("`++` joins strings or lists, not `{ty}`"),
                            ),
                        );
                        TypeId::ERROR
                    }
                }
            }
            BinaryOp::Coalesce => {
                let l = self.expr(cx, lhs, None);
                match self.types.unwrap_option(l) {
                    Some(inner) => {
                        let target = expected
                            .filter(|e| self.types.fits(inner, *e))
                            .unwrap_or(inner);
                        self.expr(cx, rhs, Some(target))
                    }
                    None => {
                        self.expr(cx, rhs, None);
                        if !self.types.has_error(l) {
                            let ty = self.types.render_plain(l);
                            self.error(
                                cx.file,
                                Diagnostic::error(
                                    "E0401",
                                    span,
                                    format!(
                                        "the left side of `??` must be an `Option`, found `{ty}`"
                                    ),
                                ),
                            );
                        }
                        TypeId::ERROR
                    }
                }
            }
        };
        (ty, Meaning::Plain)
    }

    fn arithmetic(&mut self, cx: &Cx, op: BinaryOp, l: TypeId, r: TypeId, span: Span) -> TypeId {
        let (pl, pr) = (self.types.peel(l), self.types.peel(r));
        if self.types.has_error(pl) || self.types.has_error(pr) {
            return TypeId::ERROR;
        }
        let symbol = match op {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            _ => "%",
        };
        match (pl, pr) {
            (TypeId::INT, TypeId::INT) => TypeId::INT,
            (TypeId::FLOAT, TypeId::FLOAT) if op != BinaryOp::Rem => TypeId::FLOAT,
            (TypeId::INT, TypeId::FLOAT) | (TypeId::FLOAT, TypeId::INT) if op != BinaryOp::Rem => {
                self.error(
                    cx.file,
                    Diagnostic::error("E0431", span, "cannot mix `Int` and `Float` in arithmetic")
                        .with_help("convert the `Int` with `toFloat(...)`"),
                );
                TypeId::ERROR
            }
            _ => {
                let what = if op == BinaryOp::Rem {
                    "two `Int` values"
                } else {
                    "two `Int` or two `Float` values"
                };
                let (l, r) = (self.types.render_plain(l), self.types.render_plain(r));
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0401",
                        span,
                        format!("`{symbol}` needs {what}, found `{l}` and `{r}`"),
                    ),
                );
                TypeId::ERROR
            }
        }
    }

    fn unary(
        &mut self,
        cx: &mut Cx,
        unary: ast::UnaryExpr<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let (Some(op), Some(operand)) = (unary.op(), unary.operand()) else {
            return (TypeId::ERROR, Meaning::Plain);
        };
        let ty = match op {
            UnaryOp::Not => self.expr(cx, operand, Some(TypeId::BOOL)),
            UnaryOp::Neg => {
                let numeric = want.filter(|w| matches!(*w, TypeId::INT | TypeId::FLOAT));
                let ty = self.expr(cx, operand, numeric);
                let peeled = self.types.peel(ty);
                if !matches!(peeled, TypeId::INT | TypeId::FLOAT | TypeId::ERROR) {
                    let rendered = self.types.render_plain(ty);
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0401",
                            unary.syntax().span(),
                            format!("cannot negate a value of type `{rendered}`"),
                        ),
                    );
                    TypeId::ERROR
                } else {
                    peeled
                }
            }
        };
        (ty, Meaning::Plain)
    }

    fn if_expr(
        &mut self,
        cx: &mut Cx,
        branch: ast::IfExpr<'p>,
        expected: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        if let Some(condition) = branch.condition() {
            self.expr(cx, condition, Some(TypeId::BOOL));
        }
        let (then, otherwise) = (branch.then_branch(), branch.else_branch());
        let ty = match expected {
            Some(exp) => {
                for e in [then, otherwise].into_iter().flatten() {
                    self.expr(cx, e, Some(exp));
                }
                exp
            }
            None => {
                let ty = then.map_or(TypeId::ERROR, |e| self.expr(cx, e, None));
                if let Some(e) = otherwise {
                    self.expr(cx, e, Some(ty));
                }
                ty
            }
        };
        (ty, Meaning::Plain)
    }

    fn let_expr(
        &mut self,
        cx: &mut Cx,
        l: ast::LetExpr<'p>,
        expected: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let mark = cx.scope.len();
        let bindings: Vec<ast::Binding<'p>> = l.bindings().collect();
        for binding in &bindings {
            if let Some(name) = binding.name() {
                cx.scope.push((name.text().to_owned(), Binding::Pending));
            }
        }
        for binding in &bindings {
            let annotation = binding.ty().map(|t| self.resolve_local_type(cx, t));
            let ty = match (binding.value(), annotation) {
                (Some(v), Some(a)) => {
                    self.expr(cx, v, Some(a));
                    a
                }
                (Some(v), None) => self.expr(cx, v, None),
                (None, a) => a.unwrap_or(TypeId::ERROR),
            };
            let Some(name) = binding.name() else { continue };
            let id = self.local(cx, name.text(), ty, Vec::new());
            cx.scope.push((name.text().to_owned(), Binding::Local(id)));
            self.fact(cx, binding.syntax(), Fact::Let(LetFact::Local(id)));
        }
        let ty = match l.body() {
            Some(body) => self.expr(cx, body, expected),
            None => TypeId::ERROR,
        };
        cx.scope.truncate(mark);
        (ty, Meaning::Plain)
    }

    fn lambda(
        &mut self,
        cx: &mut Cx,
        lambda: ast::Lambda<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let params = match lambda.params() {
            Some(list) => self.resolve_params(cx.file, list, cx.owner),
            None => Vec::new(),
        };
        self.local_types_resolved();
        for param in &params {
            if param.default.is_some() {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0444",
                        param.span,
                        format!(
                            "the lambda parameter `{}` cannot have a default",
                            param.name
                        ),
                    )
                    .with_help("a function value is always called with every argument"),
                );
            }
        }
        let expected_ret = want.and_then(|w| match self.types.get(w) {
            Type::Fn(_, ret) if !self.types.has_error(*ret) => Some(*ret),
            _ => None,
        });
        let ret = lambda
            .return_type()
            .map(|t| self.resolve_local_type(cx, t))
            .or(expected_ret);
        let saved = std::mem::replace(&mut cx.self_mode, SelfMode::Forbidden("a lambda"));
        let mark = cx.scope.len();
        let mut binds = Vec::new();
        for param in &params {
            let id = self.local(cx, &param.name, param.ty, Vec::new());
            cx.scope.push((param.name.clone(), Binding::Local(id)));
            binds.push(id);
        }
        let body_ty = match lambda.body() {
            Some(body) => self.expr(cx, body, ret),
            None => TypeId::ERROR,
        };
        cx.scope.truncate(mark);
        cx.self_mode = saved;
        let ret = ret.unwrap_or(body_ty);
        let ty = self
            .types
            .intern(Type::Fn(params.iter().map(|p| p.ty).collect(), ret));
        (ty, Meaning::Binds(binds))
    }

    /// Returns the type `ty` written inside a body or an expression
    /// denotes, with the function predicates of its refinements resolved
    /// and its references checked.
    pub(super) fn resolve_local_type(&mut self, cx: &Cx, ty: ast::Type<'p>) -> TypeId {
        let resolved = self.resolve_type(cx.file, ty, cx.owner);
        self.local_types_resolved();
        resolved
    }

    /// Resolves the function predicates and checks the references of the
    /// types resolved inside a body, which declarations did not cover.
    fn local_types_resolved(&mut self) {
        self.check_pending_predicates();
        self.check_ref_uses();
    }

    fn part_expr(
        &mut self,
        cx: &mut Cx,
        part: ast::PartExpr<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let target = want.and_then(|w| match self.types.get(w) {
            Type::Part(s) => Some(*s),
            _ => None,
        });
        let Some(s) = target else {
            if want != Some(TypeId::ERROR) {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0435",
                        part.syntax().span(),
                        "the struct of this `part` is unknown",
                    )
                    .with_help("give the binding or the function a type such as `Part<S>`"),
                );
            }
            return (TypeId::ERROR, Meaning::Plain);
        };
        if let Some(body) = part.body() {
            self.top_body(cx, body.statements(), s);
        }
        (want.unwrap_or(TypeId::ERROR), Meaning::Plain)
    }

    /// Checks the statements of a body that starts a new `self` scope: a
    /// `part { }` expression or a part file.
    pub(super) fn top_body(
        &mut self,
        cx: &mut Cx,
        statements: impl Iterator<Item = ast::Stmt<'p>>,
        s: StructId,
    ) {
        let top = self.new_top();
        let body = BodyCx {
            top,
            frames: vec![Frame {
                target: Target::Struct(s),
                path: Vec::new(),
            }],
            graph: super::body::SelfGraph::default(),
        };
        let saved_mode = std::mem::replace(&mut cx.self_mode, SelfMode::Body(body));
        let saved_reads = std::mem::take(&mut cx.reads);
        let mark = cx.scope.len();
        self.statements(cx, statements, Target::Struct(s));
        cx.scope.truncate(mark);
        let mode = std::mem::replace(&mut cx.self_mode, saved_mode);
        cx.reads = saved_reads;
        if let SelfMode::Body(body) = mode {
            self.self_cycles(cx.file, &body.graph);
        }
    }
}

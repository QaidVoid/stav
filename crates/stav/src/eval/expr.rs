//! Evaluating expressions.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::rc::Rc;

use super::body::elements;
use super::env::{Env, Slot, bind, lookup};
use super::merge::{Frame, Seg};
use super::val::{FnVal, Key as MapKey, PathVal, RefVal, SymbolVal, Val};
use super::{Eval, R};
use crate::ast::{self, BinaryOp, Expr, Key, LiteralKind, StrPart, UnaryOp};
use crate::check::FileId;
use crate::check::model::{Fact, LetFact, LocalId, Meaning, SelfRead, UnitId};
use crate::check::types::{StructId, SumId, SymbolId, Type, TypeId};
use crate::diagnostic::Diagnostic;
use crate::lexer::{parse_float, parse_int};
use crate::span::{SourceSpan, Span};
use crate::syntax::SyntaxToken;

/// The context an expression is evaluated in.
pub(crate) struct Cx<'f> {
    pub(crate) unit: UnitId,
    pub(crate) file: FileId,
    pub(crate) env: Env,
    /// The blocks `self` reads resolve against, outermost first. Empty
    /// where `self` cannot be read.
    pub(crate) frames: &'f [Frame],
}

impl<'f> Cx<'f> {
    pub(crate) fn with_env(&self, env: Env) -> Cx<'f> {
        Cx {
            unit: self.unit,
            file: self.file,
            env,
            frames: self.frames,
        }
    }

    pub(crate) fn at(&self, span: Span) -> SourceSpan {
        SourceSpan {
            file: self.file,
            span,
        }
    }
}

impl<'c> Eval<'c> {
    /// Evaluates an expression.
    pub(crate) fn expr(&mut self, cx: &Cx<'_>, e: Expr<'c>) -> R<Val> {
        let at = cx.at(e.syntax().span());
        self.step(at)?;
        self.enter(at)?;
        let result = self.expr_inner(cx, e, at);
        self.leave();
        result
    }

    /// Dispatches on the kind of expression. Every arm calls a function of
    /// its own, so that this frame stays small, because evaluation recurses
    /// through it once per nested expression.
    fn expr_inner(&mut self, cx: &Cx<'_>, e: Expr<'c>, at: SourceSpan) -> R<Val> {
        let (ty, meaning) = match self.fact(cx.unit, e.syntax().index()) {
            Fact::Expr(f) => (f.ty, &f.meaning),
            _ => return Err(self.bug(cx.file, at.span, "an expression has no facts")),
        };
        match e {
            Expr::Literal(literal) => self.literal(cx, literal, ty, meaning, at),
            Expr::Str(string) => self.string(cx, string, meaning, at),
            Expr::Name(name) => match name.token() {
                Some(token) => self.name(cx, token, meaning, at),
                None => Err(self.bug(cx.file, at.span, "a name has no token")),
            },
            Expr::Symbol(symbol) => self.symbol_word(cx, symbol, meaning, at),
            Expr::SelfExpr(_) => Err(self.bug(cx.file, at.span, "`self` stands alone")),
            Expr::List(list) => self.list(cx, list, ty, at),
            Expr::ListComp(comp) => self.list_comp(cx, comp, ty, meaning, at),
            Expr::Map(map) => match meaning {
                Meaning::Struct(s) => self.struct_value(cx, *s, map.entries().collect(), at),
                _ => self.map_literal(cx, map, ty, at),
            },
            Expr::MapComp(comp) => self.map_comp(cx, comp, ty, meaning, at),
            Expr::StructLit(lit) => match meaning {
                Meaning::Struct(s) => self.struct_value(cx, *s, lit.entries().collect(), at),
                _ => Err(self.bug(cx.file, at.span, "a struct literal has no struct")),
            },
            Expr::Paren(paren) => match paren.expr() {
                Some(inner) => self.expr(cx, inner),
                None => Err(self.bug(cx.file, at.span, "parentheses are empty")),
            },
            Expr::Call(call) => match meaning {
                Meaning::Call(fact) => self.call(cx, call, fact, at),
                _ => Err(self.bug(cx.file, at.span, "a call has no callee")),
            },
            Expr::Field(field) => self.field(cx, field, meaning, at),
            Expr::Index(index) => self.index(cx, index, at),
            Expr::Binary(binary) => self.binary(cx, binary, at),
            Expr::Unary(unary) => self.unary(cx, unary, at),
            Expr::If(branch) => self.if_expr(cx, branch, at),
            Expr::Match(m) => self.match_expr(cx, m, at),
            Expr::Let(l) => self.let_expr(cx, l, at),
            Expr::Lambda(lambda) => Ok(Val::Fn(Rc::new(FnVal::Lambda {
                unit: cx.unit,
                node: lambda.syntax().index(),
                env: cx.env.clone(),
            }))),
            Expr::Part(part) => self.part(cx, part),
        }
    }

    fn string(
        &mut self,
        cx: &Cx<'_>,
        string: ast::Str<'c>,
        meaning: &Meaning,
        at: SourceSpan,
    ) -> R<Val> {
        let text = self.string_text(cx, string)?;
        match meaning {
            Meaning::Symbol(s) => self.symbol(*s, &text, at),
            Meaning::RefLabel(t) => Ok(self.reference(*t, text, at)),
            _ => self.new_str(text, at),
        }
    }

    fn symbol_word(
        &mut self,
        cx: &Cx<'_>,
        symbol: ast::Symbol<'c>,
        meaning: &Meaning,
        at: SourceSpan,
    ) -> R<Val> {
        match meaning {
            Meaning::Symbol(s) => {
                let text = symbol.token().map_or("", |t| t.text());
                self.symbol(*s, text, at)
            }
            _ => Err(self.bug(cx.file, at.span, "a symbol word has no symbol type")),
        }
    }

    fn list(&mut self, cx: &Cx<'_>, list: ast::List<'c>, ty: TypeId, at: SourceSpan) -> R<Val> {
        let mut items = Vec::new();
        for item in list.items() {
            items.push(self.expr(cx, item)?);
        }
        self.sequence(items, ty, at)
    }

    /// Returns the elements as a set when `ty` is a set type, and as a
    /// list otherwise.
    fn sequence(&mut self, items: Vec<Val>, ty: TypeId, at: SourceSpan) -> R<Val> {
        if matches!(self.c.types.get(self.c.types.peel(ty)), Type::Set(_)) {
            self.new_set(items, at)
        } else {
            self.new_list(items, at)
        }
    }

    fn list_comp(
        &mut self,
        cx: &Cx<'_>,
        comp: ast::ListComp<'c>,
        ty: TypeId,
        meaning: &Meaning,
        at: SourceSpan,
    ) -> R<Val> {
        let Meaning::Binds(binds) = meaning else {
            return Err(self.bug(cx.file, at.span, "a comprehension has no bindings"));
        };
        let Some(source) = comp.source() else {
            return self.new_list(Vec::new(), at);
        };
        let source = self.expr(cx, source)?;
        let mut items = Vec::new();
        for element in elements(&source) {
            let inner = cx.with_env(bind_element(&cx.env, binds, element));
            if let Some(guard) = comp.guard()
                && !self.truth(&inner, guard)?
            {
                continue;
            }
            if let Some(value) = comp.value() {
                items.push(self.expr(&inner, value)?);
            }
        }
        self.sequence(items, ty, at)
    }

    fn if_expr(&mut self, cx: &Cx<'_>, branch: ast::IfExpr<'c>, at: SourceSpan) -> R<Val> {
        let Some(condition) = branch.condition() else {
            return Err(self.bug(cx.file, at.span, "an `if` has no condition"));
        };
        let taken = if self.truth(cx, condition)? {
            branch.then_branch()
        } else {
            branch.else_branch()
        };
        match taken {
            Some(e) => self.expr(cx, e),
            None => Err(self.bug(cx.file, at.span, "an `if` has no branch")),
        }
    }

    fn let_expr(&mut self, cx: &Cx<'_>, l: ast::LetExpr<'c>, at: SourceSpan) -> R<Val> {
        let mut env = cx.env.clone();
        for binding in l.bindings() {
            let Fact::Let(LetFact::Local(id)) = self.fact(cx.unit, binding.syntax().index()) else {
                continue;
            };
            let Some(value) = binding.value() else {
                continue;
            };
            let val = self.expr(&cx.with_env(env.clone()), value)?;
            if binding.ty().is_some() {
                let ty = self.local_type(cx.unit, *id);
                self.refine(ty, &val, cx.at(value.syntax().span()))?;
            }
            env = bind(&env, *id, Slot::Ready(val));
        }
        match l.body() {
            Some(body) => self.expr(&cx.with_env(env), body),
            None => Err(self.bug(cx.file, at.span, "a `let` has no body")),
        }
    }

    /// Evaluates a condition.
    pub(crate) fn truth(&mut self, cx: &Cx<'_>, e: Expr<'c>) -> R<bool> {
        match self.expr(cx, e)? {
            Val::Bool(b) => Ok(b),
            _ => Err(self.bug(cx.file, e.syntax().span(), "a condition is not a `Bool`")),
        }
    }

    pub(crate) fn local_type(&self, unit: UnitId, id: LocalId) -> TypeId {
        self.c
            .units
            .get(unit.index())
            .and_then(|u| u.locals.get(id.index()))
            .map_or(TypeId::ERROR, |l| l.ty)
    }

    fn literal(
        &mut self,
        cx: &Cx<'_>,
        literal: ast::Literal<'c>,
        ty: TypeId,
        meaning: &Meaning,
        at: SourceSpan,
    ) -> R<Val> {
        let text = literal.token().map_or("", |t| t.text());
        let bug = || self.bug(cx.file, at.span, "a literal is malformed");
        Ok(match literal.kind().ok_or_else(bug)? {
            LiteralKind::Int => {
                let value = parse_int(text).ok_or_else(bug)?;
                if self.c.types.peel(ty) == TypeId::FLOAT {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "an integer literal written as a float"
                    )]
                    let value = value as f64;
                    Val::Float(value)
                } else {
                    Val::Int(value)
                }
            }
            LiteralKind::Float => Val::Float(parse_float(text).ok_or_else(bug)?),
            LiteralKind::True => Val::Bool(true),
            LiteralKind::False => Val::Bool(false),
            LiteralKind::None => Val::None,
            LiteralKind::Path => match meaning {
                Meaning::Path { root, rel } => Val::Path(Rc::new(PathVal {
                    root: *root,
                    rel: rel
                        .as_ref()
                        .map_or_else(String::new, |r| r.as_str().to_owned()),
                })),
                _ => return Err(bug()),
            },
        })
    }

    /// Returns the value of a string, with its escapes decoded and its
    /// interpolations evaluated.
    pub(crate) fn string_text(&mut self, cx: &Cx<'_>, string: ast::Str<'c>) -> R<String> {
        let mut text = String::new();
        for part in string.parts() {
            match part {
                StrPart::Text(token) => text.push_str(token.text()),
                StrPart::Escape(token) => match ast::unescape(token.text()) {
                    Some(c) => text.push(c),
                    None => {
                        return Err(self.bug(cx.file, token.span(), "an escape is invalid"));
                    }
                },
                StrPart::Interp(interp) => {
                    let Some(e) = interp.expr() else { continue };
                    let value = self.expr(cx, e)?;
                    text.push_str(&value.text(&self.c.types));
                }
            }
            self.reserve_str(text.len(), cx.at(string.syntax().span()))?;
        }
        Ok(text)
    }

    /// Returns a value of a symbol type, with the text the host's parser
    /// makes of it.
    pub(crate) fn symbol(&mut self, ty: SymbolId, text: &str, at: SourceSpan) -> R<Val> {
        let Some(def) = self.c.symbols.get(ty.index()) else {
            return Err(self.bug(at.file, at.span, "a symbol type is unknown"));
        };
        match (def.parse)(text) {
            Ok(canonical) => Ok(Val::Symbol(Rc::new(SymbolVal {
                ty,
                text: canonical,
                file: at.file,
                span: at.span,
            }))),
            Err(reason) => Err(self.stop(
                at,
                Diagnostic::error(
                    "E0613",
                    at.span,
                    format!("`{text}` is not a valid `{}`: {reason}", def.name),
                ),
            )),
        }
    }

    pub(crate) fn reference(&self, target: StructId, label: String, at: SourceSpan) -> Val {
        Val::Ref(Rc::new(RefVal {
            target,
            label,
            file: at.file,
            span: at.span,
        }))
    }

    /// Evaluates an identifier, or a map comprehension key written as one.
    fn name(
        &mut self,
        cx: &Cx<'_>,
        token: SyntaxToken<'c>,
        meaning: &Meaning,
        at: SourceSpan,
    ) -> R<Val> {
        match meaning {
            Meaning::Local(id) => self.local(cx, *id, at),
            Meaning::Global(g) => self.global(*g, at, cx.frames),
            Meaning::Variant { sum, variant } => self.new_variant(*sum, *variant, None, at),
            Meaning::Symbol(s) => self.symbol(*s, token.text(), at),
            Meaning::RefLabel(t) => Ok(self.reference(*t, token.text().to_owned(), at)),
            _ => Err(self.bug(cx.file, at.span, "a name means nothing")),
        }
    }

    /// Returns the value of a local binding.
    pub(crate) fn local(&mut self, cx: &Cx<'_>, id: LocalId, at: SourceSpan) -> R<Val> {
        match lookup(&cx.env, id).cloned() {
            Some(Slot::Ready(v)) => Ok(v),
            Some(Slot::Late(late)) => {
                let e = self.expr_at(late.unit, late.node)?;
                let inner = Cx {
                    unit: late.unit,
                    file: self.unit_file(late.unit),
                    env: late.env.clone(),
                    frames: cx.frames,
                };
                let value = self.expr(&inner, e)?;
                if let Some(ty) = late.annotation {
                    self.refine(ty, &value, inner.at(e.syntax().span()))?;
                }
                Ok(value)
            }
            None => Err(self.bug(cx.file, at.span, "a local is not bound")),
        }
    }

    /// Reads `self.field`: the merged value of a field of an enclosing
    /// block, or of the struct a default is being evaluated for.
    pub(crate) fn self_read(&mut self, cx: &Cx<'_>, read: SelfRead, at: SourceSpan) -> R<Val> {
        let field = u32::try_from(read.field).unwrap_or(u32::MAX);
        match cx.frames.get(read.depth) {
            Some(Frame::Node(node)) => {
                let child = self.merge.child(*node, &Seg::Field(field));
                match child {
                    Some(child) => self.finalize(child),
                    None => Ok(Val::None),
                }
            }
            Some(Frame::Building(index)) => self.building_field(*index, read.field, at),
            None => Err(self.bug(cx.file, at.span, "`self` has no block here")),
        }
    }

    fn field(
        &mut self,
        cx: &Cx<'_>,
        field: ast::FieldExpr<'c>,
        meaning: &Meaning,
        at: SourceSpan,
    ) -> R<Val> {
        match meaning {
            Meaning::Field { index, optional } => {
                let Some(base) = field.base() else {
                    return Err(self.bug(cx.file, at.span, "a field access has no base"));
                };
                match self.expr(cx, base)? {
                    Val::None if *optional => Ok(Val::None),
                    Val::Struct(s) => s
                        .fields
                        .get(*index)
                        .cloned()
                        .ok_or_else(|| self.bug(cx.file, at.span, "a field is missing")),
                    _ => Err(self.bug(cx.file, at.span, "a field access has no struct")),
                }
            }
            Meaning::Global(g) => self.global(*g, at, cx.frames),
            Meaning::Variant { sum, variant } => self.new_variant(*sum, *variant, None, at),
            Meaning::SelfField(read) => self.self_read(cx, *read, at),
            _ => Err(self.bug(cx.file, at.span, "a field access means nothing")),
        }
    }

    fn index(&mut self, cx: &Cx<'_>, index: ast::IndexExpr<'c>, at: SourceSpan) -> R<Val> {
        let (Some(base), Some(key)) = (index.base(), index.index()) else {
            return Err(self.bug(cx.file, at.span, "an index is incomplete"));
        };
        let base = self.expr(cx, base)?;
        let key = self.expr(cx, key)?;
        match (&base, &key) {
            (Val::List(list), Val::Int(i)) => usize::try_from(*i)
                .ok()
                .and_then(|i| list.items.get(i))
                .cloned()
                .ok_or_else(|| {
                    self.fail(
                        at,
                        "E0604",
                        format!(
                            "index {i} is out of range for a list of {} elements",
                            list.items.len()
                        ),
                    )
                }),
            (Val::Map(map), key) => map_get(map, key).ok_or_else(|| {
                self.fail(
                    at,
                    "E0604",
                    format!("the map has no key `{}`", key.text(&self.c.types)),
                )
                .with_help("use `get(map, key)` for an `Option`")
            }),
            _ => Err(self.bug(cx.file, at.span, "an index has the wrong types")),
        }
    }

    fn map_literal(
        &mut self,
        cx: &Cx<'_>,
        map: ast::Map<'c>,
        ty: TypeId,
        at: SourceSpan,
    ) -> R<Val> {
        let key_sum = match self.c.types.get(self.c.types.peel(ty)) {
            Type::Map(k, _) => self.c.types.as_sum(*k),
            _ => None,
        };
        let mut entries: BTreeMap<MapKey, Val> = BTreeMap::new();
        for entry in map.entries() {
            let (Some(key), Some(value)) = (entry.key(), entry.value()) else {
                continue;
            };
            let text = self.key_text(cx, key)?;
            let key = self.map_key(key_sum, text, cx.at(key.span()))?;
            let value = self.expr(cx, value)?;
            self.step(at)?;
            match entries.entry(MapKey(key)) {
                Entry::Vacant(slot) => {
                    slot.insert(value);
                }
                Entry::Occupied(slot) => {
                    return Err(self.fail(
                        at,
                        "E0605",
                        format!(
                            "the key `{}` is given more than once",
                            slot.key().0.text(&self.c.types)
                        ),
                    ));
                }
            }
        }
        self.new_map(entries.into_iter().map(|(k, v)| (k.0, v)).collect(), at)
    }

    /// Returns the key of a map entry: the variant of that name for a map
    /// keyed by a sum type, or the string.
    fn map_key(&mut self, sum: Option<SumId>, text: String, at: SourceSpan) -> R<Val> {
        let Some(sum) = sum else {
            return self.new_str(text, at);
        };
        let def = self.c.types.sum(sum);
        match def.variant(&text) {
            Some((index, _)) => self.new_variant(sum, index, None, at),
            None => Err(self.fail(
                at,
                "E0604",
                format!("`{text}` is not a variant of `{}`", def.name),
            )),
        }
    }

    fn map_comp(
        &mut self,
        cx: &Cx<'_>,
        comp: ast::MapComp<'c>,
        ty: TypeId,
        meaning: &Meaning,
        at: SourceSpan,
    ) -> R<Val> {
        let (binds, key_meaning) = match meaning {
            Meaning::Binds(binds) => (binds, None),
            Meaning::MapCompKey { binds, key } => (binds, Some(key.as_ref())),
            _ => return Err(self.bug(cx.file, at.span, "a comprehension has no bindings")),
        };
        let key_sum = match self.c.types.get(self.c.types.peel(ty)) {
            Type::Map(k, _) => self.c.types.as_sum(*k),
            _ => None,
        };
        let Some(source) = comp.source() else {
            return self.new_map(Vec::new(), at);
        };
        let source = self.expr(cx, source)?;
        let mut entries: BTreeMap<MapKey, Val> = BTreeMap::new();
        for element in elements(&source) {
            let inner = cx.with_env(bind_element(&cx.env, binds, element));
            if let Some(guard) = comp.guard()
                && !self.truth(&inner, guard)?
            {
                continue;
            }
            let (Some(key), Some(value)) = (comp.key(), comp.value()) else {
                continue;
            };
            let key_at = inner.at(key.span());
            let key = match (key, key_meaning) {
                (Key::Name(token), Some(meaning)) => self.name(&inner, token, meaning, key_at)?,
                (key, _) => {
                    let text = self.key_text(&inner, key)?;
                    self.map_key(key_sum, text, key_at)?
                }
            };
            let value = self.expr(&inner, value)?;
            self.step(key_at)?;
            match entries.entry(MapKey(key)) {
                Entry::Vacant(slot) => {
                    slot.insert(value);
                }
                Entry::Occupied(slot) => {
                    return Err(self.fail(
                        key_at,
                        "E0605",
                        format!(
                            "the comprehension produces the key `{}` more than once",
                            slot.key().0.text(&self.c.types)
                        ),
                    ));
                }
            }
        }
        self.new_map(entries.into_iter().map(|(k, v)| (k.0, v)).collect(), at)
    }

    fn binary(&mut self, cx: &Cx<'_>, binary: ast::BinaryExpr<'c>, at: SourceSpan) -> R<Val> {
        let (Some(op), Some(lhs), Some(rhs)) = (binary.op(), binary.lhs(), binary.rhs()) else {
            return Err(self.bug(cx.file, at.span, "an operator is incomplete"));
        };
        let op_at = binary.op_token().map_or(at, |t| cx.at(t.span()));
        match op {
            BinaryOp::And => {
                return Ok(Val::Bool(self.truth(cx, lhs)? && self.truth(cx, rhs)?));
            }
            BinaryOp::Or => {
                return Ok(Val::Bool(self.truth(cx, lhs)? || self.truth(cx, rhs)?));
            }
            BinaryOp::Coalesce => {
                let left = self.expr(cx, lhs)?;
                return if left.is_none() {
                    self.expr(cx, rhs)
                } else {
                    Ok(left)
                };
            }
            _ => {}
        }
        let a = self.expr(cx, lhs)?;
        let b = self.expr(cx, rhs)?;
        match op {
            BinaryOp::Eq => Ok(Val::Bool(a.equals(&b))),
            BinaryOp::NotEq => Ok(Val::Bool(!a.equals(&b))),
            BinaryOp::Lt => Ok(Val::Bool(a.order(&b).is_lt())),
            BinaryOp::LtEq => Ok(Val::Bool(a.order(&b).is_le())),
            BinaryOp::Gt => Ok(Val::Bool(a.order(&b).is_gt())),
            BinaryOp::GtEq => Ok(Val::Bool(a.order(&b).is_ge())),
            BinaryOp::Concat => match (&a, &b) {
                (Val::Str(x), Val::Str(y)) => {
                    self.reserve_str(x.len().saturating_add(y.len()), at)?;
                    self.new_str(format!("{x}{y}"), at)
                }
                (Val::List(x), Val::List(y)) => {
                    let len = x.items.len().saturating_add(y.items.len());
                    self.reserve(u64::try_from(len).unwrap_or(u64::MAX), at)?;
                    let mut items = Vec::with_capacity(len);
                    items.extend(x.items.iter().cloned());
                    items.extend(y.items.iter().cloned());
                    self.new_list(items, at)
                }
                _ => Err(self.bug(cx.file, op_at.span, "`++` has the wrong operands")),
            },
            _ => self.arithmetic(op, &a, &b, op_at),
        }
    }

    fn arithmetic(&mut self, op: BinaryOp, a: &Val, b: &Val, at: SourceSpan) -> R<Val> {
        match (a, b) {
            (Val::Int(x), Val::Int(y)) => {
                let (x, y) = (*x, *y);
                if matches!(op, BinaryOp::Div | BinaryOp::Rem) && y == 0 {
                    return Err(self.fail(at, "E0603", "division by zero"));
                }
                let result = match op {
                    BinaryOp::Add => x.checked_add(y),
                    BinaryOp::Sub => x.checked_sub(y),
                    BinaryOp::Mul => x.checked_mul(y),
                    BinaryOp::Div => x.checked_div(y),
                    _ => Some(x.wrapping_rem(y)),
                };
                result.map(Val::Int).ok_or_else(|| {
                    self.fail(
                        at,
                        "E0601",
                        format!("integer overflow in `{x} {} {y}`", symbol(op)),
                    )
                })
            }
            (Val::Float(x), Val::Float(y)) => {
                let (x, y) = (*x, *y);
                if op == BinaryOp::Div && y == 0.0 {
                    return Err(self.fail(at, "E0603", "division by zero"));
                }
                let result = match op {
                    BinaryOp::Add => x + y,
                    BinaryOp::Sub => x - y,
                    BinaryOp::Mul => x * y,
                    _ => x / y,
                };
                self.finite(result, at)
            }
            _ => Err(self.bug(at.file, at.span, "arithmetic has the wrong operands")),
        }
    }

    /// Returns a float, or `E0602` when it is not finite.
    pub(crate) fn finite(&self, f: f64, at: SourceSpan) -> R<Val> {
        if f.is_finite() {
            Ok(Val::Float(f))
        } else {
            Err(self.fail(at, "E0602", "the result is not a finite number"))
        }
    }

    fn unary(&mut self, cx: &Cx<'_>, unary: ast::UnaryExpr<'c>, at: SourceSpan) -> R<Val> {
        let (Some(op), Some(operand)) = (unary.op(), unary.operand()) else {
            return Err(self.bug(cx.file, at.span, "an operator is incomplete"));
        };
        let value = self.expr(cx, operand)?;
        match (op, value) {
            (UnaryOp::Not, Val::Bool(b)) => Ok(Val::Bool(!b)),
            (UnaryOp::Neg, Val::Int(i)) => i
                .checked_neg()
                .map(Val::Int)
                .ok_or_else(|| self.fail(at, "E0601", format!("integer overflow in `-({i})`"))),
            (UnaryOp::Neg, Val::Float(f)) => Ok(Val::Float(-f)),
            _ => Err(self.bug(cx.file, at.span, "an operator has the wrong operand")),
        }
    }
}

/// Binds the names of a loop or comprehension to one element.
pub(crate) fn bind_element(
    env: &Env,
    binds: &[LocalId],
    (key, value): (Option<Val>, Option<Val>),
) -> Env {
    let mut env = env.clone();
    let mut ids = binds.iter();
    if let (Some(&id), Some(key)) = (ids.next(), key) {
        env = bind(&env, id, Slot::Ready(key));
    }
    if let (Some(&id), Some(value)) = (ids.next(), value) {
        env = bind(&env, id, Slot::Ready(value));
    }
    env
}

/// Returns the value of a map key.
pub(crate) fn map_get(map: &super::val::MapVal, key: &Val) -> Option<Val> {
    map.entries
        .binary_search_by(|(k, _)| k.order(key))
        .ok()
        .and_then(|i| map.entries.get(i))
        .map(|(_, v)| v.clone())
}

fn symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        _ => "%",
    }
}

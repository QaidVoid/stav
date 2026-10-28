//! `match` expressions: typing patterns, binding their names, and checking
//! that the arms are exhaustive and reachable.

use std::collections::BTreeSet;

use super::decls::{TypeItem, ValueItem};
use super::model::{Fact, LocalId, Meaning, PatternFact};
use super::suggest::did_you_mean;
use super::types::{SumId, Type, TypeId};
use super::{Binding, Checker, Cx};
use crate::ast::{self, Expr, LiteralKind, UnaryOp};
use crate::diagnostic::Diagnostic;
use crate::lexer::parse_int;
use crate::span::Span;
use crate::syntax::SyntaxToken;

/// The most work the exhaustiveness check does for one `match`, counted in
/// steps of its search. Beyond it, the match is reported as not provably
/// exhaustive, so that checking always finishes quickly.
const BUDGET: usize = 200_000;

/// A constructor of a value, as far as patterns can tell values apart.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Ctor {
    True,
    False,
    None,
    Some,
    Variant(usize),
    Int(i64),
    Str(String),
}

/// A pattern reduced to what exhaustiveness needs.
#[derive(Debug, Clone)]
enum Pat {
    Wild,
    Ctor(Ctor, Vec<Pat>),
    Or(Vec<Pat>),
}

/// A name a pattern binds, with the node index of the name pattern that
/// binds it, or `None` for a field pattern without a nested pattern.
struct Bind {
    name: String,
    ty: TypeId,
    node: Option<usize>,
}

/// A variant pattern whose fact waits for the locals of its bindings.
struct PendingVariant {
    node: usize,
    sum: SumId,
    variant: usize,
    fields: Vec<(usize, Option<String>)>,
}

#[derive(Default)]
struct Arm {
    binds: Vec<Bind>,
    variants: Vec<PendingVariant>,
}

impl<'p> Checker<'p> {
    pub(super) fn match_expr(
        &mut self,
        cx: &mut Cx,
        m: ast::MatchExpr<'p>,
        expected: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let mut scrutinee = match m.scrutinee() {
            Some(e) => self.expr(cx, e, None),
            None => TypeId::ERROR,
        };
        if self.types.peel(scrutinee) == TypeId::DATA {
            let span = m
                .scrutinee()
                .map_or(m.syntax().span(), |e| e.syntax().span());
            self.error(
                cx.file,
                Diagnostic::error("E0401", span, "a `Data` value cannot be matched")
                    .with_help("match on a typed value instead"),
            );
            scrutinee = TypeId::ERROR;
        }
        let mut first: Option<TypeId> = None;
        let mut rows: Vec<(Pat, bool, Span)> = Vec::new();
        for arm in m.arms() {
            let mark = cx.scope.len();
            let mut state = Arm::default();
            let pat = match arm.pattern() {
                Some(p) => self.pattern(cx, p, scrutinee, &mut state),
                None => Pat::Wild,
            };
            self.bind_arm(cx, state);
            if let Some(guard) = arm.guard() {
                self.expr(cx, guard, Some(TypeId::BOOL));
            }
            if let Some(value) = arm.value() {
                let target = expected.or(first);
                let ty = self.expr(cx, value, target);
                first.get_or_insert(ty);
            }
            cx.scope.truncate(mark);
            let span = arm
                .pattern()
                .map_or(arm.syntax().span(), |p| p.syntax().span());
            rows.push((pat, arm.guard().is_some(), span));
        }
        let span = m.syntax().span();
        let span = Span {
            start: span.start,
            end: m.scrutinee().map_or(span.end, |s| s.syntax().span().end),
        };
        if !self.types.has_error(scrutinee) {
            self.exhaustive(cx, scrutinee, &rows, span);
        }
        (expected.or(first).unwrap_or(TypeId::ERROR), Meaning::Plain)
    }

    /// Creates the locals of an arm's bindings and records the pattern facts
    /// that refer to them.
    fn bind_arm(&mut self, cx: &mut Cx, arm: Arm) {
        let mut locals: Vec<(String, LocalId)> = Vec::new();
        for bind in &arm.binds {
            let id = match locals.iter().find(|(n, _)| *n == bind.name) {
                Some((_, id)) => *id,
                None => {
                    let id = self.local(cx, &bind.name, bind.ty, Vec::new());
                    cx.scope.push((bind.name.clone(), Binding::Local(id)));
                    locals.push((bind.name.clone(), id));
                    id
                }
            };
            if let Some(node) = bind.node {
                self.set_fact(cx, node, Fact::Pattern(PatternFact::Binder(id)));
            }
        }
        for variant in arm.variants {
            let fields = variant
                .fields
                .into_iter()
                .map(|(field, name)| {
                    let local =
                        name.and_then(|n| locals.iter().find(|(l, _)| *l == n).map(|(_, id)| *id));
                    (field, local)
                })
                .collect();
            self.set_fact(
                cx,
                variant.node,
                Fact::Pattern(PatternFact::Variant {
                    sum: variant.sum,
                    variant: variant.variant,
                    fields,
                }),
            );
        }
    }

    fn set_fact(&mut self, cx: &Cx, node: usize, fact: Fact) {
        if let Some(slot) = self
            .units
            .get_mut(cx.unit.index())
            .and_then(|u| u.facts.get_mut(node))
        {
            *slot = fact;
        }
    }

    /// Types a pattern against the scrutinee type `ty`, collects its
    /// bindings and returns its reduced form.
    fn pattern(&mut self, cx: &mut Cx, p: ast::Pattern<'p>, ty: TypeId, arm: &mut Arm) -> Pat {
        let option = self.types.unwrap_option(ty);
        let inner = option.unwrap_or_else(|| self.types.peel(ty));
        let some = |pat: Pat| match option {
            Some(_) => Pat::Ctor(Ctor::Some, vec![pat]),
            None => pat,
        };
        let node = p.syntax().index();
        match p {
            ast::Pattern::Wildcard(_) => {
                self.set_fact(cx, node, Fact::Pattern(PatternFact::Wildcard));
                Pat::Wild
            }
            ast::Pattern::Literal(literal) => {
                self.set_fact(cx, node, Fact::Pattern(PatternFact::Literal));
                let Some(expr) = literal.expr() else {
                    return Pat::Wild;
                };
                let span = literal.syntax().span();
                let (ctor, lit_ty) = match literal_ctor(expr) {
                    Some(found) => found,
                    None => {
                        self.error(
                            cx.file,
                            Diagnostic::error("E0401", span, "a string pattern cannot interpolate"),
                        );
                        return Pat::Wild;
                    }
                };
                if ctor == Ctor::None {
                    if option.is_none() && !self.types.has_error(ty) {
                        let rendered = self.types.render_plain(ty);
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0401",
                                span,
                                format!("`none` cannot match a value of type `{rendered}`"),
                            ),
                        );
                    }
                    return Pat::Ctor(Ctor::None, Vec::new());
                }
                if !self.types.fits(lit_ty, inner) {
                    self.mismatch(cx.file, span, inner, lit_ty);
                    return Pat::Wild;
                }
                some(Pat::Ctor(ctor, Vec::new()))
            }
            ast::Pattern::Name(name) => {
                let segments: Vec<SyntaxToken<'p>> = name.segments().collect();
                let span = name.syntax().span();
                if let [token] = segments.as_slice() {
                    let found = self.types.as_sum(inner).and_then(|s| {
                        self.types
                            .sum(s)
                            .variant(token.text())
                            .map(|(i, v)| (s, i, v.fields.is_some()))
                    });
                    let Some((s, index, payload)) = found else {
                        arm.binds.push(Bind {
                            name: token.text().to_owned(),
                            ty: inner,
                            node: Some(node),
                        });
                        return some(Pat::Wild);
                    };
                    return some(self.variant_name_pattern(cx, s, index, payload, node, span, arm));
                }
                let Some((s, index)) = self.qualified_variant(cx, &segments, inner, span) else {
                    return Pat::Wild;
                };
                let payload = self.types.sum(s).variants[index].fields.is_some();
                some(self.variant_name_pattern(cx, s, index, payload, node, span, arm))
            }
            ast::Pattern::Variant(variant) => {
                let segments: Vec<SyntaxToken<'p>> = variant.segments().collect();
                let span = variant.syntax().span();
                let found = match segments.as_slice() {
                    [token] => match self.types.as_sum(inner) {
                        Some(s) => match self.types.sum(s).variant(token.text()) {
                            Some((i, _)) => Some((s, i)),
                            None => {
                                let sum = self.types.sum(s);
                                let help = did_you_mean(
                                    token.text(),
                                    sum.variants.iter().map(|v| v.name.as_str()),
                                );
                                let name = sum.name.clone();
                                self.error(
                                    cx.file,
                                    Diagnostic::error(
                                        "E0302",
                                        token.span(),
                                        format!("`{name}` has no variant `{}`", token.text()),
                                    )
                                    .with_optional_help(help),
                                );
                                None
                            }
                        },
                        None => {
                            if !self.types.has_error(inner) {
                                let rendered = self.types.render_plain(inner);
                                self.error(
                                    cx.file,
                                    Diagnostic::error(
                                        "E0401",
                                        span,
                                        format!("a variant pattern cannot match a value of type `{rendered}`"),
                                    ),
                                );
                            }
                            None
                        }
                    },
                    _ => self.qualified_variant(cx, &segments, inner, span),
                };
                let Some((s, index)) = found else {
                    return Pat::Wild;
                };
                some(self.variant_pattern(cx, variant, s, index, arm))
            }
            ast::Pattern::Or(or) => {
                self.set_fact(cx, node, Fact::Pattern(PatternFact::Or));
                let mut alternatives = Vec::new();
                let mut first: Option<Vec<(String, TypeId)>> = None;
                for alternative in or.alternatives() {
                    let mut inner_arm = Arm::default();
                    alternatives.push(self.pattern(cx, alternative, ty, &mut inner_arm));
                    let mut names: Vec<(String, TypeId)> = inner_arm
                        .binds
                        .iter()
                        .map(|b| (b.name.clone(), b.ty))
                        .collect();
                    names.sort_by(|a, b| a.0.cmp(&b.0));
                    match &first {
                        None => first = Some(names),
                        Some(expected) => {
                            let same = expected.len() == names.len()
                                && expected
                                    .iter()
                                    .zip(&names)
                                    .all(|(a, b)| a.0 == b.0 && self.types.fits(a.1, b.1));
                            if !same {
                                self.error(
                                    cx.file,
                                    Diagnostic::error(
                                        "E0433",
                                        alternative.syntax().span(),
                                        "the alternatives of a pattern must bind the same names with the same types",
                                    ),
                                );
                            }
                        }
                    }
                    arm.binds.extend(inner_arm.binds);
                    arm.variants.extend(inner_arm.variants);
                }
                Pat::Or(alternatives)
            }
        }
    }

    #[allow(clippy::too_many_arguments, reason = "the parts of one pattern")]
    fn variant_name_pattern(
        &mut self,
        cx: &mut Cx,
        s: SumId,
        index: usize,
        payload: bool,
        node: usize,
        span: Span,
        arm: &mut Arm,
    ) -> Pat {
        let fields = self.types.sum(s).variants[index]
            .fields
            .as_ref()
            .map_or(0, Vec::len);
        if payload {
            let name = self.types.sum(s).variants[index].name.clone();
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0432",
                    span,
                    format!("the variant `{name}` has payload fields"),
                )
                .with_help(format!("write `{name}(..)` to ignore them")),
            );
        }
        arm.variants.push(PendingVariant {
            node,
            sum: s,
            variant: index,
            fields: Vec::new(),
        });
        Pat::Ctor(Ctor::Variant(index), vec![Pat::Wild; fields])
    }

    /// Resolves a qualified variant such as `Level.info` or
    /// `alias.Level.info` in a pattern whose scrutinee has type `inner`.
    fn qualified_variant(
        &mut self,
        cx: &Cx,
        segments: &[SyntaxToken<'p>],
        inner: TypeId,
        span: Span,
    ) -> Option<(SumId, usize)> {
        let (type_segments, variant) = segments.split_at(segments.len().saturating_sub(1));
        let variant = variant.first()?;
        let item = match type_segments {
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
        let Some(TypeItem::Sum(s)) = item else {
            let path: Vec<&str> = type_segments.iter().map(|t| t.text()).collect();
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0302",
                    span,
                    format!("unknown sum type `{}`", path.join(".")),
                ),
            );
            return None;
        };
        if self.types.as_sum(inner) != Some(s) && !self.types.has_error(inner) {
            let expected = self.types.render_plain(inner);
            let found = self.types.sum(s).name.clone();
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0401",
                    span,
                    format!("expected a pattern of type `{expected}`, found `{found}`"),
                ),
            );
            return None;
        }
        let sum = self.types.sum(s);
        match sum.variant(variant.text()) {
            Some((index, _)) => Some((s, index)),
            None => {
                let help =
                    did_you_mean(variant.text(), sum.variants.iter().map(|v| v.name.as_str()));
                let name = sum.name.clone();
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0302",
                        variant.span(),
                        format!("`{name}` has no variant `{}`", variant.text()),
                    )
                    .with_optional_help(help),
                );
                None
            }
        }
    }

    fn variant_pattern(
        &mut self,
        cx: &mut Cx,
        pattern: ast::VariantPat<'p>,
        s: SumId,
        index: usize,
        arm: &mut Arm,
    ) -> Pat {
        let def = self.types.sum(s).variants[index].clone();
        let span = pattern.syntax().span();
        let Some(params) = def.fields else {
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0432",
                    span,
                    format!("the variant `{}` has no payload fields", def.name),
                )
                .with_help(format!("write `{}` without parentheses", def.name)),
            );
            return Pat::Ctor(Ctor::Variant(index), Vec::new());
        };
        let mut sub = vec![Pat::Wild; params.len()];
        let mut mentioned: BTreeSet<usize> = BTreeSet::new();
        let mut fields = Vec::new();
        for field in pattern.fields() {
            let Some(name) = field.name() else { continue };
            let Some(field_index) = params.iter().position(|p| p.name == name.text()) else {
                let help = did_you_mean(name.text(), params.iter().map(|p| p.name.as_str()));
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0302",
                        name.span(),
                        format!("the variant `{}` has no field `{}`", def.name, name.text()),
                    )
                    .with_optional_help(help),
                );
                continue;
            };
            if mentioned.contains(&field_index) {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0306",
                        name.span(),
                        format!("the field `{}` is matched more than once", name.text()),
                    ),
                );
                continue;
            }
            mentioned.insert(field_index);
            let field_ty = params[field_index].ty;
            match field.pattern() {
                Some(nested) => {
                    sub[field_index] = self.pattern(cx, nested, field_ty, arm);
                    fields.push((field_index, None));
                }
                None => {
                    arm.binds.push(Bind {
                        name: name.text().to_owned(),
                        ty: field_ty,
                        node: None,
                    });
                    fields.push((field_index, Some(name.text().to_owned())));
                }
            }
        }
        if !pattern.has_rest() {
            let missing: Vec<String> = params
                .iter()
                .enumerate()
                .filter(|(i, _)| !mentioned.contains(i))
                .map(|(_, p)| format!("`{}`", p.name))
                .collect();
            if !missing.is_empty() {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0432",
                        span,
                        format!(
                            "the pattern does not mention the fields {}",
                            missing.join(", ")
                        ),
                    )
                    .with_help("add `..` to ignore the other fields"),
                );
            }
        }
        arm.variants.push(PendingVariant {
            node: pattern.syntax().index(),
            sum: s,
            variant: index,
            fields,
        });
        Pat::Ctor(Ctor::Variant(index), sub)
    }

    fn exhaustive(&mut self, cx: &Cx, ty: TypeId, rows: &[(Pat, bool, Span)], span: Span) {
        let mut search = Search {
            checker: self,
            budget: BUDGET,
        };
        let mut matrix: Vec<Vec<Pat>> = Vec::new();
        let mut unreachable = Vec::new();
        for (pat, guarded, arm_span) in rows {
            let row = vec![pat.clone()];
            match search.useful(&matrix, &row, &[ty], 0) {
                Some(false) => unreachable.push(*arm_span),
                Some(true) => {}
                None => break,
            }
            if !guarded {
                matrix.push(row);
            }
        }
        let mut missing = Vec::new();
        let complete = search.missing(&matrix, ty, &mut missing);
        let complete = match complete {
            Some(complete) => complete,
            None => {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0434",
                        span,
                        "cannot prove that this `match` is exhaustive",
                    )
                    .with_help("add a `_ => ...` arm"),
                );
                return;
            }
        };
        if !complete {
            let ty_name = self.types.render_plain(ty);
            let mut diagnostic = Diagnostic::error(
                "E0434",
                span,
                format!("non-exhaustive `match` on `{ty_name}`"),
            )
            .with_help("add the missing arms or a `_ => ...` arm");
            if !missing.is_empty() {
                diagnostic = diagnostic.with_note(format!("missing: {}", missing.join(", ")));
            }
            self.error(cx.file, diagnostic);
            return;
        }
        for span in unreachable {
            self.report(
                Some(cx.file),
                Diagnostic::warning("W0434", span, "this arm can never match")
                    .with_help("remove it, or move it before the arms that cover it"),
            );
        }
    }
}

/// Returns the constructor and type of a literal pattern, or `None` for a
/// string with interpolation.
fn literal_ctor(expr: Expr<'_>) -> Option<(Ctor, TypeId)> {
    match expr {
        Expr::Literal(literal) => {
            let text = literal.token().map_or("", |t| t.text());
            Some(match literal.kind()? {
                LiteralKind::True => (Ctor::True, TypeId::BOOL),
                LiteralKind::False => (Ctor::False, TypeId::BOOL),
                LiteralKind::None => (Ctor::None, TypeId::ERROR),
                LiteralKind::Int => (Ctor::Int(parse_int(text).unwrap_or(0)), TypeId::INT),
                LiteralKind::Float | LiteralKind::Path => return None,
            })
        }
        Expr::Unary(unary) if unary.op() == Some(UnaryOp::Neg) => {
            let Some(Expr::Literal(literal)) = unary.operand() else {
                return None;
            };
            let text = literal.token().map_or("", |t| t.text());
            let value = parse_int(text).and_then(i64::checked_neg).unwrap_or(0);
            Some((Ctor::Int(value), TypeId::INT))
        }
        Expr::Str(string) => Some((Ctor::Str(string.value()?), TypeId::STR)),
        _ => None,
    }
}

/// The search of the exhaustiveness check, after the classic usefulness
/// algorithm: a pattern row is useful when some value matches it and no
/// earlier row.
struct Search<'a, 'p> {
    checker: &'a Checker<'p>,
    budget: usize,
}

/// The constructors of a type, when there are finitely many.
type Ctors = Vec<(Ctor, Vec<TypeId>)>;

impl Search<'_, '_> {
    fn ctors(&self, ty: TypeId) -> Option<Ctors> {
        let types = &self.checker.types;
        match types.get(types.peel(ty)) {
            Type::Bool => Some(vec![(Ctor::True, Vec::new()), (Ctor::False, Vec::new())]),
            Type::Option(inner) => Some(vec![(Ctor::None, Vec::new()), (Ctor::Some, vec![*inner])]),
            Type::Sum(s) => Some(
                types
                    .sum(*s)
                    .variants
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let fields = v.fields.iter().flatten().map(|f| f.ty).collect();
                        (Ctor::Variant(i), fields)
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    fn step(&mut self, depth: usize) -> Option<()> {
        if self.budget == 0 || depth > 512 {
            self.budget = 0;
            return None;
        }
        self.budget -= 1;
        Some(())
    }

    /// Returns whether `row` matches a value that no row of `matrix` does,
    /// or `None` when the search ran out of budget.
    fn useful(
        &mut self,
        matrix: &[Vec<Pat>],
        row: &[Pat],
        tys: &[TypeId],
        depth: usize,
    ) -> Option<bool> {
        self.step(depth)?;
        let Some((head, rest)) = row.split_first() else {
            return Some(matrix.is_empty());
        };
        let matrix = expand_or(matrix);
        let (ty, rest_tys) = match tys.split_first() {
            Some((ty, rest)) => (*ty, rest),
            None => (TypeId::ERROR, &[][..]),
        };
        match head {
            Pat::Or(alternatives) => {
                for alternative in alternatives {
                    let mut row = vec![alternative.clone()];
                    row.extend_from_slice(rest);
                    if self.useful(&matrix, &row, tys, depth + 1)? {
                        return Some(true);
                    }
                }
                Some(false)
            }
            Pat::Ctor(ctor, args) => {
                let arg_tys = self.arg_types(ty, ctor, args.len());
                let mut row = args.clone();
                row.extend_from_slice(rest);
                let mut tys = arg_tys;
                tys.extend_from_slice(rest_tys);
                let specialized = specialize(&matrix, ctor, args.len());
                self.useful(&specialized, &row, &tys, depth + 1)
            }
            Pat::Wild => {
                let used: BTreeSet<Ctor> = matrix
                    .iter()
                    .filter_map(|r| match r.first() {
                        Some(Pat::Ctor(c, _)) => Some(c.clone()),
                        _ => None,
                    })
                    .collect();
                if let Some(ctors) = self.ctors(ty)
                    && ctors.iter().all(|(c, _)| used.contains(c))
                {
                    for (ctor, fields) in ctors {
                        let mut row = vec![Pat::Wild; fields.len()];
                        row.extend_from_slice(rest);
                        let mut tys = fields.clone();
                        tys.extend_from_slice(rest_tys);
                        let specialized = specialize(&matrix, &ctor, fields.len());
                        if self.useful(&specialized, &row, &tys, depth + 1)? {
                            return Some(true);
                        }
                    }
                    return Some(false);
                }
                let defaults: Vec<Vec<Pat>> = matrix
                    .iter()
                    .filter(|r| matches!(r.first(), Some(Pat::Wild)))
                    .map(|r| r[1..].to_vec())
                    .collect();
                self.useful(&defaults, rest, rest_tys, depth + 1)
            }
        }
    }

    fn arg_types(&self, ty: TypeId, ctor: &Ctor, count: usize) -> Vec<TypeId> {
        let found = self
            .ctors(ty)
            .and_then(|ctors| ctors.into_iter().find(|(c, _)| c == ctor).map(|(_, f)| f));
        match found {
            Some(fields) if fields.len() == count => fields,
            _ => vec![TypeId::ERROR; count],
        }
    }

    /// Lists the constructors of `ty` that no row covers, and returns
    /// whether the rows are exhaustive, or `None` when the search ran out of
    /// budget.
    fn missing(&mut self, matrix: &[Vec<Pat>], ty: TypeId, out: &mut Vec<String>) -> Option<bool> {
        if !self.useful(matrix, &[Pat::Wild], &[ty], 0)? {
            return Some(true);
        }
        let Some(ctors) = self.ctors(ty) else {
            out.push("`_`".to_owned());
            return Some(false);
        };
        for (ctor, fields) in ctors {
            let row = [Pat::Ctor(ctor.clone(), vec![Pat::Wild; fields.len()])];
            if !self.useful(matrix, &row, &[ty], 0)? {
                continue;
            }
            if ctor == Ctor::Some {
                let inner = fields.first().copied().unwrap_or(TypeId::ERROR);
                let specialized = specialize(&expand_or(matrix), &Ctor::Some, 1);
                let mut nested = Vec::new();
                self.missing(&specialized, inner, &mut nested)?;
                out.extend(nested);
                continue;
            }
            out.push(self.ctor_name(ty, &ctor));
        }
        Some(false)
    }

    fn ctor_name(&self, ty: TypeId, ctor: &Ctor) -> String {
        let types = &self.checker.types;
        match ctor {
            Ctor::True => "`true`".to_owned(),
            Ctor::False => "`false`".to_owned(),
            Ctor::None => "`none`".to_owned(),
            Ctor::Some => "`_`".to_owned(),
            Ctor::Variant(i) => match types.as_sum(ty).map(|s| &types.sum(s).variants[*i]) {
                Some(v) if v.fields.is_some() => format!("`{}(..)`", v.name),
                Some(v) => format!("`{}`", v.name),
                None => "`_`".to_owned(),
            },
            Ctor::Int(i) => format!("`{i}`"),
            Ctor::Str(s) => format!("`{s:?}`"),
        }
    }
}

/// Replaces rows whose first pattern is an alternative by one row per
/// alternative.
fn expand_or(matrix: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    let mut out = Vec::new();
    let mut stack: Vec<Vec<Pat>> = matrix.iter().rev().cloned().collect();
    while let Some(row) = stack.pop() {
        match row.first() {
            Some(Pat::Or(alternatives)) => {
                for alternative in alternatives.iter().rev() {
                    let mut expanded = vec![alternative.clone()];
                    expanded.extend_from_slice(&row[1..]);
                    stack.push(expanded);
                }
            }
            _ => out.push(row),
        }
    }
    out
}

/// Keeps the rows that match the constructor `ctor`, with its arguments in
/// place of the first column.
fn specialize(matrix: &[Vec<Pat>], ctor: &Ctor, arity: usize) -> Vec<Vec<Pat>> {
    matrix
        .iter()
        .filter_map(|row| {
            let (head, rest) = row.split_first()?;
            let mut out = match head {
                Pat::Ctor(c, args) if c == ctor => args.clone(),
                Pat::Ctor(..) => return None,
                Pat::Wild | Pat::Or(_) => vec![Pat::Wild; arity],
            };
            out.extend_from_slice(rest);
            Some(out)
        })
        .collect()
}

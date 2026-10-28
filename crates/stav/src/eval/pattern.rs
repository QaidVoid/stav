//! Evaluating `match` expressions.

use super::env::{Env, Slot, bind};
use super::expr::Cx;
use super::val::Val;
use super::{Eval, R};
use crate::ast::{self, Expr, LiteralKind, Pattern, UnaryOp};
use crate::check::model::{Fact, PatternFact, UnitId};
use crate::lexer::parse_int;
use crate::span::SourceSpan;

impl<'c> Eval<'c> {
    pub(crate) fn match_expr(
        &mut self,
        cx: &Cx<'_>,
        m: ast::MatchExpr<'c>,
        at: SourceSpan,
    ) -> R<Val> {
        let Some(scrutinee) = m.scrutinee() else {
            return Err(self.bug(cx.file, at.span, "a `match` has no scrutinee"));
        };
        let value = self.expr(cx, scrutinee)?;
        for arm in m.arms() {
            let Some(pattern) = arm.pattern() else {
                continue;
            };
            let mut env = cx.env.clone();
            if !self.matches(cx.unit, pattern, &value, &mut env) {
                continue;
            }
            let inner = cx.with_env(env);
            if let Some(guard) = arm.guard()
                && !self.truth(&inner, guard)?
            {
                continue;
            }
            return match arm.value() {
                Some(e) => self.expr(&inner, e),
                None => Err(self.bug(cx.file, at.span, "an arm has no value")),
            };
        }
        Err(self.bug(cx.file, at.span, "no arm of an exhaustive `match` matched"))
    }

    /// Returns whether `value` matches `pattern`, binding its names in
    /// `env` when it does. A name that binds a value of an `Option` type
    /// matches only a present value.
    fn matches(&self, unit: UnitId, pattern: Pattern<'c>, value: &Val, env: &mut Env) -> bool {
        let fact = self.fact(unit, pattern.syntax().index());
        match pattern {
            Pattern::Wildcard(_) => true,
            Pattern::Literal(literal) => literal.expr().is_some_and(|e| literal_matches(e, value)),
            Pattern::Or(or) => {
                for alternative in or.alternatives() {
                    let mut inner = env.clone();
                    if self.matches(unit, alternative, value, &mut inner) {
                        *env = inner;
                        return true;
                    }
                }
                false
            }
            Pattern::Name(_) => match fact {
                Fact::Pattern(PatternFact::Binder(id)) => {
                    if value.is_none() {
                        return false;
                    }
                    *env = bind(env, *id, Slot::Ready(value.clone()));
                    true
                }
                Fact::Pattern(PatternFact::Variant { variant, .. }) => {
                    variant_fields(value, *variant).is_some()
                }
                _ => false,
            },
            Pattern::Variant(pattern) => {
                let Fact::Pattern(PatternFact::Variant {
                    variant, fields, ..
                }) = fact
                else {
                    return false;
                };
                let Some(payload) = variant_fields(value, *variant) else {
                    return false;
                };
                for (field_pattern, (index, local)) in pattern.fields().zip(fields) {
                    let Some(field) = payload.get(*index) else {
                        return false;
                    };
                    match (local, field_pattern.pattern()) {
                        (Some(id), _) => *env = bind(env, *id, Slot::Ready(field.clone())),
                        (None, Some(nested)) => {
                            if !self.matches(unit, nested, field, env) {
                                return false;
                            }
                        }
                        (None, None) => {}
                    }
                }
                true
            }
        }
    }
}

/// Returns the payload of `value` when it is the variant with index
/// `variant`.
fn variant_fields(value: &Val, variant: usize) -> Option<&[Val]> {
    match value {
        Val::Tag(_, index) if *index as usize == variant => Some(&[]),
        Val::Variant(v) if v.index as usize == variant => Some(&v.fields),
        _ => None,
    }
}

fn literal_matches(e: Expr<'_>, value: &Val) -> bool {
    match e {
        Expr::Literal(literal) => {
            let text = literal.token().map_or("", |t| t.text());
            match literal.kind() {
                Some(LiteralKind::None) => value.is_none(),
                Some(LiteralKind::True) => matches!(value, Val::Bool(true)),
                Some(LiteralKind::False) => matches!(value, Val::Bool(false)),
                Some(LiteralKind::Int) => {
                    matches!((value, parse_int(text)), (Val::Int(v), Some(p)) if *v == p)
                }
                _ => false,
            }
        }
        Expr::Unary(unary) if unary.op() == Some(UnaryOp::Neg) => match unary.operand() {
            Some(Expr::Literal(literal)) => {
                let text = literal.token().map_or("", |t| t.text());
                let negated = parse_int(text).and_then(i64::checked_neg);
                matches!((value, negated), (Val::Int(v), Some(p)) if *v == p)
            }
            _ => false,
        },
        Expr::Str(string) => match (value, string.value()) {
            (Val::Str(v), Some(text)) => **v == *text,
            _ => false,
        },
        _ => false,
    }
}

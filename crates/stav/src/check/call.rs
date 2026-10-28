//! Typing calls: of top-level functions, payload variants, built-in
//! functions, symbol constructors and function values.

use super::builtins::{self, Builtin};
use super::decls::GlobalKind;
use super::expr::PathItem;
use super::model::{CallFact, Callee, ExprFact, Fact, Meaning};
use super::suggest::did_you_mean;
use super::totality::Node;
use super::types::{GlobalId, ParamDef, SumId, SymbolId, Type, TypeId};
use super::{Checker, Cx};
use crate::ast::{self, Expr};
use crate::diagnostic::Diagnostic;
use crate::span::Span;
use crate::syntax::SyntaxToken;

/// What a call calls.
#[derive(Debug, Clone, Copy)]
enum CallTarget {
    Global(GlobalId),
    Variant(SumId, usize),
    Builtin(Builtin),
    Constructor(SymbolId),
    Value(TypeId),
    Error,
}

impl<'p> Checker<'p> {
    pub(super) fn call(
        &mut self,
        cx: &mut Cx,
        call: ast::Call<'p>,
        expected: Option<TypeId>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let args: Vec<ast::Arg<'p>> = call.args().map(|a| a.args().collect()).unwrap_or_default();
        let span = call.syntax().span();
        let target = match call.callee() {
            Some(callee) => self.callee(cx, callee, want),
            None => CallTarget::Error,
        };
        match target {
            CallTarget::Global(g) => {
                let GlobalKind::Fn(sig) = &self.globals[g.index()].kind else {
                    return (TypeId::ERROR, Meaning::Plain);
                };
                let (params, ret) = (sig.params.clone(), sig.ret);
                let name = self.globals[g.index()].name.clone();
                let slots = self.bind_args(cx, &params, &args, &name, span);
                let fact = CallFact {
                    callee: Callee::Global(g),
                    args: slots,
                };
                (ret, Meaning::Call(fact))
            }
            CallTarget::Variant(s, index) => {
                let variant = self.types.sum(s).variants[index].clone();
                let Some(fields) = variant.fields else {
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0444",
                            span,
                            format!("the variant `{}` has no payload fields", variant.name),
                        )
                        .with_help(format!("write `{}` without parentheses", variant.name)),
                    );
                    self.synth_args(cx, &args);
                    return (self.types.intern(Type::Sum(s)), Meaning::Plain);
                };
                if let Some(owner) = cx.owner {
                    self.calls
                        .depend(owner, Node::SumDefaults(s), cx.file, span);
                }
                let slots = self.bind_args(cx, &fields, &args, &variant.name, span);
                let fact = CallFact {
                    callee: Callee::Variant {
                        sum: s,
                        variant: index,
                    },
                    args: slots,
                };
                (self.types.intern(Type::Sum(s)), Meaning::Call(fact))
            }
            CallTarget::Builtin(b) => {
                let ty = self.builtin(cx, b, &args, expected, span);
                let fact = CallFact {
                    callee: Callee::Builtin(b),
                    args: positional(&args),
                };
                (ty, Meaning::Call(fact))
            }
            CallTarget::Constructor(s) => {
                let name = self.types.symbols[s.index()].constructor.clone();
                if self.arity(cx, &args, 1, &name, span)
                    && let Some(v) = args[0].value()
                {
                    self.expr(cx, v, Some(TypeId::STR));
                }
                let fact = CallFact {
                    callee: Callee::Constructor(s),
                    args: positional(&args),
                };
                (self.types.intern(Type::Symbol(s)), Meaning::Call(fact))
            }
            CallTarget::Value(ty) => {
                let (params, ret) = match self.types.get(self.types.peel(ty)).clone() {
                    Type::Fn(params, ret) => (params, ret),
                    Type::Error => {
                        self.synth_args(cx, &args);
                        return (TypeId::ERROR, Meaning::Plain);
                    }
                    _ => {
                        let rendered = self.types.render_plain(ty);
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0401",
                                span,
                                format!("a value of type `{rendered}` cannot be called"),
                            ),
                        );
                        self.synth_args(cx, &args);
                        return (TypeId::ERROR, Meaning::Plain);
                    }
                };
                if self.arity(cx, &args, params.len(), "the function value", span) {
                    for (arg, param) in args.iter().zip(&params) {
                        if let Some(v) = arg.value() {
                            self.expr(cx, v, Some(*param));
                        }
                    }
                }
                let fact = CallFact {
                    callee: Callee::Value,
                    args: positional(&args),
                };
                (ret, Meaning::Call(fact))
            }
            CallTarget::Error => {
                self.synth_args(cx, &args);
                (expected.unwrap_or(TypeId::ERROR), Meaning::Plain)
            }
        }
    }

    /// Checks that `args` are `count` positional arguments, and checks the
    /// arguments without expected types otherwise.
    pub(super) fn arity(
        &mut self,
        cx: &mut Cx,
        args: &[ast::Arg<'p>],
        count: usize,
        what: &str,
        span: Span,
    ) -> bool {
        if let Some(named) = args.iter().find(|a| a.name().is_some()) {
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0444",
                    named.syntax().span(),
                    format!("{what} takes positional arguments only"),
                ),
            );
            self.synth_args(cx, args);
            return false;
        }
        if args.len() != count {
            let plural = if count == 1 { "" } else { "s" };
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0444",
                    span,
                    format!(
                        "{what} takes {count} argument{plural}, found {}",
                        args.len()
                    ),
                ),
            );
            self.synth_args(cx, args);
            return false;
        }
        true
    }

    pub(super) fn synth_args(&mut self, cx: &mut Cx, args: &[ast::Arg<'p>]) {
        for arg in args {
            if let Some(v) = arg.value() {
                self.expr(cx, v, Some(TypeId::ERROR));
            }
        }
    }

    /// Resolves what a call calls.
    fn callee(&mut self, cx: &mut Cx, callee: Expr<'p>, want: Option<TypeId>) -> CallTarget {
        let span = callee.syntax().span();
        let item = match callee {
            Expr::Name(name) => {
                let Some(token) = name.token() else {
                    return CallTarget::Error;
                };
                let text = token.text();
                if cx.lookup(text).is_some() || self.lookup_value(cx.file, text).is_some() {
                    let (ty, meaning) = self.expr_kind_name_callee(cx, token, want);
                    let target = match meaning {
                        Meaning::Global(g)
                            if matches!(self.globals[g.index()].kind, GlobalKind::Fn(_)) =>
                        {
                            CallTarget::Global(g)
                        }
                        _ => CallTarget::Value(ty),
                    };
                    self.fact(cx, callee.syntax(), Fact::Expr(ExprFact { ty, meaning }));
                    return target;
                }
                if let Some(s) = want.and_then(|w| self.types.as_sum(w))
                    && let Some((index, _)) = self.types.sum(s).variant(text)
                {
                    return CallTarget::Variant(s, index);
                }
                if let Some(b) = builtins::lookup(text) {
                    return CallTarget::Builtin(b);
                }
                if let Some(i) = self
                    .types
                    .symbols
                    .iter()
                    .position(|s| s.constructor == text)
                {
                    return CallTarget::Constructor(SymbolId::new(i));
                }
                if self.lookup_type(cx.file, text).is_some() {
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0401",
                            span,
                            format!("`{text}` is a type, not a function"),
                        )
                        .with_help(format!("write `{text} {{ ... }}` for a struct value")),
                    );
                    return CallTarget::Error;
                }
                if self.used_too_early(cx, text, span) {
                    return CallTarget::Error;
                }
                let mut candidates: Vec<String> = cx.scope.names().map(str::to_owned).collect();
                candidates.extend(self.value_names(cx.file));
                candidates.extend(builtins::names().map(str::to_owned));
                candidates.extend(self.types.symbols.iter().map(|s| s.constructor.clone()));
                if let Some(s) = want.and_then(|w| self.types.as_sum(w)) {
                    candidates.extend(self.types.sum(s).variants.iter().map(|v| v.name.clone()));
                }
                let help = did_you_mean(text, candidates.iter().map(String::as_str));
                self.error(
                    cx.file,
                    Diagnostic::error("E0302", span, format!("unknown function `{text}`"))
                        .with_optional_help(help),
                );
                return CallTarget::Error;
            }
            Expr::Field(_) => self.static_path(cx, callee),
            _ => None,
        };
        match item {
            Some(PathItem::Value(g)) => {
                let (ty, meaning) = self.global_value(cx, g, span);
                self.fact(cx, callee.syntax(), Fact::Expr(ExprFact { ty, meaning }));
                match self.globals[g.index()].kind {
                    GlobalKind::Fn(_) => CallTarget::Global(g),
                    GlobalKind::Let(_) => CallTarget::Value(ty),
                }
            }
            Some(PathItem::Variant(s, index)) => CallTarget::Variant(s, index),
            Some(PathItem::Error) => CallTarget::Error,
            Some(PathItem::Type(_) | PathItem::Module(_)) => {
                self.error(
                    cx.file,
                    Diagnostic::error("E0401", span, "this is not a function"),
                );
                CallTarget::Error
            }
            None => CallTarget::Value(self.expr(cx, callee, None)),
        }
    }

    fn expr_kind_name_callee(
        &mut self,
        cx: &mut Cx,
        token: SyntaxToken<'p>,
        want: Option<TypeId>,
    ) -> (TypeId, Meaning) {
        let name = token.text();
        let span = token.span();
        match self.lexical(cx, name, span) {
            Some(found) => {
                if let Some(s) = want.and_then(|w| self.types.as_sum(w))
                    && self
                        .types
                        .sum(s)
                        .variant(name)
                        .is_some_and(|(_, v)| v.fields.is_some())
                {
                    self.shadowed_variant(cx, name, span, want);
                }
                found
            }
            None => (TypeId::ERROR, Meaning::Plain),
        }
    }

    /// Matches the arguments of a call to the parameters, checks each
    /// argument against its parameter, and returns the argument of every
    /// parameter.
    fn bind_args(
        &mut self,
        cx: &mut Cx,
        params: &[ParamDef],
        args: &[ast::Arg<'p>],
        what: &str,
        span: Span,
    ) -> Vec<Option<usize>> {
        let mut slots: Vec<Option<usize>> = vec![None; params.len()];
        let mut filled = vec![false; params.len()];
        for (i, arg) in args.iter().enumerate() {
            let value = arg.value();
            let index = match arg.name() {
                None if i < params.len() => Some(i),
                None if i > params.len() => None,
                None => {
                    let plural = if params.len() == 1 { "" } else { "s" };
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0444",
                            arg.syntax().span(),
                            format!(
                                "`{what}` takes {} argument{plural}, found {}",
                                params.len(),
                                args.len()
                            ),
                        ),
                    );
                    None
                }
                Some(name) => match params.iter().position(|p| p.name == name.text()) {
                    Some(index) => Some(index),
                    None => {
                        let help =
                            did_you_mean(name.text(), params.iter().map(|p| p.name.as_str()));
                        self.error(
                            cx.file,
                            Diagnostic::error(
                                "E0302",
                                name.span(),
                                format!("`{what}` has no parameter `{}`", name.text()),
                            )
                            .with_optional_help(help),
                        );
                        None
                    }
                },
            };
            let index = index.filter(|&index| {
                if filled[index] {
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0204",
                            arg.syntax().span(),
                            format!("argument `{}` is given more than once", params[index].name),
                        ),
                    );
                    false
                } else {
                    true
                }
            });
            let Some(v) = value else { continue };
            match index {
                Some(index) => {
                    filled[index] = true;
                    self.expr(cx, v, Some(params[index].ty));
                    slots[index] = Some(v.syntax().index());
                }
                None => {
                    self.expr(cx, v, Some(TypeId::ERROR));
                }
            }
        }
        let missing: Vec<String> = params
            .iter()
            .zip(&filled)
            .filter(|(p, filled)| {
                !**filled && p.default.is_none() && self.types.unwrap_option(p.ty).is_none()
            })
            .map(|(p, _)| format!("`{}`", p.name))
            .collect();
        if !missing.is_empty() {
            let plural = if missing.len() == 1 { "" } else { "s" };
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0444",
                    span,
                    format!(
                        "`{what}` is missing the argument{plural} {}",
                        missing.join(", ")
                    ),
                ),
            );
        }
        slots
    }
}

/// Returns the node index of every positional argument.
fn positional(args: &[ast::Arg<'_>]) -> Vec<Option<usize>> {
    args.iter()
        .map(|a| a.value().map(|v| v.syntax().index()))
        .collect()
}

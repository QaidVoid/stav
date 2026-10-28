//! The built-in functions and how calls to them are typed.
//!
//! Built-in functions are the only generic functions. Each is typed by
//! hand: the types of earlier arguments decide what later arguments are
//! checked against.

use super::types::{Type, TypeId};
use super::{Checker, Cx};
use crate::ast::{self, Expr};
use crate::diagnostic::Diagnostic;
use crate::span::Span;

/// A built-in function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Builtin {
    Default,
    Force,
    Fail,
    ToStr,
    ToFloat,
    ToInt,
    ParseInt,
    Len,
    NonEmpty,
    Join,
    Split,
    Replace,
    Trim,
    Lower,
    Upper,
    StartsWith,
    EndsWith,
    Contains,
    Matches,
    Lines,
    Indent,
    Map,
    Filter,
    Fold,
    Flatten,
    Sort,
    Unique,
    Reverse,
    Range,
    Any,
    All,
    First,
    Last,
    Get,
    ToList,
    ToSet,
    Keys,
    Values,
    HasKey,
    MapValues,
    FilterMap,
    Union,
    Min,
    Max,
    Abs,
}

const BUILTINS: [(&str, Builtin, usize); 45] = [
    ("default", Builtin::Default, 1),
    ("force", Builtin::Force, 1),
    ("fail", Builtin::Fail, 1),
    ("toStr", Builtin::ToStr, 1),
    ("toFloat", Builtin::ToFloat, 1),
    ("toInt", Builtin::ToInt, 1),
    ("parseInt", Builtin::ParseInt, 1),
    ("len", Builtin::Len, 1),
    ("nonEmpty", Builtin::NonEmpty, 1),
    ("join", Builtin::Join, 2),
    ("split", Builtin::Split, 2),
    ("replace", Builtin::Replace, 3),
    ("trim", Builtin::Trim, 1),
    ("lower", Builtin::Lower, 1),
    ("upper", Builtin::Upper, 1),
    ("startsWith", Builtin::StartsWith, 2),
    ("endsWith", Builtin::EndsWith, 2),
    ("contains", Builtin::Contains, 2),
    ("matches", Builtin::Matches, 2),
    ("lines", Builtin::Lines, 1),
    ("indent", Builtin::Indent, 2),
    ("map", Builtin::Map, 2),
    ("filter", Builtin::Filter, 2),
    ("fold", Builtin::Fold, 3),
    ("flatten", Builtin::Flatten, 1),
    ("sort", Builtin::Sort, 1),
    ("unique", Builtin::Unique, 1),
    ("reverse", Builtin::Reverse, 1),
    ("range", Builtin::Range, 2),
    ("any", Builtin::Any, 2),
    ("all", Builtin::All, 2),
    ("first", Builtin::First, 1),
    ("last", Builtin::Last, 1),
    ("get", Builtin::Get, 2),
    ("toList", Builtin::ToList, 1),
    ("toSet", Builtin::ToSet, 1),
    ("keys", Builtin::Keys, 1),
    ("values", Builtin::Values, 1),
    ("hasKey", Builtin::HasKey, 2),
    ("mapValues", Builtin::MapValues, 2),
    ("filterMap", Builtin::FilterMap, 2),
    ("union", Builtin::Union, 2),
    ("min", Builtin::Min, 2),
    ("max", Builtin::Max, 2),
    ("abs", Builtin::Abs, 1),
];

/// Returns the built-in function with the given name.
pub(crate) fn lookup(name: &str) -> Option<Builtin> {
    BUILTINS
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, b, _)| *b)
}

/// Returns the names of every built-in function.
pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    BUILTINS.iter().map(|(n, ..)| *n)
}

impl Builtin {
    /// Returns the name the function is called by.
    pub(crate) fn name(self) -> &'static str {
        BUILTINS
            .iter()
            .find(|(_, b, _)| *b == self)
            .map_or("", |(n, ..)| n)
    }

    fn arity(self) -> usize {
        BUILTINS
            .iter()
            .find(|(_, b, _)| *b == self)
            .map_or(0, |(.., arity)| *arity)
    }
}

/// The shapes the first argument of a built-in may take.
enum Shape {
    Str,
    List(TypeId),
    Set(TypeId),
    Map(TypeId, TypeId),
    Error,
    Other,
}

impl<'p> Checker<'p> {
    fn shape(&self, ty: TypeId) -> Shape {
        match self.types.get(self.types.peel(ty)) {
            Type::Str => Shape::Str,
            Type::List(t) => Shape::List(*t),
            Type::Set(t) => Shape::Set(*t),
            Type::Map(k, v) => Shape::Map(*k, *v),
            Type::Error => Shape::Error,
            _ => Shape::Other,
        }
    }

    /// Types a call of the built-in `b` and returns its result type.
    pub(super) fn builtin(
        &mut self,
        cx: &mut Cx,
        b: Builtin,
        args: &[ast::Arg<'p>],
        expected: Option<TypeId>,
        span: Span,
    ) -> TypeId {
        let name = format!("`{}`", b.name());
        if !self.arity(cx, args, b.arity(), &name, span) {
            return match b {
                Builtin::Fail | Builtin::Default | Builtin::Force => {
                    expected.unwrap_or(TypeId::ERROR)
                }
                _ => TypeId::ERROR,
            };
        }
        let values: Vec<Option<Expr<'p>>> = args.iter().map(|a| a.value()).collect();
        let mut arg = |this: &mut Self, cx: &mut Cx, i: usize, ty: Option<TypeId>| match values[i] {
            Some(e) => this.expr(cx, e, ty),
            None => TypeId::ERROR,
        };
        let s = Some(TypeId::STR);
        match b {
            Builtin::Default | Builtin::Force => {
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0441",
                        span,
                        format!("{name} is a priority marker, valid only as the whole right side of an attribute"),
                    )
                    .with_help(format!("write `attribute = {}(...)`", b.name())),
                );
                arg(self, cx, 0, expected)
            }
            Builtin::Fail => {
                arg(self, cx, 0, s);
                expected.unwrap_or_else(|| {
                    self.error(
                        cx.file,
                        Diagnostic::error("E0430", span, "cannot infer the type of `fail(...)`")
                            .with_help("use `fail` where a type is expected"),
                    );
                    TypeId::ERROR
                })
            }
            Builtin::ToStr => {
                let t = arg(self, cx, 0, None);
                if !self.types.is_text(t) {
                    let ty = self.types.render_plain(t);
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0421",
                            span,
                            format!("cannot convert a value of type `{ty}` to a string"),
                        ),
                    );
                }
                TypeId::STR
            }
            Builtin::ToFloat => {
                arg(self, cx, 0, Some(TypeId::INT));
                TypeId::FLOAT
            }
            Builtin::ToInt => {
                arg(self, cx, 0, Some(TypeId::FLOAT));
                TypeId::INT
            }
            Builtin::ParseInt => {
                arg(self, cx, 0, s);
                self.types.option(TypeId::INT)
            }
            Builtin::Len | Builtin::NonEmpty => {
                let t = arg(self, cx, 0, None);
                if matches!(self.shape(t), Shape::Other) {
                    self.builtin_mismatch(cx, span, &name, t, "a string or a collection");
                }
                if b == Builtin::Len {
                    TypeId::INT
                } else {
                    TypeId::BOOL
                }
            }
            Builtin::Join => {
                let list = self.types.list(TypeId::STR);
                arg(self, cx, 0, Some(list));
                arg(self, cx, 1, s);
                TypeId::STR
            }
            Builtin::Split => {
                arg(self, cx, 0, s);
                arg(self, cx, 1, s);
                self.types.list(TypeId::STR)
            }
            Builtin::Replace => {
                for i in 0..3 {
                    arg(self, cx, i, s);
                }
                TypeId::STR
            }
            Builtin::Trim | Builtin::Lower | Builtin::Upper => {
                arg(self, cx, 0, s);
                TypeId::STR
            }
            Builtin::StartsWith | Builtin::EndsWith => {
                arg(self, cx, 0, s);
                arg(self, cx, 1, s);
                TypeId::BOOL
            }
            Builtin::Contains => {
                let t = arg(self, cx, 0, None);
                let needle = match self.shape(t) {
                    Shape::Str => s,
                    Shape::List(e) | Shape::Set(e) => Some(e),
                    Shape::Error => Some(TypeId::ERROR),
                    _ => {
                        self.builtin_mismatch(cx, span, &name, t, "a string, a list or a set");
                        Some(TypeId::ERROR)
                    }
                };
                arg(self, cx, 1, needle);
                TypeId::BOOL
            }
            Builtin::Matches => {
                arg(self, cx, 0, s);
                arg(self, cx, 1, s);
                if let Some(Expr::Str(pattern)) = values[1]
                    && let Some(text) = pattern.value()
                {
                    self.check_regex(cx.file, &text, pattern.syntax().span());
                }
                TypeId::BOOL
            }
            Builtin::Lines => {
                arg(self, cx, 0, s);
                self.types.list(TypeId::STR)
            }
            Builtin::Indent => {
                arg(self, cx, 0, s);
                arg(self, cx, 1, Some(TypeId::INT));
                TypeId::STR
            }
            Builtin::Map | Builtin::Filter | Builtin::Any | Builtin::All => {
                let t = arg(self, cx, 0, None);
                let Some(elem) = self.list_elem(cx, span, &name, t) else {
                    arg(self, cx, 1, Some(TypeId::ERROR));
                    return TypeId::ERROR;
                };
                let f = self.fn_arg(cx, &mut arg, 1, &[elem]);
                let ret = self.fn_ret(cx, values[1], f, &[elem]);
                match b {
                    Builtin::Map => self.types.list(ret),
                    Builtin::Filter => {
                        self.bool_ret(cx, values[1], ret);
                        self.types.peel(t)
                    }
                    _ => {
                        self.bool_ret(cx, values[1], ret);
                        TypeId::BOOL
                    }
                }
            }
            Builtin::Fold => {
                let t = arg(self, cx, 0, None);
                let elem = self.list_elem(cx, span, &name, t).unwrap_or(TypeId::ERROR);
                let init = arg(self, cx, 1, expected);
                let f = self.fn_arg(cx, &mut arg, 2, &[init, elem]);
                let ret = self.fn_ret(cx, values[2], f, &[init, elem]);
                if !self.types.fits(ret, init) {
                    let span = values[2].map_or(span, |v| v.syntax().span());
                    self.mismatch(cx.file, span, init, ret);
                }
                init
            }
            Builtin::Flatten => {
                let t = arg(self, cx, 0, None);
                let inner = self.list_elem(cx, span, &name, t);
                match inner.map(|i| self.shape(i)) {
                    Some(Shape::List(e)) => self.types.list(e),
                    Some(Shape::Error) | None => TypeId::ERROR,
                    Some(_) => {
                        self.builtin_mismatch(cx, span, &name, t, "a list of lists");
                        TypeId::ERROR
                    }
                }
            }
            Builtin::Sort | Builtin::Unique | Builtin::Reverse | Builtin::ToSet => {
                let t = arg(self, cx, 0, None);
                let Some(elem) = self.list_elem(cx, span, &name, t) else {
                    return TypeId::ERROR;
                };
                let (ok, what) = match b {
                    Builtin::Sort => (self.types.is_ordered(elem), "ordered"),
                    Builtin::Unique | Builtin::ToSet => (self.types.is_key(elem), "a key type"),
                    _ => (true, ""),
                };
                if !ok {
                    let ty = self.types.render_plain(elem);
                    self.error(
                        cx.file,
                        Diagnostic::error(
                            "E0401",
                            span,
                            format!("{name} needs elements that are {what}, found `{ty}`"),
                        ),
                    );
                }
                if b == Builtin::ToSet {
                    self.types.set(elem)
                } else {
                    self.types.list(elem)
                }
            }
            Builtin::Range => {
                arg(self, cx, 0, Some(TypeId::INT));
                arg(self, cx, 1, Some(TypeId::INT));
                self.types.list(TypeId::INT)
            }
            Builtin::First | Builtin::Last => {
                let t = arg(self, cx, 0, None);
                match self.list_elem(cx, span, &name, t) {
                    Some(elem) => self.types.option(elem),
                    None => TypeId::ERROR,
                }
            }
            Builtin::Get => {
                let t = arg(self, cx, 0, None);
                match self.shape(t) {
                    Shape::List(e) => {
                        arg(self, cx, 1, Some(TypeId::INT));
                        self.types.option(e)
                    }
                    Shape::Map(k, v) => {
                        arg(self, cx, 1, Some(k));
                        self.types.option(v)
                    }
                    Shape::Error => {
                        arg(self, cx, 1, Some(TypeId::ERROR));
                        TypeId::ERROR
                    }
                    _ => {
                        arg(self, cx, 1, Some(TypeId::ERROR));
                        self.builtin_mismatch(cx, span, &name, t, "a list or a map");
                        TypeId::ERROR
                    }
                }
            }
            Builtin::ToList => {
                let t = arg(self, cx, 0, None);
                match self.shape(t) {
                    Shape::Set(e) => self.types.list(e),
                    Shape::Error => TypeId::ERROR,
                    _ => {
                        self.builtin_mismatch(cx, span, &name, t, "a set");
                        TypeId::ERROR
                    }
                }
            }
            Builtin::Keys
            | Builtin::Values
            | Builtin::HasKey
            | Builtin::MapValues
            | Builtin::FilterMap
            | Builtin::Union => {
                let t = arg(self, cx, 0, None);
                let (k, v) = match self.shape(t) {
                    Shape::Map(k, v) => (k, v),
                    Shape::Error => (TypeId::ERROR, TypeId::ERROR),
                    _ => {
                        self.builtin_mismatch(cx, span, &name, t, "a map");
                        (TypeId::ERROR, TypeId::ERROR)
                    }
                };
                match b {
                    Builtin::Keys => self.types.list(k),
                    Builtin::Values => self.types.list(v),
                    Builtin::HasKey => {
                        arg(self, cx, 1, Some(k));
                        TypeId::BOOL
                    }
                    Builtin::Union => {
                        let map = self.types.peel(t);
                        arg(self, cx, 1, Some(map));
                        map
                    }
                    _ => {
                        let f = self.fn_arg(cx, &mut arg, 1, &[k, v]);
                        let ret = self.fn_ret(cx, values[1], f, &[k, v]);
                        if b == Builtin::FilterMap {
                            self.bool_ret(cx, values[1], ret);
                            self.types.peel(t)
                        } else {
                            self.types.map(k, ret)
                        }
                    }
                }
            }
            Builtin::Min | Builtin::Max => {
                let t = arg(self, cx, 0, None);
                let peeled = self.types.peel(t);
                if !matches!(peeled, TypeId::INT | TypeId::FLOAT | TypeId::ERROR) {
                    self.builtin_mismatch(cx, span, &name, t, "an `Int` or a `Float`");
                    arg(self, cx, 1, Some(TypeId::ERROR));
                    return TypeId::ERROR;
                }
                arg(self, cx, 1, Some(peeled));
                peeled
            }
            Builtin::Abs => {
                arg(self, cx, 0, Some(TypeId::INT));
                TypeId::INT
            }
        }
    }

    fn builtin_mismatch(&mut self, cx: &Cx, span: Span, name: &str, found: TypeId, what: &str) {
        if self.types.has_error(found) {
            return;
        }
        let ty = self.types.render_plain(found);
        self.error(
            cx.file,
            Diagnostic::error("E0401", span, format!("{name} needs {what}, found `{ty}`")),
        );
    }

    /// Returns the element type of a list argument.
    fn list_elem(&mut self, cx: &Cx, span: Span, name: &str, ty: TypeId) -> Option<TypeId> {
        match self.shape(ty) {
            Shape::List(e) => Some(e),
            Shape::Error => Some(TypeId::ERROR),
            _ => {
                self.builtin_mismatch(cx, span, name, ty, "a list");
                None
            }
        }
    }

    /// Checks a function argument against a function type with the given
    /// parameters and an unknown return type.
    fn fn_arg(
        &mut self,
        cx: &mut Cx,
        arg: &mut impl FnMut(&mut Self, &mut Cx, usize, Option<TypeId>) -> TypeId,
        i: usize,
        params: &[TypeId],
    ) -> TypeId {
        let expected = self.types.intern(Type::Fn(params.to_vec(), TypeId::ERROR));
        arg(self, cx, i, Some(expected))
    }

    /// Returns the return type of a function argument, after checking that
    /// it takes the given parameters.
    fn fn_ret(&mut self, cx: &Cx, value: Option<Expr<'p>>, f: TypeId, params: &[TypeId]) -> TypeId {
        let f = match value {
            Some(v) => self
                .units
                .get(cx.unit.index())
                .and_then(|u| u.facts.get(v.syntax().index()))
                .and_then(|fact| match fact {
                    super::model::Fact::Expr(e) => Some(e.ty),
                    _ => None,
                })
                .unwrap_or(f),
            None => f,
        };
        match self.types.get(self.types.peel(f)).clone() {
            Type::Fn(found, ret)
                if found.len() == params.len()
                    && found
                        .iter()
                        .zip(params)
                        .all(|(a, b)| self.types.fits(*b, *a)) =>
            {
                ret
            }
            Type::Error => TypeId::ERROR,
            _ => {
                let params: Vec<String> =
                    params.iter().map(|p| self.types.render_plain(*p)).collect();
                let found = self.types.render_plain(f);
                let span = value.map_or(Span { start: 0, end: 0 }, |v| v.syntax().span());
                self.error(
                    cx.file,
                    Diagnostic::error(
                        "E0401",
                        span,
                        format!(
                            "expected a function `Fn({}) -> ...`, found `{found}`",
                            params.join(", ")
                        ),
                    ),
                );
                TypeId::ERROR
            }
        }
    }

    fn bool_ret(&mut self, cx: &Cx, value: Option<Expr<'p>>, ret: TypeId) {
        if !self.types.fits(ret, TypeId::BOOL) {
            let span = value.map_or(Span { start: 0, end: 0 }, |v| v.syntax().span());
            let found = self.types.render_plain(ret);
            self.error(
                cx.file,
                Diagnostic::error(
                    "E0401",
                    span,
                    format!("expected a function returning `Bool`, found one returning `{found}`"),
                ),
            );
        }
    }
}

//! The built-in functions.

use std::rc::Rc;

use super::expr::{Cx, map_get};
use super::val::Val;
use super::{Eval, R};
use crate::check::builtins::Builtin;
use crate::diagnostic::Diagnostic;
use crate::span::SourceSpan;

/// The largest indentation `indent` accepts.
const MAX_INDENT: i64 = 64;

impl<'c> Eval<'c> {
    pub(crate) fn builtin(
        &mut self,
        cx: &Cx<'_>,
        b: Builtin,
        args: &[Option<usize>],
        at: SourceSpan,
    ) -> R<Val> {
        let mut values = Vec::with_capacity(args.len());
        let mut spans = Vec::with_capacity(args.len());
        for node in args {
            let Some(node) = node else {
                return Err(self.bug(cx.file, at.span, "an argument is missing"));
            };
            let e = self.expr_at(cx.unit, *node)?;
            spans.push(cx.at(e.syntax().span()));
            values.push(self.expr(cx, e)?);
        }
        match b {
            Builtin::Map
            | Builtin::Filter
            | Builtin::Any
            | Builtin::All
            | Builtin::Fold
            | Builtin::MapValues
            | Builtin::FilterMap => self.higher_order(b, values, at),
            _ => self.first_order(b, values, &spans, at),
        }
    }

    /// Applies a built-in function that calls a function value. It is kept
    /// apart from the others, so that the frames of nested calls through
    /// it stay small.
    fn higher_order(&mut self, b: Builtin, values: Vec<Val>, at: SourceSpan) -> R<Val> {
        let mut values = values.into_iter();
        let (a, b0, c0) = (
            values.next().unwrap_or(Val::None),
            values.next().unwrap_or(Val::None),
            values.next().unwrap_or(Val::None),
        );
        match b {
            Builtin::Fold => {
                let (Val::List(list), Val::Fn(f)) = (&a, &c0) else {
                    return Err(self.wrong(at));
                };
                let mut acc = b0;
                for item in &list.items {
                    acc = self.apply(f, vec![acc, item.clone()], at)?;
                }
                Ok(acc)
            }
            Builtin::MapValues | Builtin::FilterMap => {
                let (Val::Map(map), Val::Fn(f)) = (&a, &b0) else {
                    return Err(self.wrong(at));
                };
                let mut entries = Vec::with_capacity(map.entries.len());
                for (key, value) in &map.entries {
                    let result = self.apply(f, vec![key.clone(), value.clone()], at)?;
                    if b == Builtin::MapValues {
                        entries.push((key.clone(), result));
                    } else if matches!(result, Val::Bool(true)) {
                        entries.push((key.clone(), value.clone()));
                    }
                }
                self.new_map(entries, at)
            }
            _ => {
                let (Val::List(list), Val::Fn(f)) = (&a, &b0) else {
                    return Err(self.wrong(at));
                };
                let mut out = Vec::new();
                for item in &list.items {
                    let result = self.apply(f, vec![item.clone()], at)?;
                    match (b, &result) {
                        (Builtin::Map, _) => out.push(result),
                        (Builtin::Filter, Val::Bool(true)) => out.push(item.clone()),
                        (Builtin::Any, Val::Bool(true)) => return Ok(Val::Bool(true)),
                        (Builtin::All, Val::Bool(false)) => return Ok(Val::Bool(false)),
                        _ => {}
                    }
                }
                match b {
                    Builtin::Any => Ok(Val::Bool(false)),
                    Builtin::All => Ok(Val::Bool(true)),
                    _ => self.new_list(out, at),
                }
            }
        }
    }

    /// Applies a built-in function that calls no function value.
    fn first_order(
        &mut self,
        b: Builtin,
        values: Vec<Val>,
        spans: &[SourceSpan],
        at: SourceSpan,
    ) -> R<Val> {
        let arg = |i: usize| values.get(i).cloned().unwrap_or(Val::None);
        let span = |i: usize| spans.get(i).copied().unwrap_or(at);
        let (a, b0, c0) = (arg(0), arg(1), arg(2));
        match b {
            Builtin::Default | Builtin::Force => Ok(a),
            Builtin::Fail => {
                let message = a.text(&self.c.types);
                Err(self.fail(at, "E0608", format!("evaluation failed: {message}")))
            }
            Builtin::ToStr => {
                let text = a.text(&self.c.types);
                self.new_str(text, at)
            }
            Builtin::ToFloat => match a {
                #[allow(clippy::cast_precision_loss, reason = "the documented conversion")]
                Val::Int(i) => Ok(Val::Float(i as f64)),
                _ => Err(self.wrong(at)),
            },
            Builtin::ToInt => match a {
                Val::Float(f) => {
                    let t = f.trunc();
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "the range is checked first"
                    )]
                    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&t) {
                        Ok(Val::Int(t as i64))
                    } else {
                        Err(self.fail(at, "E0601", format!("{f:?} is out of the range of `Int`")))
                    }
                }
                _ => Err(self.wrong(at)),
            },
            Builtin::ParseInt => Ok(match &a {
                Val::Str(s) => parse_decimal(s).map_or(Val::None, Val::Int),
                _ => return Err(self.wrong(at)),
            }),
            Builtin::Len => Ok(Val::Int(length(&a))),
            Builtin::NonEmpty => Ok(Val::Bool(length(&a) > 0)),
            Builtin::Join => {
                let (Val::List(items), Val::Str(sep)) = (&a, &b0) else {
                    return Err(self.wrong(at));
                };
                let mut out = String::new();
                for (i, item) in items.items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(sep);
                    }
                    if let Val::Str(s) = item {
                        out.push_str(s);
                    }
                    self.reserve_str(out.len(), at)?;
                }
                self.new_str(out, at)
            }
            Builtin::Split => {
                let (Val::Str(s), Val::Str(sep)) = (&a, &b0) else {
                    return Err(self.wrong(at));
                };
                if sep.is_empty() {
                    return Err(self.empty_argument(span(1), "split", "sep"));
                }
                let mut items = Vec::new();
                for part in s.split(&**sep) {
                    items.push(self.new_str(part.to_owned(), at)?);
                }
                self.new_list(items, at)
            }
            Builtin::Replace => {
                let (Val::Str(s), Val::Str(from), Val::Str(to)) = (&a, &b0, &c0) else {
                    return Err(self.wrong(at));
                };
                if from.is_empty() {
                    return Err(self.empty_argument(span(1), "replace", "from"));
                }
                let count = s.matches(&**from).count();
                let len = s
                    .len()
                    .saturating_add(count.saturating_mul(to.len()))
                    .saturating_sub(count.saturating_mul(from.len()));
                self.reserve_str(len, at)?;
                self.new_str(s.replace(&**from, to), at)
            }
            Builtin::Trim => self.text_fn(&a, at, |s| {
                s.trim_matches(|c: char| c.is_ascii_whitespace()).to_owned()
            }),
            Builtin::Lower => self.text_fn(&a, at, str::to_ascii_lowercase),
            Builtin::Upper => self.text_fn(&a, at, str::to_ascii_uppercase),
            Builtin::StartsWith | Builtin::EndsWith => {
                let (Val::Str(s), Val::Str(p)) = (&a, &b0) else {
                    return Err(self.wrong(at));
                };
                Ok(Val::Bool(if b == Builtin::StartsWith {
                    s.starts_with(&**p)
                } else {
                    s.ends_with(&**p)
                }))
            }
            Builtin::Contains => Ok(Val::Bool(match (&a, &b0) {
                (Val::Str(s), Val::Str(needle)) => s.contains(&**needle),
                (Val::List(list), needle) => list.items.iter().any(|v| v.equals(needle)),
                (Val::Set(set), needle) => set.items.binary_search_by(|v| v.order(needle)).is_ok(),
                _ => return Err(self.wrong(at)),
            })),
            Builtin::Matches => {
                let (Val::Str(s), Val::Str(pattern)) = (&a, &b0) else {
                    return Err(self.wrong(at));
                };
                let Some(regex) = self.regex(pattern, span(1))? else {
                    return Err(self.fail(
                        span(1),
                        "E0609",
                        format!("`{pattern}` is not a valid regular expression"),
                    ));
                };
                self.charge(u64::try_from(s.len() / 64).unwrap_or(u64::MAX), at)?;
                Ok(Val::Bool(regex.is_match(s)))
            }
            Builtin::Lines => {
                let Val::Str(s) = &a else {
                    return Err(self.wrong(at));
                };
                let mut lines: Vec<&str> = s.split('\n').collect();
                if lines.last() == Some(&"") {
                    lines.pop();
                }
                let mut items = Vec::with_capacity(lines.len());
                for line in lines {
                    items.push(self.new_str(line.to_owned(), at)?);
                }
                self.new_list(items, at)
            }
            Builtin::Indent => {
                let (Val::Str(s), Val::Int(n)) = (&a, &b0) else {
                    return Err(self.wrong(at));
                };
                if !(0..=MAX_INDENT).contains(n) {
                    return Err(self.refinement_failed(
                        span(1),
                        format!("{n} is not in 0..={MAX_INDENT}"),
                        "the indentation of `indent` must be in 0..=64",
                    ));
                }
                let prefix = " ".repeat(usize::try_from(*n).unwrap_or(0));
                let lines: Vec<String> = s
                    .split('\n')
                    .map(|line| {
                        if line.is_empty() {
                            String::new()
                        } else {
                            format!("{prefix}{line}")
                        }
                    })
                    .collect();
                let len: usize = lines.iter().map(|l| l.len() + 1).sum();
                self.reserve_str(len, at)?;
                self.new_str(lines.join("\n"), at)
            }
            Builtin::Flatten => {
                let Val::List(list) = &a else {
                    return Err(self.wrong(at));
                };
                let len: usize = list.items.iter().map(|l| l.items().len()).sum();
                self.reserve(u64::try_from(len).unwrap_or(u64::MAX), at)?;
                let items = list
                    .items
                    .iter()
                    .flat_map(|l| l.items().iter().cloned())
                    .collect();
                self.new_list(items, at)
            }
            Builtin::Sort => {
                let mut items = a.items().to_vec();
                items.sort_by(Val::order);
                self.new_list(items, at)
            }
            Builtin::Unique => {
                let items = a.items();
                let mut order: Vec<usize> = (0..items.len()).collect();
                order.sort_by(|&x, &y| items[x].order(&items[y]).then(x.cmp(&y)));
                let mut keep = vec![false; items.len()];
                let mut previous: Option<usize> = None;
                for i in order {
                    if previous.is_none_or(|p| !items[p].equals(&items[i])) {
                        keep[i] = true;
                    }
                    previous = Some(i);
                }
                let unique = items
                    .iter()
                    .zip(keep)
                    .filter(|(_, k)| *k)
                    .map(|(v, _)| v.clone())
                    .collect();
                self.new_list(unique, at)
            }
            Builtin::Reverse => {
                let mut items = a.items().to_vec();
                items.reverse();
                self.new_list(items, at)
            }
            Builtin::Range => {
                let (Val::Int(start), Val::Int(end)) = (a, b0) else {
                    return Err(self.wrong(at));
                };
                if end <= start {
                    return self.new_list(Vec::new(), at);
                }
                let len = u64::try_from(i128::from(end) - i128::from(start)).unwrap_or(u64::MAX);
                self.reserve(len, at)?;
                self.new_list((start..end).map(Val::Int).collect(), at)
            }
            Builtin::First => Ok(a.items().first().cloned().unwrap_or(Val::None)),
            Builtin::Last => Ok(a.items().last().cloned().unwrap_or(Val::None)),
            Builtin::Get => Ok(match (&a, &b0) {
                (Val::List(list), Val::Int(i)) => usize::try_from(*i)
                    .ok()
                    .and_then(|i| list.items.get(i))
                    .cloned()
                    .unwrap_or(Val::None),
                (Val::Map(map), key) => map_get(map, key).unwrap_or(Val::None),
                _ => return Err(self.wrong(at)),
            }),
            Builtin::ToList => {
                let items = a.items().to_vec();
                self.new_list(items, at)
            }
            Builtin::ToSet => {
                let items = a.items().to_vec();
                self.new_set(items, at)
            }
            Builtin::Keys | Builtin::Values => {
                let Val::Map(map) = &a else {
                    return Err(self.wrong(at));
                };
                let items = map
                    .entries
                    .iter()
                    .map(|(k, v)| {
                        if b == Builtin::Keys {
                            k.clone()
                        } else {
                            v.clone()
                        }
                    })
                    .collect();
                self.new_list(items, at)
            }
            Builtin::HasKey => match &a {
                Val::Map(map) => Ok(Val::Bool(map_get(map, &b0).is_some())),
                _ => Err(self.wrong(at)),
            },
            Builtin::Map
            | Builtin::Filter
            | Builtin::Any
            | Builtin::All
            | Builtin::Fold
            | Builtin::MapValues
            | Builtin::FilterMap => Err(self.wrong(at)),
            Builtin::Union => {
                let (Val::Map(left), Val::Map(right)) = (&a, &b0) else {
                    return Err(self.wrong(at));
                };
                let mut entries: Vec<(Val, Val)> = left
                    .entries
                    .iter()
                    .filter(|(k, _)| map_get(right, k).is_none())
                    .cloned()
                    .collect();
                entries.extend(right.entries.iter().cloned());
                self.new_map(entries, at)
            }
            Builtin::Min | Builtin::Max => {
                let first_wins = a.order(&b0).is_le() == (b == Builtin::Min);
                Ok(if first_wins { a } else { b0 })
            }
            Builtin::Abs => match a {
                Val::Int(i) => i.checked_abs().map(Val::Int).ok_or_else(|| {
                    self.fail(at, "E0601", format!("integer overflow in `abs({i})`"))
                }),
                _ => Err(self.wrong(at)),
            },
        }
    }

    fn wrong(&self, at: SourceSpan) -> super::Stop {
        self.bug(
            at.file,
            at.span,
            "a built-in function got arguments of the wrong type",
        )
    }

    fn text_fn(&mut self, a: &Val, at: SourceSpan, f: impl Fn(&str) -> String) -> R<Val> {
        match a {
            Val::Str(s) => {
                let text = f(s);
                self.new_str(text, at)
            }
            _ => Err(self.wrong(at)),
        }
    }

    fn empty_argument(&self, at: SourceSpan, function: &str, param: &str) -> super::Stop {
        self.refinement_failed(
            at,
            "the string is empty".to_owned(),
            &format!("the argument `{param}` of `{function}` must not be empty"),
        )
    }

    /// Returns `E0803` for an argument of a built-in function that violates
    /// the refinement of its parameter.
    fn refinement_failed(&self, at: SourceSpan, detail: String, help: &str) -> super::Stop {
        self.stop(
            at,
            Diagnostic::error("E0803", at.span, "value violates a refinement")
                .with_label(Some(at.file), at.span, detail)
                .with_help(help),
        )
    }

    /// Returns the compiled regular expression that matches whole strings,
    /// or `None` when the pattern is invalid.
    pub(crate) fn regex(&mut self, pattern: &str, at: SourceSpan) -> R<Option<Rc<regex::Regex>>> {
        if let Some(found) = self.regexes.get(pattern) {
            return Ok(found.clone());
        }
        self.charge(u64::try_from(pattern.len()).unwrap_or(u64::MAX), at)?;
        let compiled = regex::RegexBuilder::new(&format!("\\A(?:{pattern})\\z"))
            .size_limit(1 << 20)
            .build()
            .ok()
            .map(Rc::new);
        self.regexes.insert(pattern.to_owned(), compiled.clone());
        Ok(compiled)
    }
}

/// Returns the length of a string in Unicode scalar values, or the number
/// of elements of a collection.
pub(crate) fn length(value: &Val) -> i64 {
    let len = match value {
        Val::Str(s) => s.chars().count(),
        Val::List(s) | Val::Set(s) => s.items.len(),
        Val::Map(m) => m.entries.len(),
        _ => 0,
    };
    i64::try_from(len).unwrap_or(i64::MAX)
}

/// Parses a decimal integer with an optional leading `-`.
fn parse_decimal(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

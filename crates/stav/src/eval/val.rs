//! Values while a program is evaluated.
//!
//! Evaluation works on [`Val`], which shares its parts through reference
//! counting so that copying a value is cheap, refers to declarations by
//! their identifiers, and can hold functions and parts. The final value is
//! converted to the public [`Value`] once merging is done.

use std::cmp::Ordering;
use std::ops::Range;
use std::rc::Rc;

use super::body::Pending;
use super::env::Env;
use super::merge::Seg;
use crate::check::model::UnitId;
use crate::check::types::{GlobalId, StructId, SumId, SymbolId, Types};
use crate::check::{FileId, ImportRoot};
use crate::span::Span;
use crate::value::Value;

/// A value during evaluation.
#[derive(Debug, Clone)]
pub(crate) enum Val {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Path(Rc<PathVal>),
    Symbol(Rc<SymbolVal>),
    Ref(Rc<RefVal>),
    List(Rc<Seq>),
    /// Elements in canonical order, without duplicates.
    Set(Rc<Seq>),
    /// Entries in canonical key order. Keys are strings or nullary
    /// variants.
    Map(Rc<MapVal>),
    Struct(Rc<StructVal>),
    /// A variant without payload.
    Tag(SumId, u32),
    /// A variant with payload.
    Variant(Rc<VariantVal>),
    Fn(Rc<FnVal>),
    Part(Rc<PartVal>),
}

/// A path literal resolved inside its import root.
#[derive(Debug)]
pub(crate) struct PathVal {
    pub(crate) root: usize,
    /// The normalized path inside the root, empty for the root itself.
    pub(crate) rel: String,
}

/// A value of a host symbol type, with the place that produced it.
#[derive(Debug)]
pub(crate) struct SymbolVal {
    pub(crate) ty: SymbolId,
    pub(crate) text: String,
    pub(crate) file: FileId,
    pub(crate) span: Span,
}

/// A reference label, with the place that wrote it.
#[derive(Debug)]
pub(crate) struct RefVal {
    pub(crate) target: StructId,
    pub(crate) label: String,
    pub(crate) file: FileId,
    pub(crate) span: Span,
}

/// The elements of a list or set.
#[derive(Debug)]
pub(crate) struct Seq {
    pub(crate) items: Vec<Val>,
    pub(crate) depth: u32,
}

#[derive(Debug)]
pub(crate) struct MapVal {
    pub(crate) entries: Vec<(Val, Val)>,
    pub(crate) depth: u32,
}

#[derive(Debug)]
pub(crate) struct StructVal {
    pub(crate) id: StructId,
    pub(crate) fields: Vec<Val>,
    /// Which fields were set, as opposed to taken from their declared
    /// defaults. Contributing the value contributes only the set ones.
    pub(crate) set: Vec<bool>,
    pub(crate) depth: u32,
}

#[derive(Debug)]
pub(crate) struct VariantVal {
    pub(crate) sum: SumId,
    pub(crate) index: u32,
    pub(crate) fields: Vec<Val>,
    /// Which payload fields were given, as opposed to defaulted.
    pub(crate) set: Vec<bool>,
    pub(crate) depth: u32,
}

/// A function value.
#[derive(Debug)]
pub(crate) enum FnVal {
    Global(GlobalId),
    /// A lambda with the bindings it captured.
    Lambda {
        unit: UnitId,
        node: usize,
        env: Env,
    },
}

/// A part value: the contributions of a `part { }` body, with paths
/// relative to the part's struct, and the parts included into it.
#[derive(Debug)]
pub(crate) struct PartVal {
    /// The identity of the part. Evaluation is pure, so the `part { }`
    /// expression that made a part and the environment it was evaluated in
    /// decide its contributions. Every evaluation with an equal
    /// environment gives the same identity.
    pub(crate) id: u32,
    pub(crate) contributions: Vec<Pending>,
    /// The parts included into the body, outer ones before the ones
    /// included into them.
    pub(crate) nested: Vec<Nested>,
}

/// The identity of an included part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PartId {
    /// A part file.
    File(FileId),
    /// A part value, by [`PartVal::id`].
    Value(u32),
}

/// A part included into the body of another part.
#[derive(Debug, Clone)]
pub(crate) struct Nested {
    /// The path of the including block, relative to the outer part.
    pub(crate) base: Vec<Seg>,
    pub(crate) part: PartId,
    /// The index of the nested part this one was included into, if any.
    pub(crate) parent: Option<usize>,
    /// The contributions of the outer part that this include made.
    pub(crate) range: Range<usize>,
}

/// Returns the depth of a collection holding `items`.
pub(crate) fn depth_of<'a>(items: impl IntoIterator<Item = &'a Val>) -> u32 {
    items
        .into_iter()
        .map(Val::depth)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// A value ordered by [`Val::order`], so that map keys can be found in
/// ordered collections. Keys are strings and variants without payload,
/// whose order agrees with [`Val::equals`].
#[derive(Debug, Clone)]
pub(crate) struct Key(pub(crate) Val);

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Key {}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.order(&other.0)
    }
}

impl Val {
    /// Returns how deeply collections nest inside the value.
    pub(crate) fn depth(&self) -> u32 {
        match self {
            Val::List(s) | Val::Set(s) => s.depth,
            Val::Map(m) => m.depth,
            Val::Struct(s) => s.depth,
            Val::Variant(v) => v.depth,
            _ => 0,
        }
    }

    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Val::None)
    }

    /// Returns whether two values are structurally equal. Symbols and
    /// references compare by their text, not by where they were written.
    pub(crate) fn equals(&self, other: &Val) -> bool {
        match (self, other) {
            (Val::None, Val::None) => true,
            (Val::Bool(a), Val::Bool(b)) => a == b,
            (Val::Int(a), Val::Int(b)) => a == b,
            #[allow(clippy::float_cmp, reason = "equality of values is exact")]
            (Val::Float(a), Val::Float(b)) => a == b,
            (Val::Str(a), Val::Str(b)) => a == b,
            (Val::Path(a), Val::Path(b)) => a.root == b.root && a.rel == b.rel,
            (Val::Symbol(a), Val::Symbol(b)) => a.ty == b.ty && a.text == b.text,
            (Val::Ref(a), Val::Ref(b)) => a.target == b.target && a.label == b.label,
            (Val::List(a), Val::List(b)) | (Val::Set(a), Val::Set(b)) => {
                all_equal(&a.items, &b.items)
            }
            (Val::Map(a), Val::Map(b)) => {
                a.entries.len() == b.entries.len()
                    && a.entries
                        .iter()
                        .zip(&b.entries)
                        .all(|((k1, v1), (k2, v2))| k1.equals(k2) && v1.equals(v2))
            }
            (Val::Struct(a), Val::Struct(b)) => a.id == b.id && all_equal(&a.fields, &b.fields),
            (Val::Tag(s1, i1), Val::Tag(s2, i2)) => s1 == s2 && i1 == i2,
            (Val::Variant(a), Val::Variant(b)) => {
                a.sum == b.sum && a.index == b.index && all_equal(&a.fields, &b.fields)
            }
            _ => false,
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Val::None => 0,
            Val::Bool(_) => 1,
            Val::Int(_) => 2,
            Val::Float(_) => 3,
            Val::Str(_) => 4,
            Val::Path(_) => 5,
            Val::Symbol(_) => 6,
            Val::Ref(_) => 7,
            Val::Tag(..) => 8,
            Val::Variant(_) => 9,
            Val::List(_) => 10,
            Val::Set(_) => 11,
            Val::Map(_) => 12,
            Val::Struct(_) => 13,
            Val::Fn(_) => 14,
            Val::Part(_) => 15,
        }
    }

    /// Returns the canonical order of two values: numbers by value, strings
    /// bytewise, variants in declaration order, and collections
    /// lexicographically. Sets, map keys and `sort` use this order.
    pub(crate) fn order(&self, other: &Val) -> Ordering {
        match (self, other) {
            (Val::Bool(a), Val::Bool(b)) => a.cmp(b),
            (Val::Int(a), Val::Int(b)) => a.cmp(b),
            (Val::Float(a), Val::Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Val::Str(a), Val::Str(b)) => a.as_bytes().cmp(b.as_bytes()),
            (Val::Path(a), Val::Path(b)) => (a.root, &a.rel).cmp(&(b.root, &b.rel)),
            (Val::Symbol(a), Val::Symbol(b)) => (a.ty, &a.text).cmp(&(b.ty, &b.text)),
            (Val::Ref(a), Val::Ref(b)) => (a.target, &a.label).cmp(&(b.target, &b.label)),
            (Val::Tag(s1, i1), Val::Tag(s2, i2)) => (s1, i1).cmp(&(s2, i2)),
            (Val::Variant(a), Val::Variant(b)) => (a.sum, a.index)
                .cmp(&(b.sum, b.index))
                .then_with(|| order_all(&a.fields, &b.fields)),
            (Val::List(a), Val::List(b)) | (Val::Set(a), Val::Set(b)) => {
                order_all(&a.items, &b.items)
            }
            (Val::Map(a), Val::Map(b)) => {
                for ((k1, v1), (k2, v2)) in a.entries.iter().zip(&b.entries) {
                    let found = k1.order(k2).then_with(|| v1.order(v2));
                    if found != Ordering::Equal {
                        return found;
                    }
                }
                a.entries.len().cmp(&b.entries.len())
            }
            (Val::Struct(a), Val::Struct(b)) => {
                a.id.cmp(&b.id)
                    .then_with(|| order_all(&a.fields, &b.fields))
            }
            _ => self.rank().cmp(&other.rank()),
        }
    }

    /// Returns the text a value has in interpolation and `toStr`: strings
    /// as they are, numbers in decimal, floats in their shortest form that
    /// reads back as the same number, variant names, symbol texts and
    /// reference labels.
    pub(crate) fn text(&self, types: &Types) -> String {
        match self {
            Val::Str(s) => s.to_string(),
            Val::Int(i) => i.to_string(),
            Val::Float(f) => format!("{f:?}"),
            Val::Bool(b) => b.to_string(),
            Val::Tag(sum, index) => types
                .sum(*sum)
                .variants
                .get(*index as usize)
                .map_or_else(String::new, |v| v.name.clone()),
            Val::Symbol(s) => s.text.clone(),
            Val::Ref(r) => r.label.clone(),
            _ => String::new(),
        }
    }

    /// Returns the elements of a list or set.
    pub(crate) fn items(&self) -> &[Val] {
        match self {
            Val::List(s) | Val::Set(s) => &s.items,
            _ => &[],
        }
    }

    /// Converts the value to its public form.
    pub(crate) fn to_value(&self, types: &Types, roots: &[ImportRoot]) -> Value {
        let fields = |defs: Vec<&str>, values: &[Val]| -> Vec<(String, Value)> {
            defs.into_iter()
                .zip(values)
                .map(|(name, v)| (name.to_owned(), v.to_value(types, roots)))
                .collect()
        };
        match self {
            Val::None | Val::Fn(_) | Val::Part(_) => Value::None,
            Val::Bool(b) => Value::Bool(*b),
            Val::Int(i) => Value::Int(*i),
            Val::Float(f) => Value::Float(*f),
            Val::Str(s) => Value::Str(s.to_string()),
            Val::Path(p) => {
                let root = roots
                    .get(p.root)
                    .map(|r| r.path().to_path_buf())
                    .unwrap_or_default();
                Value::Path(if p.rel.is_empty() {
                    root
                } else {
                    root.join(&p.rel)
                })
            }
            Val::Symbol(s) => Value::Symbol {
                ty: types
                    .symbols
                    .get(s.ty.index())
                    .map_or_else(String::new, |d| d.name.clone()),
                text: s.text.clone(),
            },
            Val::Ref(r) => Value::Ref(r.label.clone()),
            Val::List(s) => Value::List(s.items.iter().map(|v| v.to_value(types, roots)).collect()),
            Val::Set(s) => Value::Set(s.items.iter().map(|v| v.to_value(types, roots)).collect()),
            Val::Map(m) => Value::Map(
                m.entries
                    .iter()
                    .map(|(k, v)| (k.text(types), v.to_value(types, roots)))
                    .collect(),
            ),
            Val::Struct(s) => {
                let def = types.structure(s.id);
                Value::Struct {
                    name: def.name.clone(),
                    fields: fields(
                        def.fields.iter().map(|f| f.name.as_str()).collect(),
                        &s.fields,
                    ),
                }
            }
            Val::Tag(sum, _) => {
                let def = types.sum(*sum);
                Value::Variant {
                    ty: def.name.clone(),
                    name: self.text(types),
                    fields: None,
                }
            }
            Val::Variant(v) => {
                let def = types.sum(v.sum);
                let variant = def.variants.get(v.index as usize);
                let names = variant
                    .and_then(|v| v.fields.as_ref())
                    .map(|f| f.iter().map(|p| p.name.as_str()).collect())
                    .unwrap_or_default();
                Value::Variant {
                    ty: def.name.clone(),
                    name: variant.map_or_else(String::new, |v| v.name.clone()),
                    fields: Some(fields(names, &v.fields)),
                }
            }
        }
    }
}

fn all_equal(a: &[Val], b: &[Val]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.equals(y))
}

fn order_all(a: &[Val], b: &[Val]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let found = x.order(y);
        if found != Ordering::Equal {
            return found;
        }
    }
    a.len().cmp(&b.len())
}

/// Sorts values into canonical order and removes duplicates.
pub(crate) fn canonical(mut items: Vec<Val>) -> Vec<Val> {
    items.sort_by(Val::order);
    items.dedup_by(|a, b| a.equals(b));
    items
}

//! The local bindings in scope while an expression is evaluated.

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

use super::val::{FnVal, Val};
use crate::check::model::{LocalId, UnitId};
use crate::check::types::TypeId;

/// The local bindings in scope: a persistent map from locals to what
/// they are bound to. Binding a local returns a new environment and leaves
/// the old one unchanged, so closures and deferred contributions capture
/// an environment by sharing it.
///
/// The map is a trie over the bits of the local, 16 children per node, so
/// a lookup visits at most eight nodes however many locals are bound. A
/// chain of bindings made a body with N `let` bindings that each read the
/// first take O(N²) work: 50 000 such bindings took minutes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Env(Option<Rc<Node>>);

/// The bits of a local that select a child of a node.
const BITS: u32 = 4;

/// The children of a node.
const WIDTH: usize = 1 << BITS;

#[derive(Debug)]
enum Node {
    /// The children for the next bits of a local, which start at bit
    /// `shift`.
    Branch {
        shift: u32,
        children: [Option<Rc<Node>>; WIDTH],
    },
    Leaf(Slot),
}

/// What a local is bound to.
#[derive(Debug, Clone)]
pub(crate) enum Slot {
    Ready(Val),
    /// A `let` statement whose value reads `self`, evaluated when a
    /// deferred contribution uses it.
    Late(Rc<LateLet>),
}

/// The value of a `let` statement that reads `self`.
#[derive(Debug)]
pub(crate) struct LateLet {
    pub(crate) unit: UnitId,
    pub(crate) node: usize,
    pub(crate) env: Env,
    /// The annotated type, whose refinements the value must satisfy.
    pub(crate) annotation: Option<TypeId>,
}

/// Returns `env` with `id` bound to `slot`, in place of any earlier
/// binding of `id`.
pub(crate) fn bind(env: &Env, id: LocalId, slot: Slot) -> Env {
    let key = id.0;
    let mut root = env.0.clone();
    let mut shift = match root.as_deref() {
        Some(Node::Branch { shift, .. }) => *shift,
        _ => 0,
    };
    while key.checked_shr(shift + BITS).is_some_and(|rest| rest != 0) {
        let mut children: [Option<Rc<Node>>; WIDTH] = Default::default();
        children[0] = root.take();
        shift += BITS;
        root = Some(Rc::new(Node::Branch { shift, children }));
    }
    Env(Some(insert(root.as_ref(), shift, key, slot)))
}

/// Returns a copy of the branch `node` at bit `shift` with `key` bound to
/// `slot`.
fn insert(node: Option<&Rc<Node>>, shift: u32, key: u32, slot: Slot) -> Rc<Node> {
    let mut children: [Option<Rc<Node>>; WIDTH] = match node.map(Rc::as_ref) {
        Some(Node::Branch { children, .. }) => children.clone(),
        _ => Default::default(),
    };
    let index = ((key >> shift) as usize) & (WIDTH - 1);
    children[index] = Some(if shift == 0 {
        Rc::new(Node::Leaf(slot))
    } else {
        insert(children[index].as_ref(), shift - BITS, key, slot)
    });
    Rc::new(Node::Branch { shift, children })
}

/// Returns the binding of `id`.
pub(crate) fn lookup(env: &Env, id: LocalId) -> Option<&Slot> {
    let key = id.0;
    let mut node = env.0.as_deref()?;
    if let Node::Branch { shift, .. } = node
        && key.checked_shr(shift + BITS).is_some_and(|rest| rest != 0)
    {
        return None;
    }
    loop {
        match node {
            Node::Branch { shift, children } => {
                node = children[((key >> shift) as usize) & (WIDTH - 1)].as_deref()?;
            }
            Node::Leaf(slot) => return Some(slot),
        }
    }
}

/// Two things whose order [`identity`] compares.
enum Pair<'a> {
    Env(&'a Env, &'a Env),
    Node(&'a Rc<Node>, &'a Rc<Node>),
    Val(&'a Val, &'a Val),
}

/// Returns a total order of environments that holds two of them equal
/// exactly when they bind the same locals to equal values, and adds the
/// number of steps it took to `work`.
///
/// Values compare as [`Val::order`] compares them, functions by where they
/// were created and by the environments they captured, and parts by their
/// identities. Bindings that read `self` compare by their expression and
/// environment. The walk keeps its own stack, because functions can
/// capture functions to any depth.
pub(crate) fn identity(a: &Env, b: &Env, work: &Cell<u64>) -> Ordering {
    let mut stack = vec![Pair::Env(a, b)];
    while let Some(pair) = stack.pop() {
        work.set(work.get().saturating_add(1));
        let found = match pair {
            Pair::Env(x, y) => match (&x.0, &y.0) {
                (Some(x), Some(y)) => {
                    stack.push(Pair::Node(x, y));
                    Ordering::Equal
                }
                (x, y) => x.is_some().cmp(&y.is_some()),
            },
            Pair::Node(x, y) if Rc::ptr_eq(x, y) => Ordering::Equal,
            Pair::Node(x, y) => match (x.as_ref(), y.as_ref()) {
                (
                    Node::Branch {
                        shift: s,
                        children: xs,
                    },
                    Node::Branch {
                        shift: t,
                        children: ys,
                    },
                ) => {
                    let mut order = s.cmp(t);
                    for (x, y) in xs.iter().zip(ys) {
                        if order != Ordering::Equal {
                            break;
                        }
                        order = x.is_some().cmp(&y.is_some());
                    }
                    if order == Ordering::Equal {
                        let pairs = xs.iter().zip(ys).rev();
                        stack.extend(
                            pairs.filter_map(|(x, y)| Some(Pair::Node(x.as_ref()?, y.as_ref()?))),
                        );
                    }
                    order
                }
                (Node::Leaf(x), Node::Leaf(y)) => match (x, y) {
                    (Slot::Ready(x), Slot::Ready(y)) => {
                        stack.push(Pair::Val(x, y));
                        Ordering::Equal
                    }
                    (Slot::Late(x), Slot::Late(y)) => {
                        let order = (x.unit, x.node).cmp(&(y.unit, y.node));
                        if order == Ordering::Equal {
                            stack.push(Pair::Env(&x.env, &y.env));
                        }
                        order
                    }
                    (x, y) => matches!(x, Slot::Late(_)).cmp(&matches!(y, Slot::Late(_))),
                },
                (x, y) => matches!(x, Node::Leaf(_)).cmp(&matches!(y, Node::Leaf(_))),
            },
            Pair::Val(x, y) => compare_val(x, y, &mut stack),
        };
        if found != Ordering::Equal {
            return found;
        }
    }
    Ordering::Equal
}

/// Compares two values as far as they can be without looking inside them,
/// and pushes the pairs of their parts that decide the rest.
fn compare_val<'a>(x: &'a Val, y: &'a Val, stack: &mut Vec<Pair<'a>>) -> Ordering {
    let mut pairs = |xs: &'a [Val], ys: &'a [Val]| {
        let order = xs.len().cmp(&ys.len());
        if order == Ordering::Equal {
            stack.extend(xs.iter().zip(ys).rev().map(|(x, y)| Pair::Val(x, y)));
        }
        order
    };
    match (x, y) {
        (Val::List(a), Val::List(b)) | (Val::Set(a), Val::Set(b)) => {
            if Rc::ptr_eq(a, b) {
                return Ordering::Equal;
            }
            pairs(&a.items, &b.items)
        }
        (Val::Struct(a), Val::Struct(b)) => {
            a.id.cmp(&b.id).then_with(|| pairs(&a.fields, &b.fields))
        }
        (Val::Variant(a), Val::Variant(b)) => (a.sum, a.index)
            .cmp(&(b.sum, b.index))
            .then_with(|| pairs(&a.fields, &b.fields)),
        (Val::Map(a), Val::Map(b)) => {
            let order = a.entries.len().cmp(&b.entries.len());
            if order == Ordering::Equal && !Rc::ptr_eq(a, b) {
                for ((k1, v1), (k2, v2)) in a.entries.iter().zip(&b.entries).rev() {
                    stack.push(Pair::Val(v1, v2));
                    stack.push(Pair::Val(k1, k2));
                }
            }
            order
        }
        (Val::Fn(a), Val::Fn(b)) => match (a.as_ref(), b.as_ref()) {
            _ if Rc::ptr_eq(a, b) => Ordering::Equal,
            (FnVal::Global(g), FnVal::Global(h)) => g.cmp(h),
            (
                FnVal::Lambda { unit, node, env },
                FnVal::Lambda {
                    unit: u,
                    node: n,
                    env: e,
                },
            ) => {
                let order = (unit, node).cmp(&(u, n));
                if order == Ordering::Equal {
                    stack.push(Pair::Env(env, e));
                }
                order
            }
            (a, _) => {
                if matches!(a, FnVal::Global(_)) {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
        },
        (Val::Part(a), Val::Part(b)) => a.id.cmp(&b.id),
        _ => x.order(y),
    }
}

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;

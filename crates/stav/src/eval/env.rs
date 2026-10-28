//! The local bindings in scope while an expression is evaluated.

use std::rc::Rc;

use super::val::Val;
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

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;

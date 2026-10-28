//! The local bindings in scope while an expression is evaluated.

use std::rc::Rc;

use super::val::Val;
use crate::check::model::{LocalId, UnitId};
use crate::check::types::TypeId;

/// The local bindings in scope, innermost first. Closures and deferred
/// contributions capture an environment by sharing it.
pub(crate) type Env = Option<Rc<Binding>>;

/// One local binding and the bindings around it.
#[derive(Debug)]
pub(crate) struct Binding {
    pub(crate) id: LocalId,
    pub(crate) slot: Slot,
    pub(crate) next: Env,
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

/// Returns `env` with one more binding.
pub(crate) fn bind(env: &Env, id: LocalId, slot: Slot) -> Env {
    Some(Rc::new(Binding {
        id,
        slot,
        next: env.clone(),
    }))
}

/// Returns the innermost binding of `id`.
pub(crate) fn lookup(env: &Env, id: LocalId) -> Option<&Slot> {
    let mut current = env.as_deref();
    while let Some(binding) = current {
        if binding.id == id {
            return Some(&binding.slot);
        }
        current = binding.next.as_deref();
    }
    None
}

impl Drop for Binding {
    /// Drops long chains of bindings one by one, so that a body with many
    /// `let` statements cannot exhaust the stack.
    fn drop(&mut self) {
        let mut next = self.next.take();
        while let Some(rc) = next {
            match Rc::try_unwrap(rc) {
                Ok(mut binding) => next = binding.next.take(),
                Err(_) => break,
            }
        }
    }
}

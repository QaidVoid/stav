//! The call graph: which declarations may evaluate which others.

use std::collections::BTreeMap;

use super::Checker;
use super::decls::GlobalKind;
use super::graph;
use super::load::FileId;
use super::types::{GlobalId, StructId, SumId};
use crate::span::Span;

/// A declaration whose evaluation may evaluate other declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Node {
    /// A top-level function or `let`.
    Global(GlobalId),
    /// The field defaults of a struct, evaluated when a complete value of
    /// the struct is built.
    StructDefaults(StructId),
    /// The payload defaults of a sum type, evaluated when a variant is
    /// built.
    SumDefaults(SumId),
}

/// The edges of the call graph, each with the place of its first
/// occurrence.
#[derive(Debug, Clone, Default)]
pub(crate) struct CallGraph {
    pub(crate) edges: BTreeMap<(Node, Node), (FileId, Span)>,
}

impl CallGraph {
    /// Records that evaluating `from` may evaluate `to`.
    pub(crate) fn depend(&mut self, from: Node, to: Node, file: FileId, span: Span) {
        self.edges.entry((from, to)).or_insert((file, span));
    }
}

impl Checker<'_> {
    fn node_index(&self, node: Node) -> usize {
        let (globals, structs) = (self.globals.len(), self.types.structs.len());
        match node {
            Node::Global(g) => g.index(),
            Node::StructDefaults(s) => globals + s.index(),
            Node::SumDefaults(s) => globals + structs + s.index(),
        }
    }

    /// Orders the top-level `let` declarations so that each comes after
    /// everything it uses.
    pub(super) fn order_lets(&mut self) {
        let total = self.globals.len() + self.types.structs.len() + self.types.sums.len();
        let mut adj = vec![Vec::new(); total];
        for &(from, to) in self.calls.edges.keys() {
            adj[self.node_index(from)].push(self.node_index(to));
        }
        for list in &mut adj {
            list.sort_unstable();
            list.dedup();
        }
        self.let_order = graph::components(&adj)
            .into_iter()
            .flatten()
            .filter(|&i| i < self.globals.len())
            .map(GlobalId::new)
            .filter(|g| matches!(self.globals[g.index()].kind, GlobalKind::Let(_)))
            .collect();
    }
}

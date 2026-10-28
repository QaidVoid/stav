//! The call graph: no function may call itself, directly or through other
//! functions, top-level `let` values or the defaults of types.

use std::collections::BTreeMap;

use super::Checker;
use super::decls::GlobalKind;
use super::graph;
use super::load::FileId;
use super::types::{GlobalId, StructId, SumId};
use crate::diagnostic::Diagnostic;
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

    fn index_node(&self, index: usize) -> Node {
        let (globals, structs) = (self.globals.len(), self.types.structs.len());
        if index < globals {
            Node::Global(GlobalId::new(index))
        } else if index < globals + structs {
            Node::StructDefaults(StructId::new(index - globals))
        } else {
            Node::SumDefaults(SumId::new(index - globals - structs))
        }
    }

    fn node_name(&self, node: Node) -> String {
        match node {
            Node::Global(g) => format!("`{}`", self.globals[g.index()].name),
            Node::StructDefaults(s) => {
                format!("the defaults of `{}`", self.types.structure(s).name)
            }
            Node::SumDefaults(s) => format!("the defaults of `{}`", self.types.sum(s).name),
        }
    }

    /// Reports cycles of the call graph and orders the top-level `let`
    /// declarations so that each comes after everything it uses.
    pub(super) fn check_totality(&mut self) {
        let total = self.globals.len() + self.types.structs.len() + self.types.sums.len();
        let mut adj = vec![Vec::new(); total];
        for &(from, to) in self.calls.edges.keys() {
            adj[self.node_index(from)].push(self.node_index(to));
        }
        for list in &mut adj {
            list.sort_unstable();
            list.dedup();
        }
        for cycle in graph::cycles(&adj) {
            let nodes: Vec<Node> = cycle.iter().map(|&i| self.index_node(i)).collect();
            let from = nodes[0];
            let to = nodes.get(1).copied().unwrap_or(from);
            let Some(&(file, span)) = self.calls.edges.get(&(from, to)) else {
                continue;
            };
            let mut chain: Vec<String> = nodes.iter().map(|n| self.node_name(*n)).collect();
            chain.push(self.node_name(from));
            let note = format!("cycle: {}", chain.join(" -> "));
            let is_let = |n: &Node| match n {
                Node::Global(g) => matches!(self.globals[g.index()].kind, GlobalKind::Let(_)),
                _ => false,
            };
            let diagnostic = if let Some(n) = nodes.iter().find(|n| is_let(n)) {
                Diagnostic::error(
                    "E0305",
                    span,
                    format!("{} depends on itself", self.node_name(*n)),
                )
                .with_note(note)
            } else {
                let message = if nodes.len() == 1 {
                    format!("{} calls itself", self.node_name(from))
                } else {
                    "functions may not call each other in a cycle".to_owned()
                };
                Diagnostic::error("E0610", span, message)
                    .with_note(note)
                    .with_note("evaluation must terminate, so functions cannot recurse")
                    .with_help("iterate with `fold` over `range(...)`, for example `fold(range(1, n + 1), 1, fn(acc: Int, i: Int) = acc * i)`")
            };
            self.error(file, diagnostic);
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

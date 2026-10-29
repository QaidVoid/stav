//! Merging contributions into the final value.
//!
//! Contributions are stored in a trie keyed by the segments of their
//! paths. Merging runs in two phases. The structure phase walks the trie
//! from the root with the type of every attribute, adds the declared
//! defaults of every struct instance, decides which optional attributes are
//! present and which variant each tagged sum selects, and picks the
//! winning contributions of every other attribute by priority. Deferred
//! contributions take part with their priorities only. The `self`
//! dependencies of the winners are then checked for cycles over block
//! instances. The finalize phase computes each attribute on demand and
//! memoizes it, evaluating deferred contributions after the attributes
//! they read.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::body::{Contrib, Deferred, Kind, Pending, Prio};
use super::call::Building;
use super::env::Env;
use super::val::{Key, Val};
use super::{Eval, MAX_VALUE_DEPTH, R, Stop};
use crate::check::FileId;
use crate::check::graph;
use crate::check::model::SelfRead;
use crate::check::types::{StructId, SumId, Type, TypeId};
use crate::diagnostic::{Diagnostic, FileDiagnostic};
use crate::span::{SourceSpan, Span};
use crate::value::AttrPath;

/// A node of the contribution trie: one attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NodeId(u32);

impl NodeId {
    pub(crate) const ROOT: NodeId = NodeId(0);

    fn index(self) -> usize {
        self.0 as usize
    }
}

/// One step of an attribute path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Seg {
    /// A struct field, or a payload field below a variant.
    Field(u32),
    /// A map key: the string, or the variant name of a sum key.
    Key(Rc<str>),
    /// The payload of a variant of a tagged sum.
    Variant(u32),
}

/// What `self` refers to at one depth.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Frame {
    /// A block instance of the merged value.
    Node(NodeId),
    /// A struct value being built, by its index on the building stack.
    Building(usize),
}

/// Values computed once per evaluation.
#[derive(Debug, Default)]
pub(crate) struct Caches {
    pub(crate) building: Vec<Building>,
    /// The `self` reads of each field default.
    pub(crate) reads: BTreeMap<(StructId, usize), Rc<Vec<SelfRead>>>,
    pub(crate) complete: BTreeMap<(StructId, usize), Val>,
    pub(crate) payload: BTreeMap<(SumId, usize, usize), Val>,
    /// The contributions of field defaults that do not read `self`,
    /// relative to the field.
    pub(crate) defaults: BTreeMap<(StructId, usize), Rc<Vec<Pending>>>,
}

#[derive(Debug, Clone)]
enum Shape {
    /// Not reached by the structure phase.
    Open,
    /// An option that is `none`.
    Absent,
    /// A struct or map that nothing sets.
    Unset,
    /// Contributions that conflict, already reported.
    Conflict,
    Struct(StructId),
    /// A map with the given key type.
    Map(TypeId),
    /// A sum selected by tagged blocks, with the selected variant once it
    /// is known.
    Tagged(Option<u32>),
    /// The payload of the selected variant of a tagged sum.
    Payload(SumId, u32),
    Leaf,
}

#[derive(Debug, Clone)]
enum State {
    Open,
    Busy,
    Done(Val),
    Missing,
}

#[derive(Debug)]
struct Node {
    parent: Option<NodeId>,
    seg: Option<Seg>,
    depth: u32,
    children: BTreeMap<Seg, NodeId>,
    contribs: Vec<u32>,
    ty: TypeId,
    shape: Shape,
    winners: Vec<u32>,
    /// Deferred contributions to a struct, map, option or tagged sum,
    /// which must be evaluated before anything inside it is final.
    pending: Vec<u32>,
    defaults: bool,
    state: State,
}

impl Node {
    fn new(parent: Option<NodeId>, seg: Option<Seg>, depth: u32) -> Self {
        Self {
            parent,
            seg,
            depth,
            children: BTreeMap::new(),
            contribs: Vec::new(),
            ty: TypeId::ERROR,
            shape: Shape::Open,
            winners: Vec::new(),
            pending: Vec::new(),
            defaults: false,
            state: State::Open,
        }
    }
}

/// The contribution trie.
#[derive(Debug, Default)]
pub(crate) struct Merge {
    pub(crate) contribs: Vec<Contrib>,
    nodes: Vec<Node>,
}

impl Merge {
    fn node(&self, n: NodeId) -> &Node {
        &self.nodes[n.index()]
    }

    fn node_mut(&mut self, n: NodeId) -> &mut Node {
        &mut self.nodes[n.index()]
    }

    pub(crate) fn child(&self, n: NodeId, seg: &Seg) -> Option<NodeId> {
        self.nodes.get(n.index())?.children.get(seg).copied()
    }

    fn child_or_new(&mut self, n: NodeId, seg: Seg) -> NodeId {
        if let Some(found) = self.child(n, &seg) {
            return found;
        }
        let id = NodeId(u32::try_from(self.nodes.len()).unwrap_or(u32::MAX));
        let depth = self.node(n).depth + 1;
        self.nodes
            .push(Node::new(Some(n), Some(seg.clone()), depth));
        self.node_mut(n).children.insert(seg, id);
        id
    }

    fn insert(&mut self, base: NodeId, path: &[Seg], contrib: Contrib) -> NodeId {
        let mut n = base;
        for seg in path {
            n = self.child_or_new(n, seg.clone());
        }
        let id = u32::try_from(self.contribs.len()).unwrap_or(u32::MAX);
        self.contribs.push(contrib);
        self.node_mut(n).contribs.push(id);
        n
    }

    fn path(&self, n: NodeId) -> Vec<Seg> {
        let mut segs = Vec::new();
        let mut current = Some(n);
        while let Some(c) = current {
            let node = self.node(c);
            if let Some(seg) = &node.seg {
                segs.push(seg.clone());
            }
            current = node.parent;
        }
        segs.reverse();
        segs
    }

    fn resolve(&self, path: &[Seg]) -> Option<NodeId> {
        let mut n = NodeId::ROOT;
        for seg in path {
            n = self.child(n, seg)?;
        }
        Some(n)
    }

    fn contrib(&self, id: u32) -> &Contrib {
        &self.contribs[id as usize]
    }

    /// Returns whether a contribution exists at or under `n`.
    fn is_set(&self, n: NodeId) -> bool {
        let mut stack = vec![n];
        while let Some(c) = stack.pop() {
            let node = self.node(c);
            if node
                .contribs
                .iter()
                .any(|&id| !matches!(self.contrib(id).kind, Kind::Spent))
            {
                return true;
            }
            stack.extend(node.children.values().copied());
        }
        false
    }

    /// Returns the contributions under `n`, not at it.
    fn below(&self, n: NodeId) -> Vec<u32> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.node(n).children.values().copied().collect();
        while let Some(c) = stack.pop() {
            let node = self.node(c);
            out.extend(
                node.contribs
                    .iter()
                    .copied()
                    .filter(|&id| !matches!(self.contrib(id).kind, Kind::Spent)),
            );
            stack.extend(node.children.values().copied());
        }
        out.sort_unstable();
        out
    }

    /// Returns the nodes the structure phase reached from `top`, `top`
    /// first, each before the nodes inside it.
    fn reached(&self, top: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![top];
        while let Some(n) = stack.pop() {
            out.push(n);
            let mut inner = self.inside(n);
            inner.reverse();
            stack.extend(inner);
        }
        out
    }

    /// Returns the nodes whose values make up the value of `n`.
    fn inside(&self, n: NodeId) -> Vec<NodeId> {
        let node = self.node(n);
        match node.shape {
            Shape::Struct(_) | Shape::Payload(..) => node
                .children
                .iter()
                .filter(|(seg, _)| matches!(seg, Seg::Field(_)))
                .map(|(_, &c)| c)
                .collect(),
            Shape::Map(..) => node
                .children
                .iter()
                .filter(|(seg, _)| matches!(seg, Seg::Key(_)))
                .map(|(_, &c)| c)
                .collect(),
            Shape::Tagged(Some(v)) => self.child(n, &Seg::Variant(v)).into_iter().collect(),
            _ => Vec::new(),
        }
    }
}

/// Whether an optional attribute is present.
enum Presence {
    Absent,
    Present,
    Conflict(Vec<u32>),
}

impl<'c> Eval<'c> {
    /// Builds the trie from the contributions of every body, runs the
    /// structure phase and checks the `self` dependencies of the winners.
    pub(crate) fn merge_all(&mut self, pending: Vec<Pending>) -> R<Val> {
        self.merge.nodes.push(Node::new(None, None, 0));
        for p in pending {
            if p.path.len() > MAX_VALUE_DEPTH as usize {
                let at = p.contrib.at();
                return Err(self.fail(
                    at,
                    "E0612",
                    format!("an attribute path is longer than {MAX_VALUE_DEPTH} steps"),
                ));
            }
            self.merge.insert(NodeId::ROOT, &p.path, p.contrib);
        }
        self.structure(NodeId::ROOT, self.c.root_ty)?;
        if !self.errors.is_empty() {
            return Ok(Val::None);
        }
        let order = self.check_cycles(NodeId::ROOT)?;
        if !self.errors.is_empty() {
            return Ok(Val::None);
        }
        for n in order {
            let node = self.merge.node(n);
            let deferred = !node.pending.is_empty()
                || node
                    .winners
                    .iter()
                    .any(|&id| matches!(self.merge.contrib(id).kind, Kind::Deferred(_)));
            if deferred {
                self.finalize(n)?;
            }
        }
        self.finalize(NodeId::ROOT)
    }

    /// Returns a place for diagnostics about `n`: its first contribution,
    /// or the first one inside it, or the first one of an enclosing node.
    pub(crate) fn node_at(&self, n: NodeId) -> SourceSpan {
        let merge = &self.merge;
        let first = |ids: &[u32]| {
            ids.iter()
                .map(|&id| merge.contrib(id))
                .find(|c| c.prio != Prio::Declared)
                .or_else(|| ids.first().map(|&id| merge.contrib(id)))
                .map(Contrib::at)
        };
        if let Some(at) = first(&merge.node(n).contribs) {
            return at;
        }
        if n != NodeId::ROOT
            && let Some(at) = first(&merge.below(n))
        {
            return at;
        }
        let mut current = merge.node(n).parent;
        while let Some(c) = current {
            if let Some(at) = first(&merge.node(c).contribs) {
                return at;
            }
            current = merge.node(c).parent;
        }
        SourceSpan {
            file: self
                .c
                .entries
                .first()
                .map_or(FileId::new(0), |&u| self.unit_file(u)),
            span: Span { start: 0, end: 0 },
        }
    }

    fn structure(&mut self, n: NodeId, ty: TypeId) -> R<()> {
        let at = self.node_at(n);
        if self.merge.node(n).depth > MAX_VALUE_DEPTH {
            return Err(self.fail(
                at,
                "E0612",
                format!("a value nests more than {MAX_VALUE_DEPTH} levels deep"),
            ));
        }
        self.enter(at)?;
        let result = self.structure_inner(n, ty);
        self.leave();
        result
    }

    fn structure_inner(&mut self, n: NodeId, ty: TypeId) -> R<()> {
        {
            let node = self.merge.node_mut(n);
            node.ty = ty;
            node.winners.clear();
            node.pending.clear();
        }
        let types = &self.c.types;
        let option = types.unwrap_option(ty).is_some();
        let inner = types.unwrap_option(ty).unwrap_or_else(|| types.peel(ty));
        if option {
            match self.presence(n) {
                Presence::Absent => {
                    self.merge.node_mut(n).shape = Shape::Absent;
                    return Ok(());
                }
                Presence::Conflict(ids) => {
                    self.conflict(n, &ids, "E0501");
                    self.merge.node_mut(n).shape = Shape::Conflict;
                    return Ok(());
                }
                Presence::Present => {}
            }
        }
        let deferred: Vec<u32> = self
            .merge
            .node(n)
            .contribs
            .iter()
            .copied()
            .filter(|&id| matches!(self.merge.contrib(id).kind, Kind::Deferred(_)))
            .collect();
        match self.c.types.get(inner).clone() {
            Type::Struct(s) => {
                if !option && n != NodeId::ROOT && !self.merge.is_set(n) {
                    self.merge.node_mut(n).shape = Shape::Unset;
                    return Ok(());
                }
                self.merge.node_mut(n).pending = deferred;
                self.merge.node_mut(n).shape = Shape::Struct(s);
                self.add_defaults(n, s)?;
                let fields: Vec<TypeId> = self
                    .c
                    .types
                    .structure(s)
                    .fields
                    .iter()
                    .map(|f| f.ty)
                    .collect();
                for (i, field_ty) in fields.into_iter().enumerate() {
                    let child = self.merge.child_or_new(n, field_seg(i));
                    self.structure(child, field_ty)?;
                }
            }
            Type::Map(k, v) => {
                if !option && !self.merge.is_set(n) {
                    self.merge.node_mut(n).shape = Shape::Unset;
                    return Ok(());
                }
                self.merge.node_mut(n).pending = deferred;
                self.merge.node_mut(n).shape = Shape::Map(k);
                let children: Vec<NodeId> = self
                    .merge
                    .node(n)
                    .children
                    .iter()
                    .filter(|(seg, _)| matches!(seg, Seg::Key(_)))
                    .map(|(_, &c)| c)
                    .collect();
                for child in children {
                    self.structure(child, v)?;
                }
            }
            Type::Sum(s) if self.has_selection(n) => self.tagged(n, s, deferred)?,
            _ => {
                let candidates: Vec<u32> = self
                    .merge
                    .node(n)
                    .contribs
                    .iter()
                    .copied()
                    .filter(|&id| match &self.merge.contrib(id).kind {
                        Kind::Value(Val::None) => !option,
                        Kind::Value(_) | Kind::Deferred(_) => true,
                        _ => false,
                    })
                    .collect();
                let top = candidates
                    .iter()
                    .map(|&id| self.merge.contrib(id).prio)
                    .max();
                let mut winners: Vec<u32> = candidates
                    .into_iter()
                    .filter(|&id| Some(self.merge.contrib(id).prio) == top)
                    .collect();
                winners.sort_by_key(|&id| self.merge.contrib(id).seq);
                let node = self.merge.node_mut(n);
                node.winners = winners;
                node.shape = Shape::Leaf;
            }
        }
        Ok(())
    }

    fn has_selection(&self, n: NodeId) -> bool {
        self.merge
            .node(n)
            .contribs
            .iter()
            .any(|&id| matches!(self.merge.contrib(id).kind, Kind::Select(_)))
    }

    /// Decides whether an optional attribute is present: when a
    /// contribution sets it, or something inside it, with a higher
    /// priority than every `none`.
    fn presence(&self, n: NodeId) -> Presence {
        let merge = &self.merge;
        let mut none: Vec<u32> = Vec::new();
        let mut present: Vec<u32> = Vec::new();
        for &id in &merge.node(n).contribs {
            match merge.contrib(id).kind {
                Kind::Value(Val::None) => none.push(id),
                Kind::Spent => {}
                _ => present.push(id),
            }
        }
        present.extend(merge.below(n));
        let top = |ids: &[u32]| ids.iter().map(|&id| merge.contrib(id).prio).max();
        match (top(&none), top(&present)) {
            (_, None) => Presence::Absent,
            (None, Some(_)) => Presence::Present,
            (Some(a), Some(b)) if a > b => Presence::Absent,
            (Some(a), Some(b)) if a < b => Presence::Present,
            (Some(a), _) => {
                let mut ids: Vec<u32> = none
                    .into_iter()
                    .chain(present)
                    .filter(|&id| merge.contrib(id).prio == a)
                    .collect();
                ids.sort_by_key(|&id| merge.contrib(id).seq);
                Presence::Conflict(ids)
            }
        }
    }

    /// Adds the declared defaults of the fields of a struct instance once.
    /// A field that an enclosing default already sets keeps that value.
    fn add_defaults(&mut self, n: NodeId, s: StructId) -> R<()> {
        if std::mem::replace(&mut self.merge.node_mut(n).defaults, true) {
            return Ok(());
        }
        let def = self.c.types.structure(s);
        let file = def.file;
        let Some(unit) = self.file_unit(file) else {
            return Err(self.bug(file, def.span, "a declaration has no unit"));
        };
        for (i, field) in def.fields.iter().enumerate() {
            let Some(node) = field.default else { continue };
            let child = self.merge.child_or_new(n, field_seg(i));
            if self
                .merge
                .node(child)
                .contribs
                .iter()
                .any(|&id| self.merge.contrib(id).prio == Prio::Declared)
            {
                continue;
            }
            let span = self.tree(file).node(node).map_or(def.span, |e| e.span());
            let template = Contrib {
                prio: Prio::Declared,
                kind: Kind::Present,
                file,
                span,
                via: None,
                seq: 0,
                elements: None,
            };
            let reads = self.default_reads(s, i);
            if reads.is_empty() {
                for p in self.field_default(s, i, unit, field.ty, &template)?.iter() {
                    self.merge.insert(child, &p.path, p.contrib.clone());
                }
            } else {
                let instance: Rc<[Seg]> = self.merge.path(n).into();
                let deferred = Deferred {
                    unit,
                    node,
                    env: Env::default(),
                    frames: vec![instance],
                    ty: field.ty,
                    reads: reads.to_vec(),
                };
                self.merge
                    .insert(child, &[], template.with(Kind::Deferred(Rc::new(deferred))));
            }
        }
        Ok(())
    }

    /// Returns the contributions of a field default that does not read
    /// `self`, evaluated once.
    fn field_default(
        &mut self,
        s: StructId,
        field: usize,
        unit: crate::check::model::UnitId,
        ty: TypeId,
        template: &Contrib,
    ) -> R<Rc<Vec<Pending>>> {
        if let Some(found) = self.caches.defaults.get(&(s, field)) {
            return Ok(found.clone());
        }
        let node = self
            .c
            .types
            .structure(s)
            .fields
            .get(field)
            .and_then(|f| f.default);
        let Some(node) = node else {
            return Ok(Rc::new(Vec::new()));
        };
        let e = self.expr_at(unit, node)?;
        let cx = super::expr::Cx {
            unit,
            file: template.file,
            env: Env::default(),
            frames: &[],
        };
        let mut out = Vec::new();
        self.contribute(&cx, Vec::new(), ty, e, template, &mut out)?;
        let out = Rc::new(out);
        self.caches.defaults.insert((s, field), out.clone());
        Ok(out)
    }

    /// Structures a sum selected by tagged blocks. Whole values become a
    /// selection and payload contributions at their own priority.
    fn tagged(&mut self, n: NodeId, s: SumId, deferred: Vec<u32>) -> R<()> {
        let ids = self.merge.node(n).contribs.clone();
        for id in ids {
            let contrib = self.merge.contrib(id).clone();
            let Kind::Value(value) = &contrib.kind else {
                continue;
            };
            let (variant, fields, set) = match value {
                Val::Tag(_, v) => (*v, Vec::new(), Vec::new()),
                Val::Variant(v) => (v.index, v.fields.clone(), v.set.clone()),
                _ => continue,
            };
            self.merge.contribs[id as usize].kind = Kind::Select(variant);
            let params: Vec<TypeId> = self
                .c
                .types
                .sum(s)
                .variants
                .get(variant as usize)
                .and_then(|v| v.fields.as_ref())
                .map(|f| f.iter().map(|p| p.ty).collect())
                .unwrap_or_default();
            let payload = self.merge.child_or_new(n, Seg::Variant(variant));
            let mut out = Vec::new();
            for (i, (ty, value)) in params.into_iter().zip(fields).enumerate() {
                if set.get(i).copied().unwrap_or(true) {
                    self.decompose(vec![field_seg(i)], ty, value, &contrib, &mut out);
                }
            }
            for p in out {
                self.merge.insert(payload, &p.path, p.contrib);
            }
        }
        let selections: Vec<u32> = self
            .merge
            .node(n)
            .contribs
            .iter()
            .copied()
            .filter(|&id| matches!(self.merge.contrib(id).kind, Kind::Select(_)))
            .collect();
        let top = selections
            .iter()
            .map(|&id| self.merge.contrib(id).prio)
            .max();
        let undecided = deferred
            .iter()
            .any(|&id| Some(self.merge.contrib(id).prio) >= top);
        self.merge.node_mut(n).pending = deferred;
        if undecided {
            self.merge.node_mut(n).shape = Shape::Tagged(None);
            return Ok(());
        }
        let mut winners: Vec<u32> = selections
            .into_iter()
            .filter(|&id| Some(self.merge.contrib(id).prio) == top)
            .collect();
        winners.sort_by_key(|&id| self.merge.contrib(id).seq);
        let variants: Vec<u32> = winners
            .iter()
            .filter_map(|&id| match self.merge.contrib(id).kind {
                Kind::Select(v) => Some(v),
                _ => None,
            })
            .collect();
        let Some(&variant) = variants.first() else {
            self.merge.node_mut(n).shape = Shape::Tagged(None);
            return Ok(());
        };
        if variants.iter().any(|&v| v != variant) {
            self.conflict(n, &winners, "E0502");
            self.merge.node_mut(n).shape = Shape::Conflict;
            return Ok(());
        }
        self.merge.node_mut(n).shape = Shape::Tagged(Some(variant));
        let payload = self.merge.child_or_new(n, Seg::Variant(variant));
        self.payload(payload, s, variant)
    }

    fn payload(&mut self, n: NodeId, s: SumId, variant: u32) -> R<()> {
        {
            let node = self.merge.node_mut(n);
            node.ty = TypeId::ERROR;
            node.shape = Shape::Payload(s, variant);
        }
        let params: Vec<TypeId> = self
            .c
            .types
            .sum(s)
            .variants
            .get(variant as usize)
            .and_then(|v| v.fields.as_ref())
            .map(|f| f.iter().map(|p| p.ty).collect())
            .unwrap_or_default();
        if !std::mem::replace(&mut self.merge.node_mut(n).defaults, true) {
            for (i, &ty) in params.iter().enumerate() {
                let child = self.merge.child_or_new(n, field_seg(i));
                if self
                    .merge
                    .node(child)
                    .contribs
                    .iter()
                    .any(|&id| self.merge.contrib(id).prio == Prio::Declared)
                {
                    continue;
                }
                let Some(value) = self.payload_default(s, variant as usize, i)? else {
                    continue;
                };
                let def = self.c.types.sum(s);
                let template = Contrib {
                    prio: Prio::Declared,
                    kind: Kind::Present,
                    file: def.file,
                    span: def.span,
                    via: None,
                    seq: 0,
                    elements: None,
                };
                let mut out = Vec::new();
                self.decompose(Vec::new(), ty, value, &template, &mut out);
                for p in out {
                    self.merge.insert(child, &p.path, p.contrib);
                }
            }
        }
        for (i, ty) in params.into_iter().enumerate() {
            let child = self.merge.child_or_new(n, field_seg(i));
            self.structure(child, ty)?;
        }
        Ok(())
    }

    /// Returns the merged value of an attribute, computing it on first use.
    pub(crate) fn finalize(&mut self, n: NodeId) -> R<Val> {
        match &self.merge.node(n).state {
            State::Done(v) => return Ok(v.clone()),
            State::Missing => return Ok(Val::None),
            State::Busy => {
                let at = self.node_at(n);
                let path = self.display_path(n);
                return Err(self.fail(
                    at,
                    "E0607",
                    format!("`self` dependencies form a cycle through `{path}`"),
                ));
            }
            State::Open => {}
        }
        let at = self.node_at(n);
        self.enter(at)?;
        self.merge.node_mut(n).state = State::Busy;
        let result = self.finalize_inner(n);
        self.leave();
        match result {
            Ok(Some(value)) => {
                self.merge.node_mut(n).state = State::Done(value.clone());
                Ok(value)
            }
            Ok(None) => {
                self.merge.node_mut(n).state = State::Missing;
                Ok(Val::None)
            }
            Err(e) => {
                self.merge.node_mut(n).state = State::Open;
                Err(e)
            }
        }
    }

    /// Returns whether an attribute has no value after finalizing.
    fn is_missing(&self, n: NodeId) -> bool {
        matches!(self.merge.node(n).state, State::Missing)
    }

    fn finalize_inner(&mut self, n: NodeId) -> R<Option<Val>> {
        self.settle(n)?;
        let at = self.node_at(n);
        match self.merge.node(n).shape.clone() {
            Shape::Open | Shape::Absent | Shape::Unset => Ok(None),
            Shape::Conflict => Ok(Some(Val::None)),
            Shape::Struct(s) => {
                let count = self.c.types.structure(s).fields.len();
                let mut values = Vec::with_capacity(count);
                for i in 0..count {
                    let child = self.merge.child_or_new(n, field_seg(i));
                    let value = self.finalize(child)?;
                    let field = &self.c.types.structure(s).fields[i];
                    if self.is_missing(child) && field.is_required(&self.c.types) {
                        self.missing(n, child);
                    }
                    values.push(value);
                }
                self.new_struct(s, values, at).map(Some)
            }
            Shape::Map(k) => {
                let children: Vec<(Seg, NodeId)> = self
                    .merge
                    .node(n)
                    .children
                    .iter()
                    .map(|(s, &c)| (s.clone(), c))
                    .collect();
                let key_sum = self.c.types.as_sum(k);
                let mut entries = Vec::with_capacity(children.len());
                for (seg, child) in children {
                    let Seg::Key(text) = seg else { continue };
                    let value = self.finalize(child)?;
                    if self.is_missing(child) {
                        continue;
                    }
                    let key = match key_sum {
                        Some(sum) => match self.c.types.sum(sum).variant(&text) {
                            Some((index, _)) => self.new_variant(sum, index, None, at)?,
                            None => {
                                let at = self.node_at(child);
                                return Err(self.fail(
                                    at,
                                    "E0604",
                                    format!(
                                        "`{text}` is not a variant of `{}`",
                                        self.c.types.sum(sum).name
                                    ),
                                ));
                            }
                        },
                        None => Val::Str(text.clone()),
                    };
                    entries.push((key, value));
                }
                self.new_map(entries, at).map(Some)
            }
            Shape::Tagged(Some(v)) => match self.merge.child(n, &Seg::Variant(v)) {
                Some(child) => self.finalize(child).map(Some),
                None => Ok(None),
            },
            Shape::Tagged(None) => Ok(None),
            Shape::Payload(s, variant) => {
                let params = self
                    .c
                    .types
                    .sum(s)
                    .variants
                    .get(variant as usize)
                    .and_then(|v| v.fields.clone());
                let Some(params) = params else {
                    return self.new_variant(s, variant as usize, None, at).map(Some);
                };
                let mut values = Vec::with_capacity(params.len());
                for (i, param) in params.iter().enumerate() {
                    let child = self.merge.child_or_new(n, field_seg(i));
                    let value = self.finalize(child)?;
                    if self.is_missing(child)
                        && param.default.is_none()
                        && self.c.types.unwrap_option(param.ty).is_none()
                    {
                        self.missing(n, child);
                    }
                    values.push(value);
                }
                self.new_variant(s, variant as usize, Some(values), at)
                    .map(Some)
            }
            Shape::Leaf => self.leaf(n),
        }
    }

    fn missing(&mut self, parent: NodeId, child: NodeId) {
        let at = if parent == NodeId::ROOT {
            SourceSpan {
                file: self
                    .c
                    .entries
                    .first()
                    .map_or(FileId::new(0), |&u| self.unit_file(u)),
                span: Span { start: 0, end: 0 },
            }
        } else {
            self.node_at(parent)
        };
        let path = self.display_path(child);
        self.errors.push(FileDiagnostic {
            file: Some(at.file),
            diagnostic: Diagnostic::error(
                "E0801",
                at.span,
                format!("the required field `{path}` has no value"),
            )
            .with_help("set it, or give the field a default"),
        });
    }

    /// Evaluates the deferred contributions to the structs, maps, options
    /// and tagged sums around `n`, outermost first, and merges in what they
    /// contribute.
    fn settle(&mut self, n: NodeId) -> R<()> {
        let mut chain = Vec::new();
        let mut current = Some(n);
        while let Some(c) = current {
            if !self.merge.node(c).pending.is_empty() {
                chain.push(c);
            }
            current = self.merge.node(c).parent;
        }
        for c in chain.into_iter().rev() {
            let pending = std::mem::take(&mut self.merge.node_mut(c).pending);
            if pending.is_empty() {
                continue;
            }
            for id in pending {
                let contrib = self.merge.contrib(id).clone();
                let Kind::Deferred(deferred) = &contrib.kind else {
                    continue;
                };
                let out = self.run_deferred(deferred, &contrib)?;
                self.merge.contribs[id as usize].kind = Kind::Spent;
                for p in out {
                    self.merge.insert(c, &p.path, p.contrib);
                }
            }
            let ty = self.merge.node(c).ty;
            match self.merge.node(c).shape {
                Shape::Payload(s, v) => self.payload(c, s, v)?,
                _ => self.structure(c, ty)?,
            }
            self.check_cycles(c)?;
        }
        Ok(())
    }

    fn run_deferred(&mut self, deferred: &Deferred, template: &Contrib) -> R<Vec<Pending>> {
        let mut frames = Vec::with_capacity(deferred.frames.len());
        for path in &deferred.frames {
            match self.merge.resolve(path) {
                Some(node) => frames.push(Frame::Node(node)),
                None => {
                    return Err(self.bug(
                        template.file,
                        template.span,
                        "a block of `self` is missing",
                    ));
                }
            }
        }
        self.evaluate_deferred(deferred, &frames, template)
    }

    /// Merges the winning contributions of an attribute that is neither a
    /// struct, a map nor a tagged sum.
    fn leaf(&mut self, n: NodeId) -> R<Option<Val>> {
        let winners = self.merge.node(n).winners.clone();
        if winners.is_empty() {
            return Ok(None);
        }
        for &id in &winners {
            let contrib = self.merge.contrib(id).clone();
            let Kind::Deferred(deferred) = &contrib.kind else {
                continue;
            };
            let out = self.run_deferred(deferred, &contrib)?;
            let (value, elements) = out
                .into_iter()
                .find(|p| p.path.is_empty())
                .and_then(|p| match p.contrib.kind {
                    Kind::Value(v) => Some((v, p.contrib.elements)),
                    _ => None,
                })
                .unwrap_or((Val::None, None));
            let slot = &mut self.merge.contribs[id as usize];
            slot.kind = Kind::Value(value);
            slot.elements = elements;
        }
        let values: Vec<(Val, u32)> = winners
            .iter()
            .filter_map(|&id| match &self.merge.contrib(id).kind {
                Kind::Value(v) => Some((v.clone(), id)),
                _ => None,
            })
            .collect();
        let ty = self.merge.node(n).ty;
        let types = &self.c.types;
        let inner = types.unwrap_option(ty).unwrap_or_else(|| types.peel(ty));
        let at = self.node_at(n);
        if values.iter().all(|(v, _)| v.is_none()) {
            return Ok(Some(Val::None));
        }
        match self.c.types.get(inner) {
            Type::Data => self.merge_data(n, &mut Vec::new(), values, at).map(Some),
            Type::List(_) | Type::Set(_) if values.iter().all(|(v, _)| !v.is_none()) => {
                let items: Vec<Val> = values
                    .iter()
                    .flat_map(|(v, _)| v.items().iter().cloned())
                    .collect();
                if matches!(self.c.types.get(inner), Type::Set(_)) {
                    self.new_set(items, at).map(Some)
                } else {
                    self.new_list(items, at).map(Some)
                }
            }
            _ => Ok(Some(self.scalar(n, &[], values))),
        }
    }

    /// Returns the value of equal winners, or reports `E0501` and returns
    /// the first.
    fn scalar(&mut self, n: NodeId, keys: &[String], values: Vec<(Val, u32)>) -> Val {
        let Some(first) = values.first().map(|(v, _)| v.clone()) else {
            return Val::None;
        };
        if values.iter().all(|(v, _)| v.equals(&first)) {
            return first;
        }
        let ids: Vec<u32> = values.iter().map(|(_, id)| *id).collect();
        let shown: Vec<Val> = values.into_iter().map(|(v, _)| v).collect();
        self.report_conflict(n, keys, &ids, Some(&shown), "E0501");
        first
    }

    /// Merges `Data` values by their shape: maps and structs per key,
    /// lists by concatenation, sets by union, anything else as a scalar.
    fn merge_data(
        &mut self,
        n: NodeId,
        keys: &mut Vec<String>,
        values: Vec<(Val, u32)>,
        at: SourceSpan,
    ) -> R<Val> {
        let shape = |v: &Val| match v {
            Val::Map(_) | Val::Struct(_) => 0,
            Val::List(_) => 1,
            Val::Set(_) => 2,
            _ => 3,
        };
        let Some(first) = values.first().map(|(v, _)| shape(v)) else {
            return Ok(Val::None);
        };
        if values.iter().any(|(v, _)| shape(v) != first) {
            let ids: Vec<u32> = values.iter().map(|(_, id)| *id).collect();
            let shown: Vec<Val> = values.into_iter().map(|(v, _)| v).collect();
            self.report_conflict(n, keys, &ids, Some(&shown), "E0503");
            return Ok(Val::None);
        }
        match first {
            0 => {
                if let Some((Val::Struct(s), _)) = values.first()
                    && values
                        .iter()
                        .all(|(v, _)| matches!(v, Val::Struct(o) if o.id == s.id))
                {
                    let id = s.id;
                    let count = s.fields.len();
                    let mut fields = Vec::with_capacity(count);
                    for i in 0..count {
                        let name = self.c.types.structure(id).fields[i].name.clone();
                        let column = values
                            .iter()
                            .filter_map(|(v, c)| match v {
                                Val::Struct(s) => s.fields.get(i).map(|f| (f.clone(), *c)),
                                _ => None,
                            })
                            .collect();
                        keys.push(name);
                        let merged = self.merge_data(n, keys, column, at);
                        keys.pop();
                        fields.push(merged?);
                    }
                    return self.new_struct(id, fields, at);
                }
                let mut grouped: BTreeMap<Key, Vec<(Val, u32)>> = BTreeMap::new();
                for (value, id) in &values {
                    let entries = self.entries(value);
                    self.charge(u64::try_from(entries.len()).unwrap_or(u64::MAX), at)?;
                    for (key, item) in entries {
                        grouped.entry(Key(key)).or_default().push((item, *id));
                    }
                }
                let mut entries = Vec::with_capacity(grouped.len());
                for (Key(key), items) in grouped {
                    keys.push(key.text(&self.c.types));
                    let merged = self.merge_data(n, keys, items, at);
                    keys.pop();
                    entries.push((key, merged?));
                }
                self.new_map(entries, at)
            }
            1 => {
                let items = values
                    .iter()
                    .flat_map(|(v, _)| v.items().iter().cloned())
                    .collect();
                self.new_list(items, at)
            }
            2 => {
                let items = values
                    .iter()
                    .flat_map(|(v, _)| v.items().iter().cloned())
                    .collect();
                self.new_set(items, at)
            }
            _ => Ok(self.scalar(n, keys, values)),
        }
    }

    /// Returns the entries of a map, or the fields of a struct keyed by
    /// their names.
    fn entries(&self, value: &Val) -> Vec<(Val, Val)> {
        match value {
            Val::Map(m) => m.entries.clone(),
            Val::Struct(s) => self
                .c
                .types
                .structure(s.id)
                .fields
                .iter()
                .zip(&s.fields)
                .map(|(f, v)| (Val::Str(f.name.as_str().into()), v.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn conflict(&mut self, n: NodeId, ids: &[u32], code: &'static str) {
        self.report_conflict(n, &[], ids, None, code);
    }

    /// Reports contributions of equal priority that do not agree, citing
    /// every one of them.
    fn report_conflict(
        &mut self,
        n: NodeId,
        keys: &[String],
        ids: &[u32],
        values: Option<&[Val]>,
        code: &'static str,
    ) {
        let mut path = self.attr_path(n);
        for key in keys {
            path = path.join(key.clone());
        }
        let Some(&first) = ids.first() else { return };
        let primary = self.merge.contrib(first).at();
        let (message, help) = match code {
            "E0502" => (
                format!("tagged blocks select different variants of `{path}`"),
                "keep the blocks of one variant, or select the variant with `force(...)`",
            ),
            "E0503" => (
                format!("`Data` values of different shapes for `{path}`"),
                "a higher priority contribution replaces the others: mark one with `force(...)`, or the others with `default(...)`",
            ),
            _ => (
                format!("conflicting values for `{path}`"),
                "keep one of them, or mark the one that should win with `force(...)`",
            ),
        };
        let mut diagnostic = Diagnostic::error(code, primary.span, message).with_help(help);
        for (i, &id) in ids.iter().enumerate() {
            let contrib = self.merge.contrib(id).clone();
            let what = match (values.and_then(|v| v.get(i)), &contrib.kind) {
                (Some(value), _) | (None, Kind::Value(value)) => self.describe(value, code),
                (None, Kind::Select(v)) => {
                    let name = self.variant_name(n, *v);
                    format!("`{name}`")
                }
                (None, Kind::Present | Kind::Spent) => "present".to_owned(),
                (None, Kind::Deferred(_)) => "a value that reads `self`".to_owned(),
            };
            let mut label = format!("{what} ({})", contrib.prio.name());
            if let Some(site) = contrib.via.as_ref().and_then(|v| v.first()) {
                label.push_str(&format!(", included from {}", self.location(*site)));
            }
            diagnostic = diagnostic.with_label(Some(contrib.file), contrib.span, label);
        }
        self.errors.push(FileDiagnostic {
            file: Some(primary.file),
            diagnostic,
        });
    }

    fn describe(&self, value: &Val, code: &str) -> String {
        if code == "E0503" {
            return match value {
                Val::Map(_) | Val::Struct(_) => "a map",
                Val::List(_) => "a list",
                Val::Set(_) => "a set",
                _ => "a scalar",
            }
            .to_owned();
        }
        value
            .to_value(&self.c.types, &self.c.import_roots)
            .describe()
    }

    fn variant_name(&self, n: NodeId, variant: u32) -> String {
        let ty = self.merge.node(n).ty;
        let types = &self.c.types;
        let inner = types.unwrap_option(ty).unwrap_or_else(|| types.peel(ty));
        types
            .as_sum(inner)
            .and_then(|s| types.sum(s).variants.get(variant as usize))
            .map_or_else(|| variant.to_string(), |v| v.name.clone())
    }

    /// Returns `file:line:column` of a place.
    pub(crate) fn location(&self, at: SourceSpan) -> String {
        let file = self.c.program.file(at.file);
        let text = file.parsed.tree.root().text();
        let before = text.get(..at.span.start).unwrap_or(text);
        let line = before.matches('\n').count() + 1;
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[start..].chars().count() + 1;
        format!("{}:{line}:{column}", file.name)
    }

    /// Returns the name of the step from the parent of `n` to `n`.
    pub(crate) fn key_name(&self, n: NodeId) -> String {
        let node = self.merge.node(n);
        let Some(parent) = node.parent else {
            return String::new();
        };
        let types = &self.c.types;
        let parent_node = self.merge.node(parent);
        match &node.seg {
            Some(Seg::Key(k)) => k.to_string(),
            Some(Seg::Variant(v)) => self.variant_name(parent, *v),
            Some(Seg::Field(i)) => {
                let i = *i as usize;
                let name = match parent_node.shape {
                    Shape::Payload(s, v) => types
                        .sum(s)
                        .variants
                        .get(v as usize)
                        .and_then(|v| v.fields.as_ref())
                        .and_then(|f| f.get(i))
                        .map(|p| p.name.clone()),
                    _ => {
                        let ty = parent_node.ty;
                        let inner = types.unwrap_option(ty).unwrap_or_else(|| types.peel(ty));
                        types
                            .as_struct(inner)
                            .and_then(|s| types.structure(s).fields.get(i))
                            .map(|f| f.name.clone())
                    }
                };
                name.unwrap_or_else(|| format!("#{i}"))
            }
            None => String::new(),
        }
    }

    pub(crate) fn attr_path(&self, n: NodeId) -> AttrPath {
        let mut keys = Vec::new();
        let mut current = Some(n);
        while let Some(c) = current {
            if c != NodeId::ROOT {
                keys.push(self.key_name(c));
            }
            current = self.merge.node(c).parent;
        }
        keys.reverse();
        AttrPath::new(keys)
    }

    fn display_path(&self, n: NodeId) -> String {
        let path = self.attr_path(n);
        if path.keys().is_empty() {
            "the root".to_owned()
        } else {
            path.to_string()
        }
    }

    /// Reports the cycles of the `self` dependencies of the winners at and
    /// inside `top`, and stops when there is one. Otherwise returns the
    /// attributes in an order where each comes after the attributes it
    /// depends on, so that finalizing them in that order never recurses
    /// along a long chain of dependencies.
    fn check_cycles(&mut self, top: NodeId) -> R<Vec<NodeId>> {
        let nodes = self.merge.reached(top);
        let mut index = BTreeMap::new();
        for (i, &n) in nodes.iter().enumerate() {
            index.insert(n, i);
        }
        let mut edges: Vec<Vec<(usize, Option<u32>)>> = vec![Vec::new(); nodes.len()];
        for (i, &n) in nodes.iter().enumerate() {
            for c in self.merge.inside(n) {
                if let Some(&j) = index.get(&c) {
                    edges[i].push((j, None));
                }
            }
            let node = self.merge.node(n);
            let leaf_deferred = node.winners.iter().copied();
            let structural: Vec<u32> = node.pending.clone();
            for id in leaf_deferred {
                for target in self.read_targets(id) {
                    if let Some(&j) = index.get(&target) {
                        edges[i].push((j, Some(id)));
                    }
                }
            }
            if !structural.is_empty() {
                let below = self.merge.reached(n);
                for id in structural {
                    let targets = self.read_targets(id);
                    for &m in &below {
                        let Some(&k) = index.get(&m) else { continue };
                        for &target in &targets {
                            if let Some(&j) = index.get(&target) {
                                edges[k].push((j, Some(id)));
                            }
                        }
                    }
                }
            }
        }
        let adj: Vec<Vec<usize>> = edges
            .iter()
            .map(|list| {
                let mut targets: Vec<usize> = list.iter().map(|(t, _)| *t).collect();
                targets.sort_unstable();
                targets.dedup();
                targets
            })
            .collect();
        let mut found: Vec<FileDiagnostic> = Vec::new();
        let components = graph::components(&adj);
        for cycle in graph::cycles_of(&adj, &components) {
            let mut diagnostic: Option<(FileId, Diagnostic)> = None;
            for (k, &from) in cycle.iter().enumerate() {
                let to = cycle[(k + 1) % cycle.len()];
                let Some(id) = edges[from]
                    .iter()
                    .find(|(t, c)| *t == to && c.is_some())
                    .and_then(|(_, c)| *c)
                else {
                    continue;
                };
                let contrib = self.merge.contrib(id).clone();
                let message = format!(
                    "`{}` reads `{}`",
                    self.display_path(nodes[from]),
                    self.display_path(nodes[to])
                );
                diagnostic = Some(match diagnostic {
                    None => (
                        contrib.file,
                        Diagnostic::error(
                            "E0607",
                            contrib.span,
                            "`self` dependencies form a cycle",
                        )
                        .with_label(
                            Some(contrib.file),
                            contrib.span,
                            message,
                        ),
                    ),
                    Some((file, d)) => (
                        file,
                        d.with_label(Some(contrib.file), contrib.span, message),
                    ),
                });
            }
            let Some((file, diagnostic)) = diagnostic else {
                continue;
            };
            let mut chain: Vec<String> =
                cycle.iter().map(|&k| self.display_path(nodes[k])).collect();
            chain.push(self.display_path(nodes[cycle[0]]));
            found.push(FileDiagnostic {
                file: Some(file),
                diagnostic: diagnostic.with_note(format!("cycle: {}", chain.join(" -> "))),
            });
        }
        if found.is_empty() {
            return Ok(components.into_iter().flatten().map(|i| nodes[i]).collect());
        }
        let first = found.remove(0);
        self.errors.extend(found);
        Err(Stop(Box::new(first)))
    }

    /// Returns the attributes a deferred contribution reads through
    /// `self`.
    fn read_targets(&self, id: u32) -> Vec<NodeId> {
        let Kind::Deferred(deferred) = &self.merge.contrib(id).kind else {
            return Vec::new();
        };
        deferred
            .reads
            .iter()
            .filter_map(|read| {
                let frame = deferred.frames.get(read.depth)?;
                let block = self.merge.resolve(frame)?;
                self.merge.child(block, &field_seg(read.field))
            })
            .collect()
    }

    /// Returns the `self` reads of a field default.
    pub(crate) fn default_reads(&mut self, s: StructId, field: usize) -> Rc<Vec<SelfRead>> {
        if let Some(found) = self.caches.reads.get(&(s, field)) {
            return found.clone();
        }
        let def = self.c.types.structure(s);
        let mut reads = Vec::new();
        if let (Some(node), Some(unit)) = (
            def.fields.get(field).and_then(|f| f.default),
            self.file_unit(def.file),
        ) {
            let mut stack: Vec<crate::syntax::SyntaxNode<'c>> =
                self.tree(def.file).node(node).into_iter().collect();
            while let Some(node) = stack.pop() {
                if let crate::check::model::Fact::Expr(fact) = self.fact(unit, node.index())
                    && let crate::check::model::Meaning::SelfField(read) = fact.meaning
                {
                    reads.push(read);
                }
                stack.extend(node.children());
            }
        }
        let reads = Rc::new(reads);
        self.caches.reads.insert((s, field), reads.clone());
        reads
    }

    /// Returns the merged value of an attribute once it is final.
    pub(crate) fn final_value(&self, n: NodeId) -> Option<Val> {
        match &self.merge.node(n).state {
            State::Done(v) => Some(v.clone()),
            _ => None,
        }
    }

    pub(crate) fn node_type(&self, n: NodeId) -> TypeId {
        self.merge.node(n).ty
    }

    /// Returns the attributes whose values make up the value of `n`.
    pub(crate) fn inside_nodes(&self, n: NodeId) -> Vec<NodeId> {
        self.merge.inside(n)
    }

    /// Returns where each attribute of the final value came from.
    pub(crate) fn walk_spans(&self) -> SpanTree {
        let mut tree = SpanTree::default();
        let mut stack = vec![(NodeId::ROOT, 0usize)];
        tree.nodes.push(SpanNode::default());
        while let Some((n, slot)) = stack.pop() {
            let node = self.merge.node(n);
            let mut ids: Vec<u32> = node.contribs.clone();
            ids.sort_by_key(|&id| {
                let c = self.merge.contrib(id);
                (std::cmp::Reverse(c.prio), c.seq)
            });
            let mut spans: Vec<SourceSpan> = Vec::new();
            for id in ids {
                let at = self.merge.contrib(id).at();
                if !spans.contains(&at) {
                    spans.push(at);
                }
            }
            tree.nodes[slot].spans = spans;
            let mut children = Vec::new();
            for c in self.merge.inside(n) {
                let key = self.key_name(c);
                let index = tree.nodes.len();
                tree.nodes.push(SpanNode::default());
                children.push((key, index));
                stack.push((c, index));
            }
            for (i, spans) in self.element_spans(n).into_iter().enumerate() {
                children.push((i.to_string(), tree.nodes.len()));
                tree.nodes.push(SpanNode {
                    spans,
                    ..SpanNode::default()
                });
            }
            if let Shape::Tagged(Some(_)) = node.shape {
                tree.nodes[slot].transparent = true;
            }
            children.sort();
            tree.nodes[slot].children = children;
        }
        tree
    }
}

impl Eval<'_> {
    /// Returns the spans of each element of the list or set at `n`: the
    /// element of a list literal that wrote it, or else the contribution
    /// it came from. An element of a set has the spans of every element
    /// equal to it.
    fn element_spans(&self, n: NodeId) -> Vec<Vec<SourceSpan>> {
        let (set, items) = match self.final_value(n) {
            Some(Val::List(s)) => (false, s),
            Some(Val::Set(s)) => (true, s),
            _ => return Vec::new(),
        };
        let mut written: Vec<(Val, SourceSpan)> = Vec::new();
        for &id in &self.merge.node(n).winners {
            let c = self.merge.contrib(id);
            let Kind::Value(value) = &c.kind else {
                continue;
            };
            match &c.elements {
                Some(elements) if set || elements.len() == value.items().len() => {
                    written.extend(elements.iter().map(|(v, span)| {
                        let at = SourceSpan {
                            file: c.file,
                            span: *span,
                        };
                        (v.clone(), at)
                    }));
                }
                _ => written.extend(value.items().iter().map(|v| (v.clone(), c.at()))),
            }
        }
        if !set {
            if written.len() != items.items.len() {
                return Vec::new();
            }
            return written.into_iter().map(|(_, at)| vec![at]).collect();
        }
        written.sort_by(|a, b| a.0.order(&b.0));
        items
            .items
            .iter()
            .map(|item| {
                let start = written.partition_point(|(v, _)| v.order(item).is_lt());
                let mut spans: Vec<SourceSpan> = Vec::new();
                for (_, at) in written[start..].iter().take_while(|(v, _)| v.equals(item)) {
                    if !spans.contains(at) {
                        spans.push(*at);
                    }
                }
                spans
            })
            .collect()
    }
}

fn field_seg(i: usize) -> Seg {
    Seg::Field(u32::try_from(i).unwrap_or(u32::MAX))
}

/// Where each attribute of an evaluated value came from.
#[derive(Debug, Clone, Default)]
pub(crate) struct SpanTree {
    nodes: Vec<SpanNode>,
}

#[derive(Debug, Clone, Default)]
struct SpanNode {
    children: Vec<(String, usize)>,
    spans: Vec<SourceSpan>,
    /// A tagged sum, whose one child is named by its variant.
    transparent: bool,
}

impl SpanTree {
    /// Returns the spans of the attribute at `path`. When the tree ends
    /// before the path does, `inside` says whether the path still names a
    /// part of the value, which then gets the spans of the deepest
    /// attribute on the path.
    pub(crate) fn spans(&self, path: &AttrPath, inside: bool) -> Vec<SourceSpan> {
        let Some(mut current) = (!self.nodes.is_empty()).then_some(0) else {
            return Vec::new();
        };
        for key in path.keys() {
            let node = &self.nodes[current];
            match node.children.binary_search_by(|(k, _)| k.as_str().cmp(key)) {
                Ok(i) => current = node.children[i].1,
                Err(_) if inside => break,
                Err(_) => return Vec::new(),
            }
        }
        let node = &self.nodes[current];
        if !node.spans.is_empty() && !node.transparent {
            return node.spans.clone();
        }
        let mut out = node.spans.clone();
        let mut stack: Vec<usize> = node.children.iter().rev().map(|(_, i)| *i).collect();
        while let Some(i) = stack.pop() {
            for span in &self.nodes[i].spans {
                if !out.contains(span) {
                    out.push(*span);
                }
            }
            stack.extend(self.nodes[i].children.iter().rev().map(|(_, c)| *c));
        }
        out
    }
}

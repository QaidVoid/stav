//! A readable listing of what the checker inferred.

use std::fmt::Write;

use super::Checked;
use super::decls::{GlobalKind, LetState};
use super::load::{FileId, Program, Role};
use super::model::{
    BlockRule, Fact, IncludeFact, LabelFact, LetFact, Priority, SelfRead, Step, Target,
};
use super::totality::Node;
use super::types::Type;
use crate::ast;
use crate::syntax::SyntaxNode;

impl Checked {
    /// Returns a readable listing of what the checker inferred: the root
    /// struct, the signature of every top-level function and the type of
    /// every top-level `let`, the call graph, and, for every checked body,
    /// what each statement contributes to, with its type and priority and
    /// the `self` fields it reads.
    ///
    /// The listing is meant for tests and debugging. Its format may change
    /// in any release.
    pub fn summary(&self) -> String {
        let mut out = self.program.listing();
        let _ = writeln!(out, "root {}", self.types.structure(self.root).name);
        for global in &self.globals {
            let file = &self.program.file(global.file).name;
            match &global.kind {
                GlobalKind::Fn(sig) => {
                    let params: Vec<String> = sig
                        .params
                        .iter()
                        .map(|p| format!("{}: {}", p.name, self.types.render_plain(p.ty)))
                        .collect();
                    let _ = writeln!(
                        out,
                        "fn {}({}) -> {} in {file}",
                        global.name,
                        params.join(", "),
                        self.types.render_plain(sig.ret)
                    );
                }
                GlobalKind::Let(l) => {
                    let (ty, reads) = match &l.state {
                        LetState::Done { ty, reads } => {
                            (self.types.render_plain(*ty), reads.as_slice())
                        }
                        _ => ("?".to_owned(), &[][..]),
                    };
                    let _ = writeln!(
                        out,
                        "let {}: {ty}{} in {file}",
                        global.name,
                        self.reads(reads)
                    );
                }
            }
        }
        for (from, to) in self.calls.edges.keys() {
            let _ = writeln!(
                out,
                "uses {} -> {}",
                self.node_name(*from),
                self.node_name(*to)
            );
        }
        if !self.let_order.is_empty() {
            let order: Vec<&str> = self
                .let_order
                .iter()
                .map(|g| self.globals[g.index()].name.as_str())
                .collect();
            let _ = writeln!(out, "let order {}", order.join(", "));
        }
        let order: Vec<String> = self
            .implicit_parts
            .iter()
            .chain(&self.entries)
            .map(|u| format!("unit {}", u.index()))
            .collect();
        let _ = writeln!(out, "bodies {}", order.join(", "));
        for (target, steps) in &self.ref_targets {
            let mut names = Vec::new();
            let mut current = Some(self.root);
            for step in steps {
                if let (Step::Field(i), Some(s)) = (step, current) {
                    let field = &self.types.structure(s).fields[*i];
                    names.push(field.name.clone());
                    let inner = self.types.unwrap_option(field.ty).unwrap_or(field.ty);
                    current = self.types.as_struct(inner);
                }
            }
            let _ = writeln!(
                out,
                "ref {} -> {}",
                self.types.structure(*target).name,
                names.join(".")
            );
        }
        for (i, unit) in self.units.iter().enumerate() {
            let file = self.program.file(unit.file);
            let mut lines = Vec::new();
            let start = if self.entries.contains(&super::model::UnitId::new(i)) {
                Some(Target::Struct(self.root))
            } else {
                unit.target.map(Target::Struct)
            };
            let mut stack = vec![(file.parsed.tree.root(), 0usize, start)];
            while let Some((node, depth, target)) = stack.pop() {
                let fact = unit.facts.get(node.index()).unwrap_or(&Fact::None);
                let line = self.describe(i, node, fact, target);
                let nested = if line.is_some() { depth + 1 } else { depth };
                let inner = match fact {
                    Fact::Block(block) => Some(block.target),
                    Fact::Expr(e) if ast::PartExpr::cast(node).is_some() => {
                        match self.types.get(self.types.peel(e.ty)) {
                            Type::Part(s) => Some(Target::Struct(*s)),
                            _ => None,
                        }
                    }
                    _ => target,
                };
                if let Some(line) = line {
                    let (row, column) = position(file.parsed.tree.root().text(), node.span().start);
                    lines.push(format!("  {row}:{column} {}{line}", "  ".repeat(depth)));
                }
                let children: Vec<SyntaxNode<'_>> = node.children().collect();
                stack.extend(children.into_iter().rev().map(|c| (c, nested, inner)));
            }
            if lines.is_empty() {
                continue;
            }
            let target = unit
                .target
                .map(|s| format!(" as {}", self.types.structure(s).name))
                .unwrap_or_default();
            let _ = writeln!(out, "unit {i} {}{target}", file.name);
            for line in lines {
                let _ = writeln!(out, "{line}");
            }
        }
        out
    }

    fn node_name(&self, node: Node) -> String {
        match node {
            Node::Global(g) => self.globals[g.index()].name.clone(),
            Node::StructDefaults(s) => format!("{} defaults", self.types.structure(s).name),
            Node::SumDefaults(s) => format!("{} defaults", self.types.sum(s).name),
        }
    }

    fn reads(&self, reads: &[SelfRead]) -> String {
        if reads.is_empty() {
            return String::new();
        }
        let mut names: Vec<String> = reads
            .iter()
            .map(|r| {
                let s = self.types.structure(r.structure);
                let field = s.fields.get(r.field).map_or("?", |f| f.name.as_str());
                format!("{}.{field}@{}", s.name, r.depth)
            })
            .collect();
        names.sort();
        names.dedup();
        format!(" late({})", names.join(", "))
    }

    fn target(&self, target: Target) -> String {
        match target {
            Target::Struct(s) => self.types.structure(s).name.clone(),
            Target::Map { key, value } => format!(
                "Map<{}, {}>",
                self.types.render_plain(key),
                self.types.render_plain(value)
            ),
            Target::Variant { sum, variant } => {
                let sum = self.types.sum(sum);
                format!("{}.{}", sum.name, sum.variants[variant].name)
            }
        }
    }

    fn field_name(&self, target: Option<Target>, index: usize) -> String {
        let name = match target {
            Some(Target::Struct(s)) => self
                .types
                .structure(s)
                .fields
                .get(index)
                .map(|f| f.name.clone()),
            Some(Target::Variant { sum, variant }) => self.types.sum(sum).variants[variant]
                .fields
                .iter()
                .flatten()
                .nth(index)
                .map(|f| f.name.clone()),
            _ => None,
        };
        name.unwrap_or_else(|| format!("#{index}"))
    }

    fn describe(
        &self,
        unit: usize,
        node: SyntaxNode<'_>,
        fact: &Fact,
        target: Option<Target>,
    ) -> Option<String> {
        let locals = &self.units[unit].locals;
        Some(match fact {
            Fact::Attr(attr) => {
                let path = ast::Attr::cast(node)
                    .and_then(ast::Attr::path)
                    .map_or(String::new(), |p| p.syntax().text().to_owned());
                let priority = match attr.priority {
                    Priority::Default => "default",
                    Priority::Plain => "plain",
                    Priority::Force => "force",
                };
                format!(
                    "{path}: {} ({priority}){}",
                    self.types.render_plain(attr.ty),
                    self.reads(&attr.reads)
                )
            }
            Fact::Block(block) => {
                let kind = ast::Block::cast(node)
                    .and_then(ast::Block::kind)
                    .map_or("", |k| k.text());
                let label = match &block.label {
                    Some(LabelFact::Static(text)) => format!(" {text:?}"),
                    Some(LabelFact::Dynamic) => " <dynamic>".to_owned(),
                    None => String::new(),
                };
                let rule = match block.rule {
                    BlockRule::Clause => "clause",
                    BlockRule::Field => "field",
                    BlockRule::MapField => "map",
                    BlockRule::Tagged => "tagged",
                };
                format!(
                    "block {kind}{label} ({rule} {}) -> {}",
                    self.field_name(target, block.field),
                    self.target(block.target)
                )
            }
            Fact::Include(IncludeFact::File(u)) => {
                let target = &self.units[u.index()];
                format!(
                    "include unit {} {}",
                    u.index(),
                    self.program.file(target.file).name
                )
            }
            Fact::Include(IncludeFact::Value) => "include value".to_owned(),
            Fact::Let(LetFact::Local(id)) => {
                let local = &locals[id.index()];
                format!(
                    "let {}: {}{}",
                    local.name,
                    self.types.render_plain(local.ty),
                    self.reads(&local.reads)
                )
            }
            Fact::Let(LetFact::Global(g)) => {
                format!("let {} (top level)", self.globals[g.index()].name)
            }
            Fact::For(each) => {
                let binds: Vec<String> = each
                    .binds
                    .iter()
                    .map(|id| {
                        let local = &locals[id.index()];
                        format!("{}: {}", local.name, self.types.render_plain(local.ty))
                    })
                    .collect();
                format!("for {}", binds.join(", "))
            }
            Fact::None | Fact::Expr(_) | Fact::Pattern(_) => return None,
        })
    }
}

impl Program {
    /// Lists every file with its role, the files its imports and part file
    /// includes resolve to, and the order of the prelude, implicit parts and
    /// entry files.
    pub(crate) fn listing(&self) -> String {
        let mut out = String::new();
        for (i, file) in self.files.iter().enumerate() {
            let role = match file.role {
                Role::Prelude => "prelude",
                Role::Entry => "entry",
                Role::Module => "module",
                Role::Part => "part",
            };
            let _ = writeln!(out, "file {i} {} ({role})", file.name);
            for link in &file.imports {
                let target = link
                    .target
                    .map_or("?".to_owned(), |t| format!("file {}", t.index()));
                let _ = writeln!(out, "  import {} -> {target}", link.alias);
            }
            for (node, target) in &file.includes {
                let start = file.parsed.tree.node(*node).map_or(0, |n| n.span().start);
                let (row, column) = position(file.parsed.tree.root().text(), start);
                let _ = writeln!(
                    out,
                    "  include at {row}:{column} -> file {}",
                    target.index()
                );
            }
        }
        let order = |files: &[FileId]| {
            let ids: Vec<String> = files.iter().map(|f| f.index().to_string()).collect();
            ids.join(", ")
        };
        if !self.prelude.is_empty() {
            let _ = writeln!(out, "prelude files {}", order(&self.prelude));
        }
        if !self.implicit_parts.is_empty() {
            let _ = writeln!(out, "implicit parts {}", order(&self.implicit_parts));
        }
        let _ = writeln!(out, "entries {}", order(&self.entries));
        out
    }
}

/// Returns the one-based line and column of `offset` in `text`.
fn position(text: &str, offset: usize) -> (usize, usize) {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() + 1)
}

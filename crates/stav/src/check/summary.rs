//! A readable listing of the loaded program.

use std::fmt::Write;

use super::Checked;
use super::load::{FileId, Program, Role};

impl Checked {
    /// Returns a readable listing of the loaded program: every file with
    /// its role and the files its imports and part file includes resolve to,
    /// and the order of the prelude files, implicit parts and entry files.
    ///
    /// The listing is meant for tests and debugging. Its format may change
    /// in any release.
    pub fn summary(&self) -> String {
        self.program.listing()
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

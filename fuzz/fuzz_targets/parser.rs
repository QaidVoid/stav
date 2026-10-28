//! Parses arbitrary UTF-8 and checks that the tree reproduces the input,
//! that node spans match their tokens, that nesting stays bounded and that
//! the typed view can read every node.

#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use stav::ast::{self, FileKind};
use stav::{MAX_DEPTH, SyntaxElement, SyntaxNode};

fuzz_target!(|source: &str| {
    let parsed = stav::parse(source);
    let root = parsed.tree.root();
    assert_eq!((root.span().start, root.span().end), (0, source.len()));

    let mut text = String::with_capacity(source.len());
    let mut stack = vec![(SyntaxElement::Node(root), 0usize)];
    while let Some((element, depth)) = stack.pop() {
        match element {
            SyntaxElement::Token(token) => text.push_str(token.text()),
            SyntaxElement::Node(node) => {
                assert!(depth <= 5 * MAX_DEPTH);
                check_spans(node);
                read_typed(node);
                let children: Vec<_> = node.children_with_tokens().collect();
                stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
            }
        }
    }
    assert_eq!(text, source);

    for diagnostic in &parsed.diagnostics {
        assert!(source.get(diagnostic.span.range()).is_some());
    }
    for kind in [FileKind::Entry, FileKind::Library, FileKind::Part] {
        for diagnostic in parsed.file().validate(kind) {
            assert!(source.get(diagnostic.span.range()).is_some());
        }
    }
});

fn check_spans(node: SyntaxNode<'_>) {
    let mut elements = node.children_with_tokens();
    match (elements.next(), elements.last()) {
        (Some(first), last) if node.parent().is_some() => {
            let end = last.unwrap_or(first).span().end;
            assert_eq!((node.span().start, node.span().end), (first.span().start, end));
        }
        (None, _) => assert_eq!(node.span().start, node.span().end),
        _ => {}
    }
}

/// Calls the accessors of whatever typed view matches the node.
fn read_typed(node: SyntaxNode<'_>) {
    if let Some(expr) = ast::Expr::cast(node) {
        match expr {
            ast::Expr::Str(string) => {
                let _ = (string.kind(), string.value(), string.parts().count());
            }
            ast::Expr::Symbol(symbol) => {
                let _ = (symbol.name(), symbol.version(), symbol.variant());
            }
            ast::Expr::Binary(binary) => {
                let _ = (binary.lhs(), binary.op(), binary.rhs());
            }
            ast::Expr::Unary(unary) => {
                let _ = (unary.op(), unary.operand());
            }
            ast::Expr::Call(call) => {
                let _ = call.callee();
                for arg in call.args().into_iter().flat_map(|args| args.args()) {
                    let _ = (arg.name(), arg.value());
                }
            }
            ast::Expr::If(branch) => {
                let _ = (branch.condition(), branch.then_branch(), branch.else_branch());
            }
            ast::Expr::Match(m) => {
                let _ = m.scrutinee();
                for arm in m.arms() {
                    let _ = (arm.pattern(), arm.guard(), arm.value());
                }
            }
            ast::Expr::ListComp(c) => {
                let _ = (c.names().count(), c.source(), c.guard(), c.value());
            }
            ast::Expr::MapComp(c) => {
                let _ = (c.names().count(), c.source(), c.guard(), c.key(), c.value());
            }
            ast::Expr::Map(map) => {
                for entry in map.entries() {
                    let _ = (entry.key().map(ast::Key::span), entry.value());
                }
            }
            ast::Expr::Let(l) => {
                let _ = l.body();
                for binding in l.bindings() {
                    let _ = (binding.name(), binding.ty(), binding.value());
                }
            }
            _ => {}
        }
    }
    if let Some(ty) = ast::Type::cast(node) {
        match ty {
            ast::Type::Named(named) => {
                let _ = (named.path().and_then(|p| p.name()), named.args().count());
            }
            ast::Type::Fn(f) => {
                let _ = (f.params().count(), f.return_type());
            }
            ast::Type::Refined(refined) => {
                let _ = refined.base();
                for predicate in refined.predicates() {
                    if let ast::Predicate::Range(range) = predicate {
                        let _ = (range.start(), range.end(), range.is_inclusive());
                    }
                }
            }
        }
    }
    if let Some(ast::Pattern::Variant(variant)) = ast::Pattern::cast(node) {
        let _ = (variant.segments().count(), variant.fields().count(), variant.has_rest());
    }
    if let Some(stmt) = ast::Stmt::cast(node) {
        match stmt {
            ast::Stmt::Attr(attr) => {
                let _ = (attr.path().map(|p| p.keys().count()), attr.value());
            }
            ast::Stmt::Block(block) => {
                let _ = (block.kind(), block.label(), block.body());
            }
            ast::Stmt::For(each) => {
                let _ = (each.names().count(), each.collection(), each.body());
            }
            ast::Stmt::If(branch) => {
                let _ = (branch.condition(), branch.then_body(), branch.else_branch());
            }
            _ => {}
        }
    }
}

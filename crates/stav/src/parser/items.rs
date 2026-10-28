//! Files, declarations, statements and types.

use super::{Parser, is_closer, is_name, is_string_start};
use crate::span::Span;
use crate::syntax::NodeKind;
use crate::token::TokenKind as T;

/// Where a key such as `name` or `"name"` starts an element.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Keyed {
    /// A statement in a body: an attribute or a block.
    Statement,
    /// An entry of a map or struct literal.
    Entry,
}

impl Parser<'_> {
    pub(super) fn file(&mut self) {
        self.builder.start_node(NodeKind::File);
        let mut after_item = false;
        loop {
            self.separators(after_item, "a declaration or a statement");
            let kind = self.current();
            if kind == T::Eof {
                break;
            }
            after_item = true;
            if is_closer(kind) {
                self.expected("a declaration or a statement");
                self.bump_error();
                continue;
            }
            self.element(&mut Self::item);
            if !matches!(self.current(), T::Newline | T::Comma | T::Eof) {
                self.expected("a line break");
                self.skip_to_separator();
            }
        }
        self.flush();
        self.finish_node();
    }

    fn item(&mut self) {
        let import = self.at(T::ImportKw);
        self.statement(true);
        if !import {
            self.past_imports = true;
        }
    }

    fn statement(&mut self, top_level: bool) {
        if !self.enter() {
            return;
        }
        match self.current() {
            T::LetKw => self.let_decl(),
            T::IncludeKw => {
                self.start_node(NodeKind::Include);
                self.bump();
                self.expr();
                self.finish_node();
            }
            T::IfKw => self.if_stmt(),
            T::ForKw => self.for_stmt(),
            T::ImportKw | T::StructKw | T::TypeKw | T::FnKw => self.declaration(top_level),
            kind if is_name(kind) || is_string_start(kind) => self.keyed(Keyed::Statement),
            _ if top_level => self.expected("a declaration or a statement"),
            _ => self.expected("a statement"),
        }
        self.leave();
    }

    fn declaration(&mut self, top_level: bool) {
        let kind = self.current();
        let span = self.current_span();
        if !top_level {
            let keyword = self.current_text();
            self.soft_error(
                "E0208",
                span,
                format!("`{keyword}` is only allowed at the top level of a file"),
                Some("move it to the top level of the file".to_owned()),
            );
        } else if kind == T::ImportKw && self.past_imports {
            self.soft_error(
                "E0213",
                span,
                "`import` must come before every other item".to_owned(),
                Some("move the import to the start of the file".to_owned()),
            );
        }
        match kind {
            T::ImportKw => self.import(),
            T::StructKw => self.struct_decl(),
            T::TypeKw => self.type_decl(),
            _ => self.fn_decl(),
        }
    }

    fn import(&mut self) {
        self.start_node(NodeKind::Import);
        self.bump();
        if is_string_start(self.current()) {
            if let Some(span) = self.string() {
                self.soft_error(
                    "E0212",
                    span,
                    "an import path cannot contain interpolation".to_owned(),
                    Some("write the path as a plain string".to_owned()),
                );
            }
        } else {
            self.expected("a string");
        }
        if self.eat(T::AsKw) {
            self.name("an alias");
        } else {
            self.expected("`as`");
        }
        self.finish_node();
    }

    fn struct_decl(&mut self) {
        self.start_node(NodeKind::Struct);
        self.bump();
        self.type_name_decl();
        if self.at(T::LBrace) {
            let opener = self.open();
            self.sequence(opener, T::RBrace, "a field", Self::field);
        } else {
            self.expected("`{`");
        }
        self.finish_node();
    }

    fn type_name_decl(&mut self) {
        match self.current() {
            T::TypeIdent => self.bump(),
            T::Ident => {
                let name = self.current_text();
                let help = name.starts_with(|c: char| c.is_ascii_lowercase()).then(|| {
                    format!(
                        "rename it to `{}{}`",
                        name[..1].to_ascii_uppercase(),
                        &name[1..]
                    )
                });
                self.soft_error(
                    "E0205",
                    self.current_span(),
                    format!("type name `{name}` must start with an uppercase letter"),
                    help,
                );
                self.bump();
            }
            _ => self.expected("a type name"),
        }
    }

    fn field(&mut self) {
        if !self.at_binding_name("a field name") {
            return;
        }
        self.start_node(NodeKind::Field);
        self.binding_name();
        self.annotation(true);
        if self.eat(T::Eq) {
            self.expr();
        }
        if self.eat(T::BlockKw) {
            self.name("a block kind");
        }
        self.finish_node();
    }

    fn type_decl(&mut self) {
        self.start_node(NodeKind::TypeDecl);
        self.bump();
        self.type_name_decl();
        if self.eat(T::Eq) {
            self.eat(T::Pipe);
            self.variant();
            while self.eat(T::Pipe) {
                self.variant();
            }
        } else {
            self.expected("`=`");
        }
        self.finish_node();
    }

    fn variant(&mut self) {
        if !is_name(self.current()) {
            self.expected("a variant name");
            return;
        }
        self.start_node(NodeKind::Variant);
        self.bump();
        if self.at(T::LParen) {
            self.param_list();
        }
        self.finish_node();
    }

    fn fn_decl(&mut self) {
        self.start_node(NodeKind::FnDecl);
        self.bump();
        self.name("a function name");
        self.param_list();
        if self.eat(T::Arrow) {
            self.type_();
        } else {
            self.expected("`->`");
        }
        if self.eat(T::Eq) {
            self.expr();
        } else {
            self.expected("`=`");
        }
        self.finish_node();
    }

    pub(super) fn param_list(&mut self) {
        if !self.at(T::LParen) {
            self.expected("`(`");
            return;
        }
        self.start_node(NodeKind::ParamList);
        let opener = self.open();
        self.comma_list(opener, T::RParen, "a parameter", Self::param);
        self.finish_node();
    }

    fn param(&mut self) {
        if !self.at_binding_name("a parameter name") {
            return;
        }
        self.start_node(NodeKind::Param);
        self.binding_name();
        self.annotation(true);
        if self.eat(T::Eq) {
            self.expr();
        }
        self.finish_node();
    }

    fn let_decl(&mut self) {
        self.start_node(NodeKind::Let);
        self.bump();
        self.binding();
        self.finish_node();
    }

    /// Parses `name: Type = value` after `let`, with an optional type.
    pub(super) fn binding(&mut self) {
        if self.at_binding_name("a name") {
            self.binding_name();
        }
        self.annotation(false);
        if self.eat(T::Eq) {
            self.expr();
        } else {
            self.expected("`=`");
        }
    }

    /// Returns whether a name to bind follows, and reports `what` as
    /// expected otherwise.
    fn at_binding_name(&mut self, what: &str) -> bool {
        let kind = self.current();
        let found = is_name(kind) || kind == T::SymWord;
        if !found {
            self.expected(what);
        }
        found
    }

    /// Bumps the name of a field, parameter or binding. A symbol word there
    /// is almost always a name and type written without the space after
    /// `:`.
    fn binding_name(&mut self) {
        if self.at(T::SymWord) {
            let text = self.current_text();
            let help = text
                .split_once(':')
                .map(|(name, ty)| format!("write `{name}: {ty}` with a space after `:`"));
            let message = format!("expected a name, found symbol word `{text}`");
            self.error("E0206", self.current_span(), message, help);
        }
        self.bump();
    }

    /// Parses `: Type`, which may be left out unless `required`.
    fn annotation(&mut self, required: bool) {
        if !self.at(T::Colon) {
            if required {
                self.expected("`:`");
            }
            return;
        }
        let colon = self.nth_index(0);
        let before = self.tokens[colon.saturating_sub(1)];
        let after = self.tokens[colon + 1];
        if colon > 0 && is_name(before.kind) && is_name(after.kind) {
            let (name, ty) = (self.text(colon - 1), self.text(colon + 1));
            self.soft_error(
                "E0206",
                Span {
                    start: before.span.start,
                    end: after.span.end,
                },
                format!("expected `name: Type`, found `{name}:{ty}` without a space"),
                Some(format!("write `{name}: {ty}` with a space after `:`")),
            );
        }
        self.bump();
        self.type_();
    }

    fn if_stmt(&mut self) {
        self.start_node(NodeKind::IfStmt);
        self.bump();
        self.expr_no_struct();
        self.body();
        if self.eat(T::ElseKw) {
            if self.at(T::IfKw) {
                if self.enter() {
                    self.if_stmt();
                    self.leave();
                }
            } else {
                self.body();
            }
        }
        self.finish_node();
    }

    fn for_stmt(&mut self) {
        self.start_node(NodeKind::ForStmt);
        self.bump();
        self.loop_names();
        if self.eat(T::InKw) {
            self.expr_no_struct();
        } else {
            self.expected("`in`");
        }
        self.body();
        self.finish_node();
    }

    /// Parses the one or two names after `for`.
    pub(super) fn loop_names(&mut self) {
        self.name("a name");
        if self.eat(T::Comma) {
            self.name("a name");
        }
    }

    pub(super) fn body(&mut self) {
        if !self.at(T::LBrace) {
            self.expected("`{`");
            return;
        }
        self.start_node(NodeKind::Body);
        let opener = self.open();
        self.sequence(opener, T::RBrace, "a statement", |p| p.statement(false));
        self.finish_node();
    }

    /// Parses an element that starts with a name or a string: an attribute
    /// or a block in a body, or an entry in a literal.
    pub(super) fn keyed(&mut self, context: Keyed) {
        let checkpoint = self.checkpoint();
        let first = self.nth_index(0);
        let named = is_name(self.current());
        let interpolation = if named { None } else { self.string() };
        if named {
            self.bump();
        }
        match self.current() {
            T::Eq | T::Dot if context == Keyed::Statement => {
                if let Some(span) = interpolation {
                    self.attr_key_interpolation(span);
                }
                self.start_node_at(checkpoint, NodeKind::AttrPath);
                while self.eat(T::Dot) {
                    if is_string_start(self.current()) {
                        if let Some(span) = self.string() {
                            self.attr_key_interpolation(span);
                        }
                    } else if is_name(self.current()) {
                        self.bump();
                    } else {
                        self.expected("an attribute name");
                        break;
                    }
                }
                self.finish_node();
                self.start_node_at(checkpoint, NodeKind::Attr);
                if self.eat(T::Eq) {
                    self.expr();
                } else {
                    self.expected("`=`");
                }
                self.finish_node();
            }
            T::Eq => {
                self.start_node_at(checkpoint, NodeKind::MapEntry);
                self.bump();
                self.expr();
                self.finish_node();
            }
            kind if named
                && (kind == T::LBrace
                    || ((is_name(kind) || is_string_start(kind))
                        && (context == Keyed::Statement || self.brace_after_label()))) =>
            {
                if context == Keyed::Entry {
                    self.soft_error(
                        "E0202",
                        self.tokens[first].span,
                        "a block cannot appear inside a literal".to_owned(),
                        Some("write `key = { ... }` to set a nested value".to_owned()),
                    );
                }
                self.start_node_at(checkpoint, NodeKind::Block);
                if is_string_start(kind) {
                    self.string();
                } else if kind != T::LBrace {
                    self.bump();
                }
                if kind != T::LBrace && !self.at(T::LBrace) {
                    let block = self.text(first);
                    self.unexpected(
                        "E0201",
                        |found| format!("expected `{{` after the block label, found {found}"),
                        Some(format!(
                            "add a body in braces, or write `{block} = ...` to set an attribute"
                        )),
                    );
                }
                if self.at(T::LBrace) {
                    self.body();
                }
                self.finish_node();
            }
            _ => {
                if context == Keyed::Entry {
                    self.expected("`=`");
                } else if named {
                    let name = self.text(first);
                    self.unexpected(
                        "E0201",
                        |found| format!("expected `=` or `{{` after `{name}`, found {found}"),
                        None,
                    );
                } else {
                    self.unexpected(
                        "E0201",
                        |found| format!("expected `=` after the attribute key, found {found}"),
                        None,
                    );
                }
                self.start_node_at(checkpoint, NodeKind::Error);
                self.finish_node();
            }
        }
    }

    /// Returns whether a `{` follows the label at the current position.
    fn brace_after_label(&self) -> bool {
        let label = self.nth_index(0);
        let after = if is_string_start(self.tokens[label].kind) {
            self.layout.ends[label]
        } else {
            label + 1
        };
        let next = self.next_index(after);
        next < self.limit && self.tokens[next].kind == T::LBrace
    }

    fn attr_key_interpolation(&mut self, span: Span) {
        self.soft_error(
            "E0212",
            span,
            "an attribute key cannot contain interpolation".to_owned(),
            Some("write the key as a plain string".to_owned()),
        );
    }

    pub(super) fn type_(&mut self) {
        if !self.enter() {
            return;
        }
        let checkpoint = self.checkpoint();
        if self.base_type() && self.at(T::WhereKw) {
            self.start_node_at(checkpoint, NodeKind::RefinedType);
            self.bump();
            self.predicate();
            while self.eat(T::AndAnd) {
                self.predicate();
            }
            self.finish_node();
        }
        self.leave();
    }

    fn base_type(&mut self) -> bool {
        match self.current() {
            T::TypeIdent if self.current_text() == "Fn" && self.nth(1) == T::LParen => {
                self.fn_type();
            }
            T::TypeIdent | T::Ident => self.named_type(),
            _ => {
                self.expected("a type");
                return false;
            }
        }
        true
    }

    fn named_type(&mut self) {
        self.start_node(NodeKind::NamedType);
        self.start_node(NodeKind::TypePath);
        if self.at(T::Ident) && self.nth(1) == T::Dot {
            self.bump();
            self.bump();
            if self.at(T::TypeIdent) {
                self.bump();
            } else {
                self.expected("a type name");
            }
        } else {
            if self.at(T::Ident) {
                let name = self.current_text();
                self.soft_error(
                    "E0210",
                    self.current_span(),
                    format!("expected a type, found `{name}`"),
                    Some("type names start with an uppercase letter".to_owned()),
                );
            }
            self.bump();
        }
        self.finish_node();
        if self.at(T::Lt) {
            self.type_args();
        }
        self.finish_node();
    }

    fn type_args(&mut self) {
        self.start_node(NodeKind::TypeArgList);
        self.bump();
        loop {
            self.skip_newlines_before(&[T::TypeIdent, T::Ident, T::Gt]);
            self.type_();
            self.skip_newlines_before(&[T::Comma, T::Gt]);
            if self.eat(T::Comma) {
                self.skip_newlines_before(&[T::Gt]);
                if self.eat(T::Gt) {
                    break;
                }
                continue;
            }
            if !self.eat(T::Gt) {
                self.expected("`,` or `>`");
            }
            break;
        }
        self.finish_node();
    }

    fn fn_type(&mut self) {
        self.start_node(NodeKind::FnType);
        self.bump();
        let opener = self.open();
        self.comma_list(opener, T::RParen, "a type", Self::type_);
        if self.eat(T::Arrow) {
            self.type_();
        } else {
            self.expected("`->`");
        }
        self.finish_node();
    }

    fn predicate(&mut self) {
        match self.current() {
            T::Minus | T::Int | T::Float | T::DotDot | T::DotDotEq => self.range_predicate(),
            kind if is_name(kind) => {
                self.start_node(NodeKind::NamedPredicate);
                self.bump();
                while self.at(T::Dot) && is_name(self.nth(1)) {
                    self.bump();
                    self.bump();
                }
                if self.at(T::LParen) {
                    self.arg_list();
                }
                self.finish_node();
            }
            _ => self.expected("a predicate"),
        }
    }

    fn range_predicate(&mut self) {
        self.start_node(NodeKind::RangePredicate);
        if !matches!(self.current(), T::DotDot | T::DotDotEq) {
            self.range_bound();
        }
        if matches!(self.current(), T::DotDot | T::DotDotEq) {
            self.bump();
            if matches!(self.current(), T::Minus | T::Int | T::Float) {
                self.range_bound();
            }
        } else {
            self.expected("`..` or `..=`");
        }
        self.finish_node();
    }

    fn range_bound(&mut self) {
        if self.at(T::Minus) {
            self.start_node(NodeKind::UnaryExpr);
            self.bump();
            self.number();
            self.finish_node();
        } else {
            self.number();
        }
    }

    fn number(&mut self) {
        if matches!(self.current(), T::Int | T::Float) {
            self.start_node(NodeKind::Literal);
            self.bump();
            self.finish_node();
        } else {
            self.expected("a number");
        }
    }
}

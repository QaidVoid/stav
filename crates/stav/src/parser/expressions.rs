//! Expressions, strings and patterns.

use super::items::Keyed;
use super::{Parser, is_name, is_string_start};
use crate::span::Span;
use crate::syntax::NodeKind;
use crate::token::TokenKind as T;

const COALESCE: u8 = 1;
const OR: u8 = 2;
const AND: u8 = 3;
const COMPARE: u8 = 4;
const ADD: u8 = 5;
const MULTIPLY: u8 = 6;

/// Returns the precedence of a binary operator, higher binding tighter.
fn precedence(kind: T) -> Option<u8> {
    Some(match kind {
        T::QuestionQuestion => COALESCE,
        T::OrOr => OR,
        T::AndAnd => AND,
        T::EqEq | T::NotEq | T::Lt | T::LtEq | T::Gt | T::GtEq => COMPARE,
        T::Plus | T::Minus | T::PlusPlus => ADD,
        T::Star | T::Slash | T::Percent => MULTIPLY,
        _ => return None,
    })
}

impl Parser<'_> {
    pub(super) fn expr(&mut self) {
        self.expr_with(false);
    }

    /// Parses an expression in which a type name followed by `{` is not a
    /// struct literal, because the `{` opens the body or arms that follow,
    /// as in `if`, `for` and `match`.
    pub(super) fn expr_no_struct(&mut self) {
        self.expr_with(true);
    }

    fn expr_with(&mut self, no_struct: bool) {
        if !self.enter() {
            return;
        }
        match self.current() {
            T::LetKw => self.let_expr(no_struct),
            T::FnKw => self.lambda(no_struct),
            T::IfKw => self.if_expr(),
            T::MatchKw => self.match_expr(),
            T::PartKw => self.part_expr(),
            _ => self.binary(0, no_struct),
        }
        self.leave();
    }

    fn binary(&mut self, min: u8, no_struct: bool) {
        let checkpoint = self.checkpoint();
        if !self.unary(no_struct) {
            return;
        }
        let depth = self.depth;
        let mut compared = false;
        while let Some(prec) = precedence(self.current()) {
            if prec < min || !self.enter() {
                break;
            }
            if prec == COMPARE {
                if compared {
                    self.soft_error(
                        "E0207",
                        self.current_span(),
                        "comparison operators cannot be chained".to_owned(),
                        Some("join the comparisons with `&&`, as in `a < b && b < c`".to_owned()),
                    );
                }
                compared = true;
            }
            self.start_node_at(checkpoint, NodeKind::BinaryExpr);
            self.bump();
            // `??` is right associative, every other operator is left
            // associative.
            let next = if prec == COALESCE { prec } else { prec + 1 };
            self.binary(next, no_struct);
            self.finish_node();
        }
        self.depth = depth;
    }

    /// Parses a unary expression and returns whether it found one.
    fn unary(&mut self, no_struct: bool) -> bool {
        if !matches!(self.current(), T::Bang | T::Minus) {
            return self.postfix(no_struct);
        }
        if !self.enter() {
            return false;
        }
        self.start_node(NodeKind::UnaryExpr);
        self.bump();
        self.unary(no_struct);
        self.finish_node();
        self.leave();
        true
    }

    fn postfix(&mut self, no_struct: bool) -> bool {
        let checkpoint = self.checkpoint();
        if !self.primary(no_struct) {
            return false;
        }
        let depth = self.depth;
        loop {
            let kind = match self.current() {
                T::Dot | T::QuestionDot => NodeKind::FieldExpr,
                T::LBracket => NodeKind::IndexExpr,
                T::LParen => NodeKind::Call,
                _ => break,
            };
            if !self.enter() {
                break;
            }
            self.start_node_at(checkpoint, kind);
            match kind {
                NodeKind::FieldExpr => {
                    self.bump();
                    self.name("a field name");
                }
                NodeKind::IndexExpr => {
                    let opener = self.open();
                    self.expr();
                    self.skip_newlines_before(&[T::RBracket]);
                    self.close(opener, T::RBracket);
                }
                _ => self.arg_list(),
            }
            self.finish_node();
        }
        self.depth = depth;
        true
    }

    /// Parses a primary expression and returns whether it found one.
    fn primary(&mut self, no_struct: bool) -> bool {
        let kind = self.current();
        match kind {
            T::Int | T::Float | T::Path | T::TrueKw | T::FalseKw | T::NoneKw => {
                self.single(NodeKind::Literal);
            }
            T::SymWord => self.single(NodeKind::Symbol),
            T::SelfKw => self.single(NodeKind::SelfExpr),
            T::LBracket if self.nth(1) == T::ForKw => self.list_comp(),
            T::LBracket => self.list(),
            T::LBrace if self.nth(1) == T::ForKw => self.map_comp(),
            T::LBrace => self.map(),
            T::LParen => {
                self.start_node(NodeKind::Paren);
                let opener = self.open();
                self.expr();
                self.close(opener, T::RParen);
                self.finish_node();
            }
            _ if is_string_start(kind) => {
                self.string();
            }
            T::TypeIdent if !no_struct && self.nth(1) == T::LBrace => self.struct_lit(),
            T::Ident
                if !no_struct
                    && self.nth(1) == T::Dot
                    && self.nth(2) == T::TypeIdent
                    && self.nth(3) == T::LBrace =>
            {
                self.struct_lit();
            }
            T::Ident | T::TypeIdent => self.single(NodeKind::Name),
            T::LetKw | T::FnKw | T::IfKw | T::MatchKw | T::PartKw => {
                let keyword = self.current_text();
                let article = if kind == T::IfKw { "an" } else { "a" };
                self.soft_error(
                    "E0210",
                    self.current_span(),
                    format!("{article} `{keyword}` expression cannot be an operand"),
                    Some("wrap it in parentheses".to_owned()),
                );
                self.expr_with(no_struct);
            }
            _ => {
                self.expected("an expression");
                return false;
            }
        }
        true
    }

    fn single(&mut self, kind: NodeKind) {
        self.start_node(kind);
        self.bump();
        self.finish_node();
    }

    fn struct_lit(&mut self) {
        self.start_node(NodeKind::StructLit);
        self.start_node(NodeKind::TypePath);
        if self.at(T::Ident) {
            self.bump();
            self.bump();
        }
        self.bump();
        self.finish_node();
        self.map();
        self.finish_node();
    }

    /// Parses a string and returns the span of its first interpolation, if
    /// it has one.
    pub(super) fn string(&mut self) -> Option<Span> {
        self.start_node(NodeKind::Str);
        let end = self.end_of_string();
        self.bump_raw();
        let mut interpolation = None;
        while self.pos < end {
            if self.tokens[self.pos].kind == T::InterpStart {
                let span = self.interpolation();
                interpolation.get_or_insert(span);
            } else {
                self.bump_raw();
            }
        }
        self.finish_node();
        interpolation
    }

    /// Parses `#{ expression }` and returns its span.
    fn interpolation(&mut self) -> Span {
        self.start_node(NodeKind::Interp);
        let start = self.tokens[self.pos].span.start;
        let end = self.end_of_string();
        let terminated = self.tokens[end].kind == T::InterpEnd;
        self.bump_raw();
        // An interpolation cut off by the end of its line belongs to an
        // unterminated string, which the lexer reports.
        if !terminated {
            self.recovering = true;
        }
        let limit = self.limit;
        let delims = std::mem::take(&mut self.delims);
        self.limit = end;
        self.expr();
        if !self.at(T::Eof) {
            self.expected("`}`");
            self.start_node(NodeKind::Error);
            while !self.at(T::Eof) {
                self.bump();
            }
            self.finish_node();
        }
        self.flush();
        self.limit = limit;
        self.delims = delims;
        if terminated {
            self.bump_raw();
        }
        self.finish_node();
        Span {
            start,
            end: self.tokens[end].span.end,
        }
    }

    fn list(&mut self) {
        self.start_node(NodeKind::List);
        let opener = self.open();
        self.sequence(opener, T::RBracket, "an expression", Self::expr);
        self.finish_node();
    }

    fn map(&mut self) {
        self.start_node(NodeKind::Map);
        let opener = self.open();
        self.sequence(opener, T::RBrace, "a map entry", Self::entry);
        self.finish_node();
    }

    fn entry(&mut self) {
        let kind = self.current();
        if is_name(kind) || is_string_start(kind) {
            self.keyed(Keyed::Entry);
        } else {
            self.expected("a map entry");
        }
    }

    fn list_comp(&mut self) {
        self.start_node(NodeKind::ListComp);
        let opener = self.open();
        self.comprehension_head();
        self.expr();
        self.skip_newlines_before(&[T::RBracket]);
        self.close(opener, T::RBracket);
        self.finish_node();
    }

    fn map_comp(&mut self) {
        self.start_node(NodeKind::MapComp);
        let opener = self.open();
        self.comprehension_head();
        let kind = self.current();
        if is_string_start(kind) {
            self.string();
        } else if is_name(kind) {
            self.bump();
        } else {
            self.expected("a map key");
        }
        if self.eat(T::Eq) {
            self.expr();
        } else {
            self.expected("`=`");
        }
        self.skip_newlines_before(&[T::RBrace]);
        self.close(opener, T::RBrace);
        self.finish_node();
    }

    /// Parses `for names in source if guard ->` of a comprehension.
    fn comprehension_head(&mut self) {
        self.bump();
        self.loop_names();
        if self.eat(T::InKw) {
            self.expr();
        } else {
            self.expected("`in`");
        }
        self.skip_newlines_before(&[T::IfKw, T::Arrow]);
        if self.eat(T::IfKw) {
            self.expr();
            self.skip_newlines_before(&[T::Arrow]);
        }
        if !self.eat(T::Arrow) {
            self.expected("`->`");
        }
    }

    pub(super) fn arg_list(&mut self) {
        self.start_node(NodeKind::ArgList);
        let opener = self.open();
        let mut named: Vec<&str> = Vec::new();
        self.comma_list(opener, T::RParen, "an argument", |p| {
            if !is_expr_start(p.current()) {
                p.expected("an argument");
                return;
            }
            p.start_node(NodeKind::Arg);
            if is_name(p.current()) && p.nth(1) == T::Eq {
                let name = p.current_text();
                if named.contains(&name) {
                    p.soft_error(
                        "E0204",
                        p.current_span(),
                        format!("argument `{name}` is given more than once"),
                        Some("remove one of the arguments".to_owned()),
                    );
                }
                named.push(name);
                p.bump();
                p.bump();
            } else if !named.is_empty() {
                p.soft_error(
                    "E0203",
                    p.current_span(),
                    "positional argument after a named argument".to_owned(),
                    Some("move positional arguments before named ones".to_owned()),
                );
            }
            p.expr();
            p.finish_node();
        });
        self.finish_node();
    }

    fn if_expr(&mut self) {
        self.start_node(NodeKind::IfExpr);
        self.bump();
        self.expr_no_struct();
        self.branch();
        if self.eat(T::ElseKw) {
            if self.at(T::IfKw) {
                if self.enter() {
                    self.if_expr();
                    self.leave();
                }
            } else {
                self.branch();
            }
        } else {
            self.unexpected(
                "E0210",
                |found| format!("expected `else`, found {found}"),
                Some("an `if` expression needs an `else` branch".to_owned()),
            );
        }
        self.finish_node();
    }

    fn branch(&mut self) {
        if !self.at(T::LBrace) {
            self.expected("`{`");
            return;
        }
        let opener = self.open();
        self.expr();
        self.skip_newlines_before(&[T::RBrace]);
        self.close(opener, T::RBrace);
    }

    fn match_expr(&mut self) {
        self.start_node(NodeKind::MatchExpr);
        self.bump();
        self.expr_no_struct();
        if self.at(T::LBrace) {
            let opener = self.open();
            self.sequence(opener, T::RBrace, "a match arm", Self::arm);
        } else {
            self.expected("`{`");
        }
        self.finish_node();
    }

    fn arm(&mut self) {
        if !is_pattern_start(self.current()) {
            self.expected("a match arm");
            return;
        }
        self.start_node(NodeKind::MatchArm);
        self.pattern();
        if self.eat(T::IfKw) {
            self.expr();
        }
        if self.eat(T::FatArrow) {
            self.expr();
        } else {
            self.expected("`=>`");
        }
        self.finish_node();
    }

    fn let_expr(&mut self, no_struct: bool) {
        self.start_node(NodeKind::LetExpr);
        self.bump();
        loop {
            if !matches!(self.current(), T::Ident | T::TypeIdent | T::SymWord) {
                self.expected("a name");
                break;
            }
            self.start_node(NodeKind::Binding);
            self.binding();
            self.finish_node();
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.skip_newlines_before(&[T::InKw]);
        if self.eat(T::InKw) {
            self.expr_with(no_struct);
        } else {
            self.expected("`in`");
        }
        self.finish_node();
    }

    fn lambda(&mut self, no_struct: bool) {
        self.start_node(NodeKind::Lambda);
        self.bump();
        self.param_list();
        if self.eat(T::Arrow) {
            self.type_();
        }
        if self.eat(T::Eq) {
            self.expr_with(no_struct);
        } else {
            self.expected("`=`");
        }
        self.finish_node();
    }

    fn part_expr(&mut self) {
        self.start_node(NodeKind::PartExpr);
        self.bump();
        self.body();
        self.finish_node();
    }

    fn pattern(&mut self) {
        if !self.enter() {
            return;
        }
        let checkpoint = self.checkpoint();
        if self.alternative() && self.at(T::Pipe) {
            self.start_node_at(checkpoint, NodeKind::OrPat);
            while self.eat(T::Pipe) {
                self.alternative();
            }
            self.finish_node();
        }
        self.leave();
    }

    /// Parses a pattern without `|` and returns whether it found one.
    fn alternative(&mut self) -> bool {
        let kind = self.current();
        match kind {
            T::Ident if self.current_text() == "_" => self.single(NodeKind::WildcardPat),
            T::Int | T::TrueKw | T::FalseKw | T::NoneKw => {
                self.start_node(NodeKind::LiteralPat);
                self.single(NodeKind::Literal);
                self.finish_node();
            }
            T::Minus => {
                self.start_node(NodeKind::LiteralPat);
                self.start_node(NodeKind::UnaryExpr);
                self.bump();
                if self.at(T::Int) {
                    self.single(NodeKind::Literal);
                } else {
                    self.expected("an integer");
                }
                self.finish_node();
                self.finish_node();
            }
            _ if is_string_start(kind) => {
                self.start_node(NodeKind::LiteralPat);
                self.string();
                self.finish_node();
            }
            T::Ident | T::TypeIdent => {
                let checkpoint = self.checkpoint();
                self.bump();
                while self.at(T::Dot) && is_name(self.nth(1)) {
                    self.bump();
                    self.bump();
                }
                if self.at(T::LParen) {
                    self.start_node_at(checkpoint, NodeKind::VariantPat);
                    let opener = self.open();
                    let mut rest = false;
                    self.comma_list(opener, T::RParen, "a field pattern", |p| {
                        if rest {
                            p.soft_error(
                                "E0210",
                                p.current_span(),
                                "`..` must come last in a variant pattern".to_owned(),
                                Some("move `..` to the end".to_owned()),
                            );
                        }
                        if p.at(T::DotDot) {
                            p.bump();
                            rest = true;
                        } else if is_name(p.current()) {
                            p.start_node(NodeKind::FieldPat);
                            p.bump();
                            if p.eat(T::Eq) {
                                p.pattern();
                            }
                            p.finish_node();
                        } else {
                            p.expected("a field pattern");
                        }
                    });
                } else {
                    self.start_node_at(checkpoint, NodeKind::NamePat);
                }
                self.finish_node();
            }
            _ => {
                self.expected("a pattern");
                return false;
            }
        }
        true
    }
}

fn is_pattern_start(kind: T) -> bool {
    is_name(kind)
        || is_string_start(kind)
        || matches!(kind, T::Int | T::Minus | T::TrueKw | T::FalseKw | T::NoneKw)
}

fn is_expr_start(kind: T) -> bool {
    is_name(kind)
        || is_string_start(kind)
        || matches!(
            kind,
            T::Int
                | T::Float
                | T::Path
                | T::TrueKw
                | T::FalseKw
                | T::NoneKw
                | T::SymWord
                | T::SelfKw
                | T::LBracket
                | T::LBrace
                | T::LParen
                | T::LetKw
                | T::FnKw
                | T::IfKw
                | T::MatchKw
                | T::PartKw
                | T::Bang
                | T::Minus
        )
}

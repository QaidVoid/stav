//! Stav, a statically typed, eager and total configuration language.
//!
//! A Stav program is a set of `.stav` files written in block syntax. The
//! library type checks the whole program before evaluating it, merges the
//! contributions of every file by priority, and produces a value that can be
//! rendered or converted to Rust types.
//!
//! The entry points so far are [`lex`], which splits source text into
//! tokens, and [`parse`], which builds a lossless [`SyntaxTree`] with a typed
//! view in [`ast`].

#![forbid(unsafe_code)]

pub mod ast;
mod diagnostic;
mod lexer;
mod parser;
mod span;
mod syntax;
mod token;

pub use diagnostic::Diagnostic;
pub use lexer::{Lexed, lex};
pub use parser::{MAX_DEPTH, Parsed, parse};
pub use span::Span;
pub use syntax::{NodeKind, SyntaxElement, SyntaxNode, SyntaxToken, SyntaxTree};
pub use token::{Token, TokenKind};

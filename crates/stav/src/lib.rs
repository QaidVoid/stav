//! Stav, a statically typed, eager and total configuration language.
//!
//! A Stav program is a set of `.stav` files written in block syntax. The
//! library type checks the whole program before evaluating it, merges the
//! contributions of every file by priority, and produces a value that can be
//! rendered or converted to Rust types.
//!
//! The entry point so far is [`lex`], which splits source text into tokens.

#![forbid(unsafe_code)]

mod diagnostic;
mod lexer;
mod span;
mod token;

pub use diagnostic::Diagnostic;
pub use lexer::{Lexed, lex};
pub use span::Span;
pub use token::{Token, TokenKind};

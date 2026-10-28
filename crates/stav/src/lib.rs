//! Stav, a statically typed, eager and total configuration language.
//!
//! A Stav program is a set of `.stav` files written in block syntax. The
//! library type checks the whole program before evaluating it, merges the
//! contributions of every file by priority, and produces a value that can be
//! rendered or converted to Rust types.
//!
//! The entry points so far are [`lex`], which splits source text into
//! tokens, [`parse`], which builds a lossless [`SyntaxTree`] with a typed
//! view in [`ast`], [`format()`], which lays source text out in its
//! canonical form, and [`check`], which loads the files of a root through a
//! host's [`Loader`] and type checks the whole program.

#![forbid(unsafe_code)]

pub mod ast;
mod check;
mod diagnostic;
mod format;
mod hash;
mod lexer;
mod parser;
mod span;
mod syntax;
mod token;

pub use check::{
    Checked, FileId, Host, HostModule, ImportRoot, LoadError, Loader, MemoryLoader, RelPath,
    RootSpec, SchemaField, SchemaIndex, SchemaStruct, SchemaSum, SchemaVariant, Source, SymbolType,
    check,
};
pub use diagnostic::{Diagnostic, Diagnostics, FileDiagnostic, Label, Severity};
pub use format::format;
pub use lexer::{Lexed, lex};
pub use parser::{MAX_DEPTH, Parsed, parse};
pub use span::Span;
pub use syntax::{NodeKind, SyntaxElement, SyntaxNode, SyntaxToken, SyntaxTree};
pub use token::{Token, TokenKind};

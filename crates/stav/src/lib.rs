//! Stav, a statically typed, eager and total configuration language.
//!
//! A Stav program is a set of `.stav` files written in block syntax. This
//! crate is the whole language: it parses, formats, type checks and
//! evaluates programs, merges the contributions of every file by priority,
//! and turns the result into a [`Value`] that renders to JSON, TOML or YAML
//! or converts to Rust types through serde.
//!
//! Evaluation always terminates, reads nothing except the files the host
//! serves, and gives the same value for the same inputs. The whole program
//! is type checked before any of it is evaluated. The crate performs no
//! I/O of its own.
//!
//! # Embedding
//!
//! A [`Host`] is what the embedding program contributes: a [`Loader`] that
//! serves source files, and optionally a prelude, standard modules, symbol
//! types and host modules. A [`RootSpec`] names the root struct, the entry
//! files and the import roots. [`check`] loads and type checks the program,
//! [`Checked::eval`] evaluates it into an [`Evaluated`] value, and
//! [`Evaluated::get`] converts an attribute into any type that implements
//! [`serde::Deserialize`]. Every failure is reported as [`Diagnostics`].
//!
//! ```
//! use serde::Deserialize;
//! use stav::{AttrPath, Format, Host, ImportRoot, Limits, MemoryLoader, RootSpec};
//!
//! #[derive(Deserialize)]
//! struct Server {
//!     name: String,
//!     port: u16,
//! }
//!
//! let source = "struct Server {\n  name: Str\n  port: Int where 1..=65535 = 80\n}\n\
//!               struct Root { server: Server }\n\
//!               server { name = \"web\", port = 8080 }\n";
//! let host = Host::new(MemoryLoader::new().with_file("app/main.stav", source));
//! let root = RootSpec {
//!     root_struct: "Root".to_owned(),
//!     entries: vec!["app/main.stav".into()],
//!     import_roots: vec![ImportRoot::new("app")],
//! };
//! let evaluated = stav::check(&host, &root)?.eval(&Limits::default())?;
//!
//! let server: Server = evaluated.get(&AttrPath::new(["server"]))?;
//! assert_eq!((server.name.as_str(), server.port), ("web", 8080));
//! assert_eq!(
//!     evaluated.render(Format::Toml)?,
//!     "[server]\nname = \"web\"\nport = 8080\n"
//! );
//! # Ok::<(), stav::Diagnostics>(())
//! ```
//!
//! # Other entry points
//!
//! [`lex`] splits source text into tokens, [`parse`] builds a lossless
//! [`SyntaxTree`] with a typed view in [`ast`], and [`format()`] lays source
//! text out in its canonical form.

#![forbid(unsafe_code)]

pub mod ast;
mod check;
mod de;
mod diagnostic;
mod eval;
mod format;
mod lexer;
mod parser;
mod render;
mod span;
mod syntax;
mod token;
mod value;

pub use check::{
    Checked, FileId, Host, HostModule, ImplicitRefs, ImportRoot, LoadError, Loader, MAX_SOURCE_LEN,
    MemoryLoader, RelPath, RootSpec, SchemaField, SchemaIndex, SchemaStruct, SchemaSum,
    SchemaVariant, Source, SymbolType, check,
};
pub use de::ConvertError;
pub use diagnostic::{Diagnostic, Diagnostics, FileDiagnostic, Label, Severity};
pub use eval::{Evaluated, Limits};
pub use format::format;
pub use lexer::{Lexed, lex};
pub use parser::{MAX_DEPTH, Parsed, parse};
pub use render::{Format, RenderError};
pub use span::{SourceSpan, Span};
pub use syntax::{NodeKind, SyntaxElement, SyntaxNode, SyntaxToken, SyntaxTree};
pub use token::{Token, TokenKind};
pub use value::{AttrPath, AttrPathError, Value};

/// Compiles the Rust examples of the repository's README as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;

//! Stav, a statically typed, eager and total configuration language.
//!
//! A Stav program is a set of `.stav` files written in block syntax. The
//! library type checks the whole program before evaluating it, merges the
//! contributions of every file by priority, and produces a value that can be
//! rendered or converted to Rust types.
//!
//! The entry points are [`lex`], which splits source text into tokens,
//! [`parse`], which builds a lossless [`SyntaxTree`] with a typed view in
//! [`ast`], [`format()`], which lays source text out in its canonical form,
//! [`check`], which loads the files of a root through a host's [`Loader`]
//! and type checks the whole program, and [`Checked::eval`], which
//! evaluates a checked program into an [`Evaluated`] value.
//!
//! # Examples
//!
//! ```
//! use stav::{Format, Host, ImportRoot, Limits, MemoryLoader, RootSpec};
//!
//! let source = "struct Root {\n  name: Str\n  port: Int where 1..=65535 = 80\n}\n\
//!               name = \"web\"\nport = 8080\n";
//! let host = Host::new(MemoryLoader::new().with_file("app/main.stav", source));
//! let root = RootSpec {
//!     root_struct: "Root".to_owned(),
//!     entries: vec!["app/main.stav".into()],
//!     import_roots: vec![ImportRoot::new("app")],
//! };
//! let evaluated = stav::check(&host, &root)?.eval(&Limits::default())?;
//! assert_eq!(
//!     evaluated.render(Format::Toml)?,
//!     "name = \"web\"\nport = 8080\n"
//! );
//! # Ok::<(), stav::Diagnostics>(())
//! ```

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
    Checked, FileId, Host, HostModule, ImplicitRefs, ImportRoot, LoadError, Loader, MemoryLoader,
    RelPath, RootSpec, SchemaField, SchemaIndex, SchemaStruct, SchemaSum, SchemaVariant, Source,
    SymbolType, check,
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

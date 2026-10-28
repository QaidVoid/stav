//! Rendering values as JSON, TOML and YAML.

pub(crate) mod json;
pub(crate) mod toml;
pub(crate) mod yaml;

use std::fmt;

use crate::value::{AttrPath, Value};

/// A text format that values render to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// JSON, pretty printed.
    Json,
    /// TOML, which needs a table at the top.
    Toml,
    /// YAML 1.2 in block style.
    Yaml,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Format::Json => "JSON",
            Format::Toml => "TOML",
            Format::Yaml => "YAML",
        })
    }
}

/// Why a value cannot be rendered in a format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderError {
    /// The attribute whose value the format cannot express.
    pub path: AttrPath,
    /// What the format cannot express, starting lowercase and without a
    /// trailing period.
    pub message: String,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.keys().is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "`{}`: {}", self.path, self.message)
        }
    }
}

impl std::error::Error for RenderError {}

/// The text of a value that renders as a string in every format: a string,
/// a symbol, a reference, a path or a variant without payload.
pub(crate) fn text(value: &Value) -> Option<std::borrow::Cow<'_, str>> {
    Some(match value {
        Value::Str(s) | Value::Ref(s) | Value::Symbol { text: s, .. } => s.as_str().into(),
        Value::Path(p) => p.to_string_lossy(),
        Value::Variant {
            name, fields: None, ..
        } => name.as_str().into(),
        _ => return None,
    })
}

/// The entries of a value that renders as an object, a table or a
/// mapping: a struct, a map, or a variant with payload as an object with
/// the variant name as its one key.
pub(crate) enum Entries<'a> {
    Fields(&'a [(String, Value)]),
    Variant(&'a str, &'a [(String, Value)]),
}

impl<'a> Entries<'a> {
    pub(crate) fn of(value: &'a Value) -> Option<Self> {
        match value {
            Value::Struct { fields, .. } | Value::Map(fields) => Some(Entries::Fields(fields)),
            Value::Variant {
                name,
                fields: Some(fields),
                ..
            } => Some(Entries::Variant(name, fields)),
            _ => None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Entries::Fields(fields) => fields.len(),
            Entries::Variant(..) => 1,
        }
    }

    /// Calls `f` with every key and value. The value of a variant's one
    /// key is its payload, given as entries.
    pub(crate) fn each<E>(
        &self,
        mut f: impl FnMut(&'a str, Item<'a>) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Entries::Fields(fields) => {
                for (key, value) in fields.iter() {
                    f(key, Item::Value(value))?;
                }
                Ok(())
            }
            Entries::Variant(name, fields) => f(name, Item::Payload(fields)),
        }
    }
}

/// A value inside [`Entries`]: a value, or the payload fields of a variant.
#[derive(Clone, Copy)]
pub(crate) enum Item<'a> {
    Value(&'a Value),
    Payload(&'a [(String, Value)]),
}

impl<'a> Item<'a> {
    pub(crate) fn entries(self) -> Option<Entries<'a>> {
        match self {
            Item::Value(value) => Entries::of(value),
            Item::Payload(fields) => Some(Entries::Fields(fields)),
        }
    }

    pub(crate) fn value(self) -> Option<&'a Value> {
        match self {
            Item::Value(value) => Some(value),
            Item::Payload(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;

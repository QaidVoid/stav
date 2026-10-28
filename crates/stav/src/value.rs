//! The values Stav programs evaluate to.

use std::fmt;
use std::path::PathBuf;

use crate::render::{self, Format, RenderError};

/// A value of a Stav program, as an evaluated program produces it.
///
/// Values are finite trees of data. They hold no functions and no parts,
/// because the type checker keeps those out of everything a program
/// produces. Every collection is ordered: maps and sets in their canonical
/// order, struct fields in declaration order, so that the same program
/// always produces the same value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// The absence of a value: `none`, or an `Option` that nothing set.
    None,
    /// A `Bool`.
    Bool(bool),
    /// An `Int`, a signed 64-bit integer.
    Int(i64),
    /// A `Float`, always finite.
    Float(f64),
    /// A `Str`.
    Str(String),
    /// A `Path`: the import root of the file that wrote the path literal,
    /// joined with the path normalized inside it.
    Path(PathBuf),
    /// A value of a host symbol type.
    Symbol {
        /// The name of the symbol type, for example `Pkg`.
        ty: String,
        /// The canonical text of the symbol, as the host's parser
        /// returned it.
        text: String,
    },
    /// A `Ref<T>`: the label of a block of struct `T`.
    Ref(String),
    /// A `List<T>`, in order.
    List(Vec<Value>),
    /// A `Set<T>`, in canonical order and without duplicates.
    Set(Vec<Value>),
    /// A `Map<K, V>`, in canonical key order. A key is a string, or the
    /// name of a variant for a map keyed by a sum type.
    Map(Vec<(String, Value)>),
    /// A value of a struct, with every field in declaration order.
    Struct {
        /// The name of the struct.
        name: String,
        /// The fields in declaration order.
        fields: Vec<(String, Value)>,
    },
    /// A variant of a sum type.
    Variant {
        /// The name of the sum type.
        ty: String,
        /// The name of the variant.
        name: String,
        /// The payload fields in declaration order, or `None` for a
        /// variant without payload.
        fields: Option<Vec<(String, Value)>>,
    },
}

impl Value {
    /// Returns the field of a struct, the entry of a map or the payload
    /// field of a variant with the given name.
    pub fn get(&self, key: &str) -> Option<&Value> {
        let entries = match self {
            Value::Struct { fields, .. } => fields,
            Value::Map(entries) => entries,
            Value::Variant {
                fields: Some(fields),
                ..
            } => fields,
            _ => return None,
        };
        entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Returns the value at `path` inside this one.
    ///
    /// Each key names a struct field or a map key. A key that names the
    /// variant of a value with payload steps into its payload fields, the
    /// way the variant is rendered: `build.cargo.features`.
    pub fn at(&self, path: &AttrPath) -> Option<&Value> {
        let mut current = self;
        let mut keys = path.keys().iter();
        while let Some(key) = keys.next() {
            current = match current {
                Value::Variant {
                    name,
                    fields: Some(_),
                    ..
                } if name == key => match keys.next() {
                    Some(field) => current.get(field)?,
                    None => return Some(current),
                },
                Value::Variant { .. } => return None,
                _ => current.get(key)?,
            };
        }
        Some(current)
    }

    /// Renders the value in `format`.
    ///
    /// JSON is pretty printed with two spaces of indentation. Struct fields
    /// keep their declaration order and maps their canonical order. `none`
    /// becomes `null`, a variant without payload its name, and a variant
    /// with payload an object with one key, the variant name, whose value
    /// holds the payload fields. Symbols, references and paths become
    /// strings.
    ///
    /// TOML has no null, so a field or map entry that is `none` is left
    /// out. Keys with plain values come before the tables of a table.
    /// Lists whose elements are all tables become arrays of tables.
    ///
    /// YAML uses the block style and the YAML 1.2 core schema. Strings that
    /// would read as another type, such as `"yes"`, `"1.0"` or `"null"`,
    /// are quoted.
    ///
    /// # Errors
    ///
    /// Returns a [`RenderError`] when the value cannot be expressed in TOML:
    /// when it is not a table, or when it contains a list that mixes tables
    /// with other values or that holds `none`. JSON and YAML can express
    /// every value.
    ///
    /// # Examples
    ///
    /// ```
    /// use stav::{Format, Value};
    ///
    /// let value = Value::Map(vec![
    ///     ("name".to_owned(), Value::Str("forge".to_owned())),
    ///     ("port".to_owned(), Value::Int(22)),
    /// ]);
    /// assert_eq!(value.render(Format::Toml).unwrap(), "name = \"forge\"\nport = 22\n");
    /// assert_eq!(value.render(Format::Yaml).unwrap(), "name: forge\nport: 22\n");
    /// ```
    pub fn render(&self, format: Format) -> Result<String, RenderError> {
        match format {
            Format::Json => Ok(render::json::pretty(self)),
            Format::Toml => render::toml::render(self),
            Format::Yaml => Ok(render::yaml::render(self)),
        }
    }

    /// Returns the value as canonical JSON in the form of RFC 8785: no
    /// whitespace, object keys sorted by their UTF-16 code units, and
    /// floats written the way ECMAScript writes numbers. Integers are
    /// written exactly, even beyond the range a double holds without loss.
    ///
    /// The same value always gives the same bytes, so hosts can hash them.
    pub fn to_canonical_json(&self) -> Vec<u8> {
        render::json::canonical(self).into_bytes()
    }
}

/// A path to an attribute of a value: struct field names, map keys and
/// variant names, from the root down.
///
/// The [`Display`](fmt::Display) form separates keys with dots and quotes
/// keys that are not identifiers, the way attribute statements write them:
/// `sysctl."vm.swappiness"`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttrPath {
    keys: Vec<String>,
}

impl AttrPath {
    /// Returns the path to the root value.
    pub fn root() -> Self {
        Self::default()
    }

    /// Returns the path with the given keys, from the root down.
    ///
    /// # Examples
    ///
    /// ```
    /// use stav::AttrPath;
    ///
    /// let path = AttrPath::new(["sysctl", "vm.swappiness"]);
    /// assert_eq!(path.to_string(), "sysctl.\"vm.swappiness\"");
    /// ```
    pub fn new<I>(keys: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        Self {
            keys: keys.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns the keys of the path, from the root down.
    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// Returns the path extended by one key.
    pub fn join(&self, key: impl Into<String>) -> Self {
        let mut keys = self.keys.clone();
        keys.push(key.into());
        Self { keys }
    }
}

impl fmt::Display for AttrPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, key) in self.keys.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            let bare = !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if bare {
                f.write_str(key)?;
            } else {
                write!(f, "{key:?}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "value_tests.rs"]
mod tests;

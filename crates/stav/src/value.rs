//! The values Stav programs evaluate to.

use std::fmt;
use std::iter::Peekable;
use std::path::PathBuf;
use std::str::{Chars, FromStr};

use crate::ast::unescape;
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
    /// The same value always gives the same text, so hosts can hash its
    /// bytes.
    pub fn to_canonical_json(&self) -> String {
        render::json::canonical(self)
    }

    /// Returns a short description of the value for diagnostics, cut off
    /// after about 40 characters.
    pub(crate) fn describe(&self) -> String {
        let mut text = String::new();
        describe(self, &mut text);
        if text.chars().count() > 40 {
            let cut: String = text.chars().take(37).collect();
            return format!("{cut}...");
        }
        text
    }
}

fn describe(value: &Value, out: &mut String) {
    if out.len() > 60 {
        return;
    }
    match value {
        Value::None => out.push_str("none"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) => out.push_str(&format!("{f:?}")),
        Value::Str(s) => out.push_str(&format!("{s:?}")),
        Value::Path(p) => out.push_str(&p.display().to_string()),
        Value::Symbol { text, .. } => out.push_str(text),
        Value::Ref(label) => out.push_str(label),
        Value::List(items) | Value::Set(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                describe(item, out);
            }
            out.push(']');
        }
        Value::Map(entries)
        | Value::Struct {
            fields: entries, ..
        } => {
            if let Value::Struct { name, .. } = value {
                out.push_str(name);
                out.push(' ');
            }
            out.push('{');
            for (i, (key, item)) in entries.iter().enumerate() {
                out.push_str(if i > 0 { ", " } else { " " });
                out.push_str(key);
                out.push_str(" = ");
                describe(item, out);
            }
            out.push_str(if entries.is_empty() { "}" } else { " }" });
        }
        Value::Variant { name, fields, .. } => {
            out.push_str(name);
            if let Some(fields) = fields {
                out.push('(');
                for (i, (key, item)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(key);
                    out.push_str(" = ");
                    describe(item, out);
                }
                out.push(')');
            }
        }
    }
}

/// A path to an attribute of a value: struct field names, map keys and
/// variant names, from the root down.
///
/// The [`Display`](fmt::Display) form separates keys with dots and writes
/// a key bare when it is not empty and consists of ASCII letters, digits,
/// `_` and `-`. Any other key is written in double quotes with the escapes
/// of a string literal: `\\`, `\"`, `\n`, `\t`, `\r`, `\0`, `\#` before
/// `{`, and `\u{...}` for other control characters. For example the keys
/// `sysctl` and `vm.swappiness` display as `sysctl."vm.swappiness"`.
/// [`FromStr`] parses that form back into the same path.
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
            if !key.is_empty() && key.chars().all(is_bare) {
                f.write_str(key)?;
                continue;
            }
            f.write_str("\"")?;
            let mut chars = key.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => f.write_str("\\\\")?,
                    '"' => f.write_str("\\\"")?,
                    '\n' => f.write_str("\\n")?,
                    '\t' => f.write_str("\\t")?,
                    '\r' => f.write_str("\\r")?,
                    '\0' => f.write_str("\\0")?,
                    '#' if chars.peek() == Some(&'{') => f.write_str("\\#")?,
                    c if c.is_control() => write!(f, "\\u{{{:x}}}", u32::from(c))?,
                    c => write!(f, "{c}")?,
                }
            }
            f.write_str("\"")?;
        }
        Ok(())
    }
}

/// Parses the [`Display`](fmt::Display) form of a path.
///
/// The empty string is the root path. Otherwise keys are separated by
/// dots. A bare key consists of ASCII letters, digits, `_` and `-`. A key
/// in double quotes may contain any character, including none, and takes
/// the escapes of a string literal.
///
/// # Errors
///
/// Returns an [`AttrPathError`] when a bare key is empty, as in `a..b`,
/// `.a` or `a.`, when a bare key has a character that needs quotes, when
/// a quoted key is not terminated, has an unknown escape or an invalid
/// `\u{...}` escape, or is followed by something other than `.`.
///
/// # Examples
///
/// ```
/// use stav::AttrPath;
///
/// let path: AttrPath = r#"sysctl."vm.swappiness""#.parse()?;
/// assert_eq!(path.keys(), ["sysctl", "vm.swappiness"]);
/// assert_eq!(path.to_string().parse::<AttrPath>()?, path);
/// # Ok::<(), stav::AttrPathError>(())
/// ```
impl FromStr for AttrPath {
    type Err = AttrPathError;

    fn from_str(text: &str) -> Result<Self, AttrPathError> {
        let mut keys = Vec::new();
        if text.is_empty() {
            return Ok(Self { keys });
        }
        let mut chars = text.chars().peekable();
        loop {
            let key = if chars.next_if_eq(&'"').is_some() {
                quoted_key(&mut chars)?
            } else {
                let mut key = String::new();
                while let Some(c) = chars.next_if(|&c| c != '.') {
                    key.push(c);
                }
                if key.is_empty() {
                    return Err(error("empty key".to_owned()));
                }
                if !key.chars().all(is_bare) {
                    return Err(error(format!("`{key}` must be quoted")));
                }
                key
            };
            keys.push(key);
            match chars.next() {
                None => return Ok(Self { keys }),
                Some('.') => {}
                Some(c) => {
                    return Err(error(format!(
                        "expected `.` after a quoted key, found `{c}`"
                    )));
                }
            }
        }
    }
}

/// Reads a quoted key after its opening quote, up to and including its
/// closing quote.
fn quoted_key(chars: &mut Peekable<Chars<'_>>) -> Result<String, AttrPathError> {
    let mut key = String::new();
    loop {
        match chars.next() {
            None => return Err(error("unterminated quoted key".to_owned())),
            Some('"') => return Ok(key),
            Some('\\') => {
                let mut escape = String::from("\\");
                escape.extend(chars.next());
                if escape == "\\u" {
                    while let Some(c) = chars.next_if(|&c| c != '"') {
                        escape.push(c);
                        if c == '}' {
                            break;
                        }
                    }
                }
                match unescape(&escape) {
                    Some(c) => key.push(c),
                    None if escape.starts_with("\\u") => {
                        return Err(error(format!("invalid unicode escape `{escape}`")));
                    }
                    None => return Err(error(format!("unknown escape `{escape}`"))),
                }
            }
            Some(c) => key.push(c),
        }
    }
}

fn is_bare(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn error(message: String) -> AttrPathError {
    AttrPathError { message }
}

/// Why a text is not the [`Display`](fmt::Display) form of an
/// [`AttrPath`]. Its [`Display`](fmt::Display) form says what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrPathError {
    message: String,
}

impl fmt::Display for AttrPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AttrPathError {}

#[cfg(test)]
#[path = "value_tests.rs"]
mod tests;

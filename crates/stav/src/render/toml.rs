//! TOML, with tables for structs, maps and variants with payload.

use std::fmt::Write;

use super::{Entries, Item, RenderError, text};
use crate::value::{AttrPath, Value};

/// Where an entry of a table goes.
enum Place<'a> {
    /// Nowhere: the entry is `none`, which TOML cannot express.
    Omitted,
    /// After the key, on the line of the key.
    Inline,
    /// Under a `[header]` of its own.
    Table(Entries<'a>),
    /// Under one `[[header]]` per element.
    Array(&'a [Value]),
}

/// Returns the value as a TOML document.
pub(crate) fn render(value: &Value) -> Result<String, RenderError> {
    let Some(entries) = Entries::of(value) else {
        return Err(RenderError {
            path: AttrPath::root(),
            message: "TOML needs a table at the top".to_owned(),
        });
    };
    let mut out = String::new();
    table(&entries, &mut Vec::new(), &mut out)?;
    Ok(out)
}

fn error(path: &[String], message: &str) -> RenderError {
    RenderError {
        path: AttrPath::new(path.iter().cloned()),
        message: message.to_owned(),
    }
}

fn place<'a>(item: Item<'a>, path: &[String]) -> Result<Place<'a>, RenderError> {
    if let Some(entries) = item.entries() {
        return Ok(Place::Table(entries));
    }
    match item.value() {
        Some(Value::None) | None => Ok(Place::Omitted),
        Some(Value::List(items) | Value::Set(items)) => {
            check_list(items, path)?;
            if !items.is_empty() && items.iter().all(|i| Entries::of(i).is_some()) {
                Ok(Place::Array(items))
            } else {
                Ok(Place::Inline)
            }
        }
        Some(_) => Ok(Place::Inline),
    }
}

/// Reports a list that TOML cannot express: one that holds `none`, or
/// that mixes tables with other values.
fn check_list(items: &[Value], path: &[String]) -> Result<(), RenderError> {
    if items.iter().any(|i| matches!(i, Value::None)) {
        return Err(error(
            path,
            "the list holds `none`, which TOML cannot express",
        ));
    }
    let tables = items.iter().filter(|i| Entries::of(i).is_some()).count();
    if tables != 0 && tables != items.len() {
        return Err(error(
            path,
            "the list mixes tables with other values, which TOML cannot express",
        ));
    }
    Ok(())
}

fn table(
    entries: &Entries<'_>,
    path: &mut Vec<String>,
    out: &mut String,
) -> Result<(), RenderError> {
    let mut later: Vec<(&str, Place<'_>)> = Vec::new();
    entries.each(|key, item| {
        path.push(key.to_owned());
        let found = place(item, path);
        path.pop();
        match found? {
            Place::Omitted => {}
            Place::Inline => {
                path.push(key.to_owned());
                let value = item.value().map(|v| inline(v, path)).transpose();
                path.pop();
                if let Some(value) = value? {
                    write_key(key, out);
                    out.push_str(" = ");
                    out.push_str(&value);
                    out.push('\n');
                }
            }
            nested => later.push((key, nested)),
        }
        Ok(())
    })?;
    for (key, nested) in later {
        path.push(key.to_owned());
        match nested {
            Place::Table(sub) => {
                if needs_header(&sub, path)? {
                    header(path, "[", "]", out);
                }
                table(&sub, path, out)?;
            }
            Place::Array(items) => {
                for item in items {
                    header(path, "[[", "]]", out);
                    if let Some(sub) = Entries::of(item) {
                        table(&sub, path, out)?;
                    }
                }
            }
            Place::Omitted | Place::Inline => {}
        }
        path.pop();
    }
    Ok(())
}

/// Returns whether a table needs a header of its own: when it has keys
/// with values on their own lines, or no entries at all, so that it still
/// exists. A table with only nested tables is implied by their headers.
fn needs_header(entries: &Entries<'_>, path: &mut Vec<String>) -> Result<bool, RenderError> {
    let mut nested = 0;
    let mut inline = 0;
    entries.each(|key, item| {
        path.push(key.to_owned());
        let found = place(item, path);
        path.pop();
        match found? {
            Place::Omitted => {}
            Place::Inline => inline += 1,
            Place::Table(_) | Place::Array(_) => nested += 1,
        }
        Ok(())
    })?;
    Ok(inline > 0 || nested == 0)
}

fn header(path: &[String], open: &str, close: &str, out: &mut String) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(open);
    for (i, key) in path.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        write_key(key, out);
    }
    out.push_str(close);
    out.push('\n');
}

fn write_key(key: &str, out: &mut String) {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        out.push_str(key);
    } else {
        string(key, out);
    }
}

/// Returns a value written on one line: a scalar, an array or an inline
/// table.
fn inline(value: &Value, path: &[String]) -> Result<String, RenderError> {
    let mut out = String::new();
    write_inline(Item::Value(value), path, &mut out)?;
    Ok(out)
}

fn write_inline(item: Item<'_>, path: &[String], out: &mut String) -> Result<(), RenderError> {
    if let Some(entries) = item.entries() {
        let mut first = true;
        out.push('{');
        entries.each(|key, item| {
            if matches!(item.value(), Some(Value::None)) {
                return Ok(());
            }
            out.push_str(if first { " " } else { ", " });
            first = false;
            write_key(key, out);
            out.push_str(" = ");
            write_inline(item, path, out)
        })?;
        out.push_str(if first { "}" } else { " }" });
        return Ok(());
    }
    let Some(value) = item.value() else {
        return Ok(());
    };
    match value {
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(f) => {
            let _ = write!(out, "{f:?}");
        }
        Value::List(items) | Value::Set(items) => {
            check_list(items, path)?;
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_inline(Item::Value(item), path, out)?;
            }
            out.push(']');
        }
        Value::None => {
            return Err(error(path, "`none` cannot be written in TOML here"));
        }
        _ => {
            if let Some(text) = text(value) {
                string(&text, out);
            }
        }
    }
    Ok(())
}

/// Writes a basic string, escaping quotes, backslashes and control
/// characters.
fn string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

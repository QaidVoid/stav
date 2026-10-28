//! YAML 1.2 in block style.

use std::fmt::Write;

use super::{Item, text};
use crate::value::Value;

/// Returns the value as a YAML document.
pub(crate) fn render(value: &Value) -> String {
    let mut out = String::new();
    match block(Item::Value(value)) {
        Some(lines) => {
            for line in lines {
                out.push_str(&line);
                out.push('\n');
            }
        }
        None => {
            out.push_str(&scalar(Item::Value(value)));
            out.push('\n');
        }
    }
    out
}

/// Returns the lines of a non-empty mapping or sequence, indented as if
/// at the top. Returns `None` for a value written on one line.
fn block(item: Item<'_>) -> Option<Vec<String>> {
    if let Some(entries) = item.entries() {
        if entries.len() == 0 {
            return None;
        }
        let mut lines = Vec::new();
        let _ = entries.each(|key, item| {
            let key = string(key);
            match block(item) {
                Some(nested) => {
                    lines.push(format!("{key}:"));
                    lines.extend(nested.into_iter().map(|l| format!("  {l}")));
                }
                None => lines.push(format!("{key}: {}", scalar(item))),
            }
            Ok::<(), ()>(())
        });
        return Some(lines);
    }
    let (Value::List(items) | Value::Set(items)) = item.value()? else {
        return None;
    };
    if items.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for item in items {
        match block(Item::Value(item)) {
            Some(nested) => {
                for (i, line) in nested.into_iter().enumerate() {
                    lines.push(if i == 0 {
                        format!("- {line}")
                    } else {
                        format!("  {line}")
                    });
                }
            }
            None => lines.push(format!("- {}", scalar(Item::Value(item)))),
        }
    }
    Some(lines)
}

/// Returns a value written on one line: a scalar, or an empty mapping or
/// sequence.
fn scalar(item: Item<'_>) -> String {
    if item.entries().is_some() {
        return "{}".to_owned();
    }
    let Some(value) = item.value() else {
        return "{}".to_owned();
    };
    match value {
        Value::None => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => float(*f),
        Value::List(_) | Value::Set(_) => "[]".to_owned(),
        _ => text(value).map_or_else(|| "null".to_owned(), |t| string(&t)),
    }
}

/// Returns a float with a decimal point before any exponent, which every
/// YAML schema reads as a float.
fn float(f: f64) -> String {
    let text = format!("{f:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if !mantissa.contains('.') => {
            format!("{mantissa}.0e{exponent}")
        }
        _ => text,
    }
}

/// Returns a string as a plain scalar when no YAML schema could read it as
/// anything else, and double quoted otherwise.
fn string(text: &str) -> String {
    if is_plain(text) {
        return text.to_owned();
    }
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}') => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Words that YAML 1.2 or YAML 1.1 read as null or a boolean.
const RESERVED: [&str; 22] = [
    "null", "Null", "NULL", "true", "True", "TRUE", "false", "False", "FALSE", "y", "Y", "yes",
    "Yes", "YES", "n", "N", "no", "No", "NO", "on", "On", "ON",
];

fn is_plain(text: &str) -> bool {
    let Some(first) = text.chars().next() else {
        return false;
    };
    if !(first.is_alphabetic() || first == '_' || first == '/') {
        return false;
    }
    if text.ends_with(' ') || RESERVED.contains(&text) {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    if lower == "off" || lower == "on" {
        return false;
    }
    text.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ' '))
}

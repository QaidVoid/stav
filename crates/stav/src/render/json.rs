//! JSON, pretty printed and in the canonical form of RFC 8785.

use std::fmt::Write;

use super::{Item, text};
use crate::value::Value;

/// Returns the value as JSON with two spaces of indentation and a final
/// line feed.
pub(crate) fn pretty(value: &Value) -> String {
    let mut out = String::new();
    write_pretty(Item::Value(value), 0, &mut out);
    out.push('\n');
    out
}

fn indent(out: &mut String, level: usize) {
    out.push('\n');
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_pretty(item: Item<'_>, level: usize, out: &mut String) {
    if let Some(entries) = item.entries() {
        if entries.len() == 0 {
            out.push_str("{}");
            return;
        }
        out.push('{');
        let mut first = true;
        let _ = entries.each(|key, item| {
            if !first {
                out.push(',');
            }
            first = false;
            indent(out, level + 1);
            string(key, out);
            out.push_str(": ");
            write_pretty(item, level + 1, out);
            Ok::<(), ()>(())
        });
        indent(out, level);
        out.push('}');
        return;
    }
    let Some(value) = item.value() else { return };
    match value {
        Value::List(items) | Value::Set(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                indent(out, level + 1);
                write_pretty(Item::Value(item), level + 1, out);
            }
            indent(out, level);
            out.push(']');
        }
        _ => scalar(value, false, out),
    }
}

/// Writes a value that is neither an object nor an array.
fn scalar(value: &Value, canonical: bool, out: &mut String) {
    match value {
        Value::None => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(f) if canonical => out.push_str(&ecmascript_number(*f)),
        Value::Float(f) => {
            let _ = write!(out, "{f:?}");
        }
        _ => match text(value) {
            Some(text) => string(&text, out),
            None => out.push_str("null"),
        },
    }
}

/// Writes a JSON string with the escapes of RFC 8785: the two-character
/// escapes where JSON has them, and `\u00xx` for other control characters.
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
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Returns the value as canonical JSON.
pub(crate) fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(Item::Value(value), &mut out);
    out
}

fn write_canonical(item: Item<'_>, out: &mut String) {
    if let Some(entries) = item.entries() {
        let mut pairs: Vec<(&str, Item<'_>)> = Vec::with_capacity(entries.len());
        let _ = entries.each(|key, item| {
            pairs.push((key, item));
            Ok::<(), ()>(())
        });
        pairs.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
        out.push('{');
        for (i, (key, item)) in pairs.into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            string(key, out);
            out.push(':');
            write_canonical(item, out);
        }
        out.push('}');
        return;
    }
    let Some(value) = item.value() else { return };
    match value {
        Value::List(items) | Value::Set(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(Item::Value(item), out);
            }
            out.push(']');
        }
        _ => scalar(value, true, out),
    }
}

/// Returns a finite float the way ECMAScript's `Number.prototype.toString`
/// writes it: the shortest digits that read back as the same number,
/// without exponent from 1e-6 up to 1e21.
pub(crate) fn ecmascript_number(f: f64) -> String {
    if f == 0.0 {
        return "0".to_owned();
    }
    let sci = format!("{:e}", f.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let k = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let n = exponent + 1;
    let mut out = String::new();
    if f < 0.0 {
        out.push('-');
    }
    let zeros = |count: i64| "0".repeat(usize::try_from(count).unwrap_or(0));
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.push_str(&zeros(n - k));
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(usize::try_from(n).unwrap_or(0));
        out.push_str(int);
        out.push('.');
        out.push_str(frac);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.push_str(&zeros(-n));
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push('e');
        out.push(if n - 1 < 0 { '-' } else { '+' });
        let _ = write!(out, "{}", (n - 1).abs());
    }
    out
}

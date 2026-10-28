//! Documents of text and line breaks, and the printer that lays them out.
//!
//! The formatter describes a file as a [`Doc`]. The printer then decides,
//! group by group, whether a group fits on the rest of the current line. A
//! group that fits prints its line breaks as spaces or as nothing. A group
//! that does not fit, or that contains a break that cannot be undone, prints
//! every line break of its own as a new line.

/// The column limit that groups must fit in.
pub(super) const WIDTH: usize = 100;

/// A layout description.
pub(super) enum Doc<'a> {
    /// Text printed as it is. Only multi-line strings contain line feeds.
    Text(&'a str),
    /// A number of spaces.
    Spaces(usize),
    /// A space, or a line break.
    Line,
    /// Nothing, or a line break.
    SoftLine,
    /// A line break that always breaks every enclosing group.
    HardLine,
    /// The contents, with line breaks indented two more spaces.
    Indent(Box<Doc<'a>>),
    /// The contents, with line breaks indented to the column where the
    /// contents start.
    Column(Box<Doc<'a>>),
    /// Contents that print on one line when they fit and when the flag that
    /// forces a break is not set.
    Group(Box<Doc<'a>>, bool),
    /// The first document when the enclosing group breaks, the second one
    /// otherwise.
    IfBreak(Box<Doc<'a>>, Box<Doc<'a>>),
    /// A comment printed at the end of the current line, just before the
    /// next line break. Further comments waiting for the same line break go
    /// on lines of their own, indented like the current line or the next
    /// one, whichever is deeper. It breaks every enclosing group.
    LineSuffix(&'a str),
    /// Breaks every enclosing group and prints nothing.
    BreakParent,
    /// Documents printed one after another.
    Concat(Vec<Doc<'a>>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Flat,
    Break,
}

/// Returns a group of `docs`.
pub(super) fn group(docs: Vec<Doc<'_>>) -> Doc<'_> {
    let doc = Doc::Concat(docs);
    let breaks = forces_break(&doc);
    Doc::Group(Box::new(doc), breaks)
}

/// Returns `docs` indented by one level.
pub(super) fn indent(docs: Vec<Doc<'_>>) -> Doc<'_> {
    Doc::Indent(Box::new(Doc::Concat(docs)))
}

/// Returns `docs` aligned at the column where they start.
pub(super) fn column(docs: Vec<Doc<'_>>) -> Doc<'_> {
    Doc::Column(Box::new(Doc::Concat(docs)))
}

/// Returns `doc` when the enclosing group breaks, and nothing otherwise.
pub(super) fn if_break(doc: Doc<'_>) -> Doc<'_> {
    Doc::IfBreak(Box::new(doc), Box::new(Doc::Concat(Vec::new())))
}

/// Returns `doc` when the enclosing group stays flat, and nothing otherwise.
pub(super) fn if_flat(doc: Doc<'_>) -> Doc<'_> {
    Doc::IfBreak(Box::new(Doc::Concat(Vec::new())), Box::new(doc))
}

/// Returns whether `doc` contains a break that keeps a group around it from
/// printing flat. The search stops at nested groups, which already know.
fn forces_break(doc: &Doc<'_>) -> bool {
    match doc {
        Doc::HardLine | Doc::LineSuffix(_) | Doc::BreakParent => true,
        Doc::Group(_, breaks) => *breaks,
        Doc::Indent(inner) | Doc::Column(inner) => forces_break(inner),
        Doc::IfBreak(_, flat) => forces_break(flat),
        Doc::Concat(docs) => docs.iter().any(forces_break),
        Doc::Text(_) | Doc::Spaces(_) | Doc::Line | Doc::SoftLine => false,
    }
}

/// Returns the width of `docs` printed flat on one line, or `None` when
/// they cannot print on one line.
pub(super) fn flat_width(docs: &[Doc<'_>]) -> Option<usize> {
    let mut width = 0;
    let mut stack: Vec<&Doc<'_>> = docs.iter().rev().collect();
    while let Some(doc) = stack.pop() {
        match doc {
            Doc::Text(text) if text.contains('\n') => return None,
            Doc::Text(text) => width += text_width(text),
            Doc::Spaces(n) => width += n,
            Doc::Line => width += 1,
            Doc::SoftLine | Doc::BreakParent => {}
            Doc::HardLine | Doc::LineSuffix(_) => return None,
            Doc::Group(_, true) => return None,
            Doc::Indent(inner) | Doc::Column(inner) | Doc::Group(inner, false) => {
                stack.push(inner);
            }
            Doc::IfBreak(_, flat) => stack.push(flat),
            Doc::Concat(docs) => stack.extend(docs.iter().rev()),
        }
    }
    Some(width)
}

fn text_width(text: &str) -> usize {
    text.chars().count()
}

/// Prints `doc`. The result has no trailing whitespace on any line outside
/// of multi-line text, and ends with exactly one line feed unless it is
/// empty.
pub(super) fn print(doc: &Doc<'_>) -> String {
    let mut out = String::new();
    let mut pos = 0;
    let mut line_indent = 0;
    let mut suffixes: Vec<&str> = Vec::new();
    let mut commands: Vec<(usize, Mode, &Doc<'_>)> = vec![(0, Mode::Break, doc)];
    while let Some((indentation, mode, doc)) = commands.pop() {
        match doc {
            Doc::Text(text) => {
                out.push_str(text);
                pos = match text.rfind('\n') {
                    Some(at) => text_width(&text[at + 1..]),
                    None => pos + text_width(text),
                };
            }
            Doc::Spaces(n) => {
                out.extend(std::iter::repeat_n(' ', *n));
                pos += n;
            }
            Doc::Line if mode == Mode::Flat => {
                out.push(' ');
                pos += 1;
            }
            Doc::SoftLine if mode == Mode::Flat => {}
            Doc::Line | Doc::SoftLine | Doc::HardLine => {
                let comment_indent = line_indent.max(indentation);
                flush_suffixes(&mut out, &mut suffixes, comment_indent);
                trim_line_end(&mut out);
                out.push('\n');
                out.extend(std::iter::repeat_n(' ', indentation));
                pos = indentation;
                line_indent = indentation;
            }
            Doc::Indent(inner) => commands.push((indentation + 2, mode, inner)),
            Doc::Column(inner) => commands.push((pos, mode, inner)),
            Doc::Group(inner, breaks) => {
                let flat = mode == Mode::Flat
                    || (!breaks && fits(inner, &commands, WIDTH.saturating_sub(pos)));
                let mode = if flat { Mode::Flat } else { Mode::Break };
                commands.push((indentation, mode, inner));
            }
            Doc::IfBreak(broken, flat) => {
                let inner = if mode == Mode::Break { broken } else { flat };
                commands.push((indentation, mode, inner));
            }
            Doc::LineSuffix(comment) => suffixes.push(comment),
            Doc::BreakParent => {}
            Doc::Concat(docs) => {
                commands.extend(docs.iter().rev().map(|doc| (indentation, mode, doc)));
            }
        }
    }
    flush_suffixes(&mut out, &mut suffixes, line_indent);
    out.truncate(out.trim_end().len());
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Prints the comments waiting for the end of the line: the first one at
/// the end of the current line and each further one on a line of its own
/// at `indentation`, because two comments on one line would read as one.
fn flush_suffixes(out: &mut String, suffixes: &mut Vec<&str>, indentation: usize) {
    for (index, comment) in suffixes.drain(..).enumerate() {
        trim_line_end(out);
        if index > 0 {
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', indentation));
        } else if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push_str(comment);
    }
}

fn trim_line_end(out: &mut String) {
    out.truncate(out.trim_end_matches([' ', '\t']).len());
}

/// Returns whether `next`, printed flat, and the commands after it up to
/// their first line break fit in `width` columns.
fn fits(next: &Doc<'_>, rest: &[(usize, Mode, &Doc<'_>)], width: usize) -> bool {
    let mut remaining = width;
    let mut rest_index = rest.len();
    let mut stack = vec![(Mode::Flat, next)];
    loop {
        let (mode, doc) = match stack.pop() {
            Some(entry) => entry,
            None if rest_index == 0 => return true,
            None => {
                rest_index -= 1;
                let (_, mode, doc) = rest[rest_index];
                (mode, doc)
            }
        };
        let used = match doc {
            Doc::Text(text) => match (text.find('\n'), text.rfind('\n')) {
                (Some(first), Some(last)) => {
                    if text_width(&text[..first]) > remaining {
                        return false;
                    }
                    remaining = WIDTH;
                    text_width(&text[last + 1..])
                }
                _ => text_width(text),
            },
            Doc::Spaces(n) => *n,
            Doc::Line | Doc::SoftLine if mode == Mode::Break => return true,
            Doc::HardLine => return true,
            Doc::Line => 1,
            Doc::SoftLine | Doc::LineSuffix(_) | Doc::BreakParent => 0,
            Doc::Indent(inner) | Doc::Column(inner) => {
                stack.push((mode, inner));
                0
            }
            Doc::Group(inner, breaks) => {
                stack.push((if *breaks { Mode::Break } else { mode }, inner));
                0
            }
            Doc::IfBreak(broken, flat) => {
                stack.push((mode, if mode == Mode::Break { broken } else { flat }));
                0
            }
            Doc::Concat(docs) => {
                stack.extend(docs.iter().rev().map(|doc| (mode, doc)));
                0
            }
        };
        match remaining.checked_sub(used) {
            Some(left) => remaining = left,
            None => return false,
        }
    }
}

#[cfg(test)]
#[path = "doc_tests.rs"]
mod tests;

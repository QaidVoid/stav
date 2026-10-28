use super::{Doc, WIDTH, column, group, indent, print};

fn text(width: usize, c: &str) -> String {
    c.repeat(width)
}

#[test]
fn group_fits_exactly_at_the_width_and_breaks_one_column_past_it() {
    let left = text(50, "a");
    let fitting = text(WIDTH - 51, "b");
    let doc = group(vec![Doc::Text(&left), Doc::Line, Doc::Text(&fitting)]);
    assert_eq!(print(&doc), format!("{left} {fitting}\n"));

    let too_long = text(WIDTH - 50, "b");
    let doc = group(vec![Doc::Text(&left), Doc::Line, Doc::Text(&too_long)]);
    assert_eq!(print(&doc), format!("{left}\n{too_long}\n"));
}

#[test]
fn text_after_a_group_counts_toward_whether_it_fits() {
    let doc = |inner| {
        Doc::Concat(vec![
            group(vec![Doc::Text(inner), Doc::SoftLine]),
            Doc::Text("bc"),
        ])
    };
    let fitting = text(WIDTH - 2, "a");
    assert_eq!(print(&doc(&fitting)), format!("{fitting}bc\n"));
    let too_long = text(WIDTH - 1, "a");
    assert_eq!(print(&doc(&too_long)), format!("{too_long}\nbc\n"));
}

#[test]
fn queued_comments_go_on_lines_of_their_own_at_the_deeper_indentation() {
    let doc = Doc::Concat(vec![
        Doc::Text("a"),
        indent(vec![
            Doc::LineSuffix("# one"),
            Doc::LineSuffix("# two"),
            Doc::LineSuffix("# three"),
            Doc::HardLine,
            Doc::Text("b"),
        ]),
    ]);
    assert_eq!(print(&doc), "a # one\n  # two\n  # three\n  b\n");
}

#[test]
fn queued_comments_at_the_end_are_printed() {
    let doc = Doc::Concat(vec![
        Doc::Text("a"),
        Doc::LineSuffix("# one"),
        Doc::LineSuffix("# two"),
    ]);
    assert_eq!(print(&doc), "a # one\n# two\n");
}

#[test]
fn column_aligns_line_breaks_where_its_contents_start() {
    let doc = indent(vec![
        Doc::Text("let "),
        column(vec![Doc::Text("x = 1,"), Doc::HardLine, Doc::Text("y = 2")]),
        Doc::HardLine,
        Doc::Text("in x"),
    ]);
    assert_eq!(print(&doc), "let x = 1,\n    y = 2\n  in x\n");
}

#[test]
fn lines_have_no_trailing_whitespace() {
    let doc = Doc::Concat(vec![
        Doc::Text("a"),
        Doc::Spaces(3),
        Doc::HardLine,
        indent(vec![Doc::HardLine, Doc::Text("b"), Doc::Spaces(2)]),
        Doc::Text("\t"),
        Doc::LineSuffix("# c"),
        Doc::Spaces(1),
    ]);
    assert_eq!(print(&doc), "a\n\n  b # c\n");
}

#[test]
fn output_ends_with_exactly_one_line_feed_unless_empty() {
    let doc = Doc::Concat(vec![Doc::Text("a"), Doc::HardLine, Doc::HardLine]);
    assert_eq!(print(&doc), "a\n");
    assert_eq!(print(&Doc::Text("a")), "a\n");
    assert_eq!(print(&Doc::Concat(vec![Doc::HardLine, Doc::Spaces(2)])), "");
}

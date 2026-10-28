use super::{Diagnostic, Diagnostics, FileDiagnostic, NamedSource};
use crate::check::FileId;
use crate::span::Span;

fn diagnostics(items: Vec<FileDiagnostic>) -> Diagnostics {
    Diagnostics {
        files: vec![
            NamedSource {
                name: "a.stav".to_owned(),
                text: "first\nsecond line\n".to_owned(),
            },
            NamedSource {
                name: "b.stav".to_owned(),
                text: "äb\n".to_owned(),
            },
        ],
        items,
    }
}

#[test]
fn rendering_shows_codes_locations_labels_notes_and_help() {
    let error = Diagnostic::error("E0607", Span { start: 13, end: 17 }, "a cycle")
        .with_label(Some(FileId::new(1)), Span { start: 2, end: 3 }, "other end")
        .with_note("cycle: a -> b -> a")
        .with_help("break it");
    let warning = Diagnostic::warning("W0703", Span { start: 0, end: 1 }, "twice");
    let rendered = diagnostics(vec![
        FileDiagnostic {
            file: Some(FileId::new(0)),
            diagnostic: error,
        },
        FileDiagnostic {
            file: None,
            diagnostic: warning,
        },
    ])
    .to_string();
    assert_eq!(
        rendered,
        "error[E0607]: a cycle\n  --> a.stav:2:8\n  label: b.stav:1:2: other end\n  note: cycle: a -> b -> a\n  help: break it\nwarning[W0703]: twice\n"
    );
}

#[test]
fn errors_and_warnings_are_told_apart() {
    let warning = FileDiagnostic {
        file: None,
        diagnostic: Diagnostic::warning("W0302", Span { start: 0, end: 0 }, "shadowed"),
    };
    assert!(!diagnostics(vec![warning.clone()]).has_errors());
    let error = FileDiagnostic {
        file: None,
        diagnostic: Diagnostic::error("E0302", Span { start: 0, end: 0 }, "unknown"),
    };
    assert!(diagnostics(vec![warning, error]).has_errors());
}

#[test]
fn offsets_past_the_end_render_at_the_last_position() {
    let error = Diagnostic::error("E0302", Span { start: 99, end: 99 }, "x");
    let rendered = diagnostics(vec![FileDiagnostic {
        file: Some(FileId::new(1)),
        diagnostic: error,
    }])
    .to_string();
    assert!(rendered.contains("b.stav:2:1"), "{rendered}");
}

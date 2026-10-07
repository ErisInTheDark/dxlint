use std::path::Path;

use super::tests::{lint, linter, native_linter};

#[test]
fn numeric_reports_are_points_without_changing_fix_regions() {
    let l = linter(r#"
        in .t match word /[a-z]+/ {
            rule point warn "point" {
                report at offset + 1
                fix as upper(text)
            }
        }
    "#);
    let source = "longword";
    let diagnostics = lint(&l, source);
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.start, diagnostic.end);
    assert_eq!(diagnostic.start, 1);
    let edit = &diagnostic.fix.as_ref().expect("fix remains available")[0];
    assert_eq!((edit.start, edit.end), (0, source.len()));
    assert_eq!(edit.text, source.to_uppercase());
}

#[test]
fn numeric_points_clamp_to_source_and_utf8_boundaries() {
    for (expression, expected) in [
        ("offset - 10", 0),
        ("offset + 1", 0),
        ("end_offset", "éabc".len()),
        ("end_offset + 10", "éabc".len()),
    ] {
        let l = linter(&format!(r#"
            in .t match word /.+/ {{
                rule point warn "point" {{ report at {expression} }}
            }}
        "#));
        let diagnostics = lint(&l, "éabc");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!((diagnostics[0].start, diagnostics[0].end), (expected, expected));
    }
}

#[test]
fn function_warnings_target_heads_independent_of_body_size() {
    let l = native_linter("rule require-await");
    for source in [
        "async function task() { work(); }",
        "async function task() { work(); work(); work(); }",
        "const task = async () => { work(); work(); };",
        "const object = { async task() { work(); work(); } };",
        "const object = { task: async () => { work(); work(); } };",
        "const object = { slot: async function task() { work(); work(); } };",
        "class Task { task = async () => { work(); work(); }; }",
    ] {
        let diagnostics = l.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 1, "{source}");
        let diagnostic = &diagnostics[0];
        let span = &source[diagnostic.start..diagnostic.end];
        assert!(matches!(span, "task" | "=>"), "{source}: {span}");
        assert!(diagnostic.end <= source.rfind('{').expect("body in fixture"), "{source}");
    }
}

#[test]
fn local_ignore_follows_the_reported_function_head() {
    let l = native_linter("rule require-await");
    let source = "async function\n// zlint-ignore require-await\ntask() { work(); }\n";
    assert!(l.lint(Path::new("/root/a.ts"), source).is_empty());
    let source = "// zlint-ignore require-await\nasync function\ntask() { work(); }\n";
    assert_eq!(l.lint(Path::new("/root/a.ts"), source).len(), 1);
}

#[test]
fn local_ignore_follows_the_reported_member_gap() {
    let l = native_linter("rule lines-between-class-members");
    let source = "class Task {\nfirst() {} // zlint-ignore lines-between-class-members\nsecond() {}\n}";
    assert!(l.lint(Path::new("/root/a.ts"), source).is_empty());
    let source = "class Task {\nfirst() {}\nsecond() {} // zlint-ignore lines-between-class-members\n}";
    assert_eq!(l.lint(Path::new("/root/a.ts"), source).len(), 1);
}

#[test]
fn iterable_warnings_exclude_grouping_parentheses_and_loop_bodies() {
    for (rules, source) in [
        ("rule no-for-in-array", "for (const key in (([1, 2]))) { work(); work(); }"),
        ("rule await-thenable", "for await (const value of (([1, 2]))) { work(); work(); }"),
    ] {
        let l = native_linter(rules);
        let diagnostics = l.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 1, "{source}");
        let diagnostic = &diagnostics[0];
        assert_eq!(&source[diagnostic.start..diagnostic.end], "[1, 2]");
    }
}

#[test]
fn unnecessary_annotations_report_the_type_not_the_initialiser() {
    let l = native_linter("rule no-inferrable-types");
    for source in ["const count: number = 12;", "function task(count: number = 12) {}"] {
        let diagnostics = l.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 1, "{source}");
        let diagnostic = &diagnostics[0];
        assert_eq!(&source[diagnostic.start..diagnostic.end], "number");
    }
}

#[test]
fn whitespace_reports_do_not_include_following_code() {
    for (rules, source) in [
        ("rule no-multiple-empty-lines", "first();\n\n\n\nsecond();\nthird();"),
        (
            "rule type-generic-spacing\nrule type-generic-spacing:*",
            "function task < T > (value: T) { work(); work(); }",
        ),
        (
            "rule lines-between-class-members",
            "class Task { first() {}\nsecond() { work(); work(); } }",
        ),
    ] {
        let l = native_linter(rules);
        let diagnostics = l.lint(Path::new("/root/a.ts"), source);
        assert!(!diagnostics.is_empty(), "{source}");
        for diagnostic in diagnostics {
            let reported = &source[diagnostic.start..diagnostic.end];
            assert!(reported.chars().all(char::is_whitespace), "{source}: {reported}");
        }
    }
}

#[test]
fn empty_body_reports_keep_the_body_span() {
    for (rules, source) in [
        ("rule no-empty", "switch (value) {}"),
        ("rule no-empty-function", "function task() {}"),
        ("rule arrow-body-style", "const task = () => { return value; };"),
    ] {
        let l = native_linter(rules);
        let diagnostics = l.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 1, "{source}");
        let diagnostic = &diagnostics[0];
        let reported = &source[diagnostic.start..diagnostic.end];
        assert!(reported.starts_with('{') && reported.ends_with('}'), "{source}: {reported}");
    }
}

#[test]
fn identifier_warnings_do_not_select_their_functions_or_initialisers() {
    let l = native_linter(r#"
        rule no-underscore-dangle {
            set allowFunctionParams false
            set enforceInMethodNames true
        }
    "#);
    for source in [
        "function _task(_value: number) { work(); work(); }",
        "class Task { _task(_value: number) { work(); work(); } }",
        "const _value = work(() => { work(); work(); });",
    ] {
        let diagnostics = l.lint(Path::new("/root/a.ts"), source);
        assert!(!diagnostics.is_empty(), "{source}");
        for diagnostic in diagnostics {
            let reported = &source[diagnostic.start..diagnostic.end];
            assert!(matches!(reported, "_task" | "_value"), "{source}: {reported}");
        }
    }
}

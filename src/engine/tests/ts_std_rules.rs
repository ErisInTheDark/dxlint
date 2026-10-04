use std::path::Path;

use super::native_linter;

#[test]
fn adjacent_comments_share_outer_blank_line_requirements() {
    let lint = native_linter("rule lines-around-comment {\n set beforeBlockComment true\n set afterBlockComment true\n}");
    let source = "before();\n/* first */ /* second\n continued */\nafter();";
    let reports = lint.lint(Path::new("/root/a.ts"), source);
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].start, source.find("/* first").expect("first comment"));
    assert_eq!(reports[1].start, source.find("/* second").expect("second comment"));
}

#[test]
fn multiline_comment_style_uses_javascript_line_boundaries() {
    let linter = native_linter("rule multiline-comment-style");
    for separator in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
        let source = format!("/*bad{separator} more{separator} */");
        let diagnostics = linter.lint(Path::new("/root/file.ts"), &source);
        let offsets: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        assert_eq!(offsets, [0, 5 + separator.len()], "{source:?}: {diagnostics:#?}");
    }
}

#[test]
fn empty_interfaces_use_members_and_supertypes_not_body_text() {
    let linter = native_linter("rule no-empty-interface");
    for (source, expected) in [
        ("interface Empty {}", 1),
        ("interface Empty { /* documentation */ }", 1),
        ("interface Empty {\n // documentation\n}", 1),
        ("interface Empty extends Base {}", 1),
        ("interface Empty extends Base<string, number> { /* documentation */ }", 1),
        ("interface Intersection extends First, Second {}", 0),
        ("interface Populated { value: string }", 0),
        ("interface Callable { (): void }", 0),
    ] {
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), expected, "{source}: {diagnostics:#?}");
        if expected != 0 {
            let diagnostic = &diagnostics[0];
            assert_eq!(diagnostic.start, source.find('{').expect("interface body"), "{source}");
            assert_eq!(diagnostic.end, source.len(), "{source}");
        }
    }
}

#[test]
fn irregular_whitespace_exemptions_follow_syntax_ownership() {
    let linter = native_linter(
        "rule no-irregular-whitespace {\n set skipJSXText true\n set skipStrings false\n}",
    );
    for (source, expected) in [
        ("const value = <div>\u{00a0}</div>;", 0),
        ("const value = <div>{'\u{00a0}'}</div>;", 1),
        ("const value = <div title='\u{00a0}' />;", 1),
        ("const value = <div>{/* \u{00a0} */}</div>;", 1),
    ] {
        let diagnostics = linter.lint(Path::new("/root/file.tsx"), source);
        assert_eq!(diagnostics.len(), expected, "{source}: {diagnostics:#?}");
    }
}

#[test]
fn license_fixes_preserve_following_comments_and_are_idempotent() {
    let header = "/* Copyright Example */";
    for (rule, source, expected) in [
        (
            "missing",
            "// description\nconst value = 1;".to_owned(),
            format!("{header}\n\n// description\nconst value = 1;"),
        ),
        (
            "invalid",
            "/* Copyright Old */\n\n// description\nconst value = 1;".to_owned(),
            format!("{header}\n\n// description\nconst value = 1;"),
        ),
        (
            "before",
            format!("\n \n{header}\n\nconst value = 1;"),
            format!("{header}\n\nconst value = 1;"),
        ),
        (
            "after",
            format!("{header}\r\n\r\n\r\n// description\r\nconst value = 1;"),
            format!("{header}\r\n\r\n// description\r\nconst value = 1;"),
        ),
    ] {
        let linter = native_linter(&format!("rule header:{rule} {{ set license \"{header}\" }}"));
        let path = Path::new("/root/file.ts");
        let fixed = linter.fix(path, &source);
        assert_eq!(fixed.text, expected, "{rule}");
        assert!(fixed.remaining.is_empty(), "{rule}: {:#?}", fixed.remaining);
        let repeated = linter.fix(path, &fixed.text);
        assert_eq!(repeated.applied, 0, "{rule}");
        assert_eq!(repeated.text, fixed.text, "{rule}");
    }
}

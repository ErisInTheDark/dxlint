use std::path::Path;

use super::native_linter;

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

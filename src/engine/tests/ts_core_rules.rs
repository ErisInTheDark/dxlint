use std::path::Path;

use super::native_linter;

#[test]
fn chained_assignments_report_each_initializer_or_nested_right_hand_side() {
    let linter = native_linter("rule no-multi-assign");
    for (source, expected) in [
        ("const a = b = c = 1;", vec!["b = c = 1", "c = 1"]),
        ("a = (b = c = 1);", vec!["b = c = 1", "c = 1"]),
        ("class C { field = foo = 0; [bar = 0] = 0 }", vec!["foo = 0"]),
        ("const a = 1, b = 2; a = f(b = 0);", vec![]),
    ] {
        let actual: Vec<_> = linter.lint(Path::new("/root/a.js"), source).iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, expected, "{source}");
    }
}

#[test]
fn chained_assignment_exemption_keeps_only_direct_declaration_initializers() {
    let linter = native_linter("rule no-multi-assign { set ignoreNonDeclaration true }");
    for (source, expected) in [
        ("const a = (b = c = 1);", vec!["b = c = 1"]),
        ("a = b = c = 1;", vec![]),
        ("class C { field = (foo = bar = 0); [baz = 0] = 0 }", vec!["foo = bar = 0"]),
        ("const a = f(b = 1);", vec![]),
    ] {
        let actual: Vec<_> = linter.lint(Path::new("/root/a.js"), source).iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, expected, "{source}");
    }
}

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

#[test]
fn parameter_limits_locate_each_function_type_at_its_own_arrow() {
    let linter = native_linter("rule max-params { set max 1 }");
    for source in [
        "type F = (a: number, b: number) => number;",
        "type F = <T>(a: T, b: T) => T;",
        "type F = (cb: (a: number, b: number) => number, b: number) => number;",
        "type F = (a: number, b: number) => (c: number, d: number) => number;",
    ] {
        let expected: Vec<_> = source.match_indices("=>").map(|(offset, _)| offset).collect();
        let mut actual: Vec<_> = linter.lint(Path::new("/root/a.ts"), source).iter().map(|d| d.start).collect();
        actual.sort_unstable();
        assert_eq!(actual, expected, "{source}");
    }
    let source = "function f(a, b) {}";
    let reports = linter.lint(Path::new("/root/a.js"), source);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].start, 0);
}

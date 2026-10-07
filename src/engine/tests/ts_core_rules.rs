use std::path::Path;

use super::native_linter;

#[test]
fn rest_parameters_preference_targets_only_implicit_arguments_references() {
    let linter = native_linter("rule prefer-rest-params");
    for (source, expected) in [
        ("function f() { return arguments; }", 1),
        ("function f() { return arguments[0]; }", 1),
        ("function f() { return arguments['length']; }", 1),
        ("function f() { return () => arguments[0]; }", 1),
        ("function f(arguments) { return function () { return arguments[0]; }; }", 1),
        ("const f = function arguments() { return arguments[0]; };", 1),
        ("const object = { f() { return arguments[0]; } };", 1),
        ("class C { f() { return arguments[0]; } }", 1),
        ("function f() { return arguments.length; }", 0),
        ("function f() { return arguments?.length; }", 0),
        ("function f(arguments) { return () => arguments[0]; }", 0),
        ("function f() { const arguments = []; return arguments[0]; }", 0),
        ("function f() { { const arguments = []; return arguments[0]; } }", 0),
        ("export {}; arguments[0]; const f = () => arguments[0];", 0),
    ] {
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), expected, "{source}: {diagnostics:#?}");
    }
}

#[test]
fn method_shorthand_keeps_arrows_that_capture_outer_arguments() {
    let linter = native_linter("rule object-shorthand { set avoidExplicitReturnArrows true }");
    for (source, expected) in [
        ("function f() { return { read: () => { return arguments[0]; } }; }", 0),
        ("function f() { return { read: () => { return () => arguments[0]; } }; }", 0),
        ("function f(arguments) { return { read: () => { return arguments[0]; } }; }", 0),
        ("function f() { const arguments = []; return { read: () => { return arguments[0]; } }; }", 0),
        ("const object = { read: () => { return arguments[0]; } };", 0),
        ("const object = { read: (arguments) => { return arguments[0]; } };", 1),
        ("const object = { read: () => { const arguments = []; return arguments[0]; } };", 1),
        ("const object = { read: () => { return function () { return arguments[0]; }; } };", 1),
        ("type arguments = string; const object = { read: () => { let value: arguments; return 1; } };", 1),
        ("const object = { read: () => { return 1; } };", 1),
    ] {
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), expected, "{source}: {diagnostics:#?}");
    }
}

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

#[test]
fn restriction_name_patterns_use_javascript_unicode_classes() {
    let linter = native_linter(r#"rule no-restricted-imports {
      set patterns [{group: ["pkg"], importNamePattern: "^\\w+$"}]
    }"#);
    let source = "import { ascii, \u{00e9} } from 'pkg';";
    let reports = linter.lint(Path::new("/root/a.js"), source);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].start, source.find("ascii").expect("ascii import"));
}

#[test]
fn restriction_patterns_accept_negative_lookahead_for_each_name_route() {
    for (config, source, expected) in [
        (
            r#"rule no-restricted-exports { set restrictedNamedExportsPattern "^(?!foo)(?!bar).+$" }"#,
            "export { foo, bar, baz };",
            1,
        ),
        (
            r#"rule no-restricted-imports { set patterns [{group: ["pkg"], importNamePattern: "^(?!foo)(?!bar).+$"}] }"#,
            "import { foo, bar, baz } from 'pkg';",
            1,
        ),
        (
            r#"rule no-restricted-imports { set patterns [{group: ["pkg"], allowImportNamePattern: "^(?!foo)(?!bar).+$"}] }"#,
            "import { foo, bar, baz } from 'pkg';",
            2,
        ),
    ] {
        let linter = native_linter(config);
        assert_eq!(linter.lint(Path::new("/root/a.js"), source).len(), expected, "{config}");
    }
}

#[test]
fn restriction_import_regexes_preserve_case_sensitive_and_insensitive_flags() {
    let source = "import a from 'FOO/baz'; import b from 'foo/Bar';";
    for (sensitive, expected) in [(false, 0), (true, source.find("import b").expect("second import"))] {
        let config = format!(r#"rule no-restricted-imports {{
          set patterns [{{regex: "foo/(?!bar)", caseSensitive: {sensitive}}}]
        }}"#);
        let reports = native_linter(&config).lint(Path::new("/root/a.js"), source);
        assert_eq!(reports.len(), 1, "{config}");
        assert_eq!(reports[0].start, expected, "{config}");
    }
}

#[test]
fn restricted_pattern_options_reject_invalid_javascript_syntax_before_linting() {
    for config in [
        r#"rule no-restricted-exports { set restrictedNamedExportsPattern "(?i)foo" }"#,
        r#"rule no-restricted-imports { set patterns [{regex: "(?i)foo"}] }"#,
        r#"rule no-restricted-imports { set patterns [{group: ["pkg"], importNamePattern: "(?i)foo"}] }"#,
        r#"rule no-restricted-imports { set patterns [{group: ["pkg"], allowImportNamePattern: "(?i)foo"}] }"#,
    ] {
        let errors = super::errors(&format!("import .ts\n{config}"));
        assert!(!errors.is_empty(), "{config}");
    }
}

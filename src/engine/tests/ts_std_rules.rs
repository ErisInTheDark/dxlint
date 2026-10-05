use std::path::Path;

use super::native_linter;

#[test]
fn typed_dot_notation_keeps_visibility_options_specific_to_winning_members() {
    for (shape, option, expected) in [
        ("class C { private value: unknown; }", "allowPrivateClassPropertyAccess", 0),
        ("class C { private value: unknown; }", "allowProtectedClassPropertyAccess", 1),
        ("class Base { protected value: number; } class C extends Base {}", "allowProtectedClassPropertyAccess", 0),
        ("class Base { protected value: number; } class C extends Base { public value: unknown; }", "allowProtectedClassPropertyAccess", 1),
        ("class C { constructor(private value: unknown) {} }", "allowPrivateClassPropertyAccess", 0),
    ] {
        let lint = native_linter(&format!("rule dot-notation {{ set {option} true }}"));
        for access in ["object['value']", "object?.['value']"] {
            let source = format!("{shape} declare const object: C | undefined; {access};");
            assert_eq!(lint.lint(Path::new("/root/a.ts"), &source).len(), expected, "{source}");
        }
    }
}

#[test]
fn typed_dot_notation_index_exemption_never_hides_a_declared_unknown_property() {
    let lint = native_linter("rule dot-notation { set allowIndexSignaturePropertyAccess true }");
    for (shape, expected) in [
        ("{ [key: string]: unknown }", 0),
        ("{ value: unknown; [key: string]: unknown }", 1),
        ("{ [key: number]: unknown }", 1),
        ("unknown", 1),
        ("{ [key: string]: unknown } | { value: unknown }", 1),
    ] {
        let source = format!("declare const object: {shape}; object['value'];");
        assert_eq!(lint.lint(Path::new("/root/a.ts"), &source).len(), expected, "{source}");
    }
}

#[test]
fn typed_destructuring_distinguishes_iterables_from_numeric_properties() {
    let linter = native_linter("rule prefer-destructuring");
    let path = Path::new("/root/file.ts");
    for (ty, expected) in [
        ("{ 0: unknown }", 0),
        ("unknown", 0),
        ("{ [key: number]: unknown }", 0),
        ("unknown[]", 1),
        ("string", 1),
        ("any", 1),
        ("{ [Symbol.iterator]: unknown }", 1),
        ("unknown[] | { 0: unknown }", 0),
        ("unknown[] | { [Symbol.iterator]: unknown }", 1),
        ("unknown[] & object", 1),
    ] {
        let source = format!("declare let xs: {ty}; const x = xs[0]; x = xs[0];");
        assert_eq!(linter.lint(path, &source).len(), expected * 2, "{source}");
    }
}

#[test]
fn typed_destructuring_keeps_annotation_controls_and_category_options() {
    let path = Path::new("/root/file.ts");
    let annotated = "declare const object: { value: number }; const value: number = object.value;";
    assert!(native_linter("rule prefer-destructuring").lint(path, annotated).is_empty());
    let enforced = native_linter("rule prefer-destructuring { set options [{ object: true }, { enforceForDeclarationWithTypeAnnotation: true }] }");
    let diagnostics = enforced.lint(path, annotated);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].fix.is_none());
    for (category, expected) in [("object", 2), ("array", 0)] {
        let linter = native_linter(&format!("rule prefer-destructuring {{ set options [{{ {category}: true }}, {{ enforceForRenamedProperties: true }}] }}"));
        let source = "declare let object: { 0: unknown }; const value = object[0]; value = object[0];";
        assert_eq!(linter.lint(path, source).len(), expected);
    }
}

#[test]
fn destructuring_fixes_preserve_comments_and_leave_unsafe_rewrites_reported() {
    let linter = native_linter("rule prefer-destructuring");
    let path = Path::new("/root/file.js");
    for (source, expected) in [
        ("const value = object.value;", Some("const {value} = object;")),
        ("const value = (/* keep */ object).value;", Some("const {value} = (/* keep */ object);")),
        ("const value /* keep */ = object.value;", None),
        ("const value: number = object.value;", None),
        ("const value = array[0];", None),
    ] {
        let diagnostics = linter.lint(path, source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert_eq!(diagnostics[0].fix.is_some(), expected.is_some(), "{source}");
        if let Some(expected) = expected {
            let fixed = linter.fix(path, source);
            assert_eq!(fixed.text, expected);
            assert!(fixed.remaining.is_empty(), "{:#?}", fixed.remaining);
            assert_eq!(linter.fix(path, &fixed.text).applied, 0);
        }
    }
}

#[test]
fn dot_notation_fixes_keep_token_boundaries_and_report_unfixable_comments() {
    let linter = native_linter("rule dot-notation");
    let path = Path::new("/root/file.js");
    for (source, expected) in [
        ("42[\"foo\"];", Some("42 .foo;")),
        ("object?.[\"foo\"];", Some("object?.foo;")),
        ("object[\"foo\"]in other;", Some("object.foo in other;")),
        ("object[/* keep */ \"foo\"];", None),
    ] {
        let diagnostics = linter.lint(path, source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert_eq!(diagnostics[0].start, source.find("\"foo\"").expect("property"));
        assert_eq!(diagnostics[0].fix.is_some(), expected.is_some(), "{source}");
        if let Some(expected) = expected {
            let fixed = linter.fix(path, source);
            assert_eq!(fixed.text, expected);
            assert!(fixed.remaining.is_empty(), "{:#?}", fixed.remaining);
            assert_eq!(linter.fix(path, &fixed.text).applied, 0);
        }
    }
}

#[test]
fn precedence_and_statement_context_preserve_required_parentheses() {
    let lint = native_linter("rule no-extra-parens");
    for (source, count) in [
        ("const x = (a + b) * c;", 0),
        ("const x = a + (b * c);", 1),
        ("const x = a - (b - c);", 0),
        ("({}).method();", 0),
        ("const x = ((a));", 1),
        ("for (let a = (b in c);;);", 0),
        ("for (let a = (b + c);;);", 1),
        ("const x: (string) = \"\";", 1),
        ("type Foo = ((string | number))[];", 1),
        ("type Foo = (string | number)[];", 0),
        ("function foo(x: (number)): (boolean) {}", 2),
    ] {
        assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{source}");
    }
}

#[test]
fn construct_parens_count_but_argument_separators_do_not() {
    for (source, outside, inside) in [
        ("import(function (){}());", 0, 1),
        ("consume(function (){}());", 0, 1),
        ("consume(function (){}(), other);", 1, 1),
        ("consume(function (){}(),);", 1, 1),
        ("consume((function (){})());", 0, 0),
    ] {
        for (mode, count) in [("outside", outside), ("inside", inside)] {
            let lint = native_linter(&format!("rule wrap-iife {{ set mode \"{mode}\" }}"));
            assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{mode}: {source}");
        }
    }
}

#[test]
fn chain_depth_tracks_receiver_invocations_through_parentheses() {
    let lint = native_linter("rule newline-per-chained-call");
    for (source, count) in [
        ("a().b().c();", 1),
        ("((a().b())).c();", 1),
        ("((a().b))().c();", 1),
        ("new A().b().c();", 1),
        ("a(foo()).b();", 0),
        ("a().b.c();", 0),
        ("((a().b())\n).c();", 0),
    ] {
        assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{source}");
    }
}

#[test]
fn chain_breaks_use_code_positions_after_unicode() {
    let lint = native_linter("rule newline-per-chained-call");
    let source = "\u{03b1}\u{03b1}; a().b()\n.c();";
    let reports = lint.lint(Path::new("/root/a.ts"), source);
    assert!(reports.is_empty(), "{reports:?}");
}

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

#[test]
fn block_like_categories_do_not_leak_into_adjacent_empty_statements() {
    let lint = native_linter("rule padding-line-between-statements { set options [{ blankLine: \"always\", prev: \"block-like\", next: \"block-like\" }] }");
    assert!(lint.lint(Path::new("/root/a.ts"), "function test() {};").is_empty());
    assert_eq!(lint.lint(Path::new("/root/a.ts"), "function test() {} function other() {}").len(), 1);
}

#[test]
fn padding_categories_follow_parenthesized_expressions() {
    for (prev, source) in [("iife", "(!function(){}())\nfoo()"), ("exports", "(exports.foo) = 1\nfoo()")] {
        let lint = native_linter(&format!("rule padding-line-between-statements {{ set options [{{ blankLine: \"always\", prev: \"{prev}\", next: \"*\" }}] }}"));
        let diagnostics = lint.lint(Path::new("/root/a.js"), source);
        assert_eq!(diagnostics.len(), 1, "{prev}: {diagnostics:#?}");
        assert_eq!(diagnostics[0].start, source.rfind("foo()").expect("second statement"), "{prev}");
    }
}

#[test]
fn empty_switch_report_starts_at_its_body_not_a_comment_brace() {
    let linter = native_linter("rule no-empty");
    for source in ["switch (value) /* { */ {}", "switch (value) // {\n {}", "switch (value) /* { */ /* outside the body */ {}"] {
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        let body_start = source.rfind('{').expect("switch body");
        assert_eq!(diagnostics[0].start, body_start, "{source}: {diagnostics:#?}");
        assert_eq!(diagnostics[0].end, source.len(), "{source}: {diagnostics:#?}");
    }
}

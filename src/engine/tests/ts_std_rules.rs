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
fn parentheses_follow_leading_tokens_and_constructor_receivers() {
    let lint = native_linter("rule no-extra-parens");
    for (source, count) in [
        ("for ((let.foo) of values);", 0),
        ("for ((let()).foo of values);", 0),
        ("for (((let.a).b) of values);", 1),
        ("for ((let) in values);", 1),
        ("for ((let).a in values);", 1),
        ("(let).a;", 1),
        ("(let)[a];", 0),
        ("for ((let)[a].b;;);", 0),
        ("for (((let)[a]).b in values);", 1),
        ("new (a()).b.c;", 0),
        ("new (a().b).c;", 0),
        ("new (function(){});", 1),
        ("new (new A);", 1),
        ("new (new A)();", 0),
        ("(0123).a;", 1),
        ("(123).a;", 0),
        ("(09).a;", 0),
    ] {
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), count, "{source}: {diagnostics:#?}");
        let (fixed, applied) = crate::engine::apply_fixes(source, &diagnostics);
        assert_eq!(applied, count, "{source}: {diagnostics:#?}");
        assert!(crate::native::ts::parse(&fixed, false).is_ok(), "{source} -> {fixed}");
        assert!(lint.lint(Path::new("/root/a.ts"), &fixed).is_empty(), "{source} -> {fixed}");
    }
}

#[test]
fn parentheses_protect_only_unenclosed_for_initialiser_in_expressions() {
    let lint = native_linter("rule no-extra-parens");
    for (source, count) in [
        ("for (let a = (b in c);;);", 0),
        ("for (let a = ((b in c) in d);;);", 1),
        ("for (let a = ((b in c) && (d in e));;);", 2),
        ("for (let [a = (b in c)] = [];;);", 1),
        ("for (let { a = b && (c in d) } = {};;);", 1),
        ("for (let a = (b = c && (d in e)) => {};;);", 1),
        ("for (let a = new b(c && (d in e));;);", 1),
        ("for (let a = b[c && (d in e)];;);", 1),
        ("for (let a = b ? c && (d in e) : f;;);", 1),
        ("for (let a = b ? c : (d in e);;);", 0),
        ("for (let a = (!(b in c));;);", 1),
    ] {
        assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{source}");
    }
}

#[test]
fn parentheses_distinguish_statement_wrappers_from_expression_wrappers() {
    let lint = native_linter("rule no-extra-parens");
    for (source, count) in [
        ("((function(){ return bar(); })());", 1),
        ("async function f(){ await (+a); }", 1),
        ("switch(0){ case (1): break; }", 1),
        ("switch(0){ case (1 as 1): break; }", 0),
        ("export default (a);", 1),
        ("export default (a => a);", 1),
        ("export default (a, b);", 0),
        ("function f(){ return (\na % b == 0\n) || (a % b == 1); }", 1),
        ("function f(){ return (\na\n); }", 0),
        ("function f(){ return (\n(a % b == 0)\n || a % b == 1\n); }", 1),
    ] {
        assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{source}");
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
        "override in problem_irregular_ws let irregular_whitespace_checked { value overlapping(ts_jsx_text); count } == 0\nrule no-irregular-whitespace",
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
fn problem_selection_self_assignments_keeps_individual_destructured_targets() {
    let source = "[value, object.value] = [value, object.value];";
    let value = source.find("= [value").expect("value assignment") + 3;
    let member = source.rfind("object.value").expect("member assignment");
    for (policy, expected) in [
        ("", vec![value, member]),
        ("override in ts_assignment let self_assignment_findings filter(self_assignments, finding => !finding.member)", vec![value]),
    ] {
        let linter = native_linter(&format!("{policy}\nrule no-self-assign"));
        let diagnostics = linter.lint(Path::new("/root/file.js"), source);
        let offsets: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        assert_eq!(offsets, expected, "{policy}: {diagnostics:#?}");
    }
}

#[test]
fn problem_selection_optional_chaining_separates_arithmetic_and_throwing_uses() {
    let source = "(object?.method)(); object?.value + 1;";
    let method = source.find("object?.method").expect("method chain");
    let value = source.find("object?.value").expect("value chain");
    for (policy, expected) in [
        ("", vec![method]),
        ("override in optional_chain_user let optional_chain_findings optional_chain_risks", vec![method, value]),
        ("override in optional_chain_user let optional_chain_findings filter(optional_chain_risks, finding => finding.arithmetic)", vec![value]),
    ] {
        let linter = native_linter(&format!("{policy}\nrule no-unsafe-optional-chaining"));
        let diagnostics = linter.lint(Path::new("/root/file.js"), source);
        let offsets: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        assert_eq!(offsets, expected, "{policy}: {diagnostics:#?}");
    }
}

#[test]
fn problem_selection_template_quasis_do_not_hide_embedded_code() {
    let linter = native_linter("override in problem_irregular_ws let irregular_whitespace_checked !{ value overlapping(problem_template); any (template) any(template.quasi.node, quasi => quasi.offset <= offset && end_offset <= quasi.end_offset) }\nrule no-irregular-whitespace");
    for source in [
        "const value = `raw\u{00a0}${a\u{00a0}+b}`;",
        "const value = `raw${`nested\u{00a0}${a\u{00a0}+b}`}`;",
    ] {
        let diagnostics = linter.lint(Path::new("/root/file.js"), source);
        let offsets: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        assert_eq!(offsets, [source.find("a\u{00a0}+b").expect("embedded expression") + 1], "{source:?}: {diagnostics:#?}");
    }
}

#[test]
fn problem_selection_loop_policy_keeps_yield_and_nonloop_checks_independent() {
    let loops = "while (true) {} while (1) {} do {} while (true); for (; true;) {} function* g() { while (1) { yield 1; } }";
    for (policy, expected) in [
        ("", 3),
        ("override in ts_while, ts_do_while, ts_for let constant_loop_condition_checked true", 4),
    ] {
        let linter = native_linter(&format!("{policy}\nrule no-constant-condition"));
        let diagnostics = linter.lint(Path::new("/root/file.js"), loops);
        assert_eq!(diagnostics.len(), expected, "{policy}: {diagnostics:#?}");
    }
    let linter = native_linter("override in ts_while, ts_do_while, ts_for let constant_loop_condition_checked false\nrule no-constant-condition");
    let diagnostics = linter.lint(Path::new("/root/file.js"), "if (true) {} while (1) {}");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].start, 4, "{diagnostics:#?}");
}

#[test]
fn problem_selection_nan_searches_are_opt_in_without_hiding_switch_reports() {
    let source = "switch (NaN) { case NaN: break; } items.indexOf(NaN);";
    for (policy, expected) in [
        ("", 2),
        ("group use_isnan_searches ts_call", 3),
        ("rule use-isnan:switch off\nrule use-isnan:case off\ngroup use_isnan_searches ts_call", 1),
    ] {
        let linter = native_linter(&format!("rule use-isnan\n{policy}"));
        let diagnostics = linter.lint(Path::new("/root/file.js"), source);
        assert_eq!(diagnostics.len(), expected, "{policy}: {diagnostics:#?}");
    }
}

#[test]
fn problem_selection_relations_do_not_disable_constant_comparisons() {
    let source = "1 < 2; [] === []; value === undefined;";
    for (policy, expected) in [
        ("", 1),
        ("override in ts_binary, ts_logical let constant_relation_checked true", 2),
        ("override in ts_binary, ts_logical let constant_relation_checked false", 1),
    ] {
        let linter = native_linter(&format!("{policy}\nrule no-constant-binary-expression"));
        let diagnostics = linter.lint(Path::new("/root/file.js"), source);
        assert_eq!(diagnostics.len(), expected, "{policy}: {diagnostics:#?}");
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.start == source.find("[] === []").expect("object comparison")));
    }
}

#[test]
fn problem_selection_empty_patterns_distinguishes_parameter_defaults() {
    let source = "function f({}, {} = {}, {} = value) {} const {} = value; const [] = value;";
    let policy = "override in ts_object_pattern let empty_object_pattern_checked !(parent is ts_parameter && (parent.init == null || re_test(parent.init, /^\\{\\s*\\}$/)))";
    for (selection, expected) in [("", 5), (policy, 3)] {
        let linter = native_linter(&format!("{selection}\nrule no-empty-pattern"));
        let diagnostics = linter.lint(Path::new("/root/file.js"), source);
        assert_eq!(diagnostics.len(), expected, "{selection}: {diagnostics:#?}");
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.start == source.find("[]").expect("array pattern")));
    }
}

#[test]
fn problem_selection_unsafe_negation_checks_the_binary_owner() {
    let source = "!value in object; !value instanceof Type; !value < other; !value === other;";
    for (policy, expected) in [
        ("", 2),
        ("override in ts_binary let unsafe_negation_checked contains([\"in\", \"instanceof\", \"<\", \">\", \"<=\", \">=\"], operator)", 3),
        ("override in ts_binary let unsafe_negation_checked operator == \"instanceof\"", 1),
    ] {
        let linter = native_linter(&format!("{policy}\nrule no-unsafe-negation"));
        let diagnostics = linter.lint(Path::new("/root/file.js"), source);
        assert_eq!(diagnostics.len(), expected, "{policy}: {diagnostics:#?}");
    }
}

#[test]
fn problem_selection_character_classes_retains_unescaped_findings() {
    let source = "/[a\\u0301]/u; /[a\u{0301}]/u;";
    for (policy, expected) in [
        ("", 2),
        ("override in regex_site let misleading_character_class_findings { value (regex_findings.nonempty ?? []); filter (finding) finding.check == \"misleading_escaped\" }", 1),
    ] {
        let linter = native_linter(&format!("{policy}\nrule no-misleading-character-class"));
        let diagnostics = linter.lint(Path::new("/root/file.js"), source);
        assert_eq!(diagnostics.len(), expected, "{policy}: {diagnostics:#?}");
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.start >= source.find("; ").expect("pattern separator") + 2));
    }
}

#[test]
fn directive_policy_variants_keep_defaults_and_independent_modes() {
    let source = "// @ts-nocheck\n// @ts-check\n// @ts-ignore\n// @ts-expect-error why\nconst value = 1;\n// @ts-nocheck";
    let first = source.find("// @ts-nocheck").expect("leading nocheck");
    let check = source.find("// @ts-check").expect("check directive");
    let ignore = source.find("// @ts-ignore").expect("ignore directive");
    for (policy, expected) in [
        ("", vec![first, ignore]),
        ("rule ban-ts-comment:ignore { set mode \"allow\" }\nrule ban-ts-comment:check { set mode \"deny\" }", vec![first, check]),
        ("rule ban-ts-comment:ignore set allow\nrule ban-ts-comment:check set deny", vec![first, check]),
    ] {
        let linter = native_linter(&format!("rule ban-ts-comment\n{policy}"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        let offsets: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        assert_eq!(offsets, expected, "{policy}: {diagnostics:#?}");
    }
}

#[test]
fn directive_policy_description_settings_apply_to_the_selected_variant() {
    let source = "// @ts-expect-error: Valid\n// @ts-expect-error: wrong\n// @ts-expect-error x\n// @ts-ignore\nconst value = 1;";
    let wrong = source.find("// @ts-expect-error: wrong").expect("wrong description");
    let short = source.find("// @ts-expect-error x").expect("short description");
    let ignore = source.find("// @ts-ignore").expect("ignore directive");
    for (policy, expected) in [
        ("", vec![short, ignore]),
        ("rule ban-ts-comment:expect-error { set descriptionPattern /^: [A-Z][a-z]+$/ }\nrule ban-ts-comment:ignore { set mode \"require-description\"\nset minimumDescriptionLength 0 }", vec![wrong, short]),
    ] {
        let linter = native_linter(&format!("rule ban-ts-comment\n{policy}"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        let offsets: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        assert_eq!(offsets, expected, "{policy}: {diagnostics:#?}");
    }
}

#[test]
fn directive_policy_keeps_comment_forms_and_late_nocheck_inert() {
    let source = "// @ts-check reason\nconst value = 1;\n// @ts-nocheck reason\n/* @ts-ignore reason */\n/* @ts-ignore\n trailing */\n/*\n * @ts-expect-error reason */";
    let check = source.find("// @ts-check").expect("check directive");
    let ignore = source.find("/* @ts-ignore reason").expect("single-line block directive");
    let expect = source.find("/*\n * @ts-expect-error").expect("last-line block directive");
    for (policy, expected) in [
        ("", vec![ignore]),
        ("rule ban-ts-comment:check { set mode \"deny\" }\nrule ban-ts-comment:expect-error { set mode \"deny\" }", vec![check, ignore, expect]),
    ] {
        let linter = native_linter(&format!("rule ban-ts-comment\n{policy}"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        let offsets: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        assert_eq!(offsets, expected, "{policy}: {diagnostics:#?}");
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
fn padding_without_active_policies_does_not_read_statement_categories() {
    let source = "first(); second();";
    for options in ["[]", "[{ blankLine: \"any\", prev: \"*\", next: \"*\" }]"] {
        let lint = native_linter(&format!(r#"
override in ts_node let structure_padding_categories byte_slice("x", [])
rule padding-line-between-statements {{ set options {options} }}
"#));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        assert!(diagnostics.is_empty(), "{options}: {diagnostics:#?}");
    }
    let lint = native_linter(r#"
override in ts_node let structure_padding_categories byte_slice("x", [])
rule padding-line-between-statements { set options [{ blankLine: "always", prev: "expression", next: "*" }] }
"#);
    let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
    assert!(!diagnostics.is_empty());
    assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity == crate::config::ast::Severity::Error));
}

#[test]
fn padding_wildcards_and_late_overrides_skip_unused_categories() {
    let source = "first(); second();";
    for (options, warnings) in [
        (r#"[{ blankLine: "always", prev: "*", next: "*" }]"#, 1),
        (r#"[{ blankLine: "always", prev: [], next: "*" }]"#, 0),
        (r#"[{ blankLine: "always", prev: ["expression", "*"], next: ["*", "expression"] }]"#, 1),
        (r#"[{ blankLine: "always", prev: "expression", next: "expression" }, { blankLine: "always", prev: "*", next: "*" }]"#, 1),
        (r#"[{ blankLine: "always", prev: "*", next: "*" }, { blankLine: "any", prev: "*", next: "*" }]"#, 0),
        (r#"[{ blankLine: "any", prev: "*", next: "*" }, { blankLine: "always", prev: "*", next: "*" }]"#, 1),
    ] {
        let lint = native_linter(&format!(r#"
override in ts_node let structure_padding_categories byte_slice("x", [])
rule padding-line-between-statements {{ set options {options} }}
"#));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), warnings, "{options}: {diagnostics:#?}");
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity == crate::config::ast::Severity::Warn), "{options}: {diagnostics:#?}");
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.start == source.find("second").expect("second statement exists")));
    }
}

#[test]
fn padding_side_matching_preserves_named_and_record_selectors() {
    let source = "first(); second();";
    for (prev, warnings) in [
        (r#"["return", "expression"]"#, 1),
        (r#"{ selector: "ExpressionStatement", lineMode: "singleline" }"#, 1),
        (r#"{ selector: "ExpressionStatement", lineMode: "multiline" }"#, 0),
        (r#"{ selector: "*" }"#, 0),
    ] {
        let lint = native_linter(&format!(r#"
rule padding-line-between-statements {{ set options [{{ blankLine: "always", prev: {prev}, next: "*" }}] }}
"#));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), warnings, "{prev}: {diagnostics:#?}");
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity == crate::config::ast::Severity::Warn));
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.start == source.find("second").expect("second statement exists")));
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

#[test]
fn member_ordering_defaults_keep_fields_before_methods_and_static_before_instance() {
    let linter = native_linter("rule member-ordering");
    for source in [
        "class Example { method() {} field = 1; }",
        "interface Example { method(): void; field: number; }",
        "class Example { instance = 1; static field = 2; }",
    ] {
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert_eq!(diagnostics[0].start, source.find(if source.contains("static") { "static field" } else { "field" }).expect("out-of-order field"));
    }
    assert!(linter.lint(Path::new("/root/file.ts"),
        "class Example { static first = 1; second = 2; constructor() {} method() {} }").is_empty());
}

#[test]
fn member_ordering_rank_policy_preserves_gaps_and_checks_the_final_member() {
    let linter = native_linter(r#"
        rule member-ordering:class-declaration
        override in ts_class_property let member_ordering_rank
            if static_name == "unchecked" then null else if static_name == "late" then 0 else 1
    "#);
    let source = "class Example { first = 1; unchecked = 2; second = 3; late = 4; }";
    let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].start, source.find("late").expect("final member"));
    assert!(linter.lint(Path::new("/root/file.ts"),
        "class Example { late = 1; first = 2; unchecked = 3; second = 4; }").is_empty());
}

#[test]
fn member_ordering_modes_belong_to_the_enclosing_context() {
    let linter = native_linter(r#"
        rule member-ordering:class-declaration set alphabetically
        rule member-ordering:class-expression set as-written
        rule member-ordering:interface set natural
        rule member-ordering:type-literal set as-written
        override in ts_class_property, ts_property_signature let member_ordering_rank 0
    "#);
    let source = r#"
        class Example { z = 1; a = 2; }
        const expression = class { z = 1; a = 2; };
        interface ExampleType { value10: number; value2: number; }
        type Literal = { z: number; a: number; };
    "#;
    let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].start, source.find("a = 2").expect("class member"));
    assert_eq!(diagnostics[1].start, source.find("value2").expect("interface member"));
}

#[test]
fn member_ordering_equal_ranks_and_unchecked_optionality_are_inert() {
    let linter = native_linter(r#"
        rule member-ordering:interface { set optionality optional-first }
        override in ts_property_signature let member_ordering_rank
            if static_name == "unchecked" then null else 0
    "#);
    assert!(linter.lint(Path::new("/root/file.ts"),
        "interface Example { optional?: number; unchecked: number; required: number; }").is_empty());
    for source in [
        "interface Example { required: number; unchecked?: number; optional?: number; }",
        "interface Example { optional?: number; required: number; unchecked?: number; last?: number; }",
    ] {
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].start, source.find("required").expect("required member"));
    }
    let required_first = native_linter(r#"
        rule member-ordering:interface { set optionality required-first }
        override in ts_property_signature let member_ordering_rank 0
    "#);
    assert!(required_first.lint(Path::new("/root/file.ts"),
        "interface Example { required: number; optional?: number; }").is_empty());
    let source = "interface Example { optional?: number; required: number; }";
    let diagnostics = required_first.lint(Path::new("/root/file.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].start, source.find("optional?").expect("optional member"));
}

#[test]
fn member_ordering_explains_the_nearest_higher_group_with_its_policy_label() {
    let linter = native_linter(r#"
        rule member-ordering:class-declaration
        override in ts_class_property let member_ordering_rank
            if static_name == "early" then 0 else if static_name == "middle" then 2 else 4
        override in ts_class_property let member_ordering_label static_name
    "#);
    let source = "class Example { middle = 1; last = 2; early = 3; }";
    let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics[0].message.contains("middle"), "{diagnostics:#?}");
    assert!(!diagnostics[0].message.contains("last"), "{diagnostics:#?}");
    let diagnostics = linter.lint(Path::new("/root/file.ts"),
        "class Example { last = 1; middle = 2; early = 3; }");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    assert!(diagnostics.iter().all(|diagnostic| diagnostic.message.contains("last")), "{diagnostics:#?}");
}

#[test]
fn member_ordering_does_not_suggest_crossing_an_initialiser_dependency() {
    let linter = native_linter(r#"
        rule member-ordering:class-declaration set alphabetically
        override in ts_class_property let member_ordering_rank 0
    "#);
    assert!(linter.lint(Path::new("/root/file.ts"),
        "class Example { z = 1; a = this.z; }").is_empty());
    let source = "class Example { z = 1; a = 2; }";
    let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].start, source.find("a = 2").expect("independent member"));
}

#[test]
fn member_ordering_checks_names_even_when_another_rank_is_displaced() {
    for (context, source) in [
        ("class-declaration", "class C { c: number; b: number; high() {} low: number; }"),
        ("class-expression", "const C = class { c: number; b: number; high() {} low: number; };"),
        ("interface", "interface C { c: number; b: number; high(): void; low: number; }"),
        ("type-literal", "type C = { c: number; b: number; high(): void; low: number; };"),
    ] {
        let linter = native_linter(&format!("rule member-ordering:{context} set alphabetically"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), 2, "{context}: {diagnostics:#?}");
        for name in ["b:", "low:"] {
            let offset = source.find(name).expect("member exists");
            assert_eq!(diagnostics.iter().filter(|diagnostic| diagnostic.start == offset
                && diagnostic.severity == crate::config::ast::Severity::Warn).count(), 1, "{diagnostics:#?}");
        }
    }
}

#[test]
fn member_ordering_reports_independent_rank_and_name_errors_without_crossing_runs() {
    let linter = native_linter("rule member-ordering:class-declaration set alphabetically");
    let source = "class C { high() {} c: number; b: number; }";
    let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
    assert_eq!(diagnostics.len(), 3, "{diagnostics:#?}");
    let offset = source.find("b:").expect("last member exists");
    assert_eq!(diagnostics.iter().filter(|diagnostic| diagnostic.start == offset).count(), 2, "{diagnostics:#?}");
    let source = "class C { z: number; high() {} a: number; }";
    let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].start, source.find("a:").expect("last member exists"));
}

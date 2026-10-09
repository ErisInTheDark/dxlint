//! Control-body brace fixes preserve statement ownership and token boundaries.

use std::path::Path;

use super::native_linter;

fn fixed(config: &str, source: &str, expected: &str) {
    let linter = native_linter(config);
    let path = Path::new("/root/control.ts");
    assert!(!linter.lint(path, source).is_empty(), "{source}");
    let result = linter.fix(path, source);
    assert_eq!(result.text, expected, "{source}");
    assert!(linter.lint(path, &result.text).is_empty(), "{:#?}", linter.lint(path, &result.text));
}

#[test]
fn required_braces_wrap_exact_bodies_across_control_forms_and_chains() {
    for (source, expected) in [
        ("if (foo) bar()", "if (foo) {bar()}"),
        ("if (foo) { bar() } else baz()", "if (foo) { bar() } else {baz()}"),
        ("if (foo) { bar() } else if (other) baz()", "if (foo) { bar() } else if (other) {baz()}"),
        ("while (foo)\n bar()", "while (foo)\n {bar()}"),
        ("do bar(); while (foo)", "do {bar();} while (foo)"),
        ("for (;;) bar()", "for (;;) {bar()}"),
        ("for (const item in object) use(item);", "for (const item in object) {use(item);}"),
        ("for (const item of items) use(item);", "for (const item of items) {use(item);}"),
        ("if (foo)\n // keep\n bar();", "if (foo)\n // keep\n {bar();}"),
    ] {
        fixed("rule curly", source, expected);
    }
    fixed(r#"rule curly { set mode "multi"; set style "consistent" }"#,
        "if (foo) bar(); else { const value = 1; }",
        "if (foo) {bar();} else { const value = 1; }");
}

#[test]
fn unnecessary_braces_preserve_trivia_and_separate_adjacent_keywords() {
    for (source, expected) in [
        ("if (foo) { bar() }", "if (foo)  bar() "),
        ("do{foo();} while (bar)", "do foo(); while (bar)"),
        ("do{[1].map(foo);} while (bar)", "do[1].map(foo); while (bar)"),
        ("if (foo) foo(); else{bar();}", "if (foo) foo(); else bar();"),
        ("if (foo) { bar; }\n++baz;", "if (foo)  bar; \n++baz;"),
        ("while (foo) {\n /* keep */ bar();\n}", "while (foo) \n /* keep */ bar();\n"),
        ("if (foo) { if (other) bar(); else baz(); } else last();", "if (foo)  if (other) bar(); else baz();  else last();"),
        ("if (foo) { while (bar) {} } else {}", "if (foo)  while (bar) {}  else {}"),
        ("if (a) { while (cond) { if (b) foo(); } } else bar();", "if (a)  while (cond) { if (b) foo(); }  else bar();"),
    ] {
        fixed(r#"rule curly { set mode "multi" }"#, source, expected);
    }
}

#[test]
fn brace_removal_keeps_asi_hazards_unfixed_and_required_scope_blocks_intact() {
    let linter = native_linter(r#"rule curly { set mode "multi" }"#);
    let path = Path::new("/root/control.ts");
    for source in [
        "if (foo) {bar()} baz()",
        "if (foo) { bar }\n++baz;",
        "if (foo) { bar++ }\nbaz;",
        "if (foo) { bar }\n[1].map(foo);",
        "if (foo) { bar }\n(1).toString();",
        "if (foo) { bar }\n/regex/.test('foo');",
        "if (foo) { var fn = () => {} } else {}",
        "if (a) { if (b) one(); else two() } else three()",
    ] {
        assert!(!linter.lint(path, source).is_empty(), "{source}");
        assert_eq!(linter.fix(path, source).text, source);
    }
    for source in [
        "if (foo) { let scoped; }",
        "if (foo) { class Scoped {} }",
        "if (foo) { function scoped() {} }",
        "if (foo) { if (other) bar(); } else baz();",
    ] {
        assert!(linter.lint(path, source).is_empty(), "{source}");
        assert_eq!(linter.fix(path, source).text, source);
    }
}

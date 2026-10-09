//! Object member syntax fixes preserve callable bodies and lexical ownership.

use std::path::Path;

use super::native_linter;

fn fixed(config: &str, source: &str, expected: &str) {
    let linter = native_linter(config);
    let path = Path::new("/root/object.ts");
    let diagnostics = linter.lint(path, source);
    assert!(!diagnostics.is_empty(), "{source}");
    assert!(diagnostics.iter().all(|diagnostic| diagnostic.rule == "object-shorthand"), "{diagnostics:#?}");
    let result = linter.fix(path, source);
    assert_eq!(result.text, expected, "{source}");
    assert!(linter.lint(path, &result.text).is_empty(), "{:#?}", linter.lint(path, &result.text));
}

#[test]
fn member_fixes_preserve_keys_parameters_types_modifiers_and_body_text() {
    for (source, expected) in [
        ("const o = {x: x, 'y': y};", "const o = {x, y};"),
        ("const o = {f: function(a) { return a; }};", "const o = {f(a) { return a; }};"),
        ("const o = {[key]: function*() { yield 1; }};", "const o = {*[key]() { yield 1; }};"),
        ("const o = {'foo': async function() {}};", "const o = {async 'foo'() {}};"),
        ("const o = {foo: async function () {}};", "const o = {async foo () {}};"),
        ("const o = {['__proto__']: function () {}};", "const o = {['__proto__'] () {}};"),
        ("const o = {f: async function*() {}};", "const o = {async *f() {}};"),
        ("const o = {f: function() { /* keep */ return 1; }};", "const o = {f() { /* keep */ return 1; }};"),
        ("const o = {x: foo => { return foo; }};", "const o = {x(foo) { return foo; }};"),
        ("const o = {x: ({ foo: bar = 1 } = {}) => { return bar; }};", "const o = {x({ foo: bar = 1 } = {}) { return bar; }};"),
        ("const o = {x: ((arg) => { return arg; })};", "const o = {x(arg) { return arg; }};"),
        ("const o = {[key]: async bar => { return bar; }};", "const o = {async [key](bar) { return bar; }};"),
        ("const o = {x: <T>(arg: T): T => { return arg; }};", "const o = {x<T>(arg: T): T { return arg; }};"),
        ("const o = {x: async (): (void) => {}};", "const o = {async x(): (void) {}};"),
        ("const o = {x: () => { function inner() { return this; } }};", "const o = {x() { function inner() { return this; } }};"),
    ] {
        fixed("rule object-shorthand { set avoidExplicitReturnArrows true }", source, expected);
    }
}

#[test]
fn longform_fixes_preserve_method_modifiers_and_computed_key_parentheses() {
    for (source, expected) in [
        ("const o = {x, y() {}};", "const o = {x: x, y: function() {}};"),
        ("const o = {*x() { yield 1; }};", "const o = {x: function*() { yield 1; }};"),
        ("const o = {async x() {}};", "const o = {x: async function() {}};"),
        ("const o = {async* x() {}};", "const o = {x: async function*() {}};"),
        ("const o = {[((key))]() {}};", "const o = {[((key))]: function() {}};"),
        ("const o = {x<T>(arg: T): T { return arg; }};", "const o = {x: function<T>(arg: T): T { return arg; }};"),
    ] {
        fixed(r#"rule object-shorthand { set mode "never" }"#, source, expected);
    }
    fixed("rule object-shorthand { set avoidQuotes true }", "const o = {'x'() {}};", "const o = {'x': function() {}};");
}

#[test]
fn member_exceptions_keep_lexical_captures_annotations_and_configuration() {
    for (config, source) in [
        ("rule object-shorthand { set avoidExplicitReturnArrows true }", "function owner() { return {x: () => { return this; }, y: () => { return arguments; }, z: () => { return new.target; }}; }"),
        ("rule object-shorthand", "const o = {x: function named() {}, __proto__: value, y: /** @type {number} */ y};"),
        ("rule object-shorthand { set ignoreConstructors true }", "const o = {Constructor: function() {}};"),
        ("rule object-shorthand { set methodsIgnorePattern '^ignored$' }", "const o = {ignored: function() {}};"),
        ("rule object-shorthand { set avoidQuotes true }", "const o = {'x': function() {}, 'y': y};"),
        (r#"rule object-shorthand { set mode "consistent" }"#, "const o = {x, y};"),
    ] {
        let linter = native_linter(config);
        let path = Path::new("/root/object.ts");
        assert!(linter.lint(path, source).is_empty(), "{source}: {:#?}", linter.lint(path, source));
        assert_eq!(linter.fix(path, source).text, source);
    }
}

#[test]
fn member_rewrites_do_not_discard_comments_in_removed_syntax() {
    let linter = native_linter("rule object-shorthand { set avoidExplicitReturnArrows true }");
    let path = Path::new("/root/object.ts");
    for source in [
        "const o = {x: /* retain */ x};",
        "const o = {x: /* retain */ function() {}};",
        "const o = {x: () => /* retain */ {}};",
        "const o = {x: (/* retain */ () => {})};",
    ] {
        assert!(!linter.lint(path, source).is_empty(), "{source}");
        assert_eq!(linter.fix(path, source).text, source);
    }
    let longform = native_linter(r#"rule object-shorthand { set mode "never" }"#);
    let source = "const o = {async /* retain */ x() {}};";
    assert!(!longform.lint(path, source).is_empty());
    assert_eq!(longform.fix(path, source).text, source);
}

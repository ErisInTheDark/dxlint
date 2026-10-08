use std::path::Path;

use crate::engine::tests::native_linter;

#[test]
fn promise_boolean_consumers_check_leaves_without_duplicate_containers() {
    let lint = native_linter("rule no-misused-promises");
    let cases = [
        ("if (p) {}", 1),
        ("if (!p) {}", 1),
        ("p ? 1 : 2;", 1),
        ("p || false;", 1),
        ("p && true;", 1),
        ("true && p;", 0),
        ("(true && p) || false;", 0),
        ("(p ?? false) && true;", 0),
        ("if (true && p) {}", 1),
        ("if (p ?? false) {}", 1),
        ("p ?? false;", 0),
        ("if (b || p) {}", 1),
        ("if (b && (p)) {}", 1),
        ("if (a ?? p) {}", 1),
        ("if (b ? p : p) {}", 2),
        ("if (p || q) {}", 2),
        ("if (p && p) {}", 2),
        // The second read is unreachable after a definitely truthy object.
        ("if (p || p) {}", 1),
        ("a ?? p;", 0),
        ("b ? p : p;", 0),
        ("if (await p) {}", 0),
        ("if (maybe) {}", 0),
    ];
    let counts: Vec<_> = cases.iter().map(|(body, _)| {
        let source = format!("interface Promise<T> {{ then(callback: (value: T) => unknown): unknown }} async function f(p: Promise<boolean>, q: Promise<boolean>, maybe: Promise<boolean> | undefined, b: boolean, a: any) {{ {body} }}");
        lint.lint(Path::new("/root/file.ts"), &source).len()
    }).collect();
    assert_eq!(counts, cases.iter().map(|(_, expected)| *expected).collect::<Vec<_>>(), "{cases:?}");
}

#[test]
fn object_spreads_check_each_thenable_union_but_not_iterable_spreads() {
    let lint = native_linter("rule no-misused-promises");
    let cases = [
        ("({ ...p });", 1),
        ("({ ...(b && p) });", 1),
        ("({ ...(b || p) });", 1),
        ("({ ...(b ? {} : p) });", 1),
        ("({ ...(b ? p : {}) });", 1),
        ("({ ...p, ...p });", 2),
        ("[...p];", 0),
        ("g(...p);", 0),
        ("({ ...await p });", 0),
        ("({ ...a });", 0),
    ];
    let counts: Vec<_> = cases.iter().map(|(body, _)| {
        let source = format!("interface Promise<T> {{ then(callback: (value: T) => unknown): unknown }} async function f(p: Promise<{{ key: number }}>, b: boolean, a: any) {{ {body} }}");
        lint.lint(Path::new("/root/file.ts"), &source).len()
    }).collect();
    assert_eq!(counts, cases.iter().map(|(_, expected)| *expected).collect::<Vec<_>>(), "{cases:?}");
}

#[test]
fn structural_thenables_and_constrained_parameters_are_awaitable() {
    let lint = native_linter("rule no-misused-promises");
    let source = r#"
        interface Awaitable<T> { then(callback: (value: T) => unknown): unknown }
        function f<T extends Awaitable<number>>(custom: Awaitable<number>, constrained: T) {
            if (custom) {}
            if (constrained) {}
        }
        function safe(ordinary: { then: string }, unknown: unknown, anything: any) {
            if (ordinary) {}
            if (unknown) {}
            if (anything) {}
        }
    "#;
    assert_eq!(lint.lint(Path::new("/root/file.ts"), source).len(), 2);
}

#[test]
fn mixed_union_policy_compares_resolved_structures_in_both_directions() {
    let cases = [
        ("number | Promise<number>", [0, 1, 1]),
        ("boolean | Promise<number>", [0, 0, 1]),
        ("number | string | Promise<number | string>", [0, 1, 1]),
        ("number | string | undefined | Promise<number | string>", [0, 0, 1]),
        ("string[] | Promise<string[]>", [0, 1, 1]),
        ("string[] | Promise<number[]>", [0, 0, 1]),
        ("[number, string] | Promise<[number, string]>", [0, 1, 1]),
        ("[number, string] | Promise<[string, number]>", [0, 0, 1]),
        ("{ value: number } | Promise<{ value: number }>", [0, 1, 1]),
        ("{ value: number } | Promise<{ other: number }>", [0, 0, 1]),
        ("Promise<number> | undefined", [0, 0, 1]),
        ("Promise<number>", [1, 1, 1]),
    ];
    for (index, mode) in ["none", "strict", "all"].iter().enumerate() {
        let lint = native_linter(&format!("rule no-misused-promises:conditionals {{ set flagUnions \"{mode}\" }}"));
        let counts: Vec<_> = cases.iter().map(|(ty, _)| {
            let source = format!("interface Promise<T> {{ then(callback: (value: T) => unknown): unknown }} function f(value: {ty}) {{ if (value) {{}} }}");
            lint.lint(Path::new("/root/file.ts"), &source).len()
        }).collect();
        assert_eq!(counts, cases.iter().map(|(_, expected)| expected[index]).collect::<Vec<_>>(), "{mode}: {cases:?}");
    }
}

#[test]
fn promise_consumers_can_be_disabled_independently() {
    let source = "interface Promise<T> { then(callback: (value: T) => unknown): unknown } function f(p: Promise<number>) { if (p) {} const value = { ...p }; }";
    for (disabled, remaining) in [("spreads", "conditionals"), ("conditionals", "spreads")] {
        let lint = native_linter(&format!("rule no-misused-promises\nrule no-misused-promises:{disabled} off"));
        let reports = lint.lint(Path::new("/root/file.ts"), source);
        assert_eq!(reports.len(), 1, "{disabled}: {reports:?}");
        assert_eq!(reports[0].rule, format!("no-misused-promises:{remaining}"));
    }
}

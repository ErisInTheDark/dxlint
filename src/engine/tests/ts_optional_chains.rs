use std::path::Path;

use super::native_linter;
use crate::engine::{apply_fixes, Linter};

#[test]
fn optional_chains_report_calls_and_computed_access_once() {
    let linter = native_linter("rule prefer-optional-chain");
    for (source, chain) in [
        ("declare const foo: (() => number) | null; foo && foo();", "foo && foo()"),
        ("declare const foo: any; declare const key: string; foo && foo[key] && foo[key].value;", "foo && foo[key] && foo[key].value"),
        ("declare const foo: any; foo.bar! && foo.bar.value;", "foo.bar! && foo.bar.value"),
    ] {
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        let start = source.find(chain).expect("reported chain");
        assert_eq!(diagnostics.iter().map(|d| (d.start, d.end)).collect::<Vec<_>>(), [(start, start + chain.len())], "{source}");
    }
}

#[test]
fn optional_chains_preserve_guard_semantics_and_independent_segments() {
    let linter = native_linter("rule prefer-optional-chain");
    for (source, expected) in [
        ("declare const foo: false | { value: number }; foo && foo.value;", 0),
        ("declare const foo: 0 | { value: number }; foo && foo.value;", 0),
        ("declare const foo: '' | { value: number }; foo && foo.value;", 0),
        ("declare const foo: 0n | { value: number }; foo && foo.value;", 0),
        ("declare const foo: void | (() => void); foo && foo();", 0),
        ("declare const foo: { left: number; right: number } | null; foo != null && foo.left === foo.right;", 0),
        ("declare const foo: { value: number } | null; foo && foo.value === undefined;", 0),
        ("declare const foo: { value: number } | null | undefined; foo !== null && foo !== undefined && foo.value;", 1),
        ("declare const foo: any; declare const bar: any; foo && foo() || bar && bar();", 2),
        ("declare const foo: any; foo && (foo.bar && foo.bar.value);", 1),
        ("declare const foo: any; (foo?.bar).value && foo.bar.value.next;", 0),
        ("declare const foo: any; (foo ?? {}).value;", 1),
    ] {
        assert_eq!(linter.lint(Path::new("/root/file.ts"), source).len(), expected, "{source}");
    }
}

#[test]
fn optional_chains_obey_primitive_and_nullish_options() {
    let source = "declare const foo: string; foo && foo.length;";
    assert_eq!(native_linter("rule prefer-optional-chain").lint(Path::new("/root/file.ts"), source).len(), 1);
    for options in ["set checkString false", "set requireNullish true"] {
        let linter = native_linter(&format!("rule prefer-optional-chain {{ {options} }}"));
        assert!(linter.lint(Path::new("/root/file.ts"), source).is_empty(), "{options}");
    }
}

fn fixes(linter: &Linter, cases: &[(&str, &str, usize)]) {
    let path = Path::new("/root/optional.ts");
    let checked: Vec<_> = cases.iter().map(|&(source, expected, count)| {
        assert!(crate::native::ts::parse(source, false).is_ok(), "invalid fixture: {source}");
        let diagnostics = linter.lint(path, source);
        assert_eq!(diagnostics.len(), count, "{source}: {diagnostics:?}");
        (source, expected, count, diagnostics)
    }).collect();
    for (source, expected, count, diagnostics) in checked {
        let (fixed, applied) = apply_fixes(source, &diagnostics);
        assert_eq!(applied, count, "{source}: {diagnostics:?}");
        assert_eq!(fixed, expected);
        assert!(crate::native::ts::parse(&fixed, false).is_ok(), "{fixed}");
        assert!(linter.lint(path, &fixed).is_empty(), "{fixed}");
        assert_eq!(linter.fix(path, &fixed).text, fixed);
    }
}

#[test]
fn optional_fixes_add_only_guarded_hops_and_keep_call_receivers() {
    let linter = native_linter("rule prefer-optional-chain");
    fixes(&linter, &[
        ("declare const foo: any; foo && foo.bar;", "declare const foo: any; foo?.bar;", 1),
        ("declare const foo: any; foo.bar && foo.bar.baz.buzz;", "declare const foo: any; foo.bar?.baz.buzz;", 1),
        ("declare const foo: any; foo && foo.bar && foo.bar.baz && foo.bar.baz;",
            "declare const foo: any; foo?.bar?.baz && foo.bar.baz;", 1),
        ("declare const foo: any; foo && foo.bar && foo.bar('value');",
            "declare const foo: any; foo?.bar?.('value');", 1),
        ("declare const foo: any; declare const key: string; foo && foo[key] && foo[key].value;",
            "declare const foo: any; declare const key: string; foo?.[key]?.value;", 1),
        ("declare const foo: any; declare const key: string; foo && foo[key as string] && foo[key as string].value;",
            "declare const foo: any; declare const key: string; foo?.[key as string]?.value;", 1),
        ("declare const foo: any; foo && foo<string>() && foo<string>().value;",
            "declare const foo: any; foo?.<string>()?.value;", 1),
        ("declare const foo: any; foo && foo?.bar && foo?.bar.baz?.buzz && foo.bar.baz.buzz();",
            "declare const foo: any; foo?.bar?.baz?.buzz?.();", 1),
        ("declare const foo: any; foo && foo.bar(/*keep*/'value',\n//keep\n2,);",
            "declare const foo: any; foo?.bar(/*keep*/'value',\n//keep\n2,);", 1),
        ("declare const foo: any; foo.bar! && foo.bar!.value;",
            "declare const foo: any; foo.bar?.value;", 1),
        ("declare const foo: any; async function f() { (await foo).bar && (await foo).bar.baz; }",
            "declare const foo: any; async function f() { (await foo).bar?.baz; }", 1),
    ]);
}

#[test]
fn optional_fixes_preserve_terminal_comparisons_and_or_negation() {
    let linter = native_linter("rule prefer-optional-chain");
    fixes(&linter, &[
        ("declare const foo: any; !foo || !foo.bar || !foo.bar.baz;",
            "declare const foo: any; !foo?.bar?.baz;", 1),
        ("declare const foo: any; foo == null || foo.bar == null || !foo.bar.baz;",
            "declare const foo: any; !foo?.bar?.baz;", 1),
        ("declare const foo: any; foo !== undefined && foo !== null && foo.bar;",
            "declare const foo: any; foo?.bar;", 1),
        ("declare const foo: any; null !== foo && undefined !== foo && foo.bar;",
            "declare const foo: any; foo?.bar;", 1),
    ]);
}

#[test]
fn optional_fixes_balance_partial_groups_and_preserve_surviving_text() {
    let linter = native_linter("rule prefer-optional-chain");
    fixes(&linter, &[
        ("declare const foo: any; declare const other: any; foo && (foo.bar && other);",
            "declare const foo: any; declare const other: any; foo?.bar && other;", 1),
        ("declare const foo: any; foo && /*removed guard (((*/ (foo.bar && (foo.bar as any).baz);",
            "declare const foo: any; foo?.bar && (foo.bar as any).baz;", 1),
        ("declare const foo: any; foo && (foo.bar/*keep )))*/ && (foo.bar as any).baz);",
            "declare const foo: any; foo?.bar/*keep )))*/ && (foo.bar as any).baz;", 1),
        ("declare const foo: any; foo && foo<string>() && foo<string, number>().bar;",
            "declare const foo: any; foo?.<string>() && foo<string, number>().bar;", 1),
        ("declare const foo: any; declare const other: any; foo && foo.bar || other && other.value;",
            "declare const foo: any; declare const other: any; foo?.bar || other?.value;", 2),
        ("declare const foo: any; declare const other: any; foo && (((foo.bar && (other))));",
            "declare const foo: any; declare const other: any; foo?.bar && (other);", 1),
        ("declare const foo: any; async function f() { doThing()\nnull != (await foo).bar && (await foo).bar.baz; }",
            "declare const foo: any; async function f() { doThing()\n;(await foo).bar?.baz; }", 1),
    ]);
}

#[test]
fn optional_fixes_do_not_add_undefined_to_a_known_return_type_by_default() {
    let linter = native_linter("rule prefer-optional-chain");
    let path = Path::new("/root/optional.ts");
    for source in [
        "declare const foo: { run(): number } | null; foo && foo.run();",
        "declare const foo: { bar: number } | null; const value = foo && foo.bar;",
        "declare const foo: { bar: number } | null | undefined; foo != undefined && foo.bar;",
        "declare const foo: string | null; (foo || {}).toString();",
        "declare const foo: { bar: number } | null | undefined; foo && foo.bar === 3;",
        "declare const foo: any; foo && typeof foo.bar !== 'undefined';",
        "declare const foo: any; foo && 'undefined' !== typeof foo.bar;",
        "foo && foo.bar != null && foo.bar.baz !== undefined && foo.bar.baz.buzz;",
        "declare const foo: { bar: () => { baz: number } | null | undefined }; foo.bar !== undefined && foo.bar?.() !== undefined && foo.bar?.().baz;",
        "declare const foo: { bar: () => { baz: { buzz: (() => number) | null | undefined } | null | undefined } | null | undefined }; foo.bar === undefined || foo.bar() === undefined || foo.bar().baz === undefined || foo.bar().baz.buzz === undefined || foo.bar().baz.buzz();",
    ] {
        let diagnostics = linter.lint(path, source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert!(diagnostics[0].fix.is_none(), "{source}");
        assert_eq!(linter.fix(path, source).text, source);
    }
}

#[test]
fn optional_constructor_values_keep_function_member_return_types() {
    let source = "class Foo { constructor() { new.target && new.target.length; } }";
    let project = super::buffered_project("optional-constructor", &[
        ("tsconfig.json", r#"{"compilerOptions":{"lib":["fixture"],"types":[]}}"#),
        ("node_modules/typescript/lib/lib.fixture.d.ts", "interface Function { readonly length: number }"),
        ("main.ts", source),
    ], "rule prefer-optional-chain");
    let path = project.path("main.ts");
    let diagnostics = project.linter.lint(&path, source);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].fix.is_none());
    assert_eq!(project.linter.fix(&path, source).text, source);
}

#[test]
fn optional_fixes_allow_return_type_changes_only_with_explicit_opt_in() {
    let linter = native_linter("rule prefer-optional-chain { set allowPotentiallyUnsafeFixesThatModifyTheReturnTypeIKnowWhatImDoing true }");
    fixes(&linter, &[
        ("declare const foo: { bar: number } | null | undefined; foo != undefined && foo.bar;",
            "declare const foo: { bar: number } | null | undefined; foo?.bar;", 1),
        ("declare const foo: { run(): number } | null; foo && foo.run();",
            "declare const foo: { run(): number } | null; foo?.run();", 1),
        ("declare const foo: { bar: number } | null | undefined; foo && foo.bar === 3;",
            "declare const foo: { bar: number } | null | undefined; foo?.bar === 3;", 1),
        ("declare const foo: any; foo && typeof foo.bar !== 'undefined';",
            "declare const foo: any; typeof foo?.bar !== 'undefined';", 1),
        ("declare const foo: any; foo && 'undefined' !== typeof foo.bar;",
            "declare const foo: any; 'undefined' !== typeof foo?.bar;", 1),
    ]);
}

#[test]
fn optional_partial_groups_resolve_overlapping_segments_on_later_fix_passes() {
    let linter = native_linter("rule prefer-optional-chain");
    let path = Path::new("/root/optional.ts");
    let source = "foo && (foo.bar && bar) && (bar.baz)";
    let diagnostics = linter.lint(path, source);
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_some()));
    let (first, applied) = apply_fixes(source, &diagnostics);
    assert_eq!(applied, 1);
    assert_eq!(first, "foo?.bar && bar && (bar.baz)");
    let fixed = linter.fix(path, source).text;
    assert_eq!(fixed, "foo?.bar && bar?.baz");
    assert!(linter.lint(path, &fixed).is_empty());
}

#[test]
fn optional_terminal_pairs_stop_at_the_reported_comparison() {
    let linter = native_linter("rule prefer-optional-chain");
    let path = Path::new("/root/optional.ts");
    let checked: Vec<_> = [
        ("foo && foo[key as string] && foo[key as string].value;", "foo && foo[key as string] && foo[key as string].value",
            Some("foo?.[key as string]?.value;")),
        ("null != foo && 'undefined' !== typeof foo.bar && null !== foo.bar && undefined !== foo.bar.baz && null !== foo.bar.baz;",
            "null != foo && 'undefined' !== typeof foo.bar && null !== foo.bar && undefined !== foo.bar.baz",
            Some("undefined !== foo?.bar?.baz && null !== foo.bar.baz;")),
        ("foo != null && typeof foo.bar !== 'undefined' && foo.bar !== null && foo.bar.baz !== undefined && foo.bar.baz !== null;",
            "foo != null && typeof foo.bar !== 'undefined' && foo.bar !== null && foo.bar.baz !== undefined",
            Some("foo?.bar?.baz !== undefined && foo.bar.baz !== null;")),
        ("null != foo && 'undefined' !== typeof foo.bar && null !== foo.bar && null !== foo.bar.baz && 'undefined' !== typeof foo.bar.baz;",
            "null != foo && 'undefined' !== typeof foo.bar && null !== foo.bar && null !== foo.bar.baz", None),
        ("foo != null && typeof foo.bar !== 'undefined' && foo.bar !== null && foo.bar.baz !== null && typeof foo.bar.baz !== 'undefined';",
            "foo != null && typeof foo.bar !== 'undefined' && foo.bar !== null && foo.bar.baz !== null", None),
        ("null != foo && 'undefined' !== typeof foo.bar && null !== foo.bar && null !== foo.bar.baz && undefined !== foo.bar.baz;",
            "null != foo && 'undefined' !== typeof foo.bar && null !== foo.bar && null !== foo.bar.baz", None),
        ("foo != null && typeof foo.bar !== 'undefined' && foo.bar !== null && foo.bar.baz !== null && foo.bar.baz !== undefined;",
            "foo != null && typeof foo.bar !== 'undefined' && foo.bar !== null && foo.bar.baz !== null", None),
    ].into_iter().map(|(source, chain, expected)| {
        assert!(crate::native::ts::parse(source, false).is_ok(), "{source}");
        let diagnostics = linter.lint(path, source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        let start = source.find(chain).expect("reported chain");
        (source, expected, diagnostics, (start, start + chain.len()))
    }).collect();
    assert_eq!(checked.iter().map(|(_, _, diagnostics, _)| (diagnostics[0].start, diagnostics[0].end)).collect::<Vec<_>>(),
        checked.iter().map(|(_, _, _, span)| *span).collect::<Vec<_>>());
    for (source, expected, diagnostics, _) in checked {
        if let Some(expected) = expected {
            let (fixed, applied) = apply_fixes(source, &diagnostics);
            assert_eq!(applied, 1);
            assert_eq!(fixed, expected);
            assert!(crate::native::ts::parse(&fixed, false).is_ok(), "{fixed}");
        } else {
            assert!(diagnostics[0].fix.is_none());
        }
    }
}

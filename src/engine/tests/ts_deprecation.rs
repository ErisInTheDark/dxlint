use std::path::Path;

use super::{lint_project, native_linter};

#[test]
fn imported_overload_docs_apply_only_to_the_selected_signature() {
    let messages = lint_project(
        "deprecated-overloads",
        &[
            ("defs.ts", "/** @deprecated */ export function f(x: string): void; export function f(x: number): void; export function f(x: string | number) {}"),
            ("main.ts", "import { f } from './defs'; f(1); f('s');"),
        ],
        "main.ts",
        "rule no-deprecated",
    );
    assert_eq!(messages.len(), 1, "{messages:#?}");
}

#[test]
fn deprecation_reports_uses_not_declarations_or_unselected_overloads() {
    let lint = native_linter("rule no-deprecated");
    for (source, spans) in [
        ("/** @deprecated */ const old = 1; old;", vec!["old"]),
        ("/** @deprecated */ function old() {} old();", vec!["old"]),
        ("interface A { /** @deprecated */ old: number } declare const a: A; a.old; a['old'];", vec!["old", "'old'"]),
        ("/** @deprecated */ function f(x: string): void; function f(x: number): void; function f(x: string | number) {} f(1); f('s');", vec!["f"]),
        ("function f(x: string): void; /** @deprecated */ function f(x: number): void; function f(x: string | number) {} f('s'); f(1);", vec!["f"]),
        ("interface A { /** @deprecated */ old: number } declare const a: A; const { old: local } = a; local;", vec!["old"]),
        ("const current = 1; current;", vec![]),
    ] {
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{source}: {diagnostics:#?}");
    }
}

#[test]
fn deprecation_allow_matches_the_referenced_value_and_its_origin() {
    let source = "/** @deprecated */ const old = 1; old;";
    for (allow, spans) in [
        ("[]", vec!["old"]),
        ("['old']", vec![]),
        ("[{ from: 'file', name: 'old' }]", vec![]),
        ("[{ from: 'file', name: 'other' }]", vec!["old"]),
        ("[{ from: 'lib', name: 'old' }]", vec!["old"]),
    ] {
        let lint = native_linter(&format!("rule no-deprecated {{ set allow {allow} }}"));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{allow}: {diagnostics:#?}");
    }
}

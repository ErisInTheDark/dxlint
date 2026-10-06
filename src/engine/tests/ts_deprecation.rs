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
fn constructor_deprecation_uses_selected_overloads_and_keeps_class_docs() {
    let declarations = r#"
export class ClassWithDeprecatedConstructor {
    constructor();
    /** @deprecated */
    constructor(arg: string);
    constructor(arg?: string) {}
}
/** @deprecated */
export class DeprecatedClass {
    constructor();
    constructor(arg: string);
    constructor(arg?: string) {}
}
"#;
    let actual: Vec<_> = [
        ("import { ClassWithDeprecatedConstructor } from './defs';", "ClassWithDeprecatedConstructor", "", 0),
        ("import { ClassWithDeprecatedConstructor } from './defs';", "ClassWithDeprecatedConstructor", "'legacy'", 1),
        ("import * as imported from './defs';", "imported.ClassWithDeprecatedConstructor", "", 0),
        ("import * as imported from './defs';", "imported.ClassWithDeprecatedConstructor", "'legacy'", 1),
        ("import { DeprecatedClass } from './defs';", "DeprecatedClass", "", 1),
        ("import { DeprecatedClass } from './defs';", "DeprecatedClass", "'legacy'", 1),
    ].into_iter().map(|(import, callee, args, expected)| {
        let source = format!("{import}\nconst foo = new {callee}({args});");
        let messages = lint_project("deprecated-constructors", &[("defs.ts", declarations), ("main.ts", &source)], "main.ts", "rule no-deprecated");
        (messages.len(), expected)
    }).collect();
    assert_eq!(actual.iter().map(|(count, _)| *count).collect::<Vec<_>>(), actual.iter().map(|(_, expected)| *expected).collect::<Vec<_>>());
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
        ("/** @deprecated */ const old = 1; void { old };", vec!["old"]),
        ("/** @deprecated */ const old = 1; void { old: 2 };", vec![]),
        ("/** @deprecated */ const old = 1; void { current: old };", vec!["old"]),
        ("const current = 1; current;", vec![]),
    ] {
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{source}: {diagnostics:#?}");
    }
}

#[test]
fn shorthand_properties_resolve_imported_declarations_and_export_aliases() {
    let messages = lint_project(
        "deprecated-shorthand",
        &[
            ("defs.ts", "/** @deprecated */ export const deprecatedVariable = 1; const normalVariable = 1; /** @deprecated */ export { normalVariable };"),
            ("main.ts", "import { deprecatedVariable, normalVariable } from './defs'; void { deprecatedVariable }; void { normalVariable };"),
        ],
        "main.ts",
        "rule no-deprecated",
    );
    assert_eq!(messages.len(), 2, "{messages:#?}");
}

#[test]
fn deprecation_allow_matches_the_referenced_value_and_its_origin() {
    let source = "/** @deprecated */ const old = 1; old;";
    for (options, selection, spans) in [
        ("{ set allow [] }", "", vec!["old"]),
        ("{ set allow ['old'] }", "", vec![]),
        ("", "true", vec!["old"]),
        ("", "false", vec![]),
        ("", "!any(deprecation_metadata.declarations, declaration => any(declaration.provenance, gate => gate.from == 'file' && gate.name == 'old'))", vec![]),
        ("", "!any(deprecation_metadata.declarations, declaration => any(declaration.provenance, gate => gate.from == 'file' && gate.name == 'other'))", vec!["old"]),
        ("", "!any(deprecation_metadata.declarations, declaration => any(declaration.provenance, gate => gate.from == 'lib' && gate.name == 'old'))", vec!["old"]),
    ] {
        let selection = if selection.is_empty() { String::new() } else { format!("override in ts_node let no_deprecated_checked {selection}") };
        let lint = native_linter(&format!("rule no-deprecated {options}\n{selection}"));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{options} {selection}: {diagnostics:#?}");
    }
}

#[test]
fn inferred_object_properties_keep_declaration_docs_through_aliases_and_spreads() {
    let lint = native_linter("rule no-deprecated");
    for (source, spans) in [
        ("const a = { /** @deprecated */ old: 1, current: 2 }; a.old; a['old']; a.current;", vec!["old", "'old'"]),
        ("let a = { /** @deprecated */ ['old']: 1 }; const alias = a; alias.old;", vec!["old"]),
        ("const a = { inner: { /** @deprecated */ old: 1 } }; const alias = a.inner; alias.old;", vec!["old"]),
        ("const a = { /** @deprecated */ old: 1 }; const copy = { ...a }; copy.old;", vec!["old"]),
        ("const a = { /** @deprecated */ old: 1 }; const copy = { old: 1, ...a }; copy.old;", vec!["old"]),
        ("const a = { /** @deprecated */ old: 1 }; const copy = { ...a, old: 1 }; copy.old;", vec![]),
        ("const a = { /** @deprecated */ old: 1 }; const current = { old: 1 }; const copy = { ...a, ...current }; copy.old;", vec![]),
        ("/** @deprecated */ const a = { current: 1 }; const copy = { ...a }; copy.current;", vec!["a"]),
        ("const a = { current: 1 }; const copy = { ...a }; copy.current;", vec![]),
    ] {
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{source}: {diagnostics:#?}");
    }
}

#[test]
fn inferred_property_allow_uses_the_property_name_and_source_file() {
    let source = "const a = { /** @deprecated */ old: 1 }; const copy = { ...a }; copy.old;";
    for (options, selection, reports) in [
        ("{ set allow [] }", "", 1),
        ("{ set allow ['old'] }", "", 0),
        ("", "!any(deprecation_metadata.declarations, declaration => any(declaration.provenance, gate => gate.from == 'file' && gate.name == 'old'))", 0),
        ("", "!any(deprecation_metadata.declarations, declaration => any(declaration.provenance, gate => gate.from == 'file' && gate.name == 'other'))", 1),
        ("", "!any(deprecation_metadata.declarations, declaration => any(declaration.provenance, gate => gate.from == 'lib' && gate.name == 'old'))", 1),
    ] {
        let selection = if selection.is_empty() { String::new() } else { format!("override in ts_node let no_deprecated_checked {selection}") };
        let lint = native_linter(&format!("rule no-deprecated {options}\n{selection}"));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), reports, "{options} {selection}: {diagnostics:#?}");
    }
}

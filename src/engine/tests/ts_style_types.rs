use std::path::Path;

use super::native_linter;

#[test]
fn explicit_any_hook_selects_rest_array_context_without_hiding_other_annotations() {
    let source = "type Ordinary = any; function arrays(...args: any[]) {} type Callback = (...args: ReadonlyArray<any>) => void; function immutable(...args: readonly any[]) {} function bare(...args: any) {} function nested(...args: any[][]) {} function tuple(...args: [any]) {} let value: Array<any>; let marker: number; function unrelated(...args: Foo<any>) {} function genericNested(...args: Array<Array<any>>) {}";
    for (predicate, expected) in [
        ("true", 10),
        ("parent.rest_array_argument != true", 7),
        ("parent not ts_type_alias", 9),
        ("false", 0),
    ] {
        let linter = native_linter(&format!("override in ts_type_keyword let no_explicit_any_checked {predicate}\nrule no-explicit-any"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), expected, "{predicate}: {diagnostics:?}");
        assert!(diagnostics.iter().all(|diagnostic| &source[diagnostic.start..diagnostic.end] == "any"));
    }
}

#[test]
fn namespace_hook_controls_files_and_declarations_without_enabling_intrinsic_exemptions() {
    let source = "namespace Root { namespace Nested {} } declare namespace Ambient {} declare global { interface Global {} } declare module 'external' {} namespace global {}";
    for (predicate, regular, definition) in [
        (None, 3, 0),
        (Some("true"), 3, 3),
        (Some("namespace_declared != true"), 2, 2),
        (Some("name == \"Root\""), 1, 1),
        (Some("false"), 0, 0),
    ] {
        let config = predicate.map_or_else(String::new, |predicate| format!("override in ts_namespace let no_namespace_checked {predicate}\n"));
        let linter = native_linter(&format!("{config}rule no-namespace"));
        for (file, expected) in [("file.ts", regular), ("file.d.ts", definition), ("file.d.cts", definition), ("file.d.mts", definition)] {
            let diagnostics = linter.lint(Path::new(file), source);
            assert_eq!(diagnostics.len(), expected, "{predicate:?} {file}: {diagnostics:?}");
        }
    }
}

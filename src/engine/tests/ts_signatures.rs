use std::path::Path;

use super::native_linter;
use crate::config::ast::Severity;

#[test]
fn unused_overload_documentation_cannot_fail_signature_comparison() {
    let source = "export function f(x: string): void;\nexport function f(x: number): void;";
    let broken_doc = "override in ts_export let unified_doc byte_slice(text, [])\n";
    let ordinary = native_linter(&format!("{broken_doc}rule unified-signatures"));
    let diagnostics = ordinary.lint(Path::new("file.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|d| d.severity != Severity::Error), "{diagnostics:?}");

    let checked = native_linter(&format!("{broken_doc}rule unified-signatures {{ set ignoreOverloadsWithDifferentJSDoc true }}"));
    assert!(checked.lint(Path::new("file.ts"), source).iter().any(|d| d.severity == Severity::Error));
    assert!(checked.lint(Path::new("file.ts"), "export function f(x: string): void;").is_empty());
    assert!(checked.lint(Path::new("file.ts"), "export function f(x: string): string;\nexport function f(x: number): number;").is_empty());
}

#[test]
fn overload_name_and_documentation_filters_are_independent() {
    for (source, names_match, docs_match) in [
        ("/** first */\nexport function f(x: string): void;\n/** second */\nexport function f(y: number): void;", false, false),
        ("/** same */\nexport function f(x: string): void;\n/** same */\nexport function f(x: number): void;", true, true),
        ("interface I { /** same */ f(x: string): void; /** same */ f(y: number): void; }", false, true),
        ("namespace N { /** first */ export function f(x: string): void; /** second */ export function f(x: number): void; }", true, false),
    ] {
        for names in [false, true] {
            for docs in [false, true] {
                let linter = native_linter(&format!("rule unified-signatures {{\nset ignoreDifferentlyNamedParameters {names}\nset ignoreOverloadsWithDifferentJSDoc {docs}\n}}"));
                let diagnostics = linter.lint(Path::new("file.ts"), source);
                let expected = usize::from((!names || names_match) && (!docs || docs_match));
                assert_eq!(diagnostics.len(), expected, "{names}/{docs}: {source}\n{diagnostics:?}");
                assert!(diagnostics.iter().all(|d| d.severity != Severity::Error), "{diagnostics:?}");
            }
        }
    }
}

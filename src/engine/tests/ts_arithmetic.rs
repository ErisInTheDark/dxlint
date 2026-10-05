use std::path::Path;

use super::native_linter;

#[test]
fn addition_reports_invalid_operands_and_incompatible_pairs_at_their_owner() {
    let lint = native_linter("rule restrict-plus-operands");
    for (source, spans) in [
        ("declare const x: unknown; x + 1;", vec!["x"]),
        ("declare const x: {}; x + 1;", vec!["x"]),
        ("interface RegExp {} declare const regexp: RegExp; regexp + 1;", vec!["regexp"]),
        ("declare let x: unknown; x += 1;", vec!["x"]),
        ("1n + 1;", vec!["1n + 1"]),
        ("'s' + true;", vec![]),
        ("'s' + 1;", vec![]),
        ("function f<T extends string>(x: T){ return x + 's'; }", vec![]),
        ("declare const x: Missing; x + 1;", vec![]),
    ] {
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{source}: {diagnostics:#?}");
    }
}

#[test]
fn addition_options_change_only_their_operand_or_assignment_category() {
    for (settings, source, spans) in [
        ("set allowAny false", "declare const x: any; x + 1;", vec!["x"]),
        ("set allowBoolean false", "'s' + true;", vec!["true"]),
        ("set allowNullish false", "'s' + null;", vec!["null"]),
        ("set allowNullish false", "declare const x: number | undefined; 's' + x;", vec!["x"]),
        ("set allowRegExp false", "interface RegExp {} declare const regexp: RegExp; 's' + regexp;", vec!["regexp"]),
        ("set allowNumberAndString false", "'s' + 1;", vec!["'s' + 1"]),
        ("set skipCompoundAssignments true", "declare let x: unknown; x += 1;", vec![]),
        ("set skipCompoundAssignments true", "declare let x: unknown; x + 1;", vec!["x"]),
        ("set allowBoolean false", "'s' + 1;", vec![]),
        ("set allowRegExp false", "'s' + true;", vec![]),
        ("set allowAny false", "'s' + true;", vec![]),
        ("set allowNullish false", "interface RegExp {} declare const regexp: RegExp; 's' + regexp;", vec![]),
    ] {
        let lint = native_linter(&format!("rule restrict-plus-operands {{ {settings} }}"));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{settings}: {source}: {diagnostics:#?}");
    }
}

#[test]
fn addition_prioritises_individual_errors_and_preserves_compound_type_structure() {
    for (settings, source, spans) in [
        ("set allowBoolean false", "true + false;", vec!["true", "false"]),
        ("set allowBoolean false\nset allowNumberAndString false", "declare const x: boolean | number; x + 's';", vec!["x"]),
        ("", "declare const x: {} & string; x + 's';", vec![]),
        ("", "declare const x: {} | number; x + 's';", vec!["x"]),
        ("", "declare const x: {} & { a: number }; x + 's';", vec!["x"]),
    ] {
        let lint = native_linter(&format!("rule restrict-plus-operands {{ {settings} }}"));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{source}: {diagnostics:#?}");
    }
}

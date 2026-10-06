use std::path::Path;

use super::{lint_project, native_linter};

#[test]
fn duplicate_type_selection_covers_optional_parameters_and_both_type_shapes() {
    let source = "type U = string | string; type I = number & number; function f(value?: number | undefined) {}";
    for (selection, expected) in [
        ("", vec!["number", "string", "undefined"]),
        ("drop group duplicate_type_constituent ts_union", vec!["number"]),
        ("drop group duplicate_type_constituent ts_intersection", vec!["string", "undefined"]),
        ("override in duplicate_type_constituent let duplicate_type_constituents_checked !(is ts_union)", vec!["number"]),
        ("drop group duplicate_type_constituent ts_union, ts_intersection", vec![]),
    ] {
        let linter = native_linter(&format!("rule no-duplicate-type-constituents\n{selection}"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        let mut reported: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        reported.sort_unstable();
        assert_eq!(reported, expected, "{selection}: {diagnostics:#?}");
    }
}

#[test]
fn type_allowance_selection_uses_each_rules_subject_and_origin() {
    let declarations = "class Allowed { a = 1; } class Other { b = 1; } declare const allowed: Allowed, other: Other;";
    for (rule, target, hook, groups, statements) in [
        ("only-throw-error", "ts_node", "only_throw_error_checked", "type_specifier_groups", "throw allowed; throw other;"),
        ("prefer-promise-reject-errors", "ts_call", "prefer_promise_reject_errors_checked", "first([] argument.rejection_reason).groups", "Promise.reject(allowed); Promise.reject(other);"),
        ("no-misused-spread", "ts_spread, ts_jsx_spread_attribute", "no_misused_spread_checked", "node.argument.type_specifier_groups", "({...allowed}); ({...other});"),
    ] {
        let source = format!("{declarations} {statements}");
        for (options, selection, expected) in [
            ("", String::new(), 2),
            ("{ set allow ['Allowed'] }", String::new(), 1),
            ("", format!("override in {target} let {hook} true"), 2),
            ("", format!("override in {target} let {hook} false"), 0),
            ("", format!("override in {target} let {hook} !all({groups}, group => any(group, gate => gate.name == 'Allowed' && gate.from == 'file' && ends_with(gate.path, 'main.ts')))"), 1),
            ("", format!("override in {target} let {hook} !all({groups}, group => any(group, gate => gate.name == 'Allowed' && gate.from == 'lib'))"), 2),
        ] {
            let messages = lint_project(
                "type-allowance-selection",
                &[
                    ("tsconfig.json", r#"{"compilerOptions":{"lib":["es5"]}}"#),
                    ("node_modules/typescript/lib/lib.es5.d.ts", "interface PromiseConstructor { reject(reason?: any): unknown; } declare const Promise: PromiseConstructor;"),
                    ("main.ts", &source),
                ],
                "main.ts",
                &format!("rule {rule} {options}\n{selection}"),
            );
            assert_eq!(messages.len(), expected, "{rule} {options} {selection}: {messages:#?}");
        }
    }
}

#[test]
fn allowed_names_do_not_combine_across_union_arms_but_origin_predicates_can() {
    let source = "class Allowed { a = 1; } class Other { b = 1; } declare const union: Allowed | Other; declare const intersection: Allowed & Other; throw union; throw intersection;";
    for (options, selection, expected) in [
        ("", "", 2),
        ("{ set allow ['Allowed', 'Other'] }", "", 1),
        ("{ set allow ['Allowed'] }", "", 1),
        ("", "override in ts_node let only_throw_error_checked !all(type_specifier_groups, group => any(group, gate => contains(['Allowed', 'Other'], gate.name) && gate.from == 'file'))", 0),
    ] {
        let linter = native_linter(&format!("rule only-throw-error {options}\n{selection}"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), expected, "{options} {selection}: {diagnostics:#?}");
    }
}

#[test]
fn template_selection_preserves_other_union_clauses_and_primitive_options() {
    let source = "interface Custom { value: number; } interface URL { value: number; } declare const custom: Custom; declare const mixed: Custom | boolean; declare const shadow: URL; `${custom}`; `${mixed}`; `${shadow}`; tag`${custom}`;";
    let selected = "override in ts_node let restrict_template_expressions_type_checks filter(template_type_checks, clause => !any(clause, gate => gate.name == 'Custom' && gate.from == 'file'))";
    for (options, selection, expected) in [
        ("", "", 3),
        ("{ set allow null }", "", 3),
        ("{ set allow [] }", "", 3),
        ("{ set allow ['Custom'] }", "", 1),
        ("{ set allow ['URL'] }", "", 2),
        ("", selected, 1),
        ("{ set allowBoolean false }", selected, 2),
        ("", "override in ts_node let restrict_template_expressions_type_checks []", 0),
    ] {
        let linter = native_linter(&format!("rule restrict-template-expressions {options}\n{selection}"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), expected, "{options} {selection}: {diagnostics:#?}");
    }
}

#[test]
fn template_default_library_allowances_are_replaced_not_extended() {
    for (options, expected) in [
        ("", 1),
        ("{ set allow null }", 1),
        ("{ set allow [] }", 2),
        ("{ set allow ['Custom'] }", 1),
        ("{ set allow ['URL'] }", 1),
    ] {
        let messages = lint_project(
            "template-library-allowances",
            &[
                ("lib.example.d.ts", "export interface URL { value: number; }"),
                ("main.ts", "import { URL } from './lib.example'; interface Custom { value: number; } declare const url: URL; declare const custom: Custom; `${url}`; `${custom}`;"),
            ],
            "main.ts",
            &format!("rule restrict-template-expressions {options}"),
        );
        assert_eq!(messages.len(), expected, "{options}: {messages:#?}");
    }
}

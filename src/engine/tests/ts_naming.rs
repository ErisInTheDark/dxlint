//! Naming policy selection and shared value-only checks.

use std::path::{Path, PathBuf};

use super::native_linter;

#[test]
fn naming_contexts_own_each_name_once() {
    let l = native_linter("rule naming-convention");
    let source = r#"
        const validName = {
            bad_property: 1,
            bad_method() {},
            goodProperty: 1,
        };
        class bad_class {
            bad_property = 1;
            bad_method(BadParameter: number) {}
        }
        enum GoodEnum { bad_member }
        function BadFunction() {}
    "#;
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    let mut names: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "BadFunction", "BadParameter", "bad_class", "bad_member",
            "bad_method", "bad_method", "bad_property", "bad_property",
        ],
        "{diagnostics:#?}",
    );
}

#[test]
fn naming_context_modes_and_checked_hook_are_independent() {
    let l = native_linter(r#"
        rule naming-convention:variable { set formats ["UPPER_CASE"] }
        rule naming-convention:class { set formats ["camelCase"] }
        override in ts_identifier let naming_convention_checked !exported
    "#);
    let source = "export const ignoredName = 1; const BAD_NAME = 2; const badName = 3; class goodName {}";
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(&source[diagnostics[0].start..diagnostics[0].end], "badName");
}

#[test]
fn checked_hook_selects_binding_contexts_without_disabling_members() {
    let l = native_linter(r#"
        rule naming-convention
        override in ts_identifier let naming_convention_checked naming_context != "variable"
    "#);
    let source = "const bad_name = 1; function BadFunction() {} const object = { bad_property: 1 };";
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    let mut names: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    names.sort_unstable();
    assert_eq!(names, ["BadFunction", "bad_property"], "{diagnostics:#?}");
}

#[test]
fn custom_naming_regexes_validate_before_linting() {
    let source = r#"import .ts
        rule naming-convention:variable { set customRegex "(" }
    "#;
    let built = crate::engine::build(
        crate::config::load_text("test.zlint", source, &crate::config::source::Disk),
        PathBuf::from("/root"),
    );
    assert!(built.errors.iter().any(|error| error.message.contains("regex")), "{:#?}", built.errors);
}

#[test]
fn an_empty_custom_naming_regex_is_distinct_from_no_pattern() {
    let source = "const ordinaryName = 1;";
    for (pattern, matching, expected) in [("null", false, 0), (r#""""#, true, 0), (r#""""#, false, 1)] {
        let l = native_linter(&format!(r#"
            rule naming-convention:variable {{
                set formats []
                set customRegex {pattern}
                set customMatch {matching}
            }}
        "#));
        let diagnostics = l.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), expected, "{pattern}/{matching}: {diagnostics:#?}");
    }
}

#[test]
fn function_valued_properties_use_method_policy_and_computed_keys_stay_exempt() {
    let l = native_linter(r#"
        rule naming-convention:class-method { set formats ["PascalCase"] }
        rule naming-convention:class-property { set formats ["UPPER_CASE"] }
        rule naming-convention:type-method { set formats ["PascalCase"] }
        rule naming-convention:type-property { set formats ["UPPER_CASE"] }
    "#);
    let source = r#"
        class Example {
            GoodMethod = () => 1;
            GOOD_PROPERTY = 1;
            ["computed_key"] = 1;
            badMethod = () => 1;
        }
        interface ExampleType {
            GoodMethod: () => number;
            GOOD_PROPERTY: number;
            badMethod: () => number;
        }
    "#;
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    assert!(diagnostics.iter().all(|d| &source[d.start..d.end] == "badMethod"));
}

#[test]
fn shared_naming_checks_trim_before_format_and_accept_alternatives() {
    for (name, formats, prefix, expected) in [
        ("IS_READY", r#"["UPPER_CASE"]"#, r#"["IS_"]"#, false),
        ("IS_ready", r#"["UPPER_CASE"]"#, r#"["IS_"]"#, true),
        ("IS_ready", r#"["camelCase", "UPPER_CASE"]"#, r#"["IS_"]"#, false),
        ("__IS_READY", r#"["UPPER_CASE"]"#, r#"["IS_"]"#, true),
        ("IS_ready", r#"["camelCase"]"#, r#"["I", "IS_"]"#, true),
        ("IS_ready", r#"["camelCase"]"#, r#"["IS_", "I"]"#, false),
    ] {
        let l = native_linter(&format!(r#"
            in ts_program rule custom-naming {{
                let failure naming_failure("{name}", false, {{
                    formats: {formats}, leadingUnderscore: "allow",
                    trailingUnderscore: "allow", prefix: {prefix}
                }})
                when failure != null
                warn naming_message("{name}", failure)
            }}
        "#));
        let diagnostics = l.lint(Path::new("/root/a.ts"), "");
        assert_eq!(!diagnostics.is_empty(), expected, "{name}: {diagnostics:#?}");
        assert!(diagnostics.len() <= 1, "{diagnostics:#?}");
    }
}

#[test]
fn destructured_names_are_checked_without_checking_renamed_source_keys() {
    let l = native_linter("rule naming-convention");
    let source = "const { old_name: goodName, plain_name, nested: { bad_name } } = source;";
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    let mut names: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    names.sort_unstable();
    assert_eq!(names, ["bad_name", "plain_name"], "{diagnostics:#?}");
}

#[test]
fn member_names_remove_private_markers_but_not_required_quotes() {
    let l = native_linter("rule naming-convention");
    let source = r#"class Example { #goodName = 1; #bad_name = 2; }
        const example = { "goodName": 1, "bad-key": 2 };"#;
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    assert!(diagnostics.iter().any(|d| &source[d.start..d.end] == "#bad_name"));
    assert!(diagnostics.iter().any(|d| &source[d.start..d.end] == r#""bad-key""#));
}

#[test]
fn member_names_preserve_empty_static_keys_across_contexts() {
    let l = native_linter(r#"
        in naming_name_node rule empty-member-names {
            when naming_context != null && naming_name == "" && naming_requires_quotes
            warn "empty member"
        }
    "#);
    let source = "const example = { '': 1 }; class Example { '' = 1; } interface ExampleType { '': number; } enum ExampleEnum { '' = 1 }";
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    assert_eq!(diagnostics.len(), 4, "{diagnostics:#?}");
    assert!(diagnostics.iter().all(|d| &source[d.start..d.end] == "''"), "{diagnostics:#?}");
}

#[test]
fn enabled_hook_preserves_intrinsic_context_and_merged_declarations() {
    let l = native_linter(r#"
        rule naming-convention
        override in ts_identifier let naming_convention_checked true
    "#);
    let source = r#"
        import bad_default from "module";
        import * as bad_namespace from "module";
        import { bad_named, source_name as bad_alias, default as bad_specifier } from "module";
        interface bad_merge {}
        class bad_merge {
            constructor() {}
            ["bad_computed"] = bad_default;
        }
        bad_named; bad_alias; bad_namespace; bad_specifier;
    "#;
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    let mut names: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["bad_default", "bad_merge", "bad_merge", "bad_namespace", "bad_specifier"],
        "{diagnostics:#?}",
    );
}

#[test]
fn custom_typed_naming_policy_requires_homogeneous_non_nullish_parts() {
    let l = native_linter(r#"
        fn boolean_domain(d) = d.kind == "boolean" || d.kind == "union"
            && count(filter(d.parts, p => !contains(["null", "undefined"], p.kind))) > 0
            && all(filter(d.parts, p => !contains(["null", "undefined"], p.kind)), p => boolean_domain(p))
        in ts_identifier rule boolean-names {
            when naming_context == "variable" && boolean_domain(type_domains)
            let failure naming_failure(naming_name, false, { formats: ["UPPER_CASE"], prefix: ["IS_"] })
            when failure != null
            warn naming_message(naming_name, failure)
        }
    "#);
    let source = "declare const IS_READY: boolean | null; declare const ready: boolean | undefined; declare const mixed: boolean | string; declare const absent: null;";
    let diagnostics = l.lint(Path::new("/root/a.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(&source[diagnostics[0].start..diagnostics[0].end], "ready");
}

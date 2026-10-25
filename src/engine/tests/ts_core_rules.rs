#[test]
fn restricted_pattern_options_reject_invalid_javascript_syntax_before_linting() {
    for config in [
        r#"rule es-no-restricted-exports { set restrictedNamedExportsPattern "(?i)foo" }"#,
        r#"rule es-no-restricted-imports { set patterns [{regex: "(?i)foo"}] }"#,
        r#"rule es-no-restricted-imports { set patterns [{group: ["pkg"], importNamePattern: "(?i)foo"}] }"#,
        r#"rule es-no-restricted-imports { set patterns [{group: ["pkg"], allowImportNamePattern: "(?i)foo"}] }"#,
    ] {
        let errors = super::errors(&format!("import .ts\n{config}"));
        assert!(!errors.is_empty(), "{config}");
    }
}

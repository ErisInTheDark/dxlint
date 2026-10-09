use std::path::PathBuf;

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

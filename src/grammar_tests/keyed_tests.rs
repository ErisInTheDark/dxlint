use dx_config::source::Disk;

#[test]
fn keyed_operands_bind_targets_without_losing_the_declaring_items_fields() {
    let source = r#"
        lang t "*.t"
        in .t match holder key:/\w+/ {
            scope block
            declares value key keyed key.text it.text in nearest block
            resolves value keyed it.text
        }
    "#;
    let built = crate::engine::build(
        crate::config::load_text("keyed.dx", source, &Disk),
        std::path::PathBuf::from("/root"),
    );
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    assert!(built.linter.is_some());
}

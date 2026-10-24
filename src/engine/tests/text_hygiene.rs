use std::path::PathBuf;

use super::{config, Disk};

#[test]
fn standalone_text_languages_build_without_list_adapters() {
    for language in ["css", "markdown"] {
        let source = format!("import .{language}\nrule *\n");
        let built = super::super::build::build_for_lint(config::load_text("test.dx", &source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
    }
}

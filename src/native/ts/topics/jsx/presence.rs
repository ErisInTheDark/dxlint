//! Brace-presence policy regressions for the std rules.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn raw_jsx_text_separates_expression_containers() {
        let l = native_linter("rule jsx-curly-brace-presence");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box>\n{'a'}\n{'b'}\n</Box>;").len(), 2);
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box>{'a'}{'b'}</Box>;").is_empty());
    }
}

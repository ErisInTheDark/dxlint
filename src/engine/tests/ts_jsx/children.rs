//! Child-layout policy regressions for the std rules.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn spread_children_do_not_enable_multiline_expression_exemptions() {
        let l = native_linter("rule jsx-newline {\nset prevent true\nset allowMultilines true\n}");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box><Item />\n\n{\n...items\n}</Box>;").len(), 1);
    }

    #[test]
    fn a_child_sharing_both_tag_lines_is_reported_once() {
        let l = native_linter("rule jsx-one-expression-per-line");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box><Child>\ntext\n</Child></Box>;").len(), 1);
    }

    #[test]
    fn javascript_whitespace_does_not_form_a_child_expression() {
        let l = native_linter("rule jsx-one-expression-per-line");
        let problems = l.lint(Path::new("/root/file.tsx"), "const x = <Box>\n\u{feff}<Child />\n</Box>;");
        assert!(problems.is_empty(), "{problems:?}");
    }
}

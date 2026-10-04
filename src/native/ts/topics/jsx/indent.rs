//! Prop-indentation policy regressions for the std rules.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn ternary_continuation_indentation_stays_with_its_opening_tag() {
        let text = "const x = flag\n  ? <Box\n      a\n      b\n    />\n  : <Box\n      c\n      d\n    />;\nconst y = <Box\n  e\n  f\n/>;";
        let l = native_linter("rule jsx-indent-props { set mode 2 }");
        assert!(l.lint(Path::new("/root/file.tsx"), text).is_empty());
        let ignored = native_linter("rule jsx-indent-props {\nset mode 2\nset ignoreTernaryOperator true\n}");
        assert_eq!(ignored.lint(Path::new("/root/file.tsx"), text).len(), 4);
    }

    #[test]
    fn attribute_line_breaks_do_not_reset_ternary_indentation() {
        let l = native_linter("rule jsx-indent-props { set mode 2 }");
        for separator in ["\r", "\u{2028}", "\u{2029}"] {
            let text = format!("const x = flag\n  ? <Box\n      a=\"before{separator}<after\"\n      b\n    />\n  : null;");
            assert!(l.lint(Path::new("/root/file.tsx"), &text).is_empty(), "{separator:?}");
        }
    }
}

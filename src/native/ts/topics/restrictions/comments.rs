//! Comment and line-limit policy regressions for the std rules.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn jsx_comments_are_exempt_only_on_otherwise_empty_lines() {
        let l = native_linter("rule no-inline-comments");
        assert!(l.lint(Path::new("/root/file.tsx"), "const view = <div>\n{/* comment */}\n</div>").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const view = <div>{/* comment */}</div>").len(), 1);
    }

    #[test]
    fn blank_lines_inside_block_comments_are_comment_lines() {
        let l = native_linter("rule max-lines { set max 1\nset skipComments true\n}");
        assert!(l.lint(Path::new("/root/file.ts"), "run();\n/* comment\n\nend */").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "run();\n\n/* comment\n\nend */").len(), 1);
    }

    #[test]
    fn line_limits_count_every_javascript_line_separator() {
        let l = native_linter("rule max-lines { set max 2 }");
        for separator in ["\r", "\u{2028}", "\u{2029}"] {
            let text = format!("one();{separator}two();{separator}three();");
            let diags = l.lint(Path::new("/root/file.ts"), &text);
            assert_eq!(diags.len(), 1, "{separator:?}");
            assert_eq!(diags[0].start, text.find("three").unwrap());
        }
    }

    #[test]
    fn an_empty_file_counts_as_one_blank_line() {
        let l = native_linter("rule max-lines { set max 0 }");
        let diags = l.lint(Path::new("/root/file.ts"), "");
        assert_eq!(diags.len(), 1);
        assert_eq!((diags[0].start, diags[0].end), (0, 0));
        let skipped = native_linter("rule max-lines { set max 0\nset skipBlankLines true\n}");
        assert!(skipped.lint(Path::new("/root/file.ts"), "").is_empty());
    }

    #[test]
    fn warning_terms_preserve_multiplicity_and_the_comment_range() {
        let l = native_linter("rule no-warning-comments {\nset terms ['todo', 'fixme', 'todo']\nset location 'anywhere'\n}");
        let text = "const text = '/* TODO FIXME */'; // TODO FIXME\nconst done = true;";
        let start = text.find("//").unwrap();
        let end = text.find('\n').unwrap();
        let diags = l.lint(Path::new("/root/file.ts"), text);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().all(|diag| diag.start == start && diag.end == end));
    }
}

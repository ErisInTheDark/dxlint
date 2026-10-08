//! Tag-spacing policy regressions for the std rules.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn tag_spacing_uses_tokens_not_multiline_attribute_extents() {
        let l = native_linter("rule jsx-tag-spacing {\nset closingSlash 'allow'\nset beforeSelfClosing 'allow'\nset afterOpening 'allow'\nset beforeClosing 'always'\n}");
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a=\"a\nb\"></Box >;").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a={foo\n}></Box >;").len(), 1);
    }

    #[test]
    fn slash_spacing_checks_report_their_independent_gaps() {
        let l = native_linter("rule jsx-tag-spacing {\nset closingSlash 'never'\nset beforeSelfClosing 'never'\nset afterOpening 'allow'\nset beforeClosing 'allow'\n}");
        let text = "const x = <Outer><Box / ></Outer>;";
        let slash = text.find("/ >").unwrap();
        let diags = l.lint(Path::new("/root/file.tsx"), text);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().any(|diag| (diag.start, diag.end) == (slash - 1, slash)));
        assert!(diags.iter().any(|diag| (diag.start, diag.end) == (slash + 1, slash + 2)));
    }

    #[test]
    fn layout_diagnostics_do_not_include_unrelated_children() {
        let l = native_linter("rule jsx-indent-props { set mode 2 }\nrule jsx-closing-bracket-location\nrule jsx-closing-tag-location");
        let text = "const x = <Outer><Box\n a\n  >\n </Box>\n          </Outer>;";
        let diags = l.lint(Path::new("/root/file.tsx"), text);
        let end = text.find("</Box>").unwrap() + "</Box>".len();
        assert_eq!(diags.len(), 3);
        let header_end = text.find("\n  >").unwrap() + "\n  >".len();
        let closing_start = text.find("</Box>").unwrap();
        assert!(diags.iter().all(|diag| diag.start < closing_start && diag.end <= header_end
            || diag.start == closing_start && diag.end <= end));
        for start in [text.find("\n a").unwrap() + 2, text.find("\n  >").unwrap() + 3, text.find("</Box>").unwrap()] {
            assert!(diags.iter().any(|diag| diag.start == start));
        }
    }

    #[test]
    fn tag_spacing_diagnostics_stay_at_the_gap() {
        let l = native_linter("rule jsx-tag-spacing { set afterOpening 'always' }");
        let source = "const x = <Box>unrelated child text</Box>;";
        let opening_end = source.find('>').unwrap() + 1;
        let diagnostics = l.lint(Path::new("/root/file.tsx"), source);
        let opening: Vec<_> = diagnostics.iter().filter(|diagnostic| diagnostic.start < opening_end).collect();
        assert!(!opening.is_empty());
        assert!(opening.into_iter()
            .all(|diagnostic| diagnostic.end <= opening_end), "{diagnostics:#?}");
    }
}

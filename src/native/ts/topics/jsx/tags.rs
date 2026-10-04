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
    fn slash_spacing_checks_keep_independent_decisions_and_element_ranges() {
        let l = native_linter("rule jsx-tag-spacing {\nset closingSlash 'never'\nset beforeSelfClosing 'never'\nset afterOpening 'allow'\nset beforeClosing 'allow'\n}");
        let text = "const x = <Outer><Box / ></Outer>;";
        let slash = text.find("/ >").unwrap();
        let diags = l.lint(Path::new("/root/file.tsx"), text);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().all(|diag| diag.start == slash && diag.end == slash + "/ >".len()));
    }

    #[test]
    fn layout_decisions_keep_their_enclosing_element_range() {
        let l = native_linter("rule jsx-indent-props { set mode 2 }\nrule jsx-closing-bracket-location\nrule jsx-closing-tag-location");
        let text = "const x = <Outer><Box\n a\n  >\n </Box>\n          </Outer>;";
        let diags = l.lint(Path::new("/root/file.tsx"), text);
        let end = text.find("</Box>").unwrap() + "</Box>".len();
        assert_eq!(diags.len(), 3);
        assert!(diags.iter().all(|diag| diag.end == end));
        for start in [text.find("\n a").unwrap() + 2, text.find("\n  >").unwrap() + 3, text.find("</Box>").unwrap()] {
            assert!(diags.iter().any(|diag| diag.start == start));
        }
    }
}

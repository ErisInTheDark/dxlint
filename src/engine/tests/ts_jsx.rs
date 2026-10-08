//! JSX rules over native syntax and ordinary project overrides.

#[cfg(test)]
mod tags;
#[cfg(test)]
mod children;
#[cfg(test)]
mod presence;
#[cfg(test)]
mod indent;

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn context_spacing_object_replaces_global_spacing() {
        let l = native_linter("rule jsx-curly-spacing { set options [{when: 'always', spacing: {objectLiterals: 'never'}, attributes: {spacing: {}}}] }");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a={{x: 1}} />;").len(), 2);
    }

    #[test]
    fn spread_children_are_not_expression_containers() {
        let l = native_linter("rule jsx-curly-spacing { set options [{children: true}] }");
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box>{ ...items }</Box>;").is_empty());
    }

    #[test]
    fn conditional_policy_takes_ownership_from_the_declaration_fallback() {
        let text = "const x = flag ? (<Box>\ntext\n</Box>) : null;";
        let declaration = native_linter("rule jsx-wrap-multilines { set declaration 'parens-new-line' }");
        assert_eq!(declaration.lint(Path::new("/root/file.tsx"), text).len(), 1);
        let condition = native_linter("rule jsx-wrap-multilines {\nset declaration 'parens-new-line'\nset condition 'parens'\n}");
        assert!(condition.lint(Path::new("/root/file.tsx"), text).is_empty());
    }

    #[test]
    fn prop_limits_use_opening_tag_and_consecutive_attribute_lines() {
        let l = native_linter("rule jsx-max-props-per-line:singleline set 2\nrule jsx-max-props-per-line:multiline set 1");
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a b><Child\n /></Box>;").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a={(\nfoo\n)} b c />;").len(), 1);
    }

    #[test]
    fn jsx_quotes_decode_entities_but_ignore_expression_strings() {
        let l = native_linter("rule jsx-quotes { set mode 'prefer-single' }");
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a=\"&#39;\" b={\"text\"} />;").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a=\"text\" />;").len(), 1);
    }

    #[test]
    fn spaced_closing_slash_still_has_an_explicit_closing_tag() {
        let l = native_linter("rule jsx-self-closing-comp");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box>< /Box>;").len(), 1);
    }
}

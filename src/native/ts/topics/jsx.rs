//! JSX layout and shape (`std/ts/jsx`): @stylistic's jsx-* rules.

use crate::expr::facts::Fact;

mod metadata;
mod tags;
#[cfg(test)]
mod children;
mod presence;
mod indent;

pub const FACTS: &[Fact] = &[
    Fact { name: "jsx_syntax", doc: "native JSX elements/fragments: opening and closing tag spans {start, end}, and self_closing", get: |f, n| f.with_file(metadata::syntax, n) },
    Fact { name: "jsx_brace_presence", doc: "native JSX expressions/text/attribute values: brace policy context, removable expressions and wrap-worthy literals", get: |f, n| f.with_file(presence::policy, n) },
    Fact { name: "jsx_prop_indent", doc: "native JSX elements: opening/first-prop indentation and props with ternary continuation state", get: |f, n| f.with_file(indent::props, n) },
    Fact { name: "jsx_tag_layout", doc: "native JSX elements: opening/closing bracket locations, alignment and trailing comments", get: |f, n| f.with_file(tags::layout, n) },
    Fact { name: "jsx_closing_tag", doc: "native JSX elements/fragments: closing-tag offset, line isolation and tag/line alignment columns", get: |f, n| f.with_file(tags::closing, n) },
];

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
        let l = native_linter("rule jsx-max-props-per-line { set maximum {single: 2, multi: 1} }");
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

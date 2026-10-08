//! Prop-indentation policy regressions for the std rules.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn fragment_closing_comments_keep_one_level_without_moving_tag_tokens() {
        let l = native_linter("rule indent { set mode 4 }");
        for comment in ["// Comment\n", "/* Comment */ ", "/* Comment */\n"] {
            let source = format!("<>\n    <A />\n<\n    {comment}/>");
            let diagnostics = l.lint(Path::new("/root/file.tsx"), &source);
            assert!(diagnostics.is_empty(), "{source}: {diagnostics:#?}");
            let wrong = source.replacen("\n    /", "\n        /", 1);
            let diagnostics = l.lint(Path::new("/root/file.tsx"), &wrong);
            assert!(diagnostics.iter().any(|diagnostic| diagnostic.rule == "indent"), "{wrong}: {diagnostics:#?}");
            assert_eq!(l.fix(Path::new("/root/file.tsx"), &wrong).text, source);
        }
    }

    #[test]
    fn opening_style_does_not_change_child_indentation() {
        let l = native_linter("rule indent {\nset mode 2\nindent in ts_jsx_opening by 3\n}");
        let source = "<Box\n      prop='x'\n>\n  Text\n</Box>";
        let diagnostics = l.lint(Path::new("/root/file.tsx"), source);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let wrong = "<Box\n  prop='x'\n>\n  Text\n</Box>";
        let diagnostics = l.lint(Path::new("/root/file.tsx"), wrong);
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.rule == "indent"), "{diagnostics:#?}");
        assert_eq!(l.fix(Path::new("/root/file.tsx"), wrong).text, source);
    }

    #[test]
    fn opening_reset_leaves_children_and_attribute_expressions_checked() {
        let l = native_linter("override in ts_jsx_opening let indent_reset true\nrule indent { set mode 4 }");
        for source in [
            "<Foo\n            bar=\"1\" />",
            "<Foo\n            bar={() => {\n                return value;\n            }} />",
            "<foo\n    prop='bar'\n    >\n    Text\n</foo>",
        ] {
            assert!(l.lint(Path::new("/root/file.tsx"), source).is_empty(), "{source}");
        }
        for source in [
            "<foo\n    prop='bar'\n    >\nText\n</foo>",
            "<foo\n    prop='bar'\n    >\n    Text\n    </foo>",
            "<Foo\n            bar={() => {\nreturn value;\n            }} />",
        ] {
            let diagnostics = l.lint(Path::new("/root/file.tsx"), source);
            assert!(diagnostics.iter().any(|diagnostic| diagnostic.rule == "indent"), "opening reset must not exempt nested owners: {source}: {diagnostics:#?}");
        }
        let scoped = native_linter("override in ts_jsx_opening let indent_reset name == 'foo' || name == 'Foo'\nrule indent { set mode 4 }");
        let aligned = "<foo\n    prop='bar'\n    >\n    <Child />\n</foo>";
        assert!(scoped.lint(Path::new("/root/file.tsx"), aligned).is_empty());
        let wrong = "<foo\n    prop='bar'\n    >\n<Child />\n</foo>";
        let diagnostics = scoped.lint(Path::new("/root/file.tsx"), wrong);
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.rule == "indent"), "an unselected child header keeps its own indentation owner: {diagnostics:#?}");
    }

    #[test]
    fn ternary_continuation_indentation_stays_with_its_opening_tag() {
        let text = "const x = flag\n  ? <Box\n      a\n      b\n    />\n  : <Box\n      c\n      d\n    />;\nconst y = <Box\n  e\n  f\n/>;";
        let l = native_linter("rule jsx-indent-props { set mode 2 }");
        assert!(l.lint(Path::new("/root/file.tsx"), text).is_empty());
        let ignored = native_linter("override in ts_jsx_attribute, ts_jsx_spread_attribute let jsx_prop_indent_extra false\nrule jsx-indent-props { set mode 2 }");
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

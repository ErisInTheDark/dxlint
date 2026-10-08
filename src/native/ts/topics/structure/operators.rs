//! Operator precedence and constructor argument shape.

use crate::native::ts::{kinds::{Field as F, Kind as K}, syntax::Syntax};

pub(super) fn new_args(s: &Syntax, node: u32) -> bool {
    s.get(node, F::Callee).is_some_and(|callee| s.tree.node(callee).end < s.tree.node(node).end)
}

pub(super) fn precedence(s: &Syntax, node: u32) -> u8 {
    match s.kind(node) {
        Some(K::Sequence) => 0,
        Some(K::Assign | K::Arrow) => 1,
        Some(K::Yield) => 2,
        Some(K::Conditional | K::ConditionalType | K::FnType) => 3,
        Some(K::Union) => 4,
        Some(K::Intersection) => 5,
        Some(K::Binary | K::Logical) => match s.word(node, F::Op).unwrap_or("") {
            "||" | "??" => 4, "&&" => 5, "|" => 6, "^" => 7, "&" => 8,
            "==" | "!=" | "===" | "!==" => 9,
            "<" | ">" | "<=" | ">=" | "in" | "instanceof" => 10,
            "<<" | ">>" | ">>>" => 11, "+" | "-" => 12,
            "*" | "/" | "%" => 13, "**" => 14, _ => 21,
        },
        Some(K::Unary | K::As | K::Satisfies | K::TypeAssertion | K::TypeOperator) => 15,
        Some(K::Await) => 16,
        Some(K::Update) => 17,
        Some(K::New) if !new_args(s, node) => 18,
        Some(K::Call | K::New | K::TaggedTemplate | K::ImportCall) => 19,
        Some(K::Member | K::Index | K::NonNull | K::ArrayType) => 20,
        _ => 21,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::{apply_fixes, tests::native_linter};

    fn expression_shape(source: &str) -> String {
        fn shape(parsed: &crate::native::ts::Parsed, source: &str, id: u32) -> String {
            let node = &parsed.nodes[id as usize];
            if node.matcher == super::K::Paren as u32 || node.matcher == super::K::ParenType as u32 {
                return shape(parsed, source, node.children[0].node);
            }
            if node.children.is_empty() {
                return format!("{}:{:?}", node.matcher, &source[node.start as usize..node.end as usize]);
            }
            format!("{}:[{}]", node.matcher, node.children.iter().map(|edge| {
                format!("{}:{}", edge.label, shape(parsed, source, edge.node))
            }).collect::<Vec<_>>().join(","))
        }
        let parsed = crate::native::ts::parse(source, false).expect("valid original or fixed syntax");
        shape(&parsed, source, parsed.root)
    }

    #[test]
    fn redundant_parentheses_fixes_preserve_expression_shape_and_trivia() {
        let lint = native_linter("rule no-extra-parens");
        let path = Path::new("/root/a.ts");
        for source in [
            "(  0  );", "if ((0)) {}", "type A = (string);",
            "typeof(0);", "+(+foo);", "-(-foo);", "a/(/** keep */b);",
            "const a = (/* keep */ x /* also */);", "const a = (\r\n x\r\n);",
            "const a = ('\u{1f600}');", "const a = (x) + (y);",
            "-((bar+foo));", "((foo)\n)++;", "((a?.b)).c;", "((-2)) ** 3;",
        ] {
            let reports = lint.lint(path, source);
            assert!(!reports.is_empty(), "{source}: no redundant parentheses");
            assert!(reports.iter().all(|report| report.fix.is_some()), "{source}: {reports:?}");
            let (fixed, applied) = apply_fixes(source, &reports);
            assert!(applied > 0, "{source}");
            assert_eq!(expression_shape(source), expression_shape(&fixed), "{source} -> {fixed}");
            for comment in ["/* keep */", "/* also */", "\r\n"] {
                assert_eq!(source.matches(comment).count(), fixed.matches(comment).count(), "{source} -> {fixed}");
            }
            let final_fix = lint.fix(path, source);
            assert!(final_fix.remaining.is_empty(), "{source}: {:?}", final_fix.remaining);
            assert_eq!(expression_shape(source), expression_shape(&final_fix.text), "{source} -> {}", final_fix.text);
        }
    }

    #[test]
    fn uncertain_parenthesis_boundaries_retain_reports_without_fixes() {
        let lint = native_linter("rule no-extra-parens");
        let path = Path::new("/root/a.ts");
        for source in ["typeof(\u{03b1});", r"typeof(\u0061);"] {
            let reports = lint.lint(path, source);
            assert_eq!(reports.len(), 1, "{source}: {reports:?}");
            assert!(reports[0].fix.is_none(), "{source}: {reports:?}");
        }
        for source in [
            "a * (b + c);", "(a?.b).c;", "(-2) ** 3;", "a ?? (b || c);",
            "function f() { return (\n x); }", "(x) = function() {};",
            "const x = /** @type {number} */ (value);",
        ] {
            assert!(lint.lint(path, source).is_empty(), "{source}");
        }
    }

    #[test]
    fn parentheses_fixes_do_not_create_directives() {
        let lint = native_linter("rule no-extra-parens");
        let path = Path::new("/root/a.ts");
        for source in [
            "('use strict');", "function f() { ('abc'); }",
            "'use strict';(\"foobar\");", "foo(); ('bar');",
        ] {
            let reports = lint.lint(path, source);
            assert!(!reports.is_empty(), "{source}");
            assert!(reports.iter().all(|report| report.fix.is_none()), "{source}: {reports:?}");
            let (fixed, applied) = apply_fixes(source, &reports);
            assert_eq!(applied, 0, "{source}");
            assert_eq!(expression_shape(source), expression_shape(&fixed));
        }
        let source = "(('use strict'));";
        let reports = lint.lint(path, source);
        let (fixed, applied) = apply_fixes(source, &reports);
        assert_eq!(applied, 1);
        let remaining = lint.lint(path, &fixed);
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].fix.is_none());
    }

    #[test]
    fn parentheses_fixes_preserve_nested_jsdoc_casts() {
        let lint = native_linter("rule no-extra-parens");
        let path = Path::new("/root/a.ts");
        let source = "if ((/** @type {A} */ (/** @type {B} */ (/** @type {C} */(expr))))) {}";
        let reports = lint.lint(path, source);
        assert!(!reports.is_empty());
        assert!(reports.iter().all(|report| report.fix.is_none()), "{reports:?}");
        let (fixed, applied) = apply_fixes(source, &reports);
        assert_eq!(applied, 0);
        assert_eq!(expression_shape(source), expression_shape(&fixed));
    }

    #[test]
    fn initializer_linebreaks_respect_unicode_code_positions() {
        let lint = native_linter("rule operator-linebreak { set mode \"before\" }");
        for source in ["type \u{03b1} =\n  | A\n  | B;", "type A = \u{03b2};"] {
            let reports = lint.lint(Path::new("/root/a.ts"), source);
            assert!(reports.is_empty(), "{source}: {reports:?}");
        }
    }
}

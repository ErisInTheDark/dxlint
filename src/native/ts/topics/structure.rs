//! @stylistic code shape across lines (`std/ts/structure`): indentation,
//! parentheses, wrapping.

use crate::expr::{facts::Fact, Value};
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::Kind;

mod declarations;
mod operators;
mod lists;
mod indentation;

pub const FACTS: &[Fact] = &[
    Fact { name: "precedence", doc: "native expressions and types: binding strength, increasing from sequence to primary", get: |f, n| f.with_file(|f, n| Value::from(f64::from(operators::precedence(&f.syntax, n))), n) },
    Fact { name: "bom", doc: "native ts_program: source starts with a Unicode BOM", get: |f, n| f.with_file(bom, n) },
    Fact { name: "declaration_groups", doc: "native variable declarations: counts, previous statement and preceding groups in the owning scope", get: |f, n| f.with_file(declarations::groups, n) },
    Fact { name: "list_layout", doc: "native list owners: boundaries, item counts and spacing or wrapping pairs", get: |f, n| f.with_file(lists::sites, n) },
    Fact { name: "binary_operand_indents", doc: "native program: operand-line indentation constraints in traversal order", get: |f, n| f.with_file(indentation::operands, n) },
];

fn bom(file: &NativeFile, node: u32) -> Value<'static> {
    if file.syntax.is(node, Kind::Program) {
        Value::Bool(file.syntax.src.starts_with('\u{feff}'))
    } else {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    #[test]
    fn linebreak_reports_cover_only_the_native_line_terminator() {
        let lint = native_linter("rule linebreak-style");
        for separator in ["\r\n", "\u{2028}", "\u{2029}"] {
            let source = format!("const a = 1;{separator}const b = 2;{separator}");
            let reports = lint.lint(Path::new("/root/a.ts"), &source);
            let spans: Vec<_> = reports.iter().map(|report| (report.start, report.end)).collect();
            let expected: Vec<_> = source.match_indices(separator).map(|(start, text)| (start, start + text.len())).collect();
            assert_eq!(spans, expected, "{separator:?}");
        }
    }

    #[test]
    fn line_lengths_restart_at_every_javascript_line_boundary() {
        let lint = native_linter("rule max-len { set options [12] }");
        for separator in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            let source = format!("const a = 1;{separator}const b = 2;");
            let reports = lint.lint(Path::new("/root/a.ts"), &source);
            assert!(reports.is_empty(), "{separator:?}: {reports:?}");
        }
    }

    #[test]
    fn length_uses_tab_stops_and_unicode_characters() {
        for (source, width, length) in [("a\t+b;", 4, 7), ("ab\t+c\t+de;", 4, 12), ("'\u{1f600}'\t+x;", 4, 10), ("a\t+b;", 0, 4)] {
            for (limit, expected) in [(length, 0), (length - 1, 1)] {
                let lint = native_linter(&format!("rule max-len {{ set options [{limit}, {width}] }}"));
                assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), expected, "{source}: {limit}");
            }
        }
    }

    #[test]
    fn equality_modes_report_only_the_operators_they_forbid() {
        let cases = [
            ("rule eqeqeq", "a == b;", true),
            ("rule eqeqeq", "a === b;", false),
            ("rule eqeqeq", "a == null;", true),
            ("rule eqeqeq { set options [\"smart\"] }", "a == null;", false),
            ("rule eqeqeq { set options [\"always\", { null: \"never\" }] }", "a === null;", true),
        ];
        for (rules, source, expected) in cases {
            let lint = native_linter(rules);
            let reports = lint.lint(Path::new("/root/file.ts"), source);
            assert_eq!(!reports.is_empty(), expected, "{rules}: {source}");
            if expected {
                assert_eq!(reports.len(), 1, "{rules}: {source}");
                assert_eq!(reports[0].start, source.find("==").unwrap(), "{rules}: {source}");
            }
        }
    }
}

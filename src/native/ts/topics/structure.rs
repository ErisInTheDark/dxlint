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
    fn unsafe_fix_numeric_receivers_keep_the_member_dot_separate() {
        let lint = native_linter("rule dot-notation");
        let path = Path::new("/root/file.js");
        for receiver in ["08", "5_000_00", "1", "1.5", "1e3", "0xff", "(1)", "1?."] {
            let source = format!("{receiver}['prop'];");
            assert!(crate::native::ts::parse(&source, false).is_ok(), "{source}");
            let reports = lint.lint(path, &source);
            assert_eq!(reports.len(), 1, "{source}: {reports:?}");
            let (fixed, edits) = crate::engine::apply_fixes(&source, &reports);
            assert_eq!(edits, 1, "{source}");
            assert!(crate::native::ts::parse(&fixed, false).is_ok(), "{source} -> {fixed}");
            assert!(lint.lint(path, &fixed).is_empty(), "{fixed}");
        }
        assert!(lint.lint(path, "x['08'];").is_empty());
        let source = "x[/* keep */ 'prop'];";
        let reports = lint.lint(path, source);
        assert_eq!(reports.len(), 1);
        assert!(reports[0].fix.is_none());
    }

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
    fn line_length_reports_cover_only_the_measured_source() {
        let measured = "const longname = 1;";
        for separator in ["", "\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            let lint = native_linter("rule max-len { set options [6] }");
            let source = format!("{measured}{separator}");
            let reports = lint.lint(Path::new("/root/a.ts"), &source);
            let spans: Vec<_> = reports.iter().map(|report| &source[report.start..report.end]).collect();
            assert_eq!(spans, [measured], "{separator:?}");
        }
        for (options, source, expected) in [
            ("{ code: 6, ignoreTrailingComments: true }", "const longname = 1; // ignored\n", measured),
            ("{ code: 6, ignoreComments: true }", "const longname = 1; /* ignored */\r\n", measured),
            ("{ code: 80, comments: 6 }", "// long comment\n", "// long comment"),
            ("{ code: 6 }", "'\u{1f600}'\t+ x;\n", "'\u{1f600}'\t+ x;"),
        ] {
            let lint = native_linter(&format!("rule max-len {{ set options [{options}] }}"));
            let reports = lint.lint(Path::new("/root/a.ts"), source);
            let spans: Vec<_> = reports.iter().map(|report| &source[report.start..report.end]).collect();
            assert_eq!(spans, [expected], "{options}: {source}");
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

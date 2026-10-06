use std::path::Path;

use super::native_linter;

#[test]
fn duplicate_type_selection_covers_optional_parameters_and_both_type_shapes() {
    let source = "type U = string | string; type I = number & number; function f(value?: number | undefined) {}";
    for (selection, expected) in [
        ("", vec!["number", "string", "undefined"]),
        ("drop group duplicate_type_constituent ts_union", vec!["number"]),
        ("drop group duplicate_type_constituent ts_intersection", vec!["string", "undefined"]),
        ("override in duplicate_type_constituent let duplicate_type_constituents_checked !(is ts_union)", vec!["number"]),
        ("drop group duplicate_type_constituent ts_union, ts_intersection", vec![]),
    ] {
        let linter = native_linter(&format!("rule no-duplicate-type-constituents\n{selection}"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        let mut reported: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        reported.sort_unstable();
        assert_eq!(reported, expected, "{selection}: {diagnostics:#?}");
    }
}

//! Import policy regressions for `std/ts/restrictions`.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn import_patterns_respect_negation_and_excluded_parents() {
        let l = native_linter("rule no-restricted-imports:patterns { set options [{patterns: ['foo/*', '!foo/bar']}] }");
        assert!(l.lint(Path::new("/root/file.ts"), "import x from 'foo/bar';").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "import x from 'foo/baz';").len(), 1);
        let l = native_linter("rule no-restricted-imports:patterns { set options [{patterns: ['foo', '!foo/bar']}] }");
        assert_eq!(l.lint(Path::new("/root/file.ts"), "import x from 'foo/bar';").len(), 1);
    }

    #[test]
    fn imports_remain_file_wide_across_nested_scopes_and_other_statements() {
        let l = native_linter("rule sort-imports");
        let text = "namespace X { import z from 'z'; }\nconst text = 'import/export';\nimport a from 'a';";
        let diags = l.lint(Path::new("/root/file.ts"), text);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].start, text.rfind("import").unwrap());
        let text = "import {C, B, A} from 'x';";
        let diags = l.lint(Path::new("/root/file.ts"), text);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].start, text.find('B').unwrap());
        assert_eq!(diags[0].end, text.len());
    }

    #[test]
    fn duplicate_imports_report_each_matching_origin_once() {
        let l = native_linter("rule no-duplicate-imports\ngroup duplicate_imports_checked ts_export");
        let text = "namespace X { import {A} from 'x'; }\nimport {B} from 'x';\nexport {C} from 'x';\nimport {D} from 'x';";
        let diags = l.lint(Path::new("/root/file.ts"), text);
        assert_eq!(diags.len(), 4);
        let last = text.rfind("import").unwrap();
        assert_eq!(diags.iter().filter(|diag| diag.start == last && diag.end == text.len()).count(), 2);
    }
}

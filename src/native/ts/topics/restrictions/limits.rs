//! Boundary regressions for the limits declared in `std/ts/restrictions`.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn independent_function_and_class_paths() {
        let l = native_linter("rule complexity { set max 1 }\nrule complexity:implicit { set max 1 }");
        let code = "function outer() { function inner() { if (x) y(); } class C { field = x || y; static { if (x) y(); } } }";
        let diagnostics = l.lint(Path::new("/root/file.ts"), code);
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
        assert!(diagnostics.iter().all(|d| d.start >= code.find("function inner").unwrap()));
        assert_eq!(l.lint(Path::new("/root/file.ts"), "class C { field = () => x || y; }").len(), 1);
    }

    #[test]
    fn statement_and_depth_owners_reset_at_functions() {
        let l = native_linter("rule max-statements { set max 2 }\nrule max-depth { set max 1 }");
        let code = "function outer() { if (x) { function inner() { if (y) z(); } } }";
        assert!(l.lint(Path::new("/root/file.ts"), code).is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "function f() { if (x) { if (y) { z(); } } }").len(), 2);
    }
}

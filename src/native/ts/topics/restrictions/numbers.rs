//! Numeric policy regressions for the std rules.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn signed_types_and_parenthesised_type_indices_keep_their_context() {
        let l = native_linter("rule no-magic-numbers { set ignore [-2] }\noverride in ts_number, ts_bigint let no_magic_numbers_checked !(restriction_numeric_full.context.object || restriction_numeric_full.context.radix || restriction_numeric_full.context.jsx || restriction_numeric_full.context.type_index)");
        assert!(l.lint(Path::new("/root/file.ts"), "type A = -2; type B = A[((1 & -3) | 4)];").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "type A = 2;").len(), 1);
    }

    #[test]
    fn bigint_ignores_compare_exact_values_across_radices_and_signs() {
        let l = native_linter("rule no-magic-numbers {\nset ignore ['9007199254740993n', '-9007199254740993n', '1000000000000000000000n']\n}");
        assert!(l.lint(Path::new("/root/file.ts"), "f(9007199254740993n, 0x20000000000001n, -9007199254740993n, 1000000000000000000000n);").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "f(9007199254740992n, -9007199254740992n);").len(), 2);
    }
}

//! Identifier and property policy regressions for `std/ts/restrictions`.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn restrictions_follow_destructuring_not_key_spelling() {
        let l = native_linter("rule no-restricted-properties { set options [{object: 'source', property: 'bad'}] }");
        assert_eq!(l.lint(Path::new("/root/file.ts"), "function f({bad: alias} = source) {}").len(), 1);
        assert_eq!(l.lint(Path::new("/root/file.ts"), "({bad: target.field} = source);").len(), 1);
        assert!(l.lint(Path::new("/root/file.ts"), "({bad: target.field} = other);").is_empty());
    }

    #[test]
    fn import_attributes_do_not_own_identifier_names() {
        let l = native_linter("rule id-denylist { set names ['with', 'type', 'value'] }");
        assert!(l.lint(Path::new("/root/file.ts"), "import('x', {with: {type: 'json'}})").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "import('x', {with: {[type]: value}})").len(), 2);
    }
}

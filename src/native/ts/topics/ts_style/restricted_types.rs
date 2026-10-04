//! Configurable restrictions on named and empty types.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn restricted_type_candidates(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut candidates = Vec::new();
    match s.kind(node) {
        Some(K::TypeRef) => {
            if let Some(name) = s.get(node, F::Name) { candidates.push(name); }
            if s.all(node, F::TypeArg).next().is_some() { candidates.push(node); }
        }
        Some(K::TypeKeyword) if matches!(s.text(node), "bigint" | "boolean" | "never" | "null" | "number" | "object" | "string" | "symbol" | "undefined" | "unknown" | "void") => candidates.push(node),
        Some(K::TupleType) if s.all(node, F::Element).next().is_none() => candidates.push(node),
        Some(K::TypeLit) if s.all(node, F::Member).next().is_none() => candidates.push(node),
        _ => {}
    }
    Value::list(candidates.into_iter().map(|node| Value::record(vec![
        ("name".into(), Value::from(s.text(node).chars().filter(|c| !c.is_whitespace()).collect::<String>())),
        ("offset".into(), Value::from(s.tree.node(node).start as usize)),
    ])).collect())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn restrictions_check_generic_names_and_instantiations_independently() {
        let linter = native_linter(r#"rule no-restricted-types { set types { Foo: true, "Foo < number >": "use another type", "{}": true } }"#);
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "type A = Foo<number>; const Foo = 1; type B = { x: number };").len(), 2);
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "type A = {};").len(), 1);
        let linter = native_linter(r#"rule no-restricted-types { set types { Foo: "", number: false, unknown: null } }"#);
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "type A = Foo; type B = number; type C = unknown;").len(), 1);
    }
}

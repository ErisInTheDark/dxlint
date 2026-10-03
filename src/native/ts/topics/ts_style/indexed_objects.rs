//! Record conversions preserve recursive and key-dependent object types.

use std::collections::HashSet;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn record_convertible(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mapped = s.is(node, K::Mapped);
    if mapped {
        let Some(parameter) = s.get(node, F::TypeParam) else { return Value::Bool(false) };
        let Some(key) = s.get(parameter, F::Name) else { return Value::Bool(false) };
        if file.bindings().refs.iter().any(|(&reference, &declaration)| {
            declaration == key && file.bindings().access.get(&reference) == Some(&"type")
        }) {
            return Value::Bool(false);
        }
        if s.get(parameter, F::Constraint).is_some_and(|n| s.is(n, K::TypeOperator) && s.word(n, F::Op) == Some("keyof")) {
            return Value::Bool(false);
        }
    } else {
        let members: Vec<_> = s.all(node, F::Member).collect();
        if members.len() != 1 || !s.is(members[0], K::IndexSig)
            || s.get(members[0], F::Type).is_none()
            || !s.all(members[0], F::Param).any(|p| s.get(p, F::Type).is_some() && s.get(p, F::Name).is_some_and(|n| s.is(n, K::Ident)))
        {
            return Value::Bool(false);
        }
    }
    let mut declaration = if s.is(node, K::Interface) { Some(node) } else { s.parent(node) };
    while let Some(at) = declaration {
        if matches!(s.kind(at), Some(K::TypeAlias | K::Interface)) {
            break;
        }
        if matches!(s.kind(at), Some(K::IndexSig | K::PropSig | K::Param | K::Declarator | K::ClassProp | K::Method | K::Function | K::FnType | K::CallSig | K::MethodSig)) {
            declaration = None;
            break;
        }
        declaration = s.parent(at);
    }
    let recursive = declaration.and_then(|n| s.get(n, F::Name)).is_some_and(|name| {
        let start = if mapped { s.parent(node).unwrap_or(node) } else { node };
        references_type(file, start, name, &mut HashSet::from([name]))
    });
    Value::Bool(!recursive)
}

fn references_type(file: &NativeFile, node: u32, target: u32, visited: &mut HashSet<u32>) -> bool {
    if !visited.insert(node) {
        return false;
    }
    let s = &file.syntax;
    match s.kind(node) {
        Some(K::TypeLit | K::Interface) => s.all(node, F::Member).any(|n| references_type(file, n, target, visited)),
        Some(K::TypeAlias | K::IndexSig | K::Mapped | K::ParenType) => s.get(node, F::Type).is_some_and(|n| references_type(file, n, target, visited)),
        Some(K::Union | K::Intersection | K::IndexedAccess | K::ConditionalType | K::TypeRef) => {
            s.kids(node).any(|(_, n)| references_type(file, n, target, visited))
        }
        Some(K::Ident) => {
            let Some(declaration) = file.decl_of(node) else { return false };
            declaration == target || s.parent(declaration).is_some_and(|n| references_type(file, n, target, visited))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn record_conversion_preserves_recursive_and_key_dependent_types() {
        let linter = native_linter("rule consistent-indexed-object-style\nrule consistent-indexed-object-style:interface\nrule consistent-indexed-object-style:mapped");
        for text in [
            "type A = { [key: string]: B }; interface B { [key: string]: A; }",
            "type A<T> = { [P in keyof T]: T[P] };",
        ] {
            assert!(linter.lint(Path::new("/root/file.ts"), text).is_empty(), "{text}");
        }
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "interface A { [key: string]: A[]; }").len(), 1);
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "type A = { [P in string]: number };").len(), 1);
    }
}

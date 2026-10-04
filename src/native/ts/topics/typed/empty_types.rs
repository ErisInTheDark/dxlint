//! Type operations producing the resolved empty object, not unresolved keys.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn generated(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.is(node, K::TypeRef) {
        if s.all(node, F::TypeArg).next().is_none() { return Value::Bool(false) }
        let mut parent = s.parent(node);
        while parent.is_some_and(|parent| s.is(parent, K::ParenType)) { parent = parent.and_then(|parent| s.parent(parent)); }
        if parent.is_some_and(|parent| s.is(parent, K::Intersection)) { return Value::Bool(false) }
    } else if !s.is(node, K::Intersection) { return Value::Bool(false) }
    Value::Bool(file.typing().checker.generated_empty_object(&file.type_of(node)))
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn unsupported_generic_arguments_do_not_resolve_global_names_for_alias_parameters() {
        let linter = native_linter("rule no-generated-empty-object-type");
        let code = "type NonNullable<T> = T & {}; type T = unknown; type Identity<T> = T; type Data = { a: string }; type Result = NonNullable<Identity<Data[keyof Data]>>;";
        assert!(linter.lint(Path::new("/root/lib.fixture.d.ts"), code).is_empty());
    }

    #[test]
    fn resolved_empty_operations_report_once_but_generic_keys_and_nonempty_results_do_not() {
        let linter = native_linter("rule no-generated-empty-object-type");
        let code = "type Pick<T, K extends keyof T> = { [P in K]: T[P] }; type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>; type NonNullable<T> = T & {}; type Data = { a: string; b: number }; type Empty = Pick<Data, never>; type Nullable = Omit<Data | null, 'a'>; type EmptyIntersection = Omit<Data, keyof Data> & Omit<Data, keyof Data>; type NonemptyIntersection = Omit<Data, keyof Data> & { other: string }; type Generic<T> = Omit<T, keyof T>; type Nonempty = Omit<Data, 'a'>; type MakeRequired<Base, Key extends keyof Base> = NonNullable<Base[Key]>; type UnknownAlias = unknown; type Intrinsic = NonNullable<unknown>; type Aliased = NonNullable<UnknownAlias>;";
        assert_eq!(linter.lint(Path::new("/root/lib.fixture.d.ts"), code).len(), 5);
    }
}

//! Default values checked against the source shape, not the default itself.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn finding(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let (value, property) = match s.kind(node) {
        Some(K::Param) => (s.get(node, F::Init), false),
        Some(K::AssignPattern) => (s.get(node, F::Right), s.parent(node).is_some_and(|parent| matches!(s.kind(parent), Some(K::PatternProp | K::ArrayPattern)))),
        _ => return Value::Null,
    };
    let Some(value) = value else { return Value::Null };
    let finding = |kind| Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("owner".into(), Value::from(if property { "property" } else { "parameter" })),
        ("offset".into(), Value::from(s.tree.node(value).start as usize)),
    ]);
    if s.is(value, K::Ident) && s.text(value) == "undefined" {
        let optional = s.is(node, K::Param) && s.get(node, F::Type).is_some_and(|annotation| can_be_undefined(&file.type_of(annotation)));
        return finding(if optional { "optional" } else { "undefined" });
    }
    let ck = &file.typing().checker;
    let source = if s.is(node, K::Param) {
        let Some(function) = s.parent(node).filter(|&function| matches!(s.kind(function), Some(K::Arrow | K::Function))) else { return Value::Null };
        if s.is(function, K::Function) && s.parent(function).is_some_and(|parent| matches!(s.kind(parent), Some(K::Program | K::Block | K::Export))) { return Value::Null }
        let Some(index) = s.all(function, F::Param).position(|param| param == node) else { return Value::Null };
        let Some(source) = ck.required_contextual_parameter(&file.contextual_type_of(function), index) else { return Value::Null };
        source
    } else {
        pattern_source(file, node, 0)
    };
    if can_be_undefined(&source) { Value::Null } else { finding("required") }
}

fn can_be_undefined(ty: &Ty) -> bool {
    match ty {
        Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) | Ty::Undefined | Ty::Void => true,
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(can_be_undefined),
        _ => false,
    }
}

fn pattern_source(file: &NativeFile<'_>, pattern: u32, depth: usize) -> Ty {
    if depth >= 32 { return Ty::Unknown }
    let s = &file.syntax;
    let Some(parent) = s.parent(pattern) else { return Ty::Unknown };
    let source = |node| pattern_source(file, node, depth + 1);
    match s.kind(parent) {
        Some(K::Declarator) => s.get(parent, F::Init).map_or(Ty::Unknown, |init| file.type_of(init)),
        Some(K::Param) => s.get(parent, F::Type).map_or(Ty::Unknown, |annotation| file.type_of(annotation)),
        Some(K::AssignPattern) => source(parent),
        Some(K::PatternProp) => {
            let (Some(object), Some(name)) = (s.parent(parent), crate::native::ts::statics::property_name(s, parent)) else { return Ty::Unknown };
            let ty = source(object);
            let ck = &file.typing().checker;
            let mut names = Vec::new();
            ck.member_names(&ty, &mut names);
            if !names.iter().any(|candidate| **candidate == name) { return Ty::Unknown }
            ck.member(&ty, &name)
        }
        Some(K::ArrayPattern) => {
            let index = s.all(parent, F::Element).position(|element| element == pattern);
            match (source(parent), index) {
                (Ty::Tuple(parts), Some(index)) => {
                    if index < parts.required_length() { parts.at(index) } else { Ty::Unknown }
                }
                (Ty::Array(array), _) if !s.is(pattern, K::AssignPattern) => *array.element,
                _ => Ty::Unknown,
            }
        }
        _ => Ty::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn tuple_defaults_require_a_present_element() {
        let linter = native_linter("rule no-useless-default-assignment");
        let code = "declare const fixed: [number, number?]; const [required = 1, optional = 1] = fixed; declare const variadic: [number, ...number[]]; const [head = 1, tail = 1] = variadic; declare const middle: [boolean, ...number[], string]; const [flag, present = 0] = middle; declare const objects: { world: string }[]; const [{ world = '' }] = objects;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 4);
    }

    #[test]
    fn static_computed_properties_remain_present_across_conditional_sources() {
        let linter = native_linter("rule no-useless-default-assignment");
        let code = "declare const flag: boolean; const { value = 'fallback' } = flag ? { ['value']: 'left' } : { [`value`]: 'right' };";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn index_signatures_do_not_prove_a_destructured_property_present() {
        let linter = native_linter("rule no-useless-default-assignment");
        let code = "type Record<K extends keyof any, T> = { [P in K]: T }; declare const indexed: Record<string, string>; const { value = 'fallback' } = indexed;";
        assert!(linter.lint(Path::new("/root/lib.fixture.d.ts"), code).is_empty());
    }

    #[test]
    fn callback_defaults_receive_an_object_property_signature() {
        let linter = native_linter("rule no-useless-default-assignment");
        let code = "interface Shape { callback: (value: number) => void; mixed: boolean | ((value: number) => void); } const object: Shape = { callback: (value = 1) => {}, mixed: (value = 1) => {} };";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn defaults_follow_source_optionality_and_callback_requirements() {
        let linter = native_linter("rule no-useless-default-assignment");
        let code = "declare const source: { required: number; optional?: number }; const { required = 1, optional = 1 } = source; function declaration(value: number = 1) {} const callback: (value: number) => void = (value = 1) => {}; const optionalCallback: (value?: number) => void = (value = 1) => {}; function optionalParameter(value: number | undefined = undefined) {} const { optional: unused = undefined } = source; const nonCallable: number | undefined = (value = 1) => {};";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 4);
    }
}

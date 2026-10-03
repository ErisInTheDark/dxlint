//! Declaration-style decisions: constructor properties and inferred types.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

pub(super) fn parameter_property_modifiers(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut modifiers = Vec::new();
    for modifier in ["public", "protected", "private", "readonly"] {
        if s.has_word(node, F::Modifier, modifier) {
            modifiers.push(modifier);
        }
    }
    Value::from(modifiers.join(" "))
}

pub(super) fn parameter_property_candidate(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(key) = s.get(node, F::Key).filter(|&n| s.is(n, K::Ident)) else { return Value::Bool(false) };
    if s.get(node, F::Init).is_some() {
        return Value::Bool(false);
    }
    let Some(class) = s.parent(node) else { return Value::Bool(false) };
    let found = s.all(class, F::Member).filter(|&n| s.is(n, K::Method) && s.word(n, F::Key) == Some("constructor")).any(|constructor| {
        let parameter = s.all(constructor, F::Param).find(|&n| {
            s.word(n, F::Name) == Some(s.text(key)) && s.get(n, F::Init).is_none()
                && !["public", "protected", "private", "readonly"].iter().any(|modifier| s.has_word(n, F::Modifier, modifier))
        });
        let Some(parameter) = parameter else { return false };
        if s.word(parameter, F::Type) != s.word(node, F::Type) {
            return false;
        }
        let Some(body) = s.get(constructor, F::Body) else { return false };
        for statement in s.all(body, F::Body) {
            let Some(assignment) = s.get(statement, F::Expr).filter(|&n| s.is(statement, K::ExprStmt) && s.is(n, K::Assign)) else { break };
            let Some(left) = s.get(assignment, F::Left).filter(|&n| s.is(n, K::Member)) else { break };
            if !s.get(left, F::Object).is_some_and(|n| s.is(n, K::This)) || !s.get(left, F::Property).is_some_and(|n| s.is(n, K::Ident)) {
                break;
            }
            let Some(right) = s.get(assignment, F::Right).filter(|&n| s.is(n, K::Ident)) else { break };
            if s.text(right) == s.text(key) {
                return true;
            }
        }
        false
    });
    Value::Bool(found)
}

fn call_named(s: &Syntax, node: u32, name: &str) -> bool {
    s.is(node, K::Call) && s.get(node, F::Callee).is_some_and(|n| s.is(n, K::Ident) && s.text(n) == name)
}

pub(super) fn generic_constructor_style(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.is(node, K::Param) && !s.parent(node).is_some_and(|n| matches!(s.kind(n), Some(K::Function | K::Method))) { return Value::Null; }
    let Some(init) = s.get(node, F::Init).map(|n| s.unparen(n)).filter(|&n| s.is(n, K::New)) else { return Value::Null };
    let Some(callee) = s.get(init, F::Callee).filter(|&n| s.is(n, K::Ident)) else { return Value::Null };
    let annotation = s.get(node, F::Type);
    if let Some(annotation) = annotation {
        let Some(name) = s.get(annotation, F::Name).filter(|&n| s.is(annotation, K::TypeRef) && s.is(n, K::Ident)) else { return Value::Null };
        if s.text(name) != s.text(callee)
            || matches!(s.text(name), "Float32Array" | "Float64Array" | "Int16Array" | "Int32Array" | "Int8Array" | "Uint16Array" | "Uint32Array" | "Uint8Array" | "Uint8ClampedArray") && file.decl_of(name).is_none()
        { return Value::Null; }
        if s.all(annotation, F::TypeArg).next().is_some() && s.all(init, F::TypeArg).next().is_none() {
            return Value::from("constructor");
        }
    } else if s.all(init, F::TypeArg).next().is_some() {
        return Value::from("type-annotation");
    }
    Value::Null
}

pub(super) fn inferrable_annotation(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(annotation) = s.get(node, F::Type) else { return Value::Null };
    let Some(init) = s.get(node, F::Init).map(|n| s.unparen(n)) else { return Value::Null };
    let name = s.text(annotation);
    let unsigned = if s.is(init, K::Unary) && matches!(s.word(init, F::Op), Some("+" | "-")) {
        s.get(init, F::Arg).map(|n| s.unparen(n)).unwrap_or(init)
    } else {
        init
    };
    let inferred = match name {
        "bigint" => {
            let value = if s.word(init, F::Op) == Some("-") { unsigned } else { init };
            call_named(s, value, "BigInt") || matches!(s.kind(value), Some(K::Number | K::String | K::BigInt | K::Bool | K::Null | K::Regex))
        }
        "boolean" => s.is(init, K::Bool) || call_named(s, init, "Boolean") || s.is(init, K::Unary) && s.word(init, F::Op) == Some("!"),
        "number" => s.is(unsigned, K::Number) || call_named(s, unsigned, "Number") || s.is(unsigned, K::Ident) && matches!(s.text(unsigned), "Infinity" | "NaN"),
        "null" => s.is(init, K::Null),
        "string" => matches!(s.kind(init), Some(K::String | K::Template)) || call_named(s, init, "String"),
        "symbol" => call_named(s, init, "Symbol"),
        "undefined" => s.is(init, K::Ident) && s.text(init) == "undefined" || s.is(init, K::Unary) && s.word(init, F::Op) == Some("void"),
        "RegExp" => s.is(init, K::Regex) || call_named(s, init, "RegExp") || s.is(init, K::New) && s.word(init, F::Callee) == Some("RegExp"),
        _ => false,
    };
    if inferred { Value::from(name.to_string()) } else { Value::Null }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn parameter_properties_require_matching_types_and_leading_assignments() {
        let linter = native_linter("rule parameter-properties:class { set prefer 'parameter-property' }");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "class A { x: number; constructor(x: number) { this.x = x; } }").len(), 1);
        for text in [
            "class A { x: number; constructor(x: string) { this.x = x; } }",
            "class A { x: number; constructor(x: number) { initialise(); this.x = x; } }",
            "class A { x: number = 1; constructor(x: number) { this.x = x; } }",
        ] {
            assert!(linter.lint(Path::new("/root/file.ts"), text).is_empty(), "{text}");
        }
    }

    #[test]
    fn inferred_annotations_preserve_widening_and_nonprimitive_calls() {
        let linter = native_linter("rule no-inferrable-types\nrule no-inferrable-types:property\nrule no-inferrable-types:parameter");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "function f(x: number = -Number(1)) {} class A { x: boolean = Boolean(); readonly y: string = 'a'; z: number = source(); }").len(), 2);
    }

    #[test]
    fn generic_constructors_do_not_require_parentheses() {
        let linter = native_linter("rule consistent-generic-constructors");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "const value: Map<string, number> = new Map;").len(), 1);
    }

}

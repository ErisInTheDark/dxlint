//! Global eval-like calls: typed callback checks and JavaScript string checks.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::statics;

fn evaluated_string(file: &NativeFile<'_>, node: u32, depth: usize) -> bool {
    if depth >= 64 { return false }
    let s = &file.syntax;
    let node = s.unparen(node);
    if matches!(s.kind(node), Some(K::String | K::Template)) { return true }
    if file.static_string(node).is_some() && matches!(file.type_of(node).widen(), Ty::Prim("string")) { return true }
    if s.is(node, K::Call) && s.get(node, F::Callee).is_some_and(|callee| file.global_value(callee).as_deref() == Some("String"))
        && s.all(node, F::Arg).next().is_some_and(|arg| file.static_string(arg).is_some()) { return true }
    s.is(node, K::Binary) && s.word(node, F::Op) == Some("+")
        && [F::Left, F::Right].iter().filter_map(|&field| s.get(node, field)).any(|node| evaluated_string(file, node, depth + 1))
}

fn function(file: &NativeFile<'_>, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    match s.kind(node) {
        Some(K::Arrow | K::Function) => return true,
        Some(K::String | K::Number | K::Bool | K::Null | K::BigInt | K::Regex | K::Template) => return false,
        Some(K::Call) if s.get(node, F::Callee).is_some_and(|callee| {
            s.word(callee, F::Property) == Some("bind") || s.is(callee, K::Ident) && s.text(callee) == "bind"
        }) => return true,
        _ => {}
    }
    let ck = &file.typing().checker;
    let ty = file.type_of(node);
    ck.callable_type(&ty) || ck.is_builtin_type_or_base(&ty, "Function")
}

fn core_global(file: &NativeFile<'_>, mut object: u32) -> bool {
    let s = &file.syntax;
    for _ in 0..32 {
        object = s.unparen(object);
        if s.is(object, K::Ident) {
            return matches!(s.text(object), "global" | "globalThis" | "window" | "self")
                && file.global_setting(object).is_some_and(|setting| setting != "off");
        }
        if !matches!(s.kind(object), Some(K::Member | K::Index)) { return false }
        let Some(parent) = s.get(object, F::Object) else { return false };
        let Some(name) = statics::property_name(s, object) else { return false };
        let mut root = parent;
        while matches!(s.kind(root), Some(K::Member | K::Index)) {
            let Some(inner) = s.get(root, F::Object) else { return false };
            root = inner;
        }
        if !s.is(root, K::Ident) || name != s.text(root) { return false }
        object = parent;
    }
    false
}

pub(super) fn finding(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, F::Callee).map(|callee| s.unparen(callee)) else { return Value::Null };
    let typed = super::is_typescript(file);
    let name = if s.is(callee, K::Ident) { s.text(callee).to_string() } else {
        let Some(object) = s.get(callee, F::Object) else { return Value::Null };
        if typed {
            if !s.is(object, K::Ident) || !matches!(s.text(object), "global" | "globalThis" | "window") { return Value::Null }
        } else if !core_global(file, object) { return Value::Null }
        let Some(name) = statics::property_name(s, callee) else { return Value::Null };
        name
    };
    let mode = if typed && name == "Function" {
        let ck = &file.typing().checker;
        let ty = file.type_of(callee);
        if ck.is_builtin_type_or_base(&ty, "FunctionConstructor") || matches!(ty, Ty::Any)
            || matches!(ty, Ty::Unknown) && file.decl_of(callee).is_none() { Some(("function", node)) } else { None }
    } else if matches!(name.as_str(), "setTimeout" | "setInterval" | "execScript") || typed && name == "setImmediate" {
        let Some(handler) = s.all(node, F::Arg).next() else { return Value::Null };
        if s.is(callee, K::Ident) && file.decl_of(callee).is_some() { return Value::Null }
        if !typed && s.is(callee, K::Ident) && file.global_setting(callee).is_none_or(|setting| setting == "off") { return Value::Null }
        if typed {
            (!function(file, handler)).then_some(("typed", handler))
        } else if s.is(node, K::Call) && evaluated_string(file, handler, 0) {
            Some((if name == "execScript" { "exec" } else { "core" }, node))
        } else { None }
    } else { None };
    mode.map_or(Value::Null, |(mode, report)| Value::record(vec![
        ("mode".into(), Value::from(mode)),
        ("offset".into(), Value::from(s.tree.node(report).start as usize)),
    ]))
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn shadowed_timers_and_callable_types_are_not_eval_like() {
        let linter = native_linter("rule no-implied-eval\nrule no-implied-eval:*");
        let code = "interface Callback { (): void; } declare const cb: Callback; setTimeout(cb); setTimeout('code'); function f(setTimeout: (x: string) => void) { setTimeout('code'); } new Function('code');";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn object_callbacks_are_callable_but_mixed_unions_are_not() {
        let linter = native_linter("rule no-implied-eval");
        let code = "const obj = { method() {}, arrow: () => {}, get getter() { return () => {}; } }; const key = 'arrow'; setTimeout(obj.method); setTimeout(obj[key]); setTimeout(obj['arrow']); setTimeout(obj.getter); declare const mixed: string | (() => void); setTimeout(mixed); declare const legacy: string | Function; setTimeout(legacy);";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert_eq!(diagnostics[0].start, code.rfind("mixed").unwrap());
    }

    #[test]
    fn javascript_timers_require_a_global_and_an_actual_string_value() {
        let linter = native_linter("rule no-implied-eval:*");
        assert!(linter.lint(Path::new("/root/a.js"), "setTimeout('code');").is_empty());
        let code = "/* global setTimeout */ setTimeout(1); setTimeout(String('code'));";
        assert_eq!(linter.lint(Path::new("/root/a.js"), code).len(), 1);
    }

    #[test]
    fn unresolved_imports_are_not_proven_builtin_function_constructors() {
        let linter = native_linter("rule no-implied-eval:function");
        assert!(linter.lint(Path::new("/root/a.ts"), "import { Function } from './missing'; new Function('code');").is_empty());
    }
}

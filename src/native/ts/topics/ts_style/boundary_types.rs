//! Functions reachable through module exports and their boundary parameters.

use std::collections::HashSet;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

struct BoundaryFunctions(HashSet<u32>);

fn collect(file: &NativeFile) -> BoundaryFunctions {
    let s = &file.syntax;
    let mut functions = HashSet::new();
    let mut visited = HashSet::new();
    for node in 0..s.tree.nodes.len() as u32 {
        if s.is(node, K::Export) && s.get(node, F::From).is_none() {
            for field in [F::Decl, F::Expr] {
                if let Some(value) = s.get(node, field) { visit(file, value, &mut visited, &mut functions); }
            }
            for spec in s.all(node, F::Specifier) {
                if let Some(value) = s.get(spec, F::Local) { visit(file, value, &mut visited, &mut functions); }
            }
        } else if s.is(node, K::ExportAssign) {
            if let Some(value) = s.get(node, F::Expr) { visit(file, value, &mut visited, &mut functions); }
        }
    }
    BoundaryFunctions(functions)
}

fn private(s: &Syntax, node: u32) -> bool {
    s.has_word(node, F::Modifier, "private") || s.get(node, F::Key).is_some_and(|n| s.is(n, K::PrivateName))
}

fn visit(file: &NativeFile, node: u32, visited: &mut HashSet<u32>, functions: &mut HashSet<u32>) {
    let s = &file.syntax;
    let node = s.unparen(node);
    if !visited.insert(node) { return; }
    let field = match s.kind(node) {
        Some(K::Function | K::Arrow | K::Method) => {
            if s.is(node, K::Method) && private(s, node) { return; }
            functions.insert(node);
            let mut values = Vec::new();
            if let Some(body) = s.get(node, F::Body) {
                if s.is(node, K::Arrow) && !s.is(body, K::Block) {
                    values.push(Some(s.unparen(body)));
                } else { super::return_types::returns(s, body, &mut values); }
            }
            if !values.is_empty() && values.iter().all(|v| v.is_some_and(|v| matches!(s.kind(s.unparen(v)), Some(K::Function | K::Arrow)))) {
                for value in values.into_iter().flatten() { visit(file, value, visited, functions); }
            }
            return;
        }
        Some(K::Ident) => {
            let Some(decl) = file.decl_of(node) else { return; };
            if file.binding(decl).is_none_or(|b| matches!(b.kind, "parameter" | "catch" | "import")) { return; }
            if let Some(owner) = s.parent(decl) { visit(file, owner, visited, functions); }
            for (&reference, &target) in &file.bindings().refs {
                if target != decl { continue; }
                if let Some(write) = file.bindings().write_sites.get(&reference).filter(|&&n| s.is(n, K::Assign)) {
                    if let Some(value) = s.get(*write, F::Right) { visit(file, value, visited, functions); }
                }
            }
            return;
        }
        Some(K::Class) => F::Member,
        Some(K::Object) => F::Property,
        Some(K::Array) => F::Element,
        Some(K::Var) => F::Decl,
        Some(K::Declarator | K::ClassProp) => {
            if s.is(node, K::ClassProp) && private(s, node) { return; }
            F::Init
        }
        Some(K::Prop) => {
            if s.get(node, F::Value).is_none() {
                if let Some(key) = s.get(node, F::Key).filter(|&n| s.is(n, K::Ident)) { visit(file, key, visited, functions); }
                return;
            }
            F::Value
        }
        _ => return,
    };
    for value in s.all(node, field) { visit(file, value, visited, functions); }
}

fn name(file: &NativeFile, node: u32) -> String {
    let s = &file.syntax;
    if s.is(node, K::Method) { return super::class_styles::static_member_name(file, node).unwrap_or_default(); }
    let declaration = s.is(node, K::Function) && s.parent(node).is_some_and(|p| matches!(s.kind(p), Some(K::Program | K::Block | K::Export | K::Namespace)));
    if declaration { return s.word(node, F::Name).unwrap_or("").into(); }
    let mut at = node;
    while let Some(parent) = s.parent(at) {
        if s.is(parent, K::Paren) { at = parent; continue; }
        return match s.kind(parent) {
            Some(K::Declarator) => s.get(parent, F::Name).filter(|&n| s.is(n, K::Ident)).map(|n| s.text(n).into()).unwrap_or_default(),
            Some(K::ClassProp) => super::class_styles::static_member_name(file, parent).unwrap_or_default(),
            _ => String::new(),
        };
    }
    String::new()
}

fn overload(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let Some(parent) = s.parent(node) else { return false; };
    let exported = s.is(parent, K::Export);
    let scope = if exported { s.parent(parent) } else { Some(parent) };
    let Some(scope) = scope else { return false; };
    let default_export = exported && s.word(parent, F::Kind) == Some("default");
    let key = name(file, node);
    s.kids(scope).any(|(_, mut candidate)| {
        if exported {
            if !s.is(candidate, K::Export) || (s.word(candidate, F::Kind) == Some("default")) != default_export { return false; }
            let Some(decl) = s.get(candidate, F::Decl) else { return false; };
            candidate = decl;
        }
        matches!(s.kind(candidate), Some(K::Function | K::Method)) && s.get(candidate, F::Body).is_none()
            && (default_export || name(file, candidate) == key)
    })
}

pub(super) fn function_boundary_style(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let parameter = s.is(node, K::Param);
    let function = if parameter { s.parent(node).unwrap_or(node) } else { node };
    if !file.cached(collect).0.contains(&function) { return Value::Null; }
    let Value::Record(shape) = super::return_types::function_return_style(file, function) else { return Value::Null; };
    let mut shape = shape.to_vec();
    if let Some((_, value)) = shape.iter_mut().find(|(key, _)| key.as_ref() == "name") { *value = Value::from(name(file, function)); }
    let empty_method = s.is(function, K::Method) && s.get(function, F::Body).is_none();
    let offset = if empty_method {
        s.get(function, F::Key).map(|key| {
            let end = s.tree.node(key).end;
            end + crate::native::ts::lexer::Lexer::new(&s.src[end as usize..]).next_token().start
        })
    } else if s.parent(function).is_some_and(|p| s.is(p, K::ClassProp) && s.has_word(p, F::Modifier, "accessor")) {
        Some(s.get(function, F::Op).map_or(s.tree.node(function).start, |n| s.tree.node(n).start))
    } else { None };
    if let Some(offset) = offset {
        if let Some((_, value)) = shape.iter_mut().find(|(key, _)| key.as_ref() == "offset") { *value = Value::from(offset as usize); }
    }
    shape.push(("overload".into(), Value::Bool(overload(file, function))));
    shape.push(("empty_method".into(), Value::Bool(empty_method)));
    if parameter {
        if s.get(node, F::Init).is_some() { return Value::Null; }
        let Some(mut argument) = s.get(node, F::Name) else { return Value::Null; };
        let offset = s.tree.node(argument).start;
        let rest = s.is(argument, K::Rest);
        if rest {
            let Some(inner) = s.get(argument, F::Arg) else { return Value::Null; };
            argument = inner;
        }
        let named = matches!(s.kind(argument), Some(K::Ident | K::This));
        let display = if named { s.text(argument) } else if rest { "Rest" } else if s.is(argument, K::ArrayPattern) { "Array pattern" } else { "Object pattern" };
        let annotation = s.get(node, F::Type);
        let any = annotation.is_some_and(|n| s.text(n) == "any");
        if annotation.is_some() && !any { return Value::Null; }
        let message = if named {
            format!("Argument '{display}' should be typed{}.", if any { " with a non-any type" } else { "" })
        } else {
            format!("{display} argument should be typed{}.", if any { " with a non-any type" } else { "" })
        };
        shape.push(("any".into(), Value::Bool(any)));
        shape.push(("message".into(), Value::from(message)));
        shape.push(("parameter_offset".into(), Value::from(offset as usize)));
    }
    Value::record(shape)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn indirect_exports_require_function_and_parameter_types() {
        let linter = native_linter("rule explicit-module-boundary-types\nrule explicit-module-boundary-types:argument");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "function f(value) { return value; } export { f };").len(), 2);
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "function f(value) { return value; } export default { f };").len(), 2);
    }
}

//! Returned awaitables and control flow owned by their async function.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn findings(file: &NativeFile<'_>, function: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.has_word(function, F::Modifier, "async") { return Value::list(Vec::new()) }
    let Some(body) = s.get(function, F::Body) else { return Value::list(Vec::new()) };
    let mut expressions = if s.is(body, K::Block) {
        file.return_sites(function).filter(|&node| s.is(node, K::Return)).filter_map(|node| s.get(node, F::Arg)).collect()
    } else { vec![body] };
    let ck = &file.typing().checker;
    let mut out = Vec::new();
    while let Some(node) = expressions.pop() {
        let node = s.unparen(node);
        if s.is(node, K::Conditional) {
            expressions.extend(s.get(node, F::Consequent));
            expressions.extend(s.get(node, F::Alternate));
            continue;
        }
        let awaited = s.is(node, K::Await);
        let argument = if awaited { s.get(node, F::Arg).unwrap_or(node) } else { node };
        let ty = file.constrained_type_of(argument);
        let certainty = if uncertain(&ty) { "may" } else if ck.is_thenable(&ty) { "always" } else { "never" };
        out.push(Value::record(vec![
            ("offset".into(), Value::from(s.tree.node(node).start as usize)),
            ("awaited".into(), Value::Bool(awaited)),
            ("certainty".into(), Value::from(certainty)),
            ("handling".into(), Value::Bool(affects_handling(file, node, function))),
        ]));
    }
    Value::list(out)
}

fn uncertain(ty: &Ty) -> bool {
    match ty {
        Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => true,
        Ty::Union(parts) => parts.iter().any(uncertain),
        _ => false,
    }
}

fn affects_handling(file: &NativeFile<'_>, node: u32, function: u32) -> bool {
    let s = &file.syntax;
    let mut ancestors = vec![node];
    let mut child = node;
    while child != function {
        let Some(parent) = s.parent(child) else { break };
        if s.is(parent, K::Try) && (s.get(parent, F::Block) == Some(child)
            || s.get(parent, F::Handler) == Some(child) && s.get(parent, F::Finalizer).is_some()) { return true }
        ancestors.push(parent);
        child = parent;
    }
    let bindings = file.bindings();
    bindings.decls.iter().any(|(&decl, binding)| {
        binding.kind == "using" && bindings.decl_scope.get(&decl).is_some_and(|&scope| ancestors.contains(&bindings.scopes[scope].node))
            && s.parent(decl).is_some_and(|declarator| s.tree.node(declarator).end < s.tree.node(node).start)
    })
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn error_handling_does_not_leak_across_functions_or_finished_catch_blocks() {
        let linter = native_linter("rule return-await");
        let code = "interface Like { then(ok: (value: number) => void): Like; } declare const p: Like; async function work() { try { const inner = async () => p; return p; } catch { return await p; } } async function cleanup() { try { return 1; } catch { return p; } finally {} } async function scalar() { return await 1; }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 4);
    }

    #[test]
    fn resource_scope_and_declaration_order_control_required_awaits() {
        let linter = native_linter("rule return-await");
        let code = "interface Like { then(ok: (value: number) => void): Like; } declare const p: Like; async function before() { return p; using resource = {}; } async function after() { using resource = {}; return p; } async function finished() { { using resource = {}; } return p; } async function nested() { using resource = {}; const inner = async () => p; return 1; }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }
}

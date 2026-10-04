//! Explicit return-type eligibility and function head locations.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

fn parent(s: &Syntax, mut node: u32) -> Option<(u32, u32)> {
    while let Some(at) = s.parent(node) {
        if !s.is(at, K::Paren) { return Some((node, at)); }
        node = at;
    }
    None
}

fn typed_parent(s: &Syntax, child: u32, parent: u32) -> bool {
    matches!(s.kind(parent), Some(K::As | K::TypeAssertion | K::JsxExpression | K::JsxSpreadAttribute | K::New))
        || matches!(s.kind(parent), Some(K::Declarator | K::ClassProp)) && s.get(parent, F::Type).is_some()
        || s.is(parent, K::Param) && s.field(child) == Some(F::Init) && s.get(parent, F::Type).is_some()
        || s.is(parent, K::Call) && s.field(child) != Some(F::Callee)
}

fn typed_object(s: &Syntax, property: u32) -> bool {
    if !matches!(s.kind(property), Some(K::Prop | K::Method)) { return false; }
    let Some(object) = s.parent(property).filter(|&n| s.is(n, K::Object)) else { return false };
    typed_value(s, object)
}

fn typed_value(s: &Syntax, node: u32) -> bool {
    parent(s, node).is_some_and(|(child, at)| {
        typed_parent(s, child, at) || typed_object(s, at)
            || s.is(at, K::Array) && typed_value(s, at)
            || s.is(at, K::Return) && ancestor_return_type(s, node)
    })
}

fn ancestor_return_type(s: &Syntax, node: u32) -> bool {
    let Some((child, mut at)) = parent(s, node) else { return false };
    if !(s.is(at, K::Return) || s.is(at, K::Arrow) && s.field(child) == Some(F::Body) && s.get(at, F::Body).is_some_and(|n| !s.is(n, K::Block))) { return false; }
    loop {
        if matches!(s.kind(at), Some(K::Function | K::Arrow | K::Method)) && s.get(at, F::ReturnType).is_some() { return true; }
        if matches!(s.kind(at), Some(K::Declarator | K::ClassProp)) { return s.get(at, F::Type).is_some(); }
        if s.is(at, K::ExprStmt) { return false; }
        let Some(next) = s.parent(at) else { return false };
        at = next;
    }
}

pub(super) fn returns(s: &Syntax, node: u32, result: &mut Vec<Option<u32>>) {
    if s.is(node, K::Return) { result.push(s.get(node, F::Arg)); }
    else if !matches!(s.kind(node), Some(K::Function | K::Arrow | K::Method | K::Class)) {
        for (_, child) in s.kids(node) { returns(s, child, result); }
    }
}

pub(super) fn function_return_style(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let relation = parent(s, node);
    let at = relation.map(|(_, parent)| parent);
    let declaration = s.is(node, K::Function) && at.is_some_and(|p| matches!(s.kind(p), Some(K::Program | K::Block | K::Export | K::Namespace)));
    let expression = !declaration;
    let name = s.word(node, F::Name).or_else(|| {
        if s.is(node, K::Method) {
            return s.get(node, F::Key).filter(|&n| s.is(n, K::Ident)).map(|n| s.text(n));
        }
        let parent = at?;
        let field = if s.is(parent, K::Declarator) { F::Name } else { F::Key };
        s.get(parent, field).filter(|&n| s.is(n, K::Ident)).map(|n| s.text(n))
    }).unwrap_or("");
    let context_typed = expression && relation.is_some_and(|(child, parent)| typed_parent(s, child, parent) || typed_object(s, parent))
        || s.is(node, K::Method) && typed_object(s, node);
    let ancestor_typed = expression && ancestor_return_type(s, node);
    let typed = context_typed || ancestor_typed;
    let free_expression = expression && at.is_some_and(|p| !matches!(s.kind(p), Some(K::Declarator | K::ClassProp | K::Export | K::Class)));
    let body = s.get(node, F::Body).map(|n| s.unparen(n));
    let mut return_values = Vec::new();
    if let Some(body) = body { returns(s, body, &mut return_values); }
    let higher_order = s.is(node, K::Arrow) && body.is_some_and(|n| matches!(s.kind(n), Some(K::Arrow | K::Function)))
        || !return_values.is_empty() && return_values.iter().all(|n| n.is_some_and(|n| matches!(s.kind(s.unparen(n)), Some(K::Function | K::Arrow))));
    let const_assertion = s.is(node, K::Arrow) && body.is_some_and(|mut body| {
        while s.is(body, K::Satisfies) {
            let Some(next) = s.get(body, F::Expr) else { return false };
            body = s.unparen(next);
        }
        matches!(s.kind(body), Some(K::As | K::TypeAssertion)) && s.word(body, F::Type) == Some("const")
    });
    let offset = if s.is(node, K::Method) {
        super::class_styles::member_head(s, node)
    } else if let Some(owner) = at.filter(|&p| matches!(s.kind(p), Some(K::ClassProp | K::Prop))) {
        super::class_styles::member_head(s, owner)
    } else if s.is(node, K::Arrow) {
        s.get(node, F::Op).map_or(s.tree.node(node).start, |n| s.tree.node(n).start)
    } else { s.tree.node(node).start };
    Value::record(vec![
        ("name".into(), Value::from(name.to_string())),
        ("offset".into(), Value::from(offset as usize)),
        ("valid".into(), Value::Bool(s.get(node, F::ReturnType).is_some() || s.is(node, K::Method) && (s.word(node, F::Key) == Some("constructor") || s.word(node, F::Kind) == Some("set")))),
        ("typed".into(), Value::Bool(typed)),
        ("context_typed".into(), Value::Bool(context_typed)),
        ("ancestor_typed".into(), Value::Bool(ancestor_typed)),
        ("declaration".into(), Value::Bool(declaration)),
        ("free_expression".into(), Value::Bool(free_expression)),
        ("higher_order".into(), Value::Bool(higher_order)),
        ("const_assertion".into(), Value::Bool(const_assertion)),
        ("iife".into(), Value::Bool(at.is_some_and(|p| s.is(p, K::Call)))),
        ("void_arrow".into(), Value::Bool(s.is(node, K::Arrow) && body.is_some_and(|n| s.is(n, K::Unary) && s.word(n, F::Op) == Some("void")))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn ordinary_declarations_need_return_types() {
        let linter = native_linter("rule explicit-function-return-type");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "function f() { return 1; }").len(), 1);
    }

    #[test]
    fn typed_callbacks_and_returned_functions_inherit_context() {
        let linter = native_linter("rule explicit-function-return-type\nrule explicit-function-return-type:arrow\nrule explicit-function-return-type:method");
        assert!(linter.lint(Path::new("/root/file.ts"), "const f: () => () => number = () => () => 1; consume(() => 1); class A { constructor() {} set x(value: number) {} }").is_empty());
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "const f = () => () => 1;").len(), 1);
        assert!(linter.lint(Path::new("/root/file.ts"), "class A { handlers: Handler[] = [{ handle: x => x }]; } function f(): Handler { return { handle: x => x }; }").is_empty());
    }
}

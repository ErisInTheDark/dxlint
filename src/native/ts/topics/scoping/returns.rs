//! Return contexts for callback and function policies.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::types::Ty;

pub(super) fn consistent_returns(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(body) = (if s.is(node, K::Program) { Some(node) } else { s.get(node, F::Body) }) else { return Value::Null };
    let mut stack = vec![body];
    let mut returns = Vec::new();
    while let Some(n) = stack.pop() {
        match s.kind(n) {
            Some(K::Function | K::Arrow | K::Method | K::Class | K::StaticBlock) => continue,
            Some(K::Return) => returns.push(n),
            _ => stack.extend(s.kids(n).map(|(_, c)| c)),
        }
    }
    returns.sort_by_key(|&n| s.tree.node(n).start);
    if returns.is_empty() { return Value::Null; }
    let ts = matches!(file.path.extension().and_then(|e| e.to_str()), Some("ts" | "tsx" | "mts" | "cts"));
    if ts && returns.iter().any(|&n| s.get(n, F::Arg).is_none()) && return_void(file, node) {
        returns.retain(|&n| s.get(n, F::Arg).is_some());
    }
    let Some(first) = returns.first().copied() else { return Value::Null };
    let name = file.function_name_with_kind(node).unwrap_or_else(|| "program".to_string());
    let mut capitalized = name.clone();
    if let Some(c) = name.chars().next() {
        capitalized.replace_range(..c.len_utf8(), &c.to_uppercase().to_string());
    }
    let flags = |n| {
        let argument = s.get(n, F::Arg).map(|n| s.unparen(n));
        let undefined = argument.is_some_and(|n| s.is(n, K::Ident) && s.text(n) == "undefined"
            || s.is(n, K::Unary) && s.word(n, F::Op) == Some("void")
            || ts && matches!(file.type_of(n), Ty::Undefined));
        (argument.is_some(), undefined)
    };
    let (specified, undefined) = flags(first);
    let own_name = s.get(node, F::Name).map(|n| s.text(n)).unwrap_or("");
    let constructor = name == "constructor" || own_name.chars().next().is_some_and(char::is_uppercase);
    let head = match s.kind(node) {
        Some(K::Arrow) => s.get(node, F::Op),
        Some(K::Method) => s.get(node, F::Key),
        Some(K::Function) => s.get(node, F::Name),
        _ => None,
    };
    let offset = head.map(|n| s.tree.node(n).start as usize).unwrap_or(s.tree.node(node).start as usize);
    Value::record(vec![
        ("name".into(), Value::from(name)),
        ("capitalized".into(), Value::from(capitalized)),
        ("specified".into(), Value::Bool(specified)),
        ("undefined".into(), Value::Bool(undefined)),
        ("falls".into(), Value::Bool(!constructor && file.reach().0.falls_off.get(&node).copied().unwrap_or(false))),
        ("offset".into(), Value::from(offset)),
        ("returns".into(), Value::list(returns.into_iter().skip(1).map(|n| {
            let (specified, undefined) = flags(n);
            Value::record(vec![
                ("specified".into(), Value::Bool(specified)),
                ("undefined".into(), Value::Bool(undefined)),
                ("offset".into(), Value::from(s.tree.node(n).start as usize)),
            ])
        }).collect())),
    ])
}

fn return_void(file: &NativeFile, node: u32) -> bool {
    // A return statement's body is a block; only expression-bodied arrows
    // have an inferred callable return here, so an annotation is required.
    let Some(annotation) = file.syntax.get(node, F::ReturnType) else { return false; };
    let typing = file.typing();
    let is_async = file.syntax.has_word(node, F::Modifier, "async");
    let ty = typing.checker.eval(&crate::native::ts::summary::type_syn(&file.syntax, annotation), &file.env_at(node));
    contains_void(file, &ty, is_async)
}

fn contains_void(file: &NativeFile, ty: &Ty, promise: bool) -> bool {
    match ty {
        Ty::Void => true,
        Ty::Union(parts) => parts.iter().any(|t| contains_void(file, t, promise)),
        Ty::Named(sym, args) if promise && matches!(file.typing().checker.decl_name(sym).as_deref(), Some("Promise" | "PromiseLike")) => {
            args.first().is_some_and(|t| contains_void(file, t, true))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    #[test]
    fn bare_returns_follow_resolved_return_annotations() {
        for (annotation, asynchronous, bare_kept) in [
            ("V", false, false),
            ("V | number", false, false),
            ("Promise<V>", true, false),
            ("PromiseLike<V>", true, false),
            ("Promise<V>", false, true),
            ("number", false, true),
            ("", false, true),
        ] {
            let source = format!(
                "interface Promise<T> {{}} interface PromiseLike<T> {{}} type V = void; {}function f(x: boolean){} {{ if (x) return; return 1; }}",
                if asynchronous { "async " } else { "" },
                if annotation.is_empty() { String::new() } else { format!(": {annotation}") },
            );
            with_native_file(&source, |file, _| {
                let node = (0..file.syntax.tree.nodes.len() as u32).find(|&n| file.syntax.is(n, K::Function)).unwrap();
                let result = consistent_returns(file, node);
                assert_eq!(result.key("specified"), Value::Bool(!bare_kept), "{annotation}");
                assert_eq!(result.key("returns").item_slice().len(), usize::from(bare_kept), "{annotation}");
            });
        }
    }

    #[test]
    fn value_returns_keep_inferred_undefined_and_nested_function_boundaries() {
        let source = "declare const empty: undefined; function f(x: boolean) { function nested() { return 1; } if (x) return empty; return void nested(); }";
        with_native_file(source, |file, _| {
            let node = (0..file.syntax.tree.nodes.len() as u32).find(|&n|
                file.syntax.is(n, K::Function) && file.syntax.get(n, F::Name).is_some_and(|name| file.syntax.text(name) == "f")
            ).unwrap();
            let result = consistent_returns(file, node);
            assert_eq!(result.key("specified"), Value::Bool(true));
            assert_eq!(result.key("undefined"), Value::Bool(true));
            let rest = result.key("returns");
            let [returned] = rest.item_slice() else { panic!("outer returns only") };
            assert_eq!(returned.key("specified"), Value::Bool(true));
            assert_eq!(returned.key("undefined"), Value::Bool(true));
        });
    }
}

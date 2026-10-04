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
    let ts = matches!(file.path.extension().and_then(|e| e.to_str()), Some("ts" | "tsx" | "mts" | "cts"));
    if ts && return_void(file, node) {
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
    let typing = file.typing();
    let is_async = file.syntax.has_word(node, F::Modifier, "async");
    let is_void = |t: &Ty| contains_void(file, t, is_async);
    if let Some(annotation) = file.syntax.get(node, F::ReturnType) {
        let ty = typing.checker.eval(&crate::native::ts::summary::type_syn(&file.syntax, annotation), &file.env_at(node));
        if is_void(&ty) { return true; }
    }
    let Ty::Fn(callable) = file.type_of(node) else { return false };
    callable.ret.as_ref().is_some_and(is_void)
        || callable.sigs.iter().any(|sig| sig.ret.as_ref().is_some_and(|syn| is_void(&typing.checker.eval(syn, &callable.env))))
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

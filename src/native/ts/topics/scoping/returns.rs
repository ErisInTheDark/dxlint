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
    Value::Record(vec![
        ("name".into(), Value::from(name)),
        ("capitalized".into(), Value::from(capitalized)),
        ("specified".into(), Value::Bool(specified)),
        ("undefined".into(), Value::Bool(undefined)),
        ("falls".into(), Value::Bool(!constructor && file.reach().0.falls_off.get(&node).copied().unwrap_or(false))),
        ("offset".into(), Value::from(offset)),
        ("returns".into(), Value::List(returns.into_iter().skip(1).map(|n| {
            let (specified, undefined) = flags(n);
            Value::Record(vec![
                ("specified".into(), Value::Bool(specified)),
                ("undefined".into(), Value::Bool(undefined)),
                ("offset".into(), Value::from(s.tree.node(n).start as usize)),
            ])
        }).collect())),
    ])
}

pub(super) fn useless_return(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.get(node, F::Arg).is_some() || file.reach().0.unreachable.contains(&node) {
        return Value::Bool(false);
    }
    let mut child = node;
    while let Some(parent) = s.parent(child) {
        match s.kind(parent) {
            Some(K::Function | K::Arrow | K::Method) => return Value::Bool(true),
            Some(K::For | K::ForIn | K::ForOf | K::While | K::DoWhile) => return Value::Bool(false),
            Some(K::Try) if s.field(child) == Some(F::Finalizer) => return Value::Bool(false),
            Some(K::Block | K::Case) => {
                let mut past = false;
                for sibling in s.all(parent, F::Body) {
                    if sibling == child { past = true; continue; }
                    if past && meaningful_statement(s, sibling) {
                        return Value::Bool(false);
                    }
                }
                if s.is(parent, K::Case) {
                    let breaks = s.all(parent, F::Body).any(|n| s.is(n, K::Break));
                    let last_case = s.parent(parent).and_then(|n| s.all(n, F::Case).last()) == Some(parent);
                    if !breaks && !last_case { return Value::Bool(false); }
                }
            }
            _ => {}
        }
        child = parent;
    }
    Value::Bool(false)
}

fn meaningful_statement(s: &crate::native::ts::syntax::Syntax, node: u32) -> bool {
    match s.kind(node) {
        Some(K::Function | K::Break) => false,
        Some(K::Return) if s.get(node, F::Arg).is_none() => false,
        Some(K::Block) => s.all(node, F::Body).any(|n| meaningful_statement(s, n)),
        _ => true,
    }
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

pub(super) fn executor_return(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let arrow = s.is(node, K::Arrow);
    let expression = if arrow {
        s.get(node, F::Body).filter(|&n| !s.is(n, K::Block))
    } else {
        s.get(node, F::Arg)
    };
    let Some(expression) = expression else { return Value::Null };
    let mut function = node;
    if !arrow {
        while !matches!(s.kind(function), Some(K::Function | K::Arrow | K::Method)) {
            let Some(parent) = s.parent(function) else { return Value::Null };
            function = parent;
        }
    }
    if !matches!(s.kind(function), Some(K::Function | K::Arrow)) {
        return Value::Null;
    }
    let mut argument = function;
    while let Some(parent) = s.parent(argument).filter(|&n| s.is(n, K::Paren)) {
        argument = parent;
    }
    let Some(new) = s.parent(argument).filter(|&n| s.is(n, K::New) && s.get(n, F::Arg) == Some(argument)) else { return Value::Null };
    let Some(callee) = s.get(new, F::Callee).map(|n| s.unparen(n)).filter(|&n| s.is(n, K::Ident) && s.text(n) == "Promise") else { return Value::Null };
    if !file.bindings().unresolved.contains(&callee) || file.globals().setting("Promise") == Some("off") {
        return Value::Null;
    }
    let expression = s.unparen(expression);
    Value::Record(vec![
        ("void".into(), Value::Bool(s.is(expression, K::Unary) && s.word(expression, F::Op) == Some("void"))),
        ("offset".into(), Value::from(s.tree.node(if arrow { expression } else { node }).start as usize)),
    ])
}

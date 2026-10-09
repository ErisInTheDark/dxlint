//! Callback destinations and their own return paths.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn array_callback(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.has_word(node, F::Modifier, "*") { return Value::Null; }
    let Some(method) = array_destination(file, node) else { return Value::Null };
    let Some(body) = s.get(node, F::Body) else { return Value::Null };
    let concise = !s.is(body, K::Block);
    let mut stack = if concise { Vec::new() } else { vec![body] };
    let mut returns = Vec::new();
    while let Some(n) = stack.pop() {
        match s.kind(n) {
            Some(K::Function | K::Arrow | K::Method | K::Class | K::StaticBlock) => continue,
            Some(K::Return) => returns.push(n),
            _ => stack.extend(s.kids(n).map(|(_, c)| c)),
        }
    }
    returns.sort_by_key(|&n| s.tree.node(n).start);
    let void = |n| {
        let n = s.unparen(n);
        s.is(n, K::Unary) && s.word(n, F::Op) == Some("void")
    };
    let full = if matches!(method.as_str(), "from" | "fromAsync") {
        format!("Array.{method}")
    } else { format!("Array.prototype.{method}") };
    Value::record(vec![
        ("method".into(), Value::from(full)),
        ("foreach".into(), Value::Bool(method == "forEach")),
        ("name".into(), Value::from(file.function_name_with_kind(node).unwrap_or_else(|| "function".into()))),
        ("offset".into(), Value::from(file.function_head(node).unwrap_or(s.tree.node(node).start as usize))),
        ("concise".into(), Value::Bool(concise)),
        ("void".into(), Value::Bool(concise && void(body))),
        ("has_return".into(), Value::Bool(!returns.is_empty())),
        ("falls".into(), Value::Bool(!concise && file.reach().0.falls_off.get(&node).copied().unwrap_or(false))),
        ("returns".into(), Value::list(returns.into_iter().map(|n| {
            let arg = s.get(n, F::Arg);
            Value::record(vec![
                ("value".into(), Value::Bool(arg.is_some())),
                ("void".into(), Value::Bool(arg.is_some_and(void))),
                ("offset".into(), Value::from(s.tree.node(n).start as usize)),
            ])
        }).collect())),
    ])
}

fn array_destination(file: &NativeFile, function: u32) -> Option<String> {
    let s = &file.syntax;
    let mut current = function;
    loop {
        let parent = s.parent(current)?;
        match s.kind(parent)? {
            K::Paren | K::Logical | K::Conditional => current = parent,
            K::Return => {
                let mut owner = parent;
                loop {
                    owner = s.parent(owner)?;
                    if matches!(s.kind(owner), Some(K::Function | K::Arrow | K::Method)) { break; }
                }
                while let Some(p) = s.parent(owner).filter(|&n| s.is(n, K::Paren)) { owner = p; }
                let call = s.parent(owner)?;
                if !s.is(call, K::Call) || s.get(call, F::Callee) != Some(owner) { return None; }
                current = call;
            }
            K::Call => {
                let callee = s.unparen(s.get(parent, F::Callee)?);
                let method = crate::native::ts::statics::property_name(s, callee)?;
                let factory = array_factory(file, callee);
                let index = usize::from(factory);
                if s.all(parent, F::Arg).nth(index) != Some(current) { return None; }
                let asynchronous = s.has_word(function, F::Modifier, "async");
                if factory && (method == "fromAsync" || !asynchronous) { return Some(method); }
                if !asynchronous && matches!(method.as_str(), "every" | "filter" | "find" | "findLast" | "findIndex" | "findLastIndex" | "flatMap" | "forEach" | "map" | "reduce" | "reduceRight" | "some" | "sort" | "toSorted") {
                    return Some(method);
                }
                return None;
            }
            _ => return None,
        }
    }
}

pub(super) fn array_factory(file: &NativeFile, callee: u32) -> bool {
    let s = &file.syntax;
    let callee = s.unparen(callee);
    let method = crate::native::ts::statics::property_name(s, callee);
    s.get(callee, F::Object).is_some_and(|n| {
        let n = s.unparen(n);
        s.is(n, K::Ident) && match method.as_deref() {
            Some("from") => s.text(n).ends_with("Array"),
            Some("fromAsync") => s.text(n) == "Array",
            _ => false,
        }
    })
}

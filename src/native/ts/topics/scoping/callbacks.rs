//! Callback destinations and their own return paths.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn arrow_callback(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.has_word(node, F::Modifier, "*") { return Value::Null; }
    let binding = s.get(node, F::Name).and_then(|id| file.bindings().decls.get(&id));
    if binding.is_some_and(|b| b.uses + b.self_uses + b.writes > 0) { return Value::Null; }
    let flags = file.bindings().functions.get(&node).copied().unwrap_or(0);
    if flags & crate::native::ts::bind::USES_ARGUMENTS != 0 { return Value::Null; }
    let mut stack: Vec<_> = s.kids(node).map(|(_, n)| n).collect();
    while let Some(n) = stack.pop() {
        if matches!(s.kind(n), Some(K::Function | K::Method)) { continue; }
        if s.is(n, K::Super) || s.is(n, K::MetaProperty) && s.text(n).starts_with("new") { return Value::Null; }
        stack.extend(s.kids(n).map(|(_, c)| c));
    }
    let mut current = node;
    let mut bound = false;
    let mut lexical = false;
    loop {
        let Some(parent) = s.parent(current) else { return Value::Null };
        match s.kind(parent) {
            Some(K::Paren | K::Logical) => current = parent,
            Some(K::Conditional) if s.field(current) != Some(F::Test) => current = parent,
            Some(K::Member) if s.get(parent, F::Object) == Some(current) && s.word(parent, F::Property) == Some("bind") => {
                let mut member = parent;
                while let Some(p) = s.parent(member).filter(|&n| s.is(n, K::Paren)) { member = p; }
                let Some(call) = s.parent(member).filter(|&n| s.is(n, K::Call) && s.get(n, F::Callee) == Some(member)) else { return Value::Null };
                if !bound {
                    bound = true;
                    let args: Vec<_> = s.all(call, F::Arg).collect();
                    lexical = args.len() == 1 && s.is(s.unparen(args[0]), K::This);
                }
                current = call;
            }
            Some(K::Call | K::New) if s.field(current) == Some(F::Arg) => {
                return Value::Record(vec![
                    ("named".into(), Value::Bool(s.get(node, F::Name).is_some())),
                    ("this".into(), Value::Bool(flags & crate::native::ts::bind::USES_THIS != 0)),
                    ("lexical".into(), Value::Bool(lexical)),
                ]);
            }
            _ => return Value::Null,
        }
    }
}

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
    Value::Record(vec![
        ("method".into(), Value::from(full)),
        ("foreach".into(), Value::Bool(method == "forEach")),
        ("name".into(), Value::from(file.function_name_with_kind(node).unwrap_or_else(|| "function".into()))),
        ("offset".into(), Value::from(file.function_head(node).unwrap_or(s.tree.node(node).start as usize))),
        ("concise".into(), Value::Bool(concise)),
        ("void".into(), Value::Bool(concise && void(body))),
        ("has_return".into(), Value::Bool(!returns.is_empty())),
        ("falls".into(), Value::Bool(!concise && file.reach().0.falls_off.get(&node).copied().unwrap_or(false))),
        ("returns".into(), Value::List(returns.into_iter().map(|n| {
            let arg = s.get(n, F::Arg);
            Value::Record(vec![
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn callback_returns_belong_to_their_own_function() {
        let l = native_linter("rule array-callback-return");
        let path = Path::new("/root/a.mjs");
        assert_eq!(l.lint(path, "items.map(() => { function nested() { return 1; } });").len(), 1);
        assert!(l.lint(path, "items.map(() => { if (flag) return 1; throw Error(); });").is_empty());
        assert_eq!(l.lint(path, "items.map((() => { return () => {}; })());").len(), 1);
    }

    #[test]
    fn arrow_candidates_preserve_arguments_recursion_and_bound_this() {
        let l = native_linter("rule prefer-arrow-callback");
        let path = Path::new("/root/a.mjs");
        assert!(l.lint(path, "consume(function() { return arguments[0]; });").is_empty());
        assert!(l.lint(path, "consume(function again() { again(); });").is_empty());
        assert!(l.lint(path, "consume(function() { return this; });").is_empty());
        assert_eq!(l.lint(path, "consume(function() { return this; }.bind(this));").len(), 1);
        assert_eq!(l.lint(path, "consume(function(arguments) { return arguments; });").len(), 1);
    }
}

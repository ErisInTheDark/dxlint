//! Strict receiver validity follows lexical arrows and function destinations.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn context(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut child = node;
    let mut valid = false;
    let mut capital = false;
    while let Some(parent) = s.parent(child) {
        match s.kind(parent) {
            Some(K::StaticBlock) => { valid = true; break; }
            Some(K::Method) if matches!(s.field(child), Some(F::Body | F::Param)) => { valid = true; break; }
            Some(K::ClassProp) if s.field(child) == Some(F::Init) => { valid = true; break; }
            Some(K::Function) => {
                valid = !crate::native::ts::directives::strict_code(file, parent)
                    || s.all(parent, F::Param).any(|p| s.get(p, F::Name).is_some_and(|n| s.text(n) == "this"))
                    || documented_receiver(file, parent)
                    || assigned_receiver(file, parent, false);
                capital = assigned_receiver(file, parent, true);
                break;
            }
            Some(K::Program) => {
                valid = file.source_type() == "script" || file.source_type() == "commonjs" && !crate::native::ts::directives::strict_code(file, parent);
                break;
            }
            _ => {}
        }
        child = parent;
    }
    Value::record(vec![("valid".into(), Value::Bool(valid)), ("constructor".into(), Value::Bool(capital))])
}

fn documented_receiver(file: &NativeFile, function: u32) -> bool {
    let s = &file.syntax;
    let mut owner = function;
    loop {
        let start = s.tree.node(owner).start as usize;
        if file.comments().iter().any(|c| {
            c.end as usize <= start && s.src[c.end as usize..start].trim().is_empty()
                && c.body(s.src).lines().any(|line| line.trim_start_matches(|c: char| c.is_whitespace() || c == '*').starts_with("@this"))
        }) { return true; }
        let Some(parent) = s.parent(owner) else { break };
        if matches!(s.kind(parent), Some(K::Function | K::Arrow | K::Method | K::Block | K::Program | K::Call | K::New)) { break; }
        owner = parent;
    }
    false
}

fn immediate_call(file: &NativeFile, node: u32) -> Option<u32> {
    let s = &file.syntax;
    let mut callee = node;
    while let Some(parent) = s.parent(callee).filter(|&n| s.is(n, K::Paren)) { callee = parent; }
    s.parent(callee).filter(|&n| s.is(n, K::Call) && s.get(n, F::Callee) == Some(callee))
}

fn assigned_receiver(file: &NativeFile, function: u32, cap: bool) -> bool {
    let s = &file.syntax;
    let uppercase = |n| s.is(n, K::Ident) && s.text(n).chars().next().is_some_and(char::is_uppercase);
    let name = s.get(function, F::Name);
    if cap && name.is_some_and(uppercase) { return true; }
    let mut current = function;
    loop {
        let Some(parent) = s.parent(current) else { return false };
        match s.kind(parent) {
            Some(K::Paren | K::Logical | K::Conditional) => current = parent,
            Some(K::Return) => {
                let mut owner = parent;
                loop {
                    let Some(p) = s.parent(owner) else { return false };
                    owner = p;
                    if matches!(s.kind(owner), Some(K::Function | K::Arrow | K::Method)) { break; }
                }
                let Some(call) = immediate_call(file, owner) else { return false };
                current = call;
            }
            Some(K::Arrow) if s.field(current) == Some(F::Body) => {
                let Some(call) = immediate_call(file, parent) else { return false };
                current = call;
            }
            Some(K::Prop | K::ClassProp) => return !cap && matches!(s.field(current), Some(F::Value | F::Init)),
            Some(K::Assign | K::AssignPattern) => {
                let Some(left) = s.get(parent, F::Left) else { return false };
                return if cap { name.is_none() && uppercase(left) } else { matches!(s.kind(left), Some(K::Member | K::Index)) };
            }
            Some(K::Declarator | K::Param) => return cap && name.is_none() && s.field(current) == Some(F::Init)
                && s.get(parent, F::Name).is_some_and(uppercase),
            Some(K::Member | K::Index) if s.field(current) == Some(F::Object) => {
                let method = crate::native::ts::statics::property_name(s, parent);
                let call = immediate_call(file, parent);
                return !cap && matches!(method.as_deref(), Some("bind" | "call" | "apply"))
                    && call.and_then(|n| s.get(n, F::Arg)).is_some_and(|n| !nullish(file, n));
            }
            Some(K::Call) => {
                if cap { return false; }
                let Some(callee) = s.get(parent, F::Callee) else { return false };
                let method = crate::native::ts::statics::property_name(s, callee);
                let object = s.get(s.unparen(callee), F::Object).map(|n| s.text(s.unparen(n)));
                let args: Vec<_> = s.all(parent, F::Arg).collect();
                let index = match (object, method.as_deref()) {
                    (Some("Reflect"), Some("apply")) if args.len() == 3 && args[0] == current => Some(1),
                    (_, Some("from" | "fromAsync")) if super::callbacks::array_factory(file, callee) && args.len() == 3 && args[1] == current => Some(2),
                    (_, Some("every" | "filter" | "find" | "findIndex" | "findLast" | "findLastIndex" | "flatMap" | "forEach" | "map" | "some")) if args.len() == 2 && args[0] == current => Some(1),
                    _ => None,
                };
                return index.is_some_and(|i| !nullish(file, args[i]));
            }
            _ => return false,
        }
    }
}

fn nullish(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    s.is(node, K::Null) || s.is(node, K::Ident) && s.text(node) == "undefined"
        || s.is(node, K::Unary) && s.word(node, F::Op) == Some("void")
}

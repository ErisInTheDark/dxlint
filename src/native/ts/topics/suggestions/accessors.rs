//! Getter/setter identity, pairing and property descriptors.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics::property_name;
use crate::expr::Value;

fn equal_keys(file: &NativeFile, left: u32, right: u32) -> bool {
    let s = &file.syntax;
    match (property_name(s, left), property_name(s, right)) {
        (Some(a), Some(b)) => a == b,
        (None, None) => s.get(left, F::Key).zip(s.get(right, F::Key)).is_some_and(|(a, b)| {
            let key = |n| s.get(n, F::Expr).unwrap_or(n);
            file.equal_tokens(key(a), key(b))
        }),
        _ => false,
    }
}

fn display_name(file: &NativeFile, node: u32) -> String {
    file.function_name_with_kind(node).unwrap_or_else(|| {
        let kind = if file.syntax.word(node, F::Kind) == Some("get") { "getter" } else { "setter" };
        property_name(&file.syntax, node).map_or_else(|| kind.to_string(), |name| format!("{kind} '{name}'"))
    })
}

pub fn pair(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(kind @ ("get" | "set")) = s.word(node, F::Kind) else { return Value::Null };
    let Some(parent) = s.parent(node) else { return Value::Null };
    let (field, scope) = match s.kind(parent) {
        Some(K::Object) => (F::Property, "object"),
        Some(K::Class) => (F::Member, "class"),
        Some(K::Interface | K::TypeLit) => (F::Member, "type"),
        _ => return Value::Null,
    };
    let members: Vec<_> = s.all(parent, field).collect();
    let is_static = s.has_word(node, F::Modifier, "static");
    let mut getters = Vec::new();
    let mut setters = Vec::new();
    for (index, &member) in members.iter().enumerate() {
        if s.has_word(member, F::Modifier, "static") != is_static || !equal_keys(file, node, member) { continue; }
        match s.word(member, F::Kind) {
            Some("get") => getters.push((index, member)),
            Some("set") => setters.push((index, member)),
            _ => {}
        }
    }
    let paired = if kind == "get" { !setters.is_empty() } else { !getters.is_empty() };
    let grouping = match (getters.as_slice(), setters.as_slice()) {
        ([(gi, getter)], [(si, setter)]) => {
            let (former_index, former, latter_index, latter) = if gi < si { (*gi, *getter, *si, *setter) } else { (*si, *setter, *gi, *getter) };
            if latter == node {
                Value::Record(vec![
                    ("former".into(), Value::from(display_name(file, former))),
                    ("gap".into(), Value::Bool(latter_index - former_index > 1)),
                    ("getter_first".into(), Value::Bool(gi < si)),
                ])
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    };
    Value::Record(vec![
        ("kind".into(), Value::from(kind.to_string())),
        ("scope".into(), Value::from(scope)),
        ("paired".into(), Value::Bool(paired)),
        ("name".into(), Value::from(display_name(file, node))),
        ("grouping".into(), grouping),
        ("offset".into(), Value::Num(s.tree.node(node).start.into())),
    ])
}

fn argument_of(file: &NativeFile, mut node: u32, object: &str, method: &str, index: usize) -> bool {
    let s = &file.syntax;
    while let Some(parent) = s.parent(node).filter(|&p| s.is(p, K::Paren)) {
        node = parent;
    }
    let Some(call) = s.parent(node).filter(|&p| s.is(p, K::Call) && s.all(p, F::Arg).nth(index) == Some(node)) else { return false };
    let Some(callee) = s.get(call, F::Callee).map(|n| s.unparen(n)) else { return false };
    property_name(s, callee).as_deref() == Some(method)
        && s.get(callee, F::Object).map(|n| s.unparen(n))
            .is_some_and(|n| s.is(n, K::Ident) && s.text(n) == object && file.resolved(n) == Some("global"))
}

pub fn descriptor(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.is(node, K::Object) { return Value::Null }
    let direct = argument_of(file, node, "Object", "defineProperty", 2) || argument_of(file, node, "Reflect", "defineProperty", 2);
    let nested = s.parent(node).filter(|&p| s.is(p, K::Prop) && s.get(p, F::Value) == Some(node))
        .and_then(|p| s.parent(p))
        .is_some_and(|object| argument_of(file, object, "Object", "create", 1) || argument_of(file, object, "Object", "defineProperties", 1));
    if !direct && !nested { return Value::Null }
    let names: Vec<_> = s.all(node, F::Property)
        .filter(|&p| s.is(p, K::Prop))
        .filter_map(|p| s.get(p, F::Key))
        .filter(|&key| s.is(key, K::Ident))
        .map(|key| s.text(key))
        .collect();
    Value::Record(vec![
        ("get".into(), Value::Bool(names.contains(&"get"))),
        ("set".into(), Value::Bool(names.contains(&"set"))),
    ])
}

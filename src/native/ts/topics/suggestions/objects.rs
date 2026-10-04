//! Object literal and property-operation preferences.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics::property_name;
use crate::expr::Value;

pub fn own_property_call(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let found = (|| {
        let callee = s.unparen(s.get(node, F::Callee)?);
        if !matches!(s.kind(callee), Some(K::Member | K::Index)) || property_name(s, callee).as_deref() != Some("call") { return None }
        let method = s.unparen(s.get(callee, F::Object)?);
        if !matches!(s.kind(method), Some(K::Member | K::Index)) || property_name(s, method).as_deref() != Some("hasOwnProperty") { return None }
        let mut object = s.unparen(s.get(method, F::Object)?);
        let empty = s.is(object, K::Object) && s.all(object, F::Property).next().is_none();
        if matches!(s.kind(object), Some(K::Member | K::Index)) && property_name(s, object).as_deref() == Some("prototype") {
            object = s.unparen(s.get(object, F::Object)?);
        }
        let builtin = s.is(object, K::Ident) && s.text(object) == "Object";
        if !(empty || builtin) || file.bindings().visible(s, node, "Object", true).is_some()
            || !matches!(file.globals().setting("Object"), Some("readonly" | "writable")) { return None }
        Some(())
    })();
    Value::Bool(found.is_some())
}

struct Member {
    name: Option<String>,
    concise: bool,
    method: bool,
    function: bool,
    arrow: bool,
    quoted: bool,
    redundant: bool,
    constructor: bool,
    annotated: bool,
    proto: bool,
    computed: bool,
}

fn lexical_arrow(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let mut stack: Vec<_> = s.kids(node).map(|(_, n)| n).collect();
    while let Some(n) = stack.pop() {
        match s.kind(n) {
            Some(K::Function | K::Method | K::Class) => continue,
            Some(K::This | K::Super) => return true,
            Some(K::MetaProperty) if s.text(n) == "new.target" => return true,
            Some(K::Ident) if s.text(n) == "arguments" && file.resolved(n) == Some("global") => return true,
            _ => {}
        }
        stack.extend(s.kids(n).map(|(_, child)| child));
    }
    false
}

fn member(file: &NativeFile, node: u32) -> Option<Member> {
    let s = &file.syntax;
    let parent = s.parent(node)?;
    if !s.is(parent, K::Object) || file.write_site(parent).is_some()
        || !matches!(s.kind(node), Some(K::Prop | K::Method))
        || matches!(s.word(node, F::Kind), Some("get" | "set")) {
        return None;
    }
    let key = s.get(node, F::Key)?;
    let name = property_name(s, node);
    let proto = !s.is(key, K::Computed) && name.as_deref() == Some("__proto__");
    let value = s.get(node, F::Value).map(|n| s.unparen(n));
    let method = s.is(node, K::Method);
    let concise = method || value.is_none();
    let function = value.is_some_and(|n| s.is(n, K::Function) && s.get(n, F::Name).is_none());
    let arrow = value.is_some_and(|n| s.is(n, K::Arrow) && s.get(n, F::Body).is_some_and(|b| s.is(b, K::Block)) && !lexical_arrow(file, n));
    let redundant = !proto && (function || value.is_some_and(|n| s.is(n, K::Ident) && name.as_deref() == Some(s.text(n))));
    let constructor = name.as_deref().and_then(|name| name.chars().find(|c| !matches!(c, '_' | '$') && !c.is_ascii_digit()))
        .is_some_and(|c| c.is_alphabetic() && c.is_uppercase());
    let span = s.tree.node(node);
    let annotated = file.comments().iter().any(|comment| comment.block && comment.start >= span.start && comment.end <= span.end
        && comment.body(s.src).trim_start().starts_with('*') && comment.body(s.src).contains("@type"));
    let quoted_key = s.get(key, F::Expr).unwrap_or(key);
    Some(Member { name, concise, method, function, arrow, quoted: s.is(quoted_key, K::String), redundant, constructor, annotated, proto, computed: s.is(key, K::Computed) })
}

pub fn shorthand(file: &NativeFile, node: u32) -> Value<'static> {
    let Some(m) = member(file, node) else { return Value::Null };
    Value::record(vec![
        ("name".into(), m.name.map_or(Value::Null, Value::from)),
        ("concise".into(), Value::Bool(m.concise)),
        ("method".into(), Value::Bool(m.method)),
        ("function".into(), Value::Bool(m.function)),
        ("arrow".into(), Value::Bool(m.arrow)),
        ("quoted".into(), Value::Bool(m.quoted)),
        ("redundant".into(), Value::Bool(m.redundant)),
        ("constructor".into(), Value::Bool(m.constructor)),
        ("annotated".into(), Value::Bool(m.annotated)),
        ("proto".into(), Value::Bool(m.proto)),
        ("computed".into(), Value::Bool(m.computed)),
    ])
}

pub fn consistency(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.is(node, K::Object) || file.write_site(node).is_some() { return Value::Null }
    let members: Vec<_> = s.all(node, F::Property).filter_map(|n| member(file, n)).collect();
    let concise = members.iter().filter(|m| m.concise).count();
    if concise > 0 && concise < members.len() {
        Value::from("mixed")
    } else if concise == 0 && !members.is_empty() && members.iter().all(|m| m.redundant) {
        Value::from("redundant")
    } else {
        Value::Null
    }
}

pub fn computed_key(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(key) = s.get(node, F::Key).filter(|&n| s.is(n, K::Computed))
        .and_then(|n| s.get(n, F::Expr)).map(|n| s.unparen(n)) else { return Value::Null };
    if !matches!(s.kind(key), Some(K::String | K::Number)) { return Value::Null }
    let class = s.parent(node).is_some_and(|p| s.is(p, K::Class));
    let name = property_name(s, node);
    let static_member = s.has_word(node, F::Modifier, "static");
    let exception = if class {
        match s.kind(node) {
            Some(K::ClassProp) => name.as_deref() == Some("constructor") || static_member && name.as_deref() == Some("prototype"),
            _ => name.as_deref() == Some(if static_member { "prototype" } else { "constructor" }),
        }
    } else {
        name.as_deref() == Some("__proto__") && s.parent(node).is_some_and(|p| s.is(p, K::Object) && file.write_site(p).is_none())
    };
    if exception { return Value::Null }
    Value::record(vec![("class".into(), Value::Bool(class)), ("text".into(), Value::from(s.text(key).to_string()))])
}

pub fn assign_spread(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let args: Vec<_> = s.all(node, F::Arg).map(|n| s.unparen(n)).collect();
    let eligible = args.first().is_some_and(|&n| s.is(n, K::Object))
        && !args.iter().any(|&n| s.is(n, K::Spread))
        && (args.len() == 1 || !args.iter().enumerate().any(|(index, &arg)| {
            s.is(arg, K::Object) && s.all(arg, F::Property).any(|p| matches!(s.word(p, F::Kind), Some("get" | "set"))
                || index > 0 && property_name(s, p).as_deref() == Some("__proto__"))
        }));
    Value::Bool(eligible)
}

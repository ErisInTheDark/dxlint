//! Object literal and property-operation preferences.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics::property_name;
use crate::expr::Value;

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
            Some(K::Ident) if s.text(n) == "arguments" && (file.resolved(n) == Some("global")
                || file.bindings().arguments_owner(s, n).is_some()
                || file.access(n) != Some("type") && file.decl_of(n).is_some_and(|decl| {
                    let declaration = s.tree.node(decl);
                    let arrow = s.tree.node(node);
                    declaration.start < arrow.start || declaration.end > arrow.end
                })) => return true,
            _ => {}
        }
        stack.extend(s.kids(n).map(|(_, child)| child));
    }
    false
}

fn member(file: &NativeFile, node: u32) -> Option<Member> {
    let s = &file.syntax;
    let parent = s.parent(node)?;
    if !s.is(parent, K::Object) || file.bindings().write_sites.contains_key(&parent)
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
    if !s.is(node, K::Object) || file.bindings().write_sites.contains_key(&node) { return Value::Null }
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

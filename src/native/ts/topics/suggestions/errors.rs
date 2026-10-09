//! Thrown constructions and their enclosing catch bindings.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics;
use crate::expr::Value;

fn node_data(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let bindings = file.bindings();
    let binding = bindings.refs.get(&node).copied()
        .or_else(|| bindings.decl_scope.contains_key(&node).then_some(node));
    Value::record(vec![
        ("kind".into(), s.kind(node).map_or(Value::Null, |kind| Value::from(kind.name()))),
        ("name".into(), if s.is(node, K::Ident) { Value::from(s.text(node).to_owned()) } else { Value::Null }),
        ("binding".into(), binding.map_or(Value::Null, |id| Value::from(id as usize))),
        ("offset".into(), Value::from(s.tree.node(node).start as usize)),
    ])
}

pub fn catch_throw(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(call) = s.get(node, F::Arg).map(|n| s.unparen(n)).filter(|&n| matches!(s.kind(n), Some(K::Call | K::New))) else { return Value::Null };
    let Some(callee) = s.get(call, F::Callee).map(|n| s.unparen(n)) else { return Value::Null };
    let target = if s.is(callee, K::Member) { s.get(callee, F::Property).unwrap_or(callee) } else { callee };
    let global = s.is(callee, K::Ident) && file.resolved(callee) == Some("global") && file.global_setting(callee) != Some("off");
    let mut child = node;
    let catch = loop {
        let Some(parent) = s.parent(child) else { return Value::Null };
        if matches!(s.kind(parent), Some(K::Function | K::Method | K::Arrow | K::StaticBlock)) { return Value::Null }
        if s.is(parent, K::Try) && s.field(child) == Some(F::Handler) { break parent }
        child = parent;
    };
    let parameter = s.get(catch, F::Param);
    let offset = s.tree.node(node).start;
    let arguments = s.all(call, F::Arg).map(|arg| {
        let arg = s.unparen(arg);
        let properties = if s.is(arg, K::Object) { s.all(arg, F::Property).map(|property| {
            let value = s.get(property, F::Value).map(|n| s.unparen(n)).unwrap_or(property);
            Value::record(vec![
                ("kind".into(), s.kind(property).map_or(Value::Null, |kind| Value::from(kind.name()))),
                ("name".into(), statics::property_name(s, property).map_or(Value::Null, Value::from)),
                ("value".into(), node_data(file, value)),
                ("offset".into(), Value::from(s.tree.node(property).start as usize)),
            ])
        }).collect() } else { Vec::new() };
        Value::record(vec![
            ("kind".into(), s.kind(arg).map_or(Value::Null, |kind| Value::from(kind.name()))),
            ("properties".into(), Value::list(properties)),
        ])
    }).collect();
    Value::record(vec![
        ("name".into(), if s.is(target, K::Ident) { Value::from(s.text(target).to_owned()) } else { Value::Null }),
        ("global".into(), Value::Bool(global)),
        ("parameter".into(), parameter.map_or(Value::Null, |n| node_data(file, n))),
        ("catch_offset".into(), Value::Num(f64::from(s.get(catch, F::Kind).map_or(offset, |n| s.tree.node(n).start)))),
        ("throw_offset".into(), Value::Num(f64::from(offset))),
        ("arguments".into(), Value::list(arguments)),
    ])
}

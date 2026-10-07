//! Preservation of caught errors across rethrows.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics;
use crate::expr::Value;

fn finding(message: Option<&str>, offset: u32) -> Value<'static> {
    Value::record(vec![
        ("checked".into(), Value::Bool(message.is_some())),
        ("message".into(), Value::from(message.unwrap_or("").to_owned())),
        ("offset".into(), Value::Num(f64::from(offset))),
    ])
}

pub fn caught_error(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(call) = s.get(node, F::Arg).map(|n| s.unparen(n)).filter(|&n| matches!(s.kind(n), Some(K::Call | K::New))) else { return Value::Null };
    let Some(callee) = s.get(call, F::Callee).map(|n| s.unparen(n)) else { return Value::Null };
    let target = if s.is(callee, K::Member) { s.get(callee, F::Property).unwrap_or(callee) } else { callee };
    if !s.is(target, K::Ident) { return Value::Null }
    let name = s.text(target);
    let builtin = s.is(callee, K::Ident) && matches!(name, "Error" | "EvalError" | "RangeError" | "ReferenceError" | "SyntaxError" | "TypeError" | "URIError" | "AggregateError")
        && file.resolved(callee) == Some("global") && file.global_setting(callee) != Some("off");
    let mut child = node;
    let catch = loop {
        let Some(parent) = s.parent(child) else { return Value::Null };
        if matches!(s.kind(parent), Some(K::Function | K::Method | K::Arrow | K::StaticBlock)) { return Value::Null }
        if s.is(parent, K::Try) && s.field(child) == Some(F::Handler) { break parent }
        child = parent;
    };
    let parameter = s.get(catch, F::Param);
    let state = match parameter {
        None => "missing",
        Some(n) if !s.is(n, K::Ident) => "destructured",
        _ => "present",
    };
    let offset = s.tree.node(node).start;
    let missing = finding(Some("There is no `cause` attached to the symptom error being thrown."), offset);
    let args: Vec<_> = s.all(call, F::Arg).map(|n| s.unparen(n)).collect();
    let absent = if args.iter().any(|&n| s.is(n, K::Spread)) { finding(None, offset) } else { missing.clone() };
    let causes = args.iter().enumerate().map(|(index, &arg)| {
        if args[..=index].iter().any(|&n| s.is(n, K::Spread)) || !s.is(arg, K::Object) { return finding(None, offset) }
        let properties: Vec<_> = s.all(arg, F::Property).collect();
        if properties.iter().any(|&n| s.is(n, K::Spread)) { return finding(None, offset) }
        let Some(property) = properties.into_iter().rfind(|&n| statics::property_name(s, n).as_deref() == Some("cause")) else { return missing.clone() };
        let value = s.get(property, F::Value).map(|n| s.unparen(n)).unwrap_or(property);
        if !s.is(value, K::Ident) || parameter.is_none_or(|p| s.text(p) != s.text(value)) {
            return finding(Some("The symptom error is being thrown with an incorrect `cause`."), s.tree.node(value).start);
        }
        if file.bindings().refs.get(&value).copied() != parameter {
            return finding(Some("The caught error is being attached as `cause`, but is shadowed by a closer scoped redeclaration."), offset);
        }
        finding(None, offset)
    }).collect();
    Value::record(vec![
        ("name".into(), Value::from(name.to_owned())),
        ("builtin".into(), Value::Bool(builtin)),
        ("default_index".into(), Value::from(if name == "AggregateError" { 2.0 } else { 1.0 })),
        ("catch".into(), Value::from(state)),
        ("catch_offset".into(), Value::Num(f64::from(s.get(catch, F::Kind).map_or(offset, |n| s.tree.node(n).start)))),
        ("throw_offset".into(), Value::Num(f64::from(offset))),
        ("causes".into(), Value::list(causes)),
        ("missing".into(), absent),
    ])
}

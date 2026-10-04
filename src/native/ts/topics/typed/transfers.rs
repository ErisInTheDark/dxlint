//! Unsafe transfers compare generic positions, not structural assignability.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn empty_map(file: &NativeFile<'_>, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    s.is(node, K::New) && s.get(node, F::Callee).is_some_and(|callee| s.is(callee, K::Ident) && s.text(callee) == "Map")
        && s.all(node, F::Arg).next().is_none() && s.all(node, F::TypeArg).next().is_none()
}

fn any_kind(file: &NativeFile<'_>, node: u32, ty: &Ty) -> Option<&'static str> {
    if let Some(category) = super::unsafe_types::category(file, node) {
        return Some(if category == "error" { "error" } else { "`any`" });
    }
    if matches!(ty, Ty::Array(element) if matches!(&*element.element, Ty::Any)) { return Some("`any[]`") }
    let ck = &file.typing().checker;
    if matches!(ck.awaited(ty), Ty::Any) { return Some("`Promise<any>`") }
    None
}

pub(super) fn returned(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let (expr, mut function) = if s.is(node, K::Return) {
        let Some(expr) = s.get(node, F::Arg) else { return Value::Null };
        (expr, s.parent(node))
    } else if s.parent(node).is_some_and(|parent| s.is(parent, K::Arrow) && s.get(parent, F::Body) == Some(node) && !s.is(node, K::Block)) {
        (node, s.parent(node))
    } else { return Value::Null };
    while function.is_some_and(|node| !matches!(s.kind(node), Some(K::Arrow | K::Function | K::Method))) {
        function = function.and_then(|node| s.parent(node));
    }
    let Some(function) = function else { return Value::Null };
    let ck = &file.typing().checker;
    let sender = file.type_of(expr);
    let explicit = s.get(function, F::ReturnType);
    let targets = if let Some(annotation) = explicit {
        vec![file.type_of(annotation)]
    } else {
        ck.declared_returns(&file.contextual_type_of(function))
    };
    let asynchronous = s.has_word(function, F::Modifier, "async");
    if explicit.is_some() && targets.iter().any(|target| {
        matches!(target, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown) || sender.same_identity(target)
            || asynchronous && {
                let target = ck.awaited(target);
                matches!(target, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown) || ck.awaited(&sender).same_identity(&target)
            }
    }) { return Value::Null }
    if let Some(kind) = any_kind(file, expr, &sender) {
        if kind == "`Promise<any>`" && !asynchronous { return Value::Null }
        if targets.iter().any(|target| match kind {
            "`any`" | "error" => matches!(target, Ty::Unknown | Ty::IntrinsicUnknown),
            "`any[]`" => matches!(target, Ty::Array(element) if matches!(&*element.element, Ty::Unknown | Ty::IntrinsicUnknown)),
            "`Promise<any>`" => matches!(ck.awaited(target), Ty::Unknown | Ty::IntrinsicUnknown),
            _ => false,
        }) { return Value::Null }
        return Value::Record(vec![
            ("kind".into(), Value::from(kind)),
            ("sender".into(), Value::Null),
            ("receiver".into(), Value::Null),
        ]);
    }
    if !empty_map(file, expr) {
        if let Some(receiver) = targets.first().filter(|receiver| sender.unsafe_assignment_to(receiver)) {
            return Value::Record(vec![
                ("kind".into(), Value::Null),
                ("sender".into(), Value::from(ck.print(&sender))),
                ("receiver".into(), Value::from(ck.print(receiver))),
            ]);
        }
    }
    Value::Null
}

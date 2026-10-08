//! Unsafe transfers compare generic positions, not structural assignability.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn empty_map(file: &NativeFile<'_>, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    s.is(node, K::New) && s.get(node, F::Callee).is_some_and(|callee| s.is(callee, K::Ident) && s.text(callee) == "Map")
        && s.all(node, F::Arg).next().is_none() && s.all(node, F::TypeArg).next().is_none()
}

#[derive(Clone, Copy)]
enum AnyCarrier {
    Value { error: bool },
    Array,
    Promise,
}

fn any_carrier(file: &NativeFile<'_>, node: u32, ty: &Ty) -> Option<AnyCarrier> {
    if let Some(category) = super::unsafe_types::category(file, node) {
        return Some(AnyCarrier::Value { error: category == "error" });
    }
    if matches!(ty, Ty::Array(element) if matches!(&*element.element, Ty::Any)) { return Some(AnyCarrier::Array) }
    let ck = &file.typing().checker;
    if matches!(ck.awaited(ty), Ty::Any) { return Some(AnyCarrier::Promise) }
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
    if let Some(carrier) = any_carrier(file, expr, &sender) {
        if matches!(carrier, AnyCarrier::Promise) && !asynchronous { return Value::Null }
        if targets.iter().any(|target| match carrier {
            AnyCarrier::Value { .. } => matches!(target, Ty::Unknown | Ty::IntrinsicUnknown),
            AnyCarrier::Array => matches!(target, Ty::Array(element) if matches!(&*element.element, Ty::Unknown | Ty::IntrinsicUnknown)),
            AnyCarrier::Promise => matches!(ck.awaited(target), Ty::Unknown | Ty::IntrinsicUnknown),
        }) { return Value::Null }
        let (mode, error) = match carrier {
            AnyCarrier::Value { error } => ("value", error),
            AnyCarrier::Array => ("array", false),
            AnyCarrier::Promise => ("promise", false),
        };
        return Value::record(vec![
            ("mode".into(), Value::from(mode)),
            ("error".into(), Value::Bool(error)),
            ("sender".into(), if error { Value::Null } else { Value::from(ck.print(&sender)) }),
            ("receiver".into(), Value::Null),
        ]);
    }
    if !empty_map(file, expr) {
        if let Some(receiver) = targets.first().filter(|receiver| sender.unsafe_assignment_to(receiver)) {
            return Value::record(vec![
                ("mode".into(), Value::from("generic")),
                ("error".into(), Value::Bool(false)),
                ("sender".into(), Value::from(ck.print(&sender))),
                ("receiver".into(), Value::from(ck.print(receiver))),
            ]);
        }
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn returned_transfers_distinguish_provenance_and_carrier() {
        let code = "interface Promise<T> { then(callback: (value: T) => unknown): unknown; } interface Box<T> { value: T; } declare const value: any; declare const array: any[]; declare const promise: Promise<any>; declare const box: Box<any>; function direct(): number { return value; } function missing(): number { return spooky; } function elements(): number[] { return array; } async function awaited(): Promise<number> { return promise; } function generic(): Box<number> { return box; } const arrow: () => number = () => value;";
        let decisions = native_linter("rule no-unsafe-return\nrule no-unsafe-return:*");
        assert_eq!(decisions.lint(Path::new("/root/a.ts"), code).len(), 6);
        for (condition, expected) in [
            ("return_transfer.mode == \"value\"", 3),
            ("return_transfer.mode == \"array\"", 1),
            ("return_transfer.mode == \"promise\"", 1),
            ("return_transfer.mode == \"generic\"", 1),
            ("return_transfer.error == true", 1),
        ] {
            let probe = native_linter(&format!("in ts_return, ts_arrow.body rule transfer {{\nwhen return_transfer != null && {condition}\nwarn \"transfer\"\n}}"));
            assert_eq!(probe.lint(Path::new("/root/a.ts"), code).len(), expected, "{condition}");
        }
    }

    #[test]
    fn return_annotations_context_and_async_boundaries_keep_exemptions() {
        let linter = native_linter("rule no-unsafe-return\nrule no-unsafe-return:*");
        let code = "interface Promise<T> { then(callback: (value: T) => unknown): unknown; } interface Box<T> { value: T; } declare class Map<K = any, V = any> {} declare const value: any; declare const array: any[]; declare const promise: Promise<any>; declare const box: Box<any>; function explicitAny(): any { return value; } function missing(): unknown { return spooky; } function elements(): unknown[] { return array; } async function awaited(): Promise<unknown> { return promise; } function synchronous(): Promise<number> { return promise; } function same(): Box<any> { return box; } function empty(): Map<string, number> { return new Map(); } const arrow: () => unknown = () => value; function nothing() { return; }";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}

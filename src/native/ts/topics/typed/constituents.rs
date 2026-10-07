//! Duplicate union and intersection constituents, preserving first occurrence.

use super::*;

pub(super) fn bare(file: &NativeFile<'_>, mut node: u32) -> u32 {
    while file.syntax.is(node, Kind::ParenType) {
        let Some(inner) = file.syntax.get(node, Field::Type) else { break };
        node = inner;
    }
    node
}

pub(super) fn duplicates(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(kind @ (Kind::Union | Kind::Intersection)) = s.kind(node) else { return Value::Null };
    let mut parent = s.parent(node);
    while parent.is_some_and(|n| s.is(n, Kind::ParenType)) {
        parent = parent.and_then(|n| s.parent(n));
    }
    if parent.is_some_and(|n| s.is(n, kind)) { return Value::Null }
    let optional = kind == Kind::Union && parent.is_some_and(|n| s.is(n, Kind::Param) && s.get(n, Field::Type).is_some_and(|ty| bare(file, ty) == node) && s.get(n, Field::Optional).is_some());
    let mut seen = Vec::new();
    let mut findings = Vec::new();
    for part in s.all(node, Field::Type) {
        check(file, part, kind, optional, &mut seen, &mut findings);
    }
    Value::list(findings)
}

fn check(file: &NativeFile<'_>, node: u32, kind: Kind, optional: bool, seen: &mut Vec<(u32, Ty)>, findings: &mut Vec<Value<'static>>) {
    let node = bare(file, node);
    let s = &file.syntax;
    let ty = file.type_of(node);
    let previous = seen.iter().find(|(previous, previous_ty)| {
        s.kind(*previous) == s.kind(node) && file.equal_tokens(*previous, node) || ty.same_identity(previous_ty)
    });
    if let Some((previous, _)) = previous {
        findings.push(finding(file, node, kind, Some(*previous)));
        return;
    }
    if super::unsafe_types::missing_annotation(file, node) { return }
    if optional && matches!(ty, Ty::Undefined) {
        findings.push(finding(file, node, kind, None));
    }
    seen.push((node, ty));
    if s.is(node, kind) {
        for part in s.all(node, Field::Type) {
            check(file, part, kind, optional, seen, findings);
        }
    }
}

fn finding(file: &NativeFile<'_>, node: u32, kind: Kind, previous: Option<u32>) -> Value<'static> {
    Value::record(vec![
        ("node".into(), Value::Node(node, file.syntax.text(node).to_owned().into())),
        ("kind".into(), Value::from(if kind == Kind::Union { "Union" } else { "Intersection" })),
        ("previous".into(), previous.map_or(Value::Null, |n| Value::from(file.syntax.text(n).to_string()))),
    ])
}

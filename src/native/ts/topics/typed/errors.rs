//! Error-value types and caught-binding provenance.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

fn error_like(ty: &Ty, ck: &Checker<'_>, depth: usize) -> bool {
    if depth >= 64 { return false }
    if let Some(source) = ck.readonly_source(ty) { return error_like(source, ck, depth + 1) }
    match ty {
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| error_like(part, ck, depth + 1)),
        Ty::Inter(parts) => parts.iter().any(|part| error_like(part, ck, depth + 1)),
        _ => ck.is_builtin_type_or_base(ty, "Error"),
    }
}

fn rethrown(file: &NativeFile<'_>, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    if !s.is(node, K::Ident) { return false }
    let Some(decl) = file.decl_of(node) else { return false };
    if file.binding(decl).is_some_and(|binding| binding.kind == "catch") { return true }
    let Some(owner) = s.parent(decl) else { return false };
    if s.is(owner, K::Try) && s.get(owner, F::Param) == Some(decl) { return true }
    if !s.is(owner, K::Param) { return false }
    let Some(arrow) = s.parent(owner).filter(|&arrow| s.is(arrow, K::Arrow) && s.all(arrow, F::Param).next() == Some(owner)) else { return false };
    let Some(call) = s.parent(arrow).filter(|&call| s.is(call, K::Call) && s.field(arrow) == Some(F::Arg)) else { return false };
    super::promises::rejection_handler(file, call).is_some_and(|(handler, _)| handler == arrow)
}

pub(super) fn value(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let ty = file.constrained_type_of(node);
    let kind = match ty {
        Ty::Any => "any",
        Ty::Unknown | Ty::IntrinsicUnknown => "unknown",
        Ty::Undefined => "undefined",
        _ if error_like(&ty, &file.typing().checker, 0) => "error",
        _ => "other",
    };
    Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("rethrown".into(), Value::Bool(rethrown(file, node))),
    ])
}

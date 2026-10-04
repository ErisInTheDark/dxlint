//! Conservative type identity, distinct from mutual assignability.

use super::{Env, Ty};
use super::super::modules::ModuleAt;
use std::sync::Arc;

impl Ty {
    pub fn same_identity(&self, other: &Ty) -> bool {
        same(self, other, 0)
    }
}

fn same(a: &Ty, b: &Ty, depth: usize) -> bool {
    if depth >= 32 { return false }
    let same = |a: &Ty, b: &Ty| same(a, b, depth + 1);
    let parts = |a: &[Ty], b: &[Ty]| a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b));
    match (a, b) {
        (Ty::Prim(a), Ty::Prim(b)) => a == b,
        (Ty::Lit(a, ap), Ty::Lit(b, bp)) if ap == bp => {
            match *ap {
                "string" => super::super::regex::cook(a).zip(super::super::regex::cook(b)).is_some_and(|(a, b)| a == b),
                "number" => super::super::statics::number(a).zip(super::super::statics::number(b)).is_some_and(|(a, b)| a == b),
                _ => a == b,
            }
        }
        (Ty::Param(a), Ty::Param(b)) => a == b,
        (Ty::Named(a, aa), Ty::Named(b, ba)) => a == b && parts(aa, ba),
        (Ty::Array(a), Ty::Array(b)) => a.readonly == b.readonly && same(&a.element, &b.element),
        (Ty::Tuple(a), Ty::Tuple(b)) => a.readonly == b.readonly && a.len() == b.len()
            && a.elements.iter().zip(&b.elements).all(|(a, b)| a.optional == b.optional && a.rest == b.rest && same(&a.ty, &b.ty)),
        (Ty::Union(a), Ty::Union(b)) | (Ty::Inter(a), Ty::Inter(b)) =>
            a.iter().all(|a| b.iter().any(|b| same(a, b))) && b.iter().all(|b| a.iter().any(|a| same(a, b))),
        (Ty::Union(parts), _) | (_, Ty::Union(parts)) => !parts.is_empty() && parts.iter().all(|part| {
            if matches!(a, Ty::Union(_)) { same(part, b) } else { same(a, part) }
        }),
        (Ty::Obj(a, ae), Ty::Obj(b, be)) => Arc::ptr_eq(a, b) && same_env(ae, be, depth + 1),
        (Ty::Fn(a), Ty::Fn(b)) => Arc::ptr_eq(a, b),
        (Ty::Class(a), Ty::Class(b)) | (Ty::Ns(a), Ty::Ns(b)) => a == b,
        (Ty::IntrinsicUnknown, Ty::IntrinsicUnknown) | (Ty::Any, Ty::Any) | (Ty::Never, Ty::Never) | (Ty::Void, Ty::Void) | (Ty::Undefined, Ty::Undefined) | (Ty::Null, Ty::Null) => true,
        _ => false,
    }
}

fn same_env(a: &Env, b: &Env, depth: usize) -> bool {
    let module = matches!((a.module, b.module), (ModuleAt::Top, ModuleAt::Top))
        || matches!((a.module, b.module), (ModuleAt::Ambient(a), ModuleAt::Ambient(b)) if a == b);
    a.file == b.file && module && a.namespaces == b.namespaces
        && a.tparams.len() == b.tparams.len()
        && a.tparams.iter().zip(&b.tparams).all(|((an, at), (bn, bt))| an == bn && same(at, bt, depth))
        && match (&a.this, &b.this) {
            (None, None) => true,
            (Some(a), Some(b)) => same(a, b, depth),
            _ => false,
        }
}

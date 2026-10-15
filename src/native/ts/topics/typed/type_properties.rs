//! Structural type questions, preserving enum identity.

use crate::native::ts::types::{Checker, Ty};

pub(super) fn string_like(ty: &Ty, checker: &Checker<'_>) -> bool {
    match ty {
        Ty::EnumMember(member) => string_like(&member.base, checker),
        Ty::Prim("string") | Ty::Lit(_, "string") => true,
        Ty::Named(symbol, _) => checker.is_enum(symbol) && checker.is_string_enum(symbol),
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| string_like(part, checker)),
        Ty::Inter(parts) => !parts.iter().any(indeterminate) && parts.iter().any(|part| string_like(part, checker)),
        _ => false,
    }
}

fn indeterminate(ty: &Ty) -> bool {
    match ty {
        Ty::Any | Ty::Unknown => true,
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(indeterminate),
        _ => false,
    }
}

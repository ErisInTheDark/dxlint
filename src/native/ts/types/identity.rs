//! Conservative type identity, distinct from mutual assignability.

use super::{Checker, Env, Ty};
use super::super::modules::ModuleAt;
use std::sync::Arc;

impl Ty {
    pub fn same_identity(&self, other: &Ty) -> bool {
        same(self, other, 0)
    }
}

impl Checker<'_> {
    pub fn generic_properties(&self, ty: &Ty) -> Option<Vec<Box<str>>> {
        match ty {
            Ty::Unknown => None,
            Ty::Union(parts) | Ty::Inter(parts) => {
                let mut names = Vec::new();
                for part in parts {
                    for name in self.generic_properties(part)? {
                        if !names.contains(&name) { names.push(name) }
                    }
                }
                Some(names)
            }
            Ty::Obj(..) | Ty::Struct(..) | Ty::Named(..) => {
                let mut generic = Vec::new();
                for name in self.known_property_names(ty, 0)? {
                    match self.member(ty, &name) {
                        Ty::Fn(callable) if callable.sigs.iter().any(|signature| !signature.type_params.is_empty()) => generic.push(name),
                        Ty::Unknown => return None,
                        member @ (Ty::Named(..) | Ty::Obj(..)) if self.known_property_names(&member, 0).is_none() => return None,
                        _ => {}
                    }
                }
                Some(generic)
            }
            _ => Some(Vec::new()),
        }
    }

    pub fn property_literals(&self, ty: &Ty) -> Option<bool> {
        Some(self.known_property_names(ty, 0)?.iter().all(|name| matches!(self.member(ty, name), Ty::Lit(..))))
    }

    pub fn property_shapes_equal(&self, a: &Ty, b: &Ty) -> Option<bool> {
        self.property_shapes_equal_at(a, b, &mut 32)
    }

    fn property_shapes_equal_at(&self, a: &Ty, b: &Ty, remaining: &mut usize) -> Option<bool> {
        if a.same_identity(b) { return Some(true) }
        if *remaining == 0 || a.is_opaque() || b.is_opaque() { return None }
        *remaining -= 1;
        if !self.type_arguments_equal(a, b)? { return Some(false) }
        let names = |ty: &Ty| match ty {
            Ty::Obj(..) | Ty::Struct(..) | Ty::Named(..) | Ty::Union(..) | Ty::Inter(..) | Ty::Array(..) | Ty::Tuple(..) => self.known_property_names(ty, 0),
            Ty::Unknown => None,
            _ => Some(Vec::new()),
        };
        let (a_names, b_names) = (names(a)?, names(b)?);
        if a_names.len() != b_names.len() || a_names.iter().any(|name| !b_names.contains(name)) { return Some(false) }
        if a_names.len() > *remaining { return None }
        let (a_generic, b_generic) = (self.generic_properties(a)?, self.generic_properties(b)?);
        if a_generic.len() != b_generic.len() || a_generic.iter().any(|name| !b_generic.contains(name)) { return Some(false) }
        for name in a_names {
            if self.property_readonly(a, &name)? != self.property_readonly(b, &name)? { return Some(false) }
            if !self.property_shapes_equal_at(&self.member(a, &name), &self.member(b, &name), remaining)? { return Some(false) }
        }
        Some(true)
    }

    pub fn type_arguments_equal(&self, a: &Ty, b: &Ty) -> Option<bool> {
        fn arguments(ty: &Ty) -> Option<Vec<&Ty>> {
            match ty {
                Ty::Unknown => None,
                Ty::Named(_, args) => Some(args.iter().collect()),
                Ty::Obj(_, env) => Some(env.tparams.iter().map(|(_, ty)| ty).collect()),
                Ty::Fn(callable) => Some(callable.env.tparams.iter().map(|(_, ty)| ty).collect()),
                Ty::Array(array) => Some(vec![&array.element]),
                Ty::Tuple(tuple) => Some(tuple.iter().collect()),
                _ => Some(Vec::new()),
            }
        }
        let (a, b) = (arguments(a)?, arguments(b)?);
        Some(a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.same_identity(b)))
    }

    pub fn type_contains_any(&self, ty: &Ty) -> Option<bool> {
        self.type_contains(ty, |ty| matches!(ty, Ty::Any), &mut 32)
    }

    pub fn type_contains_variable(&self, ty: &Ty) -> Option<bool> {
        self.type_contains(ty, |ty| matches!(ty, Ty::Param(_)), &mut 32)
    }

    fn type_contains(&self, ty: &Ty, predicate: fn(&Ty) -> bool, remaining: &mut usize) -> Option<bool> {
        if predicate(ty) { return Some(true) }
        if *remaining == 0 || ty.is_opaque() { return None }
        *remaining -= 1;
        let mut nested = Vec::new();
        match ty {
            Ty::Union(parts) | Ty::Inter(parts) => nested.extend(parts.iter().cloned()),
            Ty::Named(_, args) => {
                nested.extend(args.iter().cloned());
                let names = self.known_property_names(ty, 0)?;
                if names.len() + nested.len() > *remaining { return None }
                nested.extend(names.iter().map(|name| self.member(ty, name)));
            }
            Ty::Array(array) => nested.push((*array.element).clone()),
            Ty::Tuple(tuple) => nested.extend(tuple.iter().cloned()),
            Ty::Fn(callable) => {
                for signature in &callable.sigs {
                    if nested.len() + signature.params.len() + 1 > *remaining { return None }
                    nested.extend(signature.params.iter().filter(|param| &*param.name != "this").map(|param| {
                        param.ty.as_ref().map_or(Ty::Unknown, |ty| self.eval(ty, &callable.env))
                    }));
                    nested.push(callable.ret.clone().unwrap_or_else(|| {
                        signature.ret.as_ref().map_or(Ty::Unknown, |ty| self.eval(ty, &callable.env))
                    }));
                }
            }
            Ty::Obj(..) | Ty::Struct(..) => {
                let names = self.known_property_names(ty, 0)?;
                if names.len() > *remaining { return None }
                nested.extend(names.iter().map(|name| self.member(ty, name)));
            }
            _ => {}
        }
        let mut known = true;
        for ty in nested {
            match self.type_contains(&ty, predicate, remaining) {
                Some(true) => return Some(true),
                None => known = false,
                Some(false) => {}
            }
        }
        known.then_some(false)
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

//! Conservative type identity, distinct from mutual assignability.

use super::{Checker, Env, Ty};
use super::super::modules::ModuleAt;
use crate::fx_hash::FxHashMap;
use std::sync::Arc;

#[cfg(test)]
thread_local! {
    static IDENTITY_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Ty {
    pub fn same_identity(&self, other: &Ty) -> bool {
        #[cfg(test)]
        IDENTITY_WORK.with(|work| work.set(0));
        Comparison::<false>::default().same(self, other, 0)
    }

    pub(super) fn same_representation(&self, other: &Ty) -> bool {
        Comparison::<true>::default().same(self, other, 0)
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

// Addresses identify immutable inputs only within this comparison. Depth is
// part of the result: a shallow proof cannot bypass a deeper resolution limit.
#[derive(Default)]
struct Comparison<const STORED: bool> {
    types: FxHashMap<(*const Ty, *const Ty, usize), bool>,
    environments: FxHashMap<(*const Env, *const Env, usize), bool>,
}

impl<const STORED: bool> Comparison<STORED> {
    fn same(&mut self, a: &Ty, b: &Ty, depth: usize) -> bool {
        #[cfg(test)]
        if !STORED { IDENTITY_WORK.with(|work| work.set(work.get() + 1)); }
        if depth >= 32 { return false }
        match (a, b) {
            (Ty::Unknown, Ty::Unknown) if STORED => return true,
            (Ty::Prim(a), Ty::Prim(b)) => return a == b,
            (Ty::Lit(a, ap), Ty::Lit(b, bp)) if ap == bp => return if STORED { a == b } else { match *ap {
                "string" => super::super::regex::cook(a).zip(super::super::regex::cook(b)).is_some_and(|(a, b)| a == b),
                "number" => super::super::statics::number(a).zip(super::super::statics::number(b)).is_some_and(|(a, b)| a == b),
                _ => a == b,
            } },
            (Ty::Param(a), Ty::Param(b)) => return a == b,
            (Ty::Named(a, aa), Ty::Named(b, ba)) if a != b || aa.is_empty() || ba.is_empty() =>
                return a == b && aa.is_empty() && ba.is_empty(),
            (Ty::Obj(a, ae), Ty::Obj(b, be)) => return Arc::ptr_eq(a, b) && self.environment(ae, be, depth + 1),
            (Ty::Struct(a), Ty::Struct(b)) => return Arc::ptr_eq(a, b),
            (Ty::Fn(a), Ty::Fn(b)) => return Arc::ptr_eq(a, b),
            (Ty::Class(a), Ty::Class(b)) | (Ty::Ns(a), Ty::Ns(b)) => return a == b,
            (Ty::Module(a), Ty::Module(b)) if STORED => return match (a.0, b.0) {
                (super::super::modules::ModRef::File(a), super::super::modules::ModRef::File(b)) => a == b,
                (super::super::modules::ModRef::Ambient(af, ai), super::super::modules::ModRef::Ambient(bf, bi)) => af == bf && ai == bi,
                _ => false,
            },
            (Ty::IntrinsicUnknown, Ty::IntrinsicUnknown) | (Ty::Any, Ty::Any) | (Ty::Never, Ty::Never) | (Ty::Void, Ty::Void) | (Ty::Undefined, Ty::Undefined) | (Ty::Null, Ty::Null) => return true,
            (Ty::Named(..), Ty::Named(..)) | (Ty::Array(_), Ty::Array(_)) | (Ty::Tuple(_), Ty::Tuple(_))
                | (Ty::Inter(_), Ty::Inter(_)) | (Ty::Union(_), _) | (_, Ty::Union(_)) => {}
            _ => return false,
        }
        let key = (a as *const Ty, b as *const Ty, depth);
        if let Some(&result) = self.types.get(&key) { return result }
        let result = match (a, b) {
            (Ty::Named(_, aa), Ty::Named(_, ba)) => aa.len() == ba.len()
                && aa.iter().zip(ba).all(|(a, b)| self.same(a, b, depth + 1)),
            (Ty::Array(a), Ty::Array(b)) => a.readonly == b.readonly && self.same(&a.element, &b.element, depth + 1),
            (Ty::Tuple(a), Ty::Tuple(b)) => a.readonly == b.readonly && a.len() == b.len()
                && a.elements.iter().zip(&b.elements).all(|(a, b)| a.optional == b.optional && a.rest == b.rest && self.same(&a.ty, &b.ty, depth + 1)),
            (Ty::Union(a), Ty::Union(b)) | (Ty::Inter(a), Ty::Inter(b)) =>
                if STORED { a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.same(a, b, depth + 1)) }
                else { a.iter().all(|a| b.iter().any(|b| self.same(a, b, depth + 1))) && b.iter().all(|b| a.iter().any(|a| self.same(a, b, depth + 1))) },
            (Ty::Union(parts), _) | (_, Ty::Union(parts)) => !STORED && !parts.is_empty() && parts.iter().all(|part| {
                if matches!(a, Ty::Union(_)) { self.same(part, b, depth + 1) } else { self.same(a, part, depth + 1) }
            }),
            _ => unreachable!(),
        };
        self.types.insert(key, result);
        result
    }

    fn environment(&mut self, a: &Env, b: &Env, depth: usize) -> bool {
        let key = (a as *const Env, b as *const Env, depth);
        if let Some(&result) = self.environments.get(&key) { return result }
        let module = matches!((a.module, b.module), (ModuleAt::Top, ModuleAt::Top))
            || matches!((a.module, b.module), (ModuleAt::Ambient(a), ModuleAt::Ambient(b)) if a == b);
        let result = a.file == b.file && module && a.namespaces == b.namespaces
            && a.tparams.len() == b.tparams.len()
            && a.tparams.iter().zip(&b.tparams).all(|((an, at), (bn, bt))| an == bn && self.same(at, bt, depth))
            && match (&a.this, &b.this) {
                (None, None) => true,
                (Some(a), Some(b)) => self.same(a, b, depth),
                _ => false,
            };
        self.environments.insert(key, result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::ts::modules::FileId;

    fn shared_graph(levels: usize, mut ty: Ty) -> Ty {
        let body = Arc::new(Vec::new());
        for _ in 0..levels {
            let env = Env {
                tparams: vec![("T".into(), Ty::Inter(vec![ty.clone(), ty.clone()]))],
                ..Env::file(FileId(0))
            };
            ty = Ty::Obj(body.clone(), Arc::new(env));
        }
        ty
    }

    #[test]
    fn shared_type_graph_identity_has_linear_work() {
        let levels = 8;
        let ty = shared_graph(levels, Ty::Prim("number"));
        let Ty::Obj(body, env) = &ty else { unreachable!() };
        let separate_env = Ty::Obj(body.clone(), Arc::new((**env).clone()));
        assert!(ty.same_identity(&separate_env));
        let work = IDENTITY_WORK.with(|work| work.get());
        assert!(work <= 32 * (levels + 1), "shared graph used {work} comparisons");
    }

    #[test]
    fn shared_environments_do_not_prove_opaque_type_identity() {
        let ty = shared_graph(1, Ty::Unknown);
        assert!(!ty.same_identity(&ty.clone()));
    }

    #[test]
    fn identity_proofs_do_not_bypass_deeper_resolution_limits() {
        let child = shared_graph(8, Ty::Prim("number"));
        let mut deeper = child.clone();
        for _ in 0..20 { deeper = Ty::array(deeper); }
        let ty = Ty::Obj(Arc::new(Vec::new()), Arc::new(Env {
            tparams: vec![("A".into(), child), ("B".into(), deeper)],
            ..Env::file(FileId(0))
        }));
        assert!(!ty.same_identity(&ty.clone()));
    }
}

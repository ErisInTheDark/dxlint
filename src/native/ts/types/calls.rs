//! Calls and construction: signatures, overload choice, simple generic
//! inference from arguments, `await`.

use std::sync::Arc;

use super::super::summary::{MemberKind, Sig, TParam, TypeSyn};
use super::{Checker, Env, Ty};

impl Checker<'_> {
    /// Signatures `t` can be called with (or constructed with).
    fn signatures(&self, t: &Ty, construct: bool) -> Option<(Vec<Arc<Sig>>, Arc<Env>)> {
        match t {
            Ty::Fn(c) if !construct => Some((c.sigs.clone(), c.env.clone())),
            // A function merged with a namespace.
            Ty::Inter(items) => items.iter().find_map(|i| self.signatures(i, construct)),
            Ty::Named(sym, args) => {
                let kind = if construct { MemberKind::Construct } else { MemberKind::Call };
                for &r in sym.iter() {
                    let s = self.summary_of(r.file)?;
                    let decl = &s.decls[r.decl as usize];
                    let sigs: Vec<Arc<Sig>> = decl.members.iter().filter(|m| m.kind == kind).flat_map(|m| m.sigs.iter().cloned()).collect();
                    if !sigs.is_empty() {
                        let mut env = self.decl_env(r);
                        env.tparams = self.bind_tparams(&decl.type_params, args.clone(), &env);
                        return Some((sigs, Arc::new(env)));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// The overload TypeScript would choose: the first signature whose
    /// arity fits and whose parameters are known to accept the arguments,
    /// else the first not known to reject them, else the first that fits
    /// by arity.
    fn pick<'s>(&self, sigs: &'s [Arc<Sig>], env: &Env, args: &[Ty]) -> Option<&'s Arc<Sig>> {
        // An implementation behind overloads is not callable from outside.
        let overloaded = sigs.iter().any(|s| s.body_at.is_none());
        let visible = |s: &&Arc<Sig>| !overloaded || s.body_at.is_none();
        let fits = |s: &&Arc<Sig>| {
            let required = s.params.iter().filter(|p| !p.optional && !p.rest).count();
            let rest = s.params.iter().any(|p| p.rest);
            visible(s) && args.len() >= required && (rest || args.len() <= s.params.len())
        };
        let candidates: Vec<&Arc<Sig>> = sigs.iter().filter(fits).collect();
        if candidates.len() > 1 {
            if let Some(&s) = candidates.iter().find(|s| self.accepts(s, env, args, true)) {
                return Some(s);
            }
            // Not provably one overload: only a choice when every
            // overload still possible returns the same.
            let possible: Vec<&Arc<Sig>> = candidates.iter().copied().filter(|s| self.accepts(s, env, args, false)).collect();
            return match possible.split_first() {
                Some((first, rest)) if rest.iter().all(|s| s.ret == first.ret && s.is_async == first.is_async) => Some(first),
                Some(_) => None,
                None => candidates.first().copied(),
            };
        }
        candidates.first().copied().or_else(|| sigs.iter().find(visible))
    }

    /// Every argument is known to be accepted by its parameter (`strict`),
    /// or none is known to be rejected.
    fn accepts(&self, sig: &Sig, env: &Env, args: &[Ty], strict: bool) -> bool {
        let mut env = env.clone();
        // Type parameters take what the arguments infer for them (widened,
        // as inference does); the rest stay open.
        let mut inferred: Vec<(Box<str>, Ty)> = Vec::new();
        for (p, a) in sig.params.iter().zip(args) {
            if let Some(t) = &p.ty {
                self.infer(t, a, &sig.type_params, &mut inferred);
            }
        }
        for tp in &sig.type_params {
            let t = inferred.iter().find(|(n, _)| *n == tp.name).map_or_else(|| Ty::Param(tp.name.clone()), |(_, t)| t.clone().widen());
            env.tparams.push((tp.name.clone(), t));
        }
        args.iter().enumerate().all(|(i, a)| {
            let Some(p) = sig.params.get(i).or_else(|| sig.params.last().filter(|p| p.rest)) else { return false };
            let Some(syn) = &p.ty else { return true };
            let mut t = self.eval(syn, &env);
            if p.rest {
                t = match t {
                    Ty::Array(e) => *e,
                    Ty::Tuple(items) => items.get(i - (sig.params.len() - 1)).cloned().unwrap_or(Ty::Unknown),
                    _ => Ty::Unknown,
                };
            }
            if p.optional {
                t = t.or_undefined();
            }
            match self.assignable(a, &t) {
                Some(ok) => ok,
                None => !strict,
            }
        })
    }

    /// Whether calling `f` depends on its arguments' types: an overload to
    /// choose or type parameters to infer.
    pub fn needs_args(&self, f: &Ty) -> bool {
        match f {
            Ty::Fn(c) if c.ret.is_some() => false,
            Ty::Union(items) | Ty::Inter(items) => items.iter().any(|i| self.needs_args(i)),
            _ => match self.signatures(f, false) {
                Some((sigs, _)) => sigs.len() > 1 || sigs.iter().any(|s| !s.type_params.is_empty()),
                None => false,
            },
        }
    }

    /// The result of calling `f` with arguments of these types.
    pub fn call(&self, f: &Ty, args: &[Ty], targs: Vec<Ty>) -> Ty {
        match f {
            Ty::Any => return Ty::Any,
            Ty::Union(items) => {
                let results: Vec<Ty> = items.iter().filter(|i| !matches!(i, Ty::Undefined | Ty::Null)).map(|i| self.call(i, args, targs.clone())).collect();
                return if results.iter().any(Ty::is_unknown) { Ty::Unknown } else { Ty::union(results) };
            }
            Ty::Fn(c) if c.ret.is_some() => return c.ret.clone().unwrap_or(Ty::Unknown),
            _ => {}
        }
        let Some((sigs, env)) = self.signatures(f, false) else { return Ty::Unknown };
        let Some(sig) = self.pick(&sigs, &env, args) else { return Ty::Unknown };
        self.sig_result(sig, &env, args, targs)
    }

    /// The signature a callee resolves to for these arguments, with its env.
    pub fn signature_for(&self, f: &Ty, args: &[Ty], construct: bool) -> Option<(Arc<Sig>, Arc<Env>)> {
        let (sigs, env) = match (f, construct) {
            (Ty::Class(sym), true) => {
                let r = *sym.first()?;
                let s = self.summary_of(r.file)?;
                let decl = &s.decls[r.decl as usize];
                (decl.sigs.clone(), Arc::new(self.decl_env(r)))
            }
            _ => self.signatures(f, construct)?,
        };
        Some((self.pick(&sigs, &env, args)?.clone(), env))
    }

    pub fn sig_result(&self, sig: &Sig, env: &Env, args: &[Ty], targs: Vec<Ty>) -> Ty {
        let mut env = env.clone();
        if targs.is_empty() && !sig.type_params.is_empty() {
            let mut inferred: Vec<(Box<str>, Ty)> = Vec::new();
            for (p, a) in sig.params.iter().zip(args) {
                if let Some(t) = &p.ty {
                    self.infer(t, a, &sig.type_params, &mut inferred);
                }
            }
            for tp in &sig.type_params {
                let t = inferred
                    .iter()
                    .find(|(n, _)| *n == tp.name)
                    .map(|(_, t)| t.clone().widen())
                    .unwrap_or_else(|| tp.default.as_ref().map_or(Ty::Unknown, |d| self.eval(d, &env)));
                env.tparams.push((tp.name.clone(), t));
            }
        } else {
            env.tparams = self.bind_tparams(&sig.type_params, targs, &env);
        }
        match &sig.ret {
            Some(r) => {
                let t = self.eval(r, &env);
                if sig.is_async && !matches!(t, Ty::Named(..)) {
                    self.global_named("Promise", vec![t])
                } else {
                    t
                }
            }
            None if sig.is_async => self.global_named("Promise", vec![Ty::Unknown]),
            None => Ty::Unknown,
        }
    }

    /// Binds type parameters of `params` that `syn` mentions from `actual`.
    pub fn infer(&self, syn: &TypeSyn, actual: &Ty, params: &[TParam], out: &mut Vec<(Box<str>, Ty)>) {
        match (syn, actual) {
            (TypeSyn::Ref(path, args), _) if path.len() == 1 && args.is_empty() && params.iter().any(|p| *p.name == *path[0]) => {
                if !actual.is_unknown() && !out.iter().any(|(n, _)| *n == path[0]) {
                    out.push((path[0].clone(), actual.clone()));
                }
            }
            (TypeSyn::Array(inner), Ty::Array(a)) => self.infer(inner, a, params, out),
            (TypeSyn::Ref(path, args), Ty::Array(a)) if args.len() == 1 && matches!(&*path[0], "Array" | "ReadonlyArray" | "Iterable") => {
                self.infer(&args[0], a, params, out)
            }
            (TypeSyn::Ref(_, args), Ty::Named(_, actual_args)) => {
                for (s, a) in args.iter().zip(actual_args) {
                    self.infer(s, a, params, out);
                }
            }
            (TypeSyn::Fn(sig), Ty::Fn(c)) => {
                if let (Some(r), Some(asig)) = (&sig.ret, c.sigs.first()) {
                    let ret = match (&c.ret, &asig.ret) {
                        (Some(t), _) => t.clone(),
                        (None, Some(ar)) => self.eval(ar, &c.env),
                        (None, None) => Ty::Unknown,
                    };
                    self.infer(r, &ret, params, out);
                }
            }
            (TypeSyn::Union(items), _) => {
                for i in items {
                    if !matches!(i, TypeSyn::Kw(k) if matches!(&**k, "undefined" | "null")) {
                        self.infer(i, actual, params, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// The instance `new c(...)` creates.
    pub fn construct(&self, c: &Ty, args: &[Ty], targs: Vec<Ty>) -> Ty {
        match c {
            Ty::Class(sym) => {
                let Some(&r) = sym.first() else { return Ty::Unknown };
                let Some(s) = self.summary_of(r.file) else { return Ty::Unknown };
                let decl = &s.decls[r.decl as usize];
                if !targs.is_empty() || decl.type_params.is_empty() {
                    return Ty::Named(sym.clone(), targs);
                }
                let env = self.decl_env(r);
                let mut inferred = Vec::new();
                if let Some(sig) = self.pick(&decl.sigs, &env, args) {
                    for (p, a) in sig.params.iter().zip(args) {
                        if let Some(t) = &p.ty {
                            self.infer(t, a, &decl.type_params, &mut inferred);
                        }
                    }
                }
                let args: Vec<Ty> = decl
                    .type_params
                    .iter()
                    .map(|tp| {
                        inferred
                            .iter()
                            .find(|(n, _)| *n == tp.name)
                            .map(|(_, t)| t.clone().widen())
                            .unwrap_or_else(|| tp.default.as_ref().map_or(Ty::Unknown, |d| self.eval(d, &env)))
                    })
                    .collect();
                Ty::Named(sym.clone(), args)
            }
            Ty::Any => Ty::Any,
            other => match self.signatures(other, true) {
                Some((sigs, env)) => match self.pick(&sigs, &env, args) {
                    Some(sig) => self.sig_result(sig, &env, args, targs),
                    None => Ty::Unknown,
                },
                None => Ty::Unknown,
            },
        }
    }

    /// `await x`: what a promise-like resolves to.
    pub fn awaited(&self, t: &Ty) -> Ty {
        match t {
            Ty::Named(sym, args) if self.decl_name(sym) == Some("Promise".into()) || self.decl_name(sym) == Some("PromiseLike".into()) => {
                args.first().cloned().unwrap_or(Ty::Unknown)
            }
            Ty::Union(items) => Ty::union(items.iter().map(|i| self.awaited(i)).collect()),
            t => t.clone(),
        }
    }
}

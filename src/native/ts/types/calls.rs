//! Calls and construction: signatures, overload choice, simple generic
//! inference from arguments, `await`.

use std::sync::Arc;

use super::super::summary::{MemberKind, Sig, TParam, TypeSyn};
use super::{Checker, Env, Ty};

pub struct ResolvedPredicate {
    pub parameter: Option<usize>,
    pub asserts: bool,
    pub target: Option<Ty>,
}

impl Checker<'_> {
    /// A fallback cannot be used only when every contextual signature requires
    /// this parameter; omitted, optional and rest parameters keep their defaults.
    pub fn required_contextual_parameter(&self, ty: &Ty, index: usize) -> Option<Ty> {
        if let Ty::Union(parts) = ty {
            let types = parts.iter().map(|part| self.required_contextual_parameter(part, index)).collect::<Option<Vec<_>>>()?;
            return (!types.is_empty()).then(|| Ty::union(types));
        }
        let (sigs, env) = self.signatures(ty, false)?;
        let mut types = Vec::new();
        for sig in sigs {
            let param = sig.params.get(index)?;
            if param.optional || param.rest { return None }
            types.push(self.eval(param.ty.as_ref()?, &env));
        }
        (!types.is_empty()).then(|| Ty::union(types))
    }

    /// First-signature parameter used by contextual function typing.
    pub fn contextual_parameter(&self, ty: &Ty, index: usize) -> Ty {
        if let Ty::Union(parts) = ty {
            let parameters: Vec<_> = parts.iter().filter(|part| self.callable_type(part)).map(|part| self.contextual_parameter(part, index)).collect();
            return if parameters.is_empty() { Ty::Unknown } else { Ty::union(parameters) };
        }
        let Some((sigs, env)) = self.signatures(ty, false) else { return Ty::Unknown };
        sigs.first().and_then(|sig| sig.params.get(index)).and_then(|param| param.ty.as_ref()).map_or(Ty::Unknown, |ty| self.eval(ty, &env))
    }

    /// Declared return types of a callable in its bound environment.
    pub fn declared_returns(&self, ty: &Ty) -> Vec<Ty> {
        let Some((sigs, env)) = self.signatures(ty, false) else { return Vec::new() };
        sigs.iter().filter_map(|sig| sig.ret.as_ref().map(|ty| self.eval(ty, &env))).collect()
    }

    pub fn callable_returns(&self, ty: &Ty) -> Vec<Ty> {
        match ty {
            Ty::Union(parts) | Ty::Inter(parts) => parts.iter().flat_map(|part| self.callable_returns(part)).collect(),
            Ty::Fn(callable) if callable.ret.is_some() => callable.ret.iter().cloned().collect(),
            _ => {
                let Some((sigs, env)) = self.signatures(ty, false) else { return vec![Ty::Unknown] };
                sigs.iter().map(|sig| sig.ret.as_ref().map_or(Ty::Unknown, |ty| self.eval(ty, &env))).collect()
            }
        }
    }

    pub fn call_predicate(&self, callee: &Ty, args: &[Ty], targs: Vec<Ty>) -> Option<ResolvedPredicate> {
        let (sigs, env) = self.signatures(callee, false)?;
        let sig = self.pick_matching(&sigs, &env, args, targs.len(), |a, b| a.ret == b.ret && a.is_async == b.is_async && a.predicate == b.predicate)?;
        let predicate = sig.predicate.as_ref()?;
        let env = self.signature_env(sig, &env, args, targs);
        Some(ResolvedPredicate {
            parameter: predicate.parameter,
            asserts: predicate.asserts,
            target: predicate.ty.as_ref().map(|ty| self.eval(ty, &env)),
        })
    }

    pub fn is_thenable(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(|part| self.is_thenable(part)),
            _ => {
                let Some((sigs, env)) = self.signatures(&self.member(ty, "then"), false) else { return false };
                sigs.iter().any(|sig| {
                    sig.params.first().and_then(|param| param.ty.as_ref()).is_some_and(|syn| {
                        let param = self.eval(syn, &env);
                        self.has_call_signature(&param)
                    })
                })
            }
        }
    }

    pub fn callable_type(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| self.callable_type(part)),
            Ty::Inter(parts) => parts.iter().any(|part| self.callable_type(part)),
            _ => self.signatures(ty, false).is_some(),
        }
    }

    fn has_call_signature(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(|part| self.has_call_signature(part)),
            _ => self.signatures(ty, false).is_some(),
        }
    }

    /// The unspecialised `Function` interface and its subtypes are callable
    /// only when they supply the signatures TypeScript considers safe.
    pub fn unsafe_builtin_function(&self, ty: &Ty, construct: bool) -> bool {
        self.is_builtin_type_or_base(ty, "Function") && !self.safe_function_signature(ty, construct, &mut Vec::new())
    }

    fn safe_function_signature(&self, ty: &Ty, construct: bool, seen: &mut Vec<super::super::modules::DeclRef>) -> bool {
        match ty {
            Ty::Named(sym, args) => {
                for &r in sym.iter() {
                    if seen.contains(&r) || seen.len() >= super::MAX_DEPTH as usize { continue }
                    seen.push(r);
                    let Some(summary) = self.summary_of(r.file) else { continue };
                    let decl = &summary.decls[r.decl as usize];
                    let mut env = self.decl_env(r);
                    env.tparams = self.bind_tparams(&decl.type_params, args.clone(), &env);
                    if decl.members.iter().any(|member| {
                        member.kind == MemberKind::Construct
                            || member.kind == MemberKind::Call && (!construct || member.sigs.iter().any(|sig| !matches!(sig.ret.as_ref().map_or(Ty::Unknown, |ret| self.eval(ret, &env)), Ty::Void)))
                    }) { return true }
                    if self.bases(decl).iter().any(|base| self.safe_function_signature(&self.eval(base, &env), construct, seen)) { return true }
                }
                false
            }
            Ty::Inter(parts) => parts.iter().any(|part| self.safe_function_signature(part, construct, seen)),
            Ty::Fn(callable) => !construct || callable.ret.as_ref().is_some_and(|ret| !matches!(ret, Ty::Void))
                || callable.sigs.iter().any(|sig| !matches!(sig.ret.as_ref().map_or(Ty::Unknown, |ret| self.eval(ret, &callable.env)), Ty::Void)),
            Ty::Class(_) => true,
            _ => false,
        }
    }

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
    fn pick<'s>(&self, sigs: &'s [Arc<Sig>], env: &Env, args: &[Ty], type_arguments: usize) -> Option<&'s Arc<Sig>> {
        self.pick_matching(sigs, env, args, type_arguments, |a, b| a.ret == b.ret && a.is_async == b.is_async)
    }

    fn pick_matching<'s>(&self, sigs: &'s [Arc<Sig>], env: &Env, args: &[Ty], type_arguments: usize, equivalent: impl Fn(&Sig, &Sig) -> bool) -> Option<&'s Arc<Sig>> {
        // An implementation behind overloads is not callable from outside.
        let overloaded = sigs.iter().any(|s| s.body_at.is_none());
        let visible = |s: &&Arc<Sig>| (!overloaded || s.body_at.is_none()) && (type_arguments == 0 || s.type_params.len() == type_arguments);
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
                Some((first, rest)) if rest.iter().all(|s| equivalent(s, first)) => Some(first),
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
                    Ty::Array(e) => *e.element,
                    Ty::Tuple(items) => items.at(i - (sig.params.len() - 1)),
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
                return if results.iter().any(Ty::is_opaque) { Ty::Unknown } else { Ty::union(results) };
            }
            Ty::Fn(c) if c.ret.is_some() => return c.ret.clone().unwrap_or(Ty::Unknown),
            _ => {}
        }
        let Some((sigs, env)) = self.signatures(f, false) else { return Ty::Unknown };
        let Some(sig) = self.pick(&sigs, &env, args, targs.len()) else { return Ty::Unknown };
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
        Some((self.pick(&sigs, &env, args, 0)?.clone(), env))
    }

    fn signature_env(&self, sig: &Sig, env: &Env, args: &[Ty], targs: Vec<Ty>) -> Env {
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
                    .unwrap_or_else(|| match &tp.default {
                        Some(default) => self.eval(default, &env),
                        None => match tp.constraint.as_ref().map_or(Ty::Unknown, |constraint| self.eval(constraint, &env)) {
                            Ty::Any => Ty::Unknown,
                            bound => bound,
                        },
                    });
                env.tparams.push((tp.name.clone(), t));
            }
        } else {
            env.tparams = self.bind_tparams(&sig.type_params, targs, &env);
        }
        env
    }

    /// Bound parameter types, with the final rest parameter kept as its array or tuple.
    pub fn argument_parameters(&self, callee: &Ty, args: &[Ty], targs: Vec<Ty>, construct: bool) -> Option<(Vec<Ty>, bool)> {
        let (sig, env) = self.signature_for(callee, args, construct)?;
        let env = self.signature_env(&sig, &env, args, targs);
        Some((sig.params.iter().map(|param| param.ty.as_ref().map_or(Ty::Unknown, |ty| self.eval(ty, &env))).collect(), sig.params.last().is_some_and(|param| param.rest)))
    }

    pub fn sig_result(&self, sig: &Sig, env: &Env, args: &[Ty], targs: Vec<Ty>) -> Ty {
        let env = self.signature_env(sig, env, args, targs);
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
            (TypeSyn::Readonly(inner), _) => self.infer(inner, actual, params, out),
            (TypeSyn::Ref(path, args), _) if path.len() == 1 && args.is_empty() && params.iter().any(|p| *p.name == *path[0]) => {
                if !actual.is_opaque() && !out.iter().any(|(n, _)| *n == path[0]) {
                    out.push((path[0].clone(), actual.clone()));
                }
            }
            (TypeSyn::Array(inner), Ty::Array(a)) => self.infer(inner, &a.element, params, out),
            (TypeSyn::Ref(path, args), Ty::Array(a)) if args.len() == 1 && matches!(&*path[0], "Array" | "ReadonlyArray" | "Iterable") => {
                self.infer(&args[0], &a.element, params, out)
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
                if let Some(sig) = self.pick(&decl.sigs, &env, args, 0) {
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
                Some((sigs, env)) => match self.pick(&sigs, &env, args, targs.len()) {
                    Some(sig) => self.sig_result(sig, &env, args, targs),
                    None => Ty::Unknown,
                },
                None => Ty::Unknown,
            },
        }
    }

    /// `await x`: what a promise-like resolves to.
    pub fn awaited(&self, t: &Ty) -> Ty {
        self.awaited_at(t, 0)
    }

    fn awaited_at(&self, t: &Ty, depth: usize) -> Ty {
        if depth >= super::MAX_DEPTH as usize { return Ty::Unknown }
        match t {
            Ty::Named(sym, args) if self.is_builtin_symbol(sym, "Promise") || self.is_builtin_symbol(sym, "PromiseLike") => {
                args.first().map_or(Ty::Unknown, |ty| self.awaited_at(ty, depth + 1))
            }
            Ty::Union(items) => Ty::union(items.iter().map(|i| self.awaited_at(i, depth + 1)).collect()),
            Ty::Inter(items) => {
                let thenable: Vec<_> = items.iter().filter(|ty| self.is_thenable(ty)).collect();
                if thenable.is_empty() { t.clone() } else { Ty::union(thenable.into_iter().map(|ty| self.awaited_at(ty, depth + 1)).collect()) }
            }
            _ => {
                let Some((sigs, env)) = self.signatures(&self.member(t, "then"), false) else { return t.clone() };
                let values: Vec<_> = sigs.iter().filter_map(|sig| {
                    let callback = self.eval(sig.params.first()?.ty.as_ref()?, &env);
                    fn callback_values(ck: &Checker<'_>, callback: &Ty, values: &mut Vec<Ty>) {
                        match callback {
                            Ty::Union(parts) => for part in parts { callback_values(ck, part, values) },
                            _ if ck.signatures(callback, false).is_some() => values.push(ck.contextual_parameter(callback, 0)),
                            _ => {}
                        }
                    }
                    let mut values = Vec::new();
                    callback_values(self, &callback, &mut values);
                    (!values.is_empty()).then(|| Ty::union(values))
                }).collect();
                if values.is_empty() { t.clone() } else { Ty::union(values.iter().map(|value| self.awaited_at(value, depth + 1)).collect()) }
            }
        }
    }
}

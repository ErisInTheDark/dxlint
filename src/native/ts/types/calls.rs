//! Calls and construction: signatures, overload choice, simple generic
//! inference from arguments, `await`.

use std::sync::Arc;

use super::super::summary::{MemberKind, Sig, TParam, TypeSyn};
use super::{BoundSignature, Checker, Env, Ty};

#[cfg(test)]
thread_local! {
    static CONTRACT_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct ContractQuery {
    remaining: usize,
    seen: Vec<Ty>,
}

impl ContractQuery {
    fn take(&mut self, work: usize) -> Option<()> {
        self.remaining = self.remaining.checked_sub(work)?;
        Some(())
    }
}

pub struct ResolvedPredicate {
    pub parameter: Option<usize>,
    pub asserts: bool,
    pub target: Option<Ty>,
}

impl ResolvedPredicate {
    fn agrees(&self, other: &Self) -> bool {
        self.parameter == other.parameter && self.asserts == other.asserts && match (&self.target, &other.target) {
            (None, None) => true,
            (Some(a), Some(b)) => a.same_identity(b),
            _ => false,
        }
    }
}

impl Checker<'_> {
    /// Explicit instantiation binds each overload before later calls infer arguments.
    pub fn instantiate(&self, value: &Ty, arguments: Vec<Ty>) -> Ty {
        match value {
            Ty::Any => Ty::Any,
            Ty::Fn(callable) if callable.ret.is_none() => {
                let overloaded = callable.sigs.iter().any(|bound| bound.syntax.body_at.is_none());
                let sigs = callable.sigs.iter().filter_map(|bound| {
                    if overloaded && bound.syntax.body_at.is_some() { return None }
                    let parameters = &bound.syntax.type_params;
                    let required = parameters.iter().filter(|parameter| parameter.default.is_none()).count();
                    if arguments.len() < required || arguments.len() > parameters.len() { return None }
                    let mut env = (*bound.env).clone();
                    env.tparams = self.bind_tparams(parameters, arguments.clone(), &env);
                    let mut syntax = (*bound.syntax).clone();
                    syntax.type_params.clear();
                    Some(BoundSignature { syntax: Arc::new(syntax), env: Arc::new(env) })
                }).collect::<Vec<_>>();
                if sigs.is_empty() { Ty::Unknown } else {
                    Ty::Fn(Arc::new(super::Callable { sigs, ret: None }))
                }
            }
            Ty::Inter(parts) => Ty::Inter(parts.iter().map(|part| {
                if matches!(part, Ty::Fn(_)) { self.instantiate(part, arguments.clone()) } else { part.clone() }
            }).collect()),
            _ => Ty::Unknown,
        }
    }

    /// A fallback cannot be used only when every contextual signature requires
    /// this parameter; omitted, optional and rest parameters keep their defaults.
    pub fn required_contextual_parameter(&self, ty: &Ty, index: usize) -> Option<Ty> {
        if let Ty::Union(parts) = ty {
            let types = parts.iter().map(|part| self.required_contextual_parameter(part, index)).collect::<Option<Vec<_>>>()?;
            return (!types.is_empty()).then(|| Ty::union(types));
        }
        let sigs = self.signatures(ty, false)?;
        let mut types = Vec::new();
        for sig in sigs {
            let param = sig.syntax.params.get(index)?;
            if param.optional || param.rest { return None }
            types.push(self.eval(param.ty.as_ref()?, &sig.env));
        }
        (!types.is_empty()).then(|| Ty::union(types))
    }

    /// First-signature parameter used by contextual function typing.
    pub fn contextual_parameter(&self, ty: &Ty, index: usize) -> Ty {
        if let Ty::Union(parts) = ty {
            let parameters: Vec<_> = parts.iter().filter(|part| self.callable_type(part)).map(|part| self.contextual_parameter(part, index)).collect();
            return if parameters.is_empty() { Ty::Unknown } else { Ty::union(parameters) };
        }
        let Some(sigs) = self.signatures(ty, false) else { return Ty::Unknown };
        sigs.first().map_or(Ty::Unknown, |sig| sig.syntax.params.get(index).and_then(|param| param.ty.as_ref()).map_or(Ty::Unknown, |ty| self.eval(ty, &sig.env)))
    }

    /// Declared return types of a callable in its bound environment.
    pub fn declared_returns(&self, ty: &Ty) -> Vec<Ty> {
        let Some(sigs) = self.signatures(ty, false) else { return Vec::new() };
        sigs.iter().filter_map(|sig| sig.syntax.ret.as_ref().map(|ty| self.eval(ty, &sig.env))).collect()
    }

    pub fn callable_returns(&self, ty: &Ty) -> Vec<Ty> {
        match ty {
            Ty::Union(parts) | Ty::Inter(parts) => parts.iter().flat_map(|part| self.callable_returns(part)).collect(),
            Ty::Fn(callable) if callable.ret.is_some() => callable.ret.iter().cloned().collect(),
            _ => {
                let Some(sigs) = self.signatures(ty, false) else { return vec![Ty::Unknown] };
                sigs.iter().map(|sig| sig.syntax.ret.as_ref().map_or(Ty::Unknown, |ty| self.eval(ty, &sig.env))).collect()
            }
        }
    }

    pub fn call_predicate(&self, callee: &Ty, args: &[Ty], targs: Vec<Ty>) -> Option<ResolvedPredicate> {
        if let Ty::Union(parts) = callee {
            let mut parts = parts.iter();
            let first = self.call_predicate(parts.next()?, args, targs.clone())?;
            return parts.all(|part| self.call_predicate(part, args, targs.clone()).is_some_and(|other| first.agrees(&other))).then_some(first);
        }
        let sigs = self.signatures(callee, false)?;
        let sig = self.pick_matching(&sigs, args, targs.len(), false, |a, b| {
            if !self.equivalent_returns(a, b, args, &targs) { return false }
            if Arc::ptr_eq(&a.env, &b.env) { return a.syntax.predicate == b.syntax.predicate }
            match (self.signature_predicate(a, args, targs.clone()), self.signature_predicate(b, args, targs.clone())) {
                (None, None) => true,
                (Some(a), Some(b)) => a.agrees(&b),
                _ => false,
            }
        })?;
        self.signature_predicate(sig, args, targs)
    }

    fn signature_predicate(&self, sig: &BoundSignature, args: &[Ty], targs: Vec<Ty>) -> Option<ResolvedPredicate> {
        let predicate = sig.syntax.predicate.as_ref()?;
        let env = self.signature_env(&sig.syntax, &sig.env, args, targs);
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
                let Some(sigs) = self.signatures(&self.member(ty, "then"), false) else { return false };
                sigs.iter().any(|sig| {
                    sig.syntax.params.first().and_then(|param| param.ty.as_ref()).is_some_and(|syn| {
                        let param = self.eval(syn, &sig.env);
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

    pub(crate) fn has_call_signature(&self, ty: &Ty) -> bool {
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
                || callable.sigs.iter().any(|sig| !matches!(sig.syntax.ret.as_ref().map_or(Ty::Unknown, |ret| self.eval(ret, &sig.env)), Ty::Void)),
            Ty::Class(_) => true,
            _ => false,
        }
    }

    /// Signatures `t` can be called with (or constructed with).
    fn signatures(&self, t: &Ty, construct: bool) -> Option<Vec<BoundSignature>> {
        match t {
            Ty::Fn(c) if !construct => Some(c.sigs.clone()),
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
                        return Some(super::Callable::new(sigs, Arc::new(env), None).sigs);
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
    fn pick<'s>(&self, sigs: &'s [BoundSignature], args: &[Ty], type_arguments: usize) -> Option<&'s BoundSignature> {
        self.pick_matching(sigs, args, type_arguments, true, |a, b| self.equivalent_returns(a, b, args, &[]))
    }

    fn equivalent_returns(&self, a: &BoundSignature, b: &BoundSignature, args: &[Ty], targs: &[Ty]) -> bool {
        if Arc::ptr_eq(&a.env, &b.env) {
            a.syntax.ret == b.syntax.ret && a.syntax.is_async == b.syntax.is_async
        } else {
            self.sig_result(&a.syntax, &a.env, args, targs.to_vec()).same_identity(&self.sig_result(&b.syntax, &b.env, args, targs.to_vec()))
        }
    }

    fn pick_matching<'s>(&self, sigs: &'s [BoundSignature], args: &[Ty], type_arguments: usize, trust_any: bool, equivalent: impl Fn(&BoundSignature, &BoundSignature) -> bool) -> Option<&'s BoundSignature> {
        // An implementation behind overloads is not callable from outside.
        let overloaded = sigs.iter().any(|s| s.syntax.body_at.is_none());
        let visible = |s: &&BoundSignature| (!overloaded || s.syntax.body_at.is_none()) && (type_arguments == 0 || s.syntax.type_params.len() == type_arguments);
        let fits = |s: &&BoundSignature| {
            let required = s.syntax.params.iter().filter(|p| !p.optional && !p.rest).count();
            let rest = s.syntax.params.iter().any(|p| p.rest);
            visible(s) && args.len() >= required && (rest || args.len() <= s.syntax.params.len())
        };
        let candidates: Vec<&BoundSignature> = sigs.iter().filter(fits).collect();
        let definite = trust_any || !args.iter().any(|arg| matches!(arg, Ty::Any));
        if candidates.len() > 1 {
            // Any is assignable to an overload's parameters, not evidence
            // that its assertion contract is the one the call selects.
            if definite {
                if let Some(&s) = candidates.iter().find(|s| self.accepts(&s.syntax, &s.env, args, true)) {
                    return Some(s);
                }
            }
            // Not provably one overload: only a choice when every
            // overload still possible returns the same.
            let possible: Vec<&BoundSignature> = candidates.iter().copied().filter(|s| self.accepts(&s.syntax, &s.env, args, false)).collect();
            return match possible.split_first() {
                Some((first, rest)) if rest.iter().all(|s| equivalent(s, first)) => Some(first),
                Some(_) => None,
                None => if definite { candidates.first().copied() } else { None },
            };
        }
        candidates.first().copied().or_else(|| if definite { sigs.iter().find(visible) } else { None })
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
                Some(sigs) => sigs.len() > 1 || sigs.iter().any(|s| !s.syntax.type_params.is_empty()),
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
        let Some(sigs) = self.signatures(f, false) else { return Ty::Unknown };
        let Some(sig) = self.pick(&sigs, args, targs.len()) else { return Ty::Unknown };
        self.sig_result(&sig.syntax, &sig.env, args, targs)
    }

    /// The signature a callee resolves to for these arguments, with its env.
    pub fn signature_for(&self, f: &Ty, args: &[Ty], construct: bool) -> Option<(Arc<Sig>, Arc<Env>)> {
        let sigs = match (f, construct) {
            (Ty::Class(sym), true) => {
                let r = *sym.first()?;
                let s = self.summary_of(r.file)?;
                let decl = &s.decls[r.decl as usize];
                super::Callable::new(decl.sigs.clone(), Arc::new(self.decl_env(r)), None).sigs
            }
            _ => self.signatures(f, construct)?,
        };
        let sig = self.pick(&sigs, args, 0)?;
        Some((sig.syntax.clone(), sig.env.clone()))
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

    /// Declared parameter contracts, before inferring from the values being checked.
    /// Callback contexts keep parameters open; transfer contexts use their upper bounds.
    pub fn argument_context_parameters(&self, callee: &Ty, args: &[Ty], targs: Vec<Ty>, construct: bool, bound: bool) -> Option<(Vec<Ty>, bool)> {
        let (sig, env) = self.signature_for(callee, args, construct)?;
        let mut env = (*env).clone();
        let signature_args = if let (Ty::Class(symbol), true) = (callee, construct) {
            let reference = *symbol.first()?;
            let summary = self.summary_of(reference.file)?;
            self.context_parameters(&summary.decls[reference.decl as usize].type_params, targs, bound, &mut env);
            Vec::new()
        } else { targs };
        self.context_parameters(&sig.type_params, signature_args, bound, &mut env);
        Some((sig.params.iter().map(|param| param.ty.as_ref().map_or(Ty::Unknown, |ty| self.eval(ty, &env))).collect(), sig.params.last().is_some_and(|param| param.rest)))
    }

    fn context_parameters(&self, parameters: &[TParam], arguments: Vec<Ty>, bound: bool, env: &mut Env) {
        let explicit = !arguments.is_empty();
        let mut arguments = arguments.into_iter();
        for parameter in parameters {
            let ty = if explicit {
                arguments.next().unwrap_or_else(|| parameter.default.as_ref().map_or(Ty::Unknown, |default| self.eval(default, env)))
            } else if bound {
                parameter.constraint.as_ref().map_or_else(|| Ty::Param(parameter.name.clone()), |constraint| self.eval(constraint, env))
            } else { Ty::Param(parameter.name.clone()) };
            env.tparams.push((parameter.name.clone(), ty));
        }
    }

    /// One query bounds inherited graph, signature and parameter work together.
    pub(crate) const CONTRACT_WORK_LIMIT: usize = 4096;

    /// Parameter contracts from every visible signature, independent of overload selection.
    pub fn argument_contract_parameters(&self, callee: &Ty, args: &[Ty], targs: Vec<Ty>, construct: bool) -> Option<Vec<(Vec<Ty>, bool)>> {
        let mut query = ContractQuery { remaining: Self::CONTRACT_WORK_LIMIT, seen: Vec::new() };
        self.contract_parameters_at(callee, args, targs, construct, 0, &mut query)
    }

    fn contract_parameters_at(&self, callee: &Ty, args: &[Ty], targs: Vec<Ty>, construct: bool, depth: usize, query: &mut ContractQuery) -> Option<Vec<(Vec<Ty>, bool)>> {
        #[cfg(test)]
        CONTRACT_WORK.with(|work| work.set(work.get() + 1));
        if depth >= super::MAX_DEPTH as usize { return None }
        query.take(1)?;
        if query.seen.iter().any(|seen| seen.same_identity(callee)) { return Some(Vec::new()) }
        query.seen.push(callee.clone());
        match callee {
            Ty::Union(parts) | Ty::Inter(parts) => {
                let mut contracts = Vec::new();
                for part in parts {
                    contracts.extend(self.contract_parameters_at(part, args, targs.clone(), construct, depth + 1, query)?);
                }
                Some(contracts)
            }
            Ty::Named(symbol, actuals) => {
                let kind = if construct { MemberKind::Construct } else { MemberKind::Call };
                let mut groups = Vec::new();
                let mut inherited = Vec::new();
                for &reference in symbol.iter() {
                    let summary = self.summary_of(reference.file)?;
                    let declaration = &summary.decls[reference.decl as usize];
                    query.take(declaration.members.len() + declaration.type_params.len())?;
                    let mut env = self.decl_env(reference);
                    env.tparams = self.bind_tparams(&declaration.type_params, actuals.clone(), &env);
                    let signatures = declaration.members.iter().filter(|member| member.kind == kind)
                        .flat_map(|member| member.sigs.iter().cloned()).collect();
                    for base in self.bases(declaration) {
                        inherited.extend(self.contract_parameters_at(&self.eval(base, &env), args, targs.clone(), construct, depth + 1, query)?);
                    }
                    groups.push(super::Callable::new(signatures, Arc::new(env), None).sigs);
                }
                inherited.extend(self.parameter_contract_groups(groups, args, targs, query)?);
                Some(inherited)
            }
            Ty::Class(symbol) if construct => {
                let reference = *symbol.first()?;
                let summary = self.summary_of(reference.file)?;
                let declaration = &summary.decls[reference.decl as usize];
                let mut env = self.decl_env(reference);
                env.tparams = self.bind_tparams(&declaration.type_params, targs, &env);
                self.parameter_contract_groups(vec![super::Callable::new(declaration.sigs.clone(), Arc::new(env), None).sigs], args, Vec::new(), query)
            }
            Ty::Fn(callable) if !construct =>
                self.parameter_contract_groups(vec![callable.sigs.clone()], args, targs, query),
            Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => None,
            _ => Some(Vec::new()),
        }
    }

    fn parameter_contract_groups(&self, groups: Vec<Vec<BoundSignature>>, args: &[Ty], targs: Vec<Ty>, query: &mut ContractQuery) -> Option<Vec<(Vec<Ty>, bool)>> {
        for signatures in &groups { query.take(signatures.len())?; }
        let overloaded = groups.iter().flatten().any(|signature| signature.syntax.body_at.is_none());
        let mut contracts = Vec::new();
        for signatures in groups {
            for bound in signatures {
                let signature = &bound.syntax;
                if overloaded && signature.body_at.is_some() || !targs.is_empty() && signature.type_params.len() != targs.len() { continue }
                query.take(signature.params.len() + signature.type_params.len())?;
                let env = self.signature_env(signature, &bound.env, args, targs.clone());
                contracts.push((signature.params.iter().map(|parameter| parameter.ty.as_ref().map_or(Ty::Unknown, |ty| self.eval(ty, &env))).collect(),
                    signature.params.last().is_some_and(|parameter| parameter.rest)));
            }
        }
        Some(contracts)
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
                    let ret = match (&c.ret, &asig.syntax.ret) {
                        (Some(t), _) => t.clone(),
                        (None, Some(ar)) => self.eval(ar, &asig.env),
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
                let sigs = super::Callable::new(decl.sigs.clone(), Arc::new(env.clone()), None).sigs;
                if let Some(sig) = self.pick(&sigs, args, 0) {
                    for (p, a) in sig.syntax.params.iter().zip(args) {
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
                Some(sigs) => match self.pick(&sigs, args, targs.len()) {
                    Some(sig) => self.sig_result(&sig.syntax, &sig.env, args, targs),
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
                let Some(sigs) = self.signatures(&self.member(t, "then"), false) else { return t.clone() };
                let values: Vec<_> = sigs.iter().filter_map(|sig| {
                    let callback = self.eval(sig.syntax.params.first()?.ty.as_ref()?, &sig.env);
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

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use crate::native::ts::facts::NativeFile;
    use crate::native::ts::modules::DeclRef;
    use super::Ty;

    fn merged_function(file: &NativeFile<'_>, name: &str) -> Ty {
        let checker = &file.typing().checker;
        let declarations = checker.summary.decls.iter().enumerate().filter(|(_, declaration)| &*declaration.name == name)
            .map(|(decl, _)| DeclRef { file: checker.file, decl: decl as u32 }).collect::<Vec<_>>();
        checker.value_of(&declarations)
    }

    #[test]
    fn explicit_instantiations_keep_bound_returns_without_changing_inference() {
        let source = r#"
            declare function make<T>(): T;
            declare const queried: typeof make<string>;
            const specialized = make<string>;
            declare function identity<T>(value: T): T;
            /*@*/ queried();
            /*@*/ specialized();
            /*@*/ identity(1);
        "#;
        with_native_file(source, |file, marks| {
            for (mark, expected) in marks.iter().zip(["string", "string", "number"]) {
                let expression = file.syntax.get(*mark, crate::native::ts::kinds::Field::Expr).unwrap();
                assert!(matches!(file.type_of(expression), Ty::Prim(kind) if kind == expected),
                    "{expected}: {:?}", file.type_of(expression));
            }
        });
    }

    #[test]
    fn instantiated_overloads_keep_each_signature_type_parameter_order() {
        let source = r#"
            declare function make<T, U>(tag: "first"): T;
            declare function make<U, T>(tag: "second"): T;
            const specialized = make<string, number>;
            /*@*/ specialized("first");
            /*@*/ specialized("second");
        "#;
        with_native_file(source, |file, marks| {
            for (mark, expected) in marks.iter().zip(["string", "number"]) {
                let expression = file.syntax.get(*mark, crate::native::ts::kinds::Field::Expr).unwrap();
                assert!(matches!(file.type_of(expression), Ty::Prim(kind) if kind == expected),
                    "{expected}: {:?}", file.type_of(expression));
            }
        });
    }

    #[test]
    fn instantiated_overloads_do_not_guess_a_return_for_unknown_arguments() {
        with_native_file(r#"
            declare function make<T, U>(tag: "first"): T;
            declare function make<U, T>(tag: "second"): T;
            const specialized = make<string, number>;
            declare const tag: Missing;
            /*@*/ specialized(tag);
        "#, |file, marks| {
            let expression = file.syntax.get(marks[0], crate::native::ts::kinds::Field::Expr).unwrap();
            assert!(matches!(file.type_of(expression), Ty::Unknown), "{:?}", file.type_of(expression));
        });
    }

    #[test]
    fn instantiated_predicates_require_agreement_of_bound_targets() {
        with_native_file(r#"
            declare function check<T, U>(tag: "first", value: unknown): value is T;
            declare function check<U, T>(tag: "second", value: unknown): value is T;
            const specialized = check<string, number>;
            /*@*/ specialized;
        "#, |file, marks| {
            let expression = file.syntax.get(marks[0], crate::native::ts::kinds::Field::Expr).unwrap();
            let callee = file.type_of(expression);
            let checker = &file.typing().checker;
            assert!(checker.call_predicate(&callee, &[Ty::Unknown, Ty::IntrinsicUnknown], Vec::new()).is_none());
            let predicate = checker.call_predicate(&callee, &[Ty::Lit("\"second\"".into(), "string"), Ty::IntrinsicUnknown], Vec::new()).unwrap();
            assert!(matches!(predicate.target, Some(Ty::Prim("number"))));
        });
    }

    #[test]
    fn unsupported_instantiation_shapes_do_not_publish_unbound_types() {
        with_native_file(r#"
            declare class Box<T> { value: T }
            interface Callable { <T>(): T; label: string }
            declare const callable: Callable;
            const inferred = <T>(value: T) => value;
            /*@*/ Box<string>;
            /*@*/ callable<string>;
            /*@*/ inferred<string>;
        "#, |file, marks| {
            for mark in marks {
                let expression = file.syntax.get(*mark, crate::native::ts::kinds::Field::Expr).unwrap();
                assert!(matches!(file.type_of(expression), Ty::Unknown), "{:?}", file.type_of(expression));
            }
        });
    }

    #[test]
    fn instantiation_cannot_expose_an_overload_implementation() {
        with_native_file(r#"
            function make(value: number): number;
            function make<T>(value: T): T { return value; }
            /*@*/ make<string>;
        "#, |file, marks| {
            let expression = file.syntax.get(marks[0], crate::native::ts::kinds::Field::Expr).unwrap();
            assert!(matches!(file.type_of(expression), Ty::Unknown), "{:?}", file.type_of(expression));
        });
    }

    #[test]
    fn callable_contract_queries_bound_repeated_heritage_work() {
        const LEVELS: usize = 12;
        let mut source = "interface Root { (callback: () => void): void }".to_owned();
        let mut base = "Root".to_owned();
        for level in 0..LEVELS {
            source.push_str(&format!("interface Left{level} extends {base} {{}} interface Right{level} extends {base} {{}} interface Join{level} extends Left{level}, Right{level} {{}}"));
            base = format!("Join{level}");
        }
        source.push_str(&format!("declare const call: {base}; /*@*/call;"));
        with_native_file(&source, |file, marks| {
            let expression = file.syntax.get(marks[0], crate::native::ts::kinds::Field::Expr).unwrap();
            let checker = &file.typing().checker;
            super::CONTRACT_WORK.with(|work| work.set(0));
            let contracts = checker.argument_contract_parameters(&file.type_of(expression), &[], Vec::new(), false).unwrap();
            assert!(contracts.iter().any(|(parameters, _)| parameters.first().is_some_and(|parameter|
                file.callable_return_types(parameter).iter().any(|result| matches!(result, Ty::Void)))));
            let work = super::CONTRACT_WORK.with(|work| work.get());
            assert!(work <= 4 * (1 + 3 * LEVELS), "{work} expansions for {} declarations", 1 + 3 * LEVELS);
        });
    }

    #[test]
    fn predicates_require_agreement_when_any_cannot_choose_an_overload() {
        for (fallback, expected) in [
            ("void", false),
            ("asserts value", true),
            ("value is string", false),
        ] {
            let source = format!("function check(first: number, value: unknown): asserts value; function check(first: string, value: unknown): {fallback}; function check(first: any, value: any) {{ return true; }}");
            with_native_file(&source, |file, _| {
                let checker = &file.typing().checker;
                let callee = merged_function(file, "check");
                let result = checker.call_predicate(&callee, &[Ty::Any, Ty::IntrinsicUnknown], Vec::new());
                assert_eq!(result.is_some(), expected);
                let selected = checker.call_predicate(&callee, &[Ty::Prim("number"), Ty::IntrinsicUnknown], Vec::new()).expect("known argument chooses the assertion");
                assert!(selected.asserts);
                assert_eq!(selected.parameter, Some(1));
            });
        }
        with_native_file("function result(value: number): string; function result(value: string): number; function result(value: any) { return ''; }", |file, _| {
            let checker = &file.typing().checker;
            assert!(matches!(checker.call(&merged_function(file, "result"), &[Ty::Any], Vec::new()), Ty::Prim("string")));
        });
    }

    #[test]
    fn union_predicates_require_matching_contracts_from_every_arm() {
        for (other, expected) in [
            ("value is string", true),
            ("other is string", false),
            ("value is number", false),
            ("asserts value", false),
            ("boolean", false),
        ] {
            let source = format!("declare function first(value: unknown, other: unknown): value is string; declare function second(value: unknown, other: unknown): {other};");
            with_native_file(&source, |file, _| {
                let checker = &file.typing().checker;
                let first = merged_function(file, "first");
                let second = merged_function(file, "second");
                let union = Ty::Union(vec![first.clone(), second]);
                let arguments = [Ty::IntrinsicUnknown, Ty::IntrinsicUnknown];
                let result = checker.call_predicate(&union, &arguments, Vec::new());
                assert_eq!(result.is_some(), expected);
                if let Some(result) = result {
                    assert_eq!(result.parameter, Some(0));
                    assert!(!result.asserts);
                    assert!(matches!(result.target, Some(Ty::Prim("string"))));
                }
                assert!(checker.call_predicate(&Ty::Union(vec![first, Ty::Unknown]), &arguments, Vec::new()).is_none());
            });
        }
    }
}

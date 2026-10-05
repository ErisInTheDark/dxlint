//! Types of expressions in the file being linted: bottom-up from literals,
//! declarations and annotations, through the declared-type evaluator.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::modules::{DeclRef, ModuleAt};
use super::summary::{sig, type_syn, DeclKind, Sig, TParam, TypeSyn};
use super::types::{Callable, Checker, Env, ResolvedPredicate, Ty};

fn prim_flag(p: &str) -> &'static str {
    match p {
        "string" => "string",
        "number" => "number",
        "boolean" => "boolean",
        "bigint" => "bigint",
        "symbol" => "symbol",
        _ => "object",
    }
}

/// Printing tells this type apart from every other.
fn prints_exactly(t: &Ty) -> bool {
    match t {
        Ty::Obj(..) | Ty::Fn(_) | Ty::Module(_) => false,
        Ty::Named(_, args) => args.iter().all(prints_exactly),
        Ty::Union(items) | Ty::Inter(items) => items.iter().all(prints_exactly),
        Ty::Tuple(items) => items.iter().all(prints_exactly),
        Ty::Struct(props) => props.iter().all(|(_, t)| prints_exactly(t)),
        Ty::Array(e) => prints_exactly(&e.element),
        _ => true,
    }
}

/// `any` (or a part zlint cannot read) anywhere inside.
fn mentions_any_or_unknown(t: &Ty) -> bool {
    match t {
        Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown => true,
        Ty::Named(_, args) => args.iter().any(mentions_any_or_unknown),
        Ty::Union(items) | Ty::Inter(items) => items.iter().any(mentions_any_or_unknown),
        Ty::Tuple(items) => items.iter().any(mentions_any_or_unknown),
        Ty::Array(e) => mentions_any_or_unknown(&e.element),
        _ => false,
    }
}

/// Type state of one linted file, created on the first type question.
pub struct Typing<'t> {
    pub checker: Checker<'t>,
    memo: RefCell<HashMap<u32, Ty>>,
    /// Nested `type_of` calls (`a + b + c + ...` nests one per operand).
    depth: Cell<u32>,
    /// Return types of this file's unannotated functions, by node.
    pub(super) returns: RefCell<HashMap<u32, Ty>>,
    /// Function-like nodes by start offset.
    pub(super) functions: OnceCell<HashMap<u32, u32>>,
    /// Indices of each block's early-exit `if`s (narrowing).
    pub(super) exits: RefCell<HashMap<u32, Vec<usize>>>,
}

/// Deeper expressions than this are unknown (protects the stack).
const MAX_DEPTH: u32 = 120;

impl<'t> Typing<'t> {
    pub fn new(checker: Checker<'t>) -> Self {
        Typing {
            checker,
            memo: RefCell::new(HashMap::new()),
            depth: Cell::new(0),
            returns: RefCell::new(HashMap::new()),
            functions: OnceCell::new(),
            exits: RefCell::new(HashMap::new()),
        }
    }
}

impl<'t> NativeFile<'t> {
    /// The type of expression `id` (`Unknown` when it cannot tell).
    pub fn type_of(&self, id: u32) -> Ty {
        let typing = self.typing();
        if let Some(t) = typing.memo.borrow().get(&id) {
            return t.clone();
        }
        let depth = typing.depth.get();
        if depth >= MAX_DEPTH {
            return Ty::Unknown;
        }
        typing.memo.borrow_mut().insert(id, Ty::Unknown);
        typing.depth.set(depth + 1);
        let t = self.compute(id);
        typing.depth.set(depth);
        typing.memo.borrow_mut().insert(id, t.clone());
        t
    }

    /// The upper bound of open type parameters, without changing their
    /// identity in the ordinary type cache.
    pub fn constrained_type_of(&self, id: u32) -> Ty {
        self.constrained_type(self.type_of(id), id)
    }

    pub(super) fn constrained_type(&self, ty: Ty, at: u32) -> Ty {
        self.constrain_type(ty, at, &mut Vec::new())
    }

    pub fn type_parameter_bound(&self, name: &str, at: u32) -> Option<Ty> {
        let s = &self.syntax;
        let mut node = Some(at);
        while let Some(n) = node {
            if let Some(param) = s.all(n, F::TypeParam).find(|&tp| s.get(tp, F::Name).is_some_and(|id| s.text(id) == name)) {
                return s.get(param, F::Constraint).map(|constraint| self.annotation(constraint));
            }
            node = s.parent(n);
        }
        None
    }

    fn constrain_type(&self, ty: Ty, at: u32, visiting: &mut Vec<Box<str>>) -> Ty {
        match ty {
            Ty::Param(name) => {
                if visiting.contains(&name) {
                    return Ty::Param(name);
                }
                if let Some(bound) = self.type_parameter_bound(&name, at) {
                    visiting.push(name);
                    let bound = self.constrain_type(bound, at, visiting);
                    visiting.pop();
                    return if matches!(bound, Ty::Any) { Ty::Unknown } else { bound };
                }
                Ty::Param(name)
            }
            Ty::Array(array) => Ty::Array(super::types::Array { element: Box::new(self.constrain_type(*array.element, at, visiting)), readonly: array.readonly }),
            Ty::Tuple(parts) => Ty::Tuple(parts.map_types(|part| self.constrain_type(part, at, visiting))),
            Ty::Union(parts) => Ty::union(parts.into_iter().map(|part| self.constrain_type(part, at, visiting)).collect()),
            Ty::Inter(parts) => Ty::Inter(parts.into_iter().map(|part| self.constrain_type(part, at, visiting)).collect()),
            Ty::Named(sym, args) => Ty::Named(sym, args.into_iter().map(|arg| self.constrain_type(arg, at, visiting)).collect()),
            other => other,
        }
    }

    fn ck(&self) -> &Checker<'t> {
        &self.typing().checker
    }

    /// The environment for annotations written at `at`: enclosing type
    /// parameters (unresolved) and namespaces.
    pub fn env_at(&self, at: u32) -> Env {
        let s = &self.syntax;
        let ck = self.ck();
        let mut env = Env::file(ck.file);
        let mut node = Some(at);
        let mut tparams = Vec::new();
        let mut namespaces = Vec::new();
        while let Some(n) = node {
            for tp in s.all(n, F::TypeParam) {
                if let Some(name) = s.get(tp, F::Name) {
                    let name: Box<str> = s.text(name).into();
                    tparams.push((name.clone(), Ty::Param(name)));
                }
            }
            if s.is(n, K::Namespace) {
                if let Some(name) = s.get(n, F::Name) {
                    let name = s.text(name);
                    let found = ck.summary.decls.iter().position(|d| d.kind == DeclKind::Namespace && &*d.name == name);
                    if let Some(i) = found {
                        namespaces.push(i as u32);
                    }
                }
            }
            node = s.parent(n);
        }
        tparams.reverse();
        env.tparams = tparams;
        env.namespaces = namespaces;
        env.module = ModuleAt::Top;
        env
    }

    fn annotation(&self, t: u32) -> Ty {
        let s = &self.syntax;
        if s.is(t, K::TypeRef) {
            if let Some(alias) = s.get(t, F::Name).and_then(|name| self.decl_of(name)).and_then(|decl| s.parent(decl)).filter(|&owner| s.is(owner, K::TypeAlias)) {
                let name = s.get(alias, F::Name).map_or("", |name| s.text(name));
                if self.summary_decls(name, alias).is_empty() {
                    if s.all(alias, F::TypeParam).next().is_some() || s.all(t, F::TypeArg).next().is_some() {
                        return Ty::Unknown;
                    }
                    return s.get(alias, F::Type).map_or(Ty::Unknown, |body| self.type_of(body));
                }
            }
        }
        let syn = type_syn(&self.syntax, t);
        self.ck().eval(&syn, &self.env_at(t))
    }

    /// The summary declarations a top-level (or namespace-level) statement
    /// at `stmt` became.
    fn summary_decls(&self, name: &str, stmt: u32) -> Vec<DeclRef> {
        let ck = self.ck();
        let start = self.syntax.tree.node(stmt).start;
        let file = ck.file;
        let hit = ck.summary.decls.iter().enumerate().find(|(_, d)| &*d.name == name && d.start == start);
        let Some((i, d)) = hit else { return Vec::new() };
        // Every declaration merged with it.
        let env = ck.decl_env(DeclRef { file, decl: i as u32 });
        let mut all = ck.lookup(&env, &d.name);
        if !all.iter().any(|r| r.file == file && r.decl == i as u32) {
            all = vec![DeclRef { file, decl: i as u32 }];
        }
        all
    }

    fn this_type(&self, at: u32) -> Ty {
        let s = &self.syntax;
        let mut node = s.parent(at);
        let mut is_static = false;
        while let Some(n) = node {
            if matches!(s.kind(n), Some(K::Function | K::Method)) {
                if let Some(annotation) = s.all(n, F::Param).find(|&param| s.word(param, F::Name) == Some("this")).and_then(|param| s.get(param, F::Type)) {
                    return self.annotation(annotation);
                }
            }
            match s.kind(n) {
                Some(K::Function) => return Ty::Any,
                Some(K::Method | K::ClassProp | K::StaticBlock) => is_static = s.has_word(n, F::Modifier, "static") || s.is(n, K::StaticBlock),
                Some(K::Class) => {
                    let Some(name) = s.get(n, F::Name) else { return Ty::Unknown };
                    let classes: Vec<DeclRef> = self
                        .summary_decls(s.text(name), n)
                        .into_iter()
                        .filter(|r| self.ck().summary_of(r.file).is_some_and(|sm| sm.decls[r.decl as usize].kind == DeclKind::Class))
                        .collect();
                    if classes.is_empty() {
                        return Ty::Unknown;
                    }
                    if is_static {
                        return Ty::Class(classes.into());
                    }
                    let args = s.all(n, F::TypeParam).filter_map(|tp| s.get(tp, F::Name)).map(|nm| Ty::Param(s.text(nm).into())).collect();
                    return Ty::Named(classes.into(), args);
                }
                _ => {}
            }
            node = s.parent(n);
        }
        Ty::Unknown
    }

    fn ident_type(&self, id: u32) -> Ty {
        let s = &self.syntax;
        let ck = self.ck();
        let name = s.text(id);
        if name == "undefined" {
            return Ty::Undefined;
        }
        let Some(decl) = self.decl_of(id) else {
            // A global.
            let env = Env::file(ck.file);
            let globals = ck.lookup(&env, name);
            return if globals.is_empty() && self.bindings().unresolved.contains(&id) && self.global_value(id).is_none() {
                Ty::Any
            } else { ck.value_of(&globals) };
        };
        let kind = self.binding(decl).map(|b| b.kind).unwrap_or("");
        let Some(owner) = s.parent(decl) else { return Ty::Unknown };
        match kind {
            "import" => {
                let env = Env::file(ck.file);
                ck.value_of(&ck.lookup(&env, name))
            }
            "function" | "class" | "enum" | "namespace" => {
                let found = self.summary_decls(name, owner);
                if !found.is_empty() {
                    return ck.value_of(&found);
                }
                if kind == "function" {
                    let sig = Arc::new(sig(&self.syntax, owner));
                    return Ty::Fn(Arc::new(Callable { sigs: vec![sig], env: Arc::new(self.env_at(owner)), ret: None }));
                }
                Ty::Unknown
            }
            "var" | "let" | "const" | "using" => {
                if !s.is(owner, K::Declarator) {
                    return Ty::Unknown;
                }
                if let Some(t) = s.get(owner, F::Type) {
                    return self.annotation(t);
                }
                match s.get(owner, F::Init) {
                    // `let x = []` evolves with what is pushed into it.
                    Some(init) if s.is(init, K::Array) && s.all(init, F::Element).next().is_none() => Ty::Unknown,
                    Some(init) => {
                        let t = self.type_of(init);
                        if kind == "const" {
                            t
                        } else {
                            t.widen()
                        }
                    }
                    None => Ty::Unknown,
                }
            }
            "parameter" => {
                if !s.is(owner, K::Param) {
                    return Ty::Unknown;
                }
                if let Some(t) = s.get(owner, F::Type) {
                    let t = self.annotation(t);
                    return if s.get(owner, F::Optional).is_some() { t.or_undefined() } else { t };
                }
                self.contextual_param(owner)
            }
            _ => Ty::Unknown,
        }
    }

    /// An unannotated parameter receives the function's declared/callback context.
    fn contextual_param(&self, param: u32) -> Ty {
        let s = &self.syntax;
        let Some(func) = s.parent(param) else { return Ty::Unknown };
        if !matches!(s.kind(func), Some(K::Arrow | K::Function)) {
            return Ty::Unknown;
        }
        let index = s.all(func, F::Param).position(|p| p == param).unwrap_or(0);
        self.ck().contextual_parameter(&self.contextual_type_of(func), index)
    }

    /// The type expected by a declaration, assignment or callback argument.
    /// Callback lookup excludes function arguments to avoid inference recursion.
    pub fn contextual_type_of(&self, func: u32) -> Ty {
        let s = &self.syntax;
        let mut func = func;
        while let Some(parent) = s.parent(func) {
            let inherits = match s.kind(parent) {
                Some(K::Paren) => true,
                Some(K::Logical) => matches!(s.field(func), Some(F::Left | F::Right)),
                Some(K::Conditional) => matches!(s.field(func), Some(F::Consequent | F::Alternate)),
                Some(K::Sequence) => s.all(parent, F::Expr).last() == Some(func),
                _ => false,
            };
            if !inherits { break }
            func = parent;
        }
        let Some(owner) = s.parent(func) else { return Ty::Unknown };
        if s.is(owner, K::Prop) && s.field(func) == Some(F::Value) {
            if let (Some(object), Some(name)) = (s.parent(owner), super::statics::property_name(s, owner)) {
                return self.ck().member(&self.contextual_type_of(object), &name);
            }
        }
        if s.field(func) == Some(F::Init) && matches!(s.kind(owner), Some(K::Declarator | K::ClassProp)) {
            return s.get(owner, F::Type).map_or(Ty::Unknown, |annotation| self.annotation(annotation));
        }
        if s.is(owner, K::Assign) && s.field(func) == Some(F::Right) {
            return s.get(owner, F::Left).map_or(Ty::Unknown, |left| self.type_of(left));
        }
        if s.field(func) != Some(F::Arg) || !matches!(s.kind(owner), Some(K::Call | K::New)) { return Ty::Unknown }
        let call = owner;
        let args: Vec<u32> = s.all(call, F::Arg).collect();
        let Some(arg_index) = args.iter().position(|&a| a == func) else { return Ty::Unknown };
        let construct = s.is(call, K::New);
        let Some(callee) = s.get(call, F::Callee) else { return Ty::Unknown };
        let callee_ty = self.type_of(callee);
        // Callbacks pick no overload (and typing one here would recurse).
        let arg_types: Vec<Ty> = args.iter().map(|&a| if a == func || matches!(s.kind(a), Some(K::Arrow | K::Function)) { Ty::Unknown } else { self.type_of(a) }).collect();
        let Some((sig, env)) = self.ck().signature_for(&callee_ty, &arg_types, construct) else { return Ty::Unknown };
        let Some(p) = sig.params.get(arg_index).or_else(|| sig.params.last().filter(|p| p.rest)) else { return Ty::Unknown };
        let mut env = (*env).clone();
        for tp in &sig.type_params {
            env.tparams.push((tp.name.clone(), Ty::Param(tp.name.clone())));
        }
        p.ty.as_ref().map_or(Ty::Unknown, |ty| self.ck().eval(ty, &env))
    }

    fn args(&self, call: u32) -> Vec<Ty> {
        self.syntax.all(call, F::Arg).collect::<Vec<_>>().into_iter().map(|a| self.type_of(a)).collect()
    }

    fn type_args(&self, node: u32) -> Vec<Ty> {
        self.syntax.all(node, F::TypeArg).collect::<Vec<_>>().into_iter().map(|t| self.annotation(t)).collect()
    }

    pub(super) fn resolved_predicate(&self, call: u32) -> Option<(u32, ResolvedPredicate)> {
        let s = &self.syntax;
        if !s.is(call, K::Call) { return None }
        let callee = s.get(call, F::Callee)?;
        let predicate = self.ck().call_predicate(&self.type_of(callee), &self.args(call), self.type_args(call))?;
        let index = predicate.parameter?;
        let argument = s.all(call, F::Arg).nth(index)?;
        if s.all(call, F::Arg).take(index + 1).any(|arg| s.is(arg, K::Spread)) { return None }
        Some((argument, predicate))
    }

    pub(super) fn callable_return_types(&self, ty: &Ty) -> Vec<Ty> {
        match ty {
            Ty::Union(parts) | Ty::Inter(parts) => parts.iter().flat_map(|part| self.callable_return_types(part)).collect(),
            Ty::Fn(callable) if callable.ret.is_none() => callable.sigs.iter().map(|sig| {
                self.own_return(sig, callable.env.file).unwrap_or_else(|| self.ck().sig_result(sig, &callable.env, &[], Vec::new()))
            }).collect(),
            _ => self.ck().callable_returns(ty),
        }
    }

    /// A literal assigned directly to a destructuring target inherits its shape.
    fn destructuring_receiver(&self, node: u32) -> Option<u32> {
        let s = &self.syntax;
        let parent = s.parent(node)?;
        match (s.kind(parent)?, s.field(node)?) {
            (K::Declarator | K::Param, F::Init) => s.get(parent, F::Name),
            (K::Assign | K::AssignPattern, F::Right) => s.get(parent, F::Left),
            (K::Paren, F::Expr) => self.destructuring_receiver(parent),
            (K::Array, F::Element) => {
                let receiver = self.destructuring_receiver(parent)?;
                if !matches!(s.kind(receiver), Some(K::ArrayPattern | K::Array)) { return None }
                let index = s.all(parent, F::Element).position(|element| element == node)?;
                s.all(receiver, F::Element).nth(index)
            }
            _ => None,
        }
    }

    /// The generic a node with type arguments instantiates (a call, `new`,
    /// a type reference or a class's `extends`): its type parameters, their
    /// environment, and for calls the chosen signature.
    fn generic_target(&self, id: u32) -> Option<(Vec<TParam>, Env, Option<Arc<Sig>>)> {
        let s = &self.syntax;
        let ck = self.ck();
        let decl_params = |sym: &[DeclRef]| {
            let r = *sym.first()?;
            let sm = ck.summary_of(r.file)?;
            Some((sm.decls[r.decl as usize].type_params.clone(), ck.decl_env(r), None))
        };
        match s.kind(id)? {
            K::Call | K::New => {
                let construct = s.is(id, K::New);
                let callee = self.type_of(s.get(id, F::Callee)?);
                let args = self.args(id);
                match (&callee, construct) {
                    (Ty::Class(sym), true) => {
                        let r = *sym.first()?;
                        let sm = ck.summary_of(r.file)?;
                        let decl = &sm.decls[r.decl as usize];
                        let sig = decl.sigs.iter().find(|sg| sg.params.len() >= args.len()).or(decl.sigs.first()).cloned();
                        Some((decl.type_params.clone(), ck.decl_env(r), sig))
                    }
                    _ => {
                        let (sig, env) = ck.signature_for(&callee, &args, construct)?;
                        Some((sig.type_params.clone(), (*env).clone(), Some(sig)))
                    }
                }
            }
            K::TypeRef => {
                let path = super::summary::path_of(s, s.get(id, F::Name)?);
                decl_params(&ck.type_decls(&path, &self.env_at(id)))
            }
            K::Class => match self.type_of(s.get(id, F::Extends)?) {
                Ty::Class(sym) => decl_params(&sym),
                _ => None,
            },
            _ => None,
        }
    }

    /// Each type parameter's default. A default mentioning an earlier
    /// parameter stays generic (as the checker sees it at its declaration),
    /// so it equals no argument.
    fn defaults(&self, params: &[TParam], env: &Env) -> Vec<Ty> {
        let ck = self.ck();
        let mut env = env.clone();
        let mut out = Vec::new();
        for p in params {
            out.push(p.default.as_ref().map_or(Ty::Unknown, |d| ck.eval(d, &env)));
            env.tparams.push((p.name.clone(), Ty::Param(p.name.clone())));
        }
        out
    }

    /// What the arguments of call `id` infer for each type parameter, as
    /// typescript-eslint judges it: only parameters declared `T` (or
    /// `T | ...`) count, with the argument's literal type widened.
    fn inferred(&self, id: u32, params: &[TParam], sig: Option<&Sig>) -> Vec<Ty> {
        let s = &self.syntax;
        let mut inferred: Vec<(Box<str>, Ty)> = Vec::new();
        if let Some(sig) = sig {
            for (p, a) in sig.params.iter().zip(s.all(id, F::Arg).collect::<Vec<_>>()) {
                let direct = |t: &TypeSyn| matches!(t, TypeSyn::Ref(path, args) if path.len() == 1 && args.is_empty() && params.iter().any(|tp| *tp.name == *path[0]));
                let named = match &p.ty {
                    Some(t @ TypeSyn::Ref(..)) if direct(t) => Some(t.clone()),
                    Some(TypeSyn::Union(items)) => items.iter().find(|t| direct(t)).cloned(),
                    _ => None,
                };
                // `this` has the polymorphic `this` type, never just the
                // class.
                if s.is(a, K::This) {
                    continue;
                }
                if let Some(TypeSyn::Ref(path, _)) = named {
                    if !inferred.iter().any(|(n, _)| *n == path[0]) {
                        inferred.push((path[0].clone(), self.type_of(a).widen()));
                    }
                }
            }
        }
        params.iter().map(|p| inferred.iter().find(|(n, _)| *n == p.name).map_or(Ty::Unknown, |(_, t)| t.clone().widen())).collect()
    }

    /// For a node with type arguments: each type parameter's default and
    /// what the arguments would infer, printed (`None` where there is none).
    pub fn type_params(&self, id: u32) -> Option<super::facts::TypeParamFacts> {
        let ck = self.ck();
        let print = |t: Ty| (!t.is_unknown()).then(|| ck.print(&t));
        let (params, env, sig) = self.generic_target(id)?;
        let defaults = self.defaults(&params, &env).into_iter().map(print).collect();
        let inferred = match self.syntax.kind(id) {
            Some(K::Call | K::New) => self.inferred(id, &params, sig.as_deref()).into_iter().map(print).collect(),
            _ => vec![None; params.len()],
        };
        Some((defaults, inferred))
    }

    /// On the last explicit type argument of a call, `new`, type reference
    /// or class `extends`, whether it is redundant:
    /// `"default"` when it is the parameter's default, `"inferred"` when
    /// the arguments infer it anyway. Types are the same when each is
    /// assignable to the other (a numeric enum and `number` are); `any`
    /// never is.
    pub fn type_arg_redundant(&self, last: u32) -> Option<&'static str> {
        let s = &self.syntax;
        let ck = self.ck();
        if s.field(last) != Some(F::TypeArg) {
            return None;
        }
        let id = s.parent(last)?;
        if s.all(id, F::TypeArg).last() != Some(last) {
            return None;
        }
        let n = s.all(id, F::TypeArg).count();
        let arg = self.annotation(last);
        if mentions_any_or_unknown(&arg) {
            return None;
        }
        let (params, env, sig) = self.generic_target(id)?;
        // Printed text is a fallback for what the relation cannot decide;
        // object types and functions print lossily, so not for them.
        let same = |t: &Ty| {
            !mentions_any_or_unknown(t)
                && match ck.equivalent(&arg, t) {
                    Some(same) => same,
                    None => prints_exactly(&arg) && ck.print(&arg) == ck.print(t),
                }
        };
        if self.defaults(&params, &env).get(n - 1).is_some_and(same) {
            return Some("default");
        }
        if matches!(s.kind(id), Some(K::Call | K::New)) && self.inferred(id, &params, sig.as_deref()).get(n - 1).is_some_and(same) {
            return Some("inferred");
        }
        None
    }

    /// Flags describing the type of `id` for rules (`array`, `string`,
    /// `nullable`, `any`...); parts zlint cannot read add `unknown`. `None`
    /// when nothing about it is known.
    pub fn type_flags(&self, id: u32) -> Option<Vec<&'static str>> {
        let t = self.type_of(id);
        if matches!(t, Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_)) {
            return None;
        }
        let mut out = Vec::new();
        self.flags_of(&t, &mut out);
        Some(out)
    }

    pub(super) fn flags_of(&self, t: &Ty, out: &mut Vec<&'static str>) {
        let ck = self.ck();
        let mut add = |f: &'static str| {
            if !out.contains(&f) {
                out.push(f);
            }
        };
        match t {
            Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => add("unknown"),
            Ty::Any => add("any"),
            Ty::Never => add("never"),
            Ty::Void | Ty::Undefined => {
                add("undefined");
                add("nullable");
            }
            Ty::Null => {
                add("null");
                add("nullable");
            }
            Ty::Prim(p) => add(prim_flag(p)),
            Ty::Lit(_, p) => {
                add(prim_flag(p));
                add("literal");
            }
            Ty::Named(sym, _) if ck.is_enum(sym) => {
                add("enum");
                add(if ck.is_string_enum(sym) { "string" } else { "number" });
            }
            Ty::Named(sym, _) => match ck.decl_name(sym).as_deref() {
                Some("Array" | "ReadonlyArray") => add("array"),
                Some("Promise" | "PromiseLike") => {
                    add("promise");
                    add("object");
                }
                _ => add("object"),
            },
            Ty::Array(_) => add("array"),
            Ty::Tuple(_) => {
                add("array");
                add("tuple");
            }
            Ty::Fn(_) | Ty::Class(_) => add("function"),
            Ty::Obj(..) | Ty::Struct(_) | Ty::Ns(_) | Ty::Module(_) => add("object"),
            Ty::Union(items) | Ty::Inter(items) => {
                if matches!(t, Ty::Union(_)) {
                    add("union");
                }
                items.iter().for_each(|i| self.flags_of(i, out));
            }
        }
    }

    fn compute(&self, id: u32) -> Ty {
        let s = &self.syntax;
        let ck = self.ck();
        let Some(kind) = s.kind(id) else { return Ty::Unknown };
        // Type syntax: what it denotes.
        if kind as u16 >= K::TypeRef as u16 || kind == K::TypeKeyword {
            return self.annotation(id);
        }
        let inner = |f| s.get(id, f).map_or(Ty::Unknown, |e| self.type_of(e));
        match kind {
            K::Ident => {
                let t = self.ident_type(id);
                self.narrow(id, t)
            }
            K::This => self.this_type(id),
            K::Number => Ty::Lit(s.text(id).into(), "number"),
            K::String => Ty::Lit(s.text(id).into(), "string"),
            K::Template => Ty::Prim("string"),
            K::Bool => Ty::Lit(s.text(id).into(), "boolean"),
            K::BigInt => Ty::Lit(s.text(id).into(), "bigint"),
            K::Null => Ty::Null,
            K::Hole => Ty::Undefined,
            K::Paren | K::Satisfies => inner(F::Expr),
            K::NonNull => inner(F::Expr).non_null(),
            K::As | K::TypeAssertion => match s.get(id, F::Type) {
                Some(t) if s.text(t) == "const" => {
                    match s.get(id, F::Expr).map(|expr| s.unparen(expr)) {
                        Some(expr) if s.is(expr, K::Array) && !s.all(expr, F::Element).any(|element| s.is(element, K::Spread)) =>
                            Ty::Tuple(s.all(expr, F::Element).map(|element| self.type_of(element)).collect()),
                        _ => inner(F::Expr),
                    }
                }
                Some(t) => self.annotation(t),
                None => Ty::Unknown,
            },
            K::Assign => match s.word(id, F::Op) {
                Some("=") => inner(F::Right),
                Some("+=" | "&&=" | "||=" | "??=") if matches!(inner(F::Left), Ty::Any) || matches!(inner(F::Right), Ty::Any) => Ty::Any,
                _ => Ty::Unknown,
            },
            K::Member | K::Index | K::Call => {
                let t = self.access_type(id);
                if kind == K::Member && s.get(id, F::Object).is_some_and(|object| s.is(object, K::Ident) && s.text(object) == "globalThis" && self.decl_of(object).is_none()) {
                    return t;
                }
                if self.optional_chain_can_short_circuit(id) {
                    t.or_undefined()
                } else if matches!(kind, K::Member | K::Index) {
                    self.narrow(id, t)
                } else {
                    t
                }
            }
            K::New => {
                let c = inner(F::Callee);
                let args = self.args(id);
                let targs = self.type_args(id);
                let infer_context = args.is_empty() && targs.is_empty() && ck.signature_for(&c, &args, true).is_some_and(|(sig, _)| !sig.type_params.is_empty());
                let result = ck.construct(&c, &args, targs);
                if infer_context {
                    let context = self.contextual_type_of(id);
                    if matches!((&result, &context), (Ty::Named(a, _), Ty::Named(b, _)) if a == b) {
                        return context;
                    }
                }
                result
            }
            K::Await => ck.awaited(&inner(F::Arg)),
            K::Conditional => {
                let (a, b) = (inner(F::Consequent), inner(F::Alternate));
                if a.is_unknown() || b.is_unknown() {
                    Ty::Unknown
                } else {
                    Ty::Union(vec![a, b])
                }
            }
            K::Sequence => s.all(id, F::Expr).last().map_or(Ty::Unknown, |e| self.type_of(e)),
            K::Logical => {
                let (left, right) = (inner(F::Left), inner(F::Right));
                // One known side still says something (`false | ?`).
                if left.is_opaque() && right.is_opaque() {
                    return Ty::Unknown;
                }
                // `x ?? []`: the empty array adds nothing to an array type.
                let empty = |t: &Ty| matches!(t, Ty::Array(e) if matches!(*e.element, Ty::Never));
                match s.word(id, F::Op) {
                    Some("??") => {
                        if !left.may_be_nullish() { return left }
                        let left = left.non_null();
                        if empty(&right) && matches!(left, Ty::Array(_) | Ty::Tuple(_)) {
                            left
                        } else {
                            Ty::union(vec![left, right])
                        }
                    }
                    Some("||") => {
                        let always_truthy = matches!(super::flow::truthy(left.clone(), false, ck), Ty::Never);
                        let left = super::flow::truthy(left, true, ck);
                        if always_truthy { left } else { Ty::union(vec![left, right]) }
                    }
                    Some("&&") => {
                        let always_falsy = matches!(super::flow::truthy(left.clone(), true, ck), Ty::Never);
                        let left = super::flow::truthy(left, false, ck);
                        if always_falsy { left } else { Ty::union(vec![left, right]) }
                    }
                    _ => Ty::Unknown,
                }
            }
            K::Unary => match s.word(id, F::Op) {
                Some("!" | "delete") => Ty::Prim("boolean"),
                Some("typeof") => Ty::Prim("string"),
                Some("void") => Ty::Undefined,
                Some("-" | "+") => {
                    if s.word(id, F::Op) == Some("-") {
                        if let Some(argument) = s.get(id, F::Arg).map(|argument| s.unparen(argument)).filter(|&argument| s.is(argument, K::BigInt)) {
                            return Ty::Lit(format!("-{}", s.text(argument)).into(), "bigint");
                        }
                    }
                    if matches!(inner(F::Arg), Ty::Prim("bigint") | Ty::Lit(_, "bigint")) {
                        return if s.word(id, F::Op) == Some("-") { Ty::Prim("bigint") } else { Ty::Unknown };
                    }
                    super::statics::literal_value(s, id).filter(|(kind, _)| *kind == "number")
                        .map_or(Ty::Prim("number"), |(_, value)| Ty::Lit(value.into(), "number"))
                }
                Some("~") => if matches!(inner(F::Arg), Ty::Prim("bigint") | Ty::Lit(_, "bigint")) { Ty::Prim("bigint") } else { Ty::Prim("number") },
                _ => Ty::Unknown,
            },
            K::Update => Ty::Prim("number"),
            K::Binary => match s.word(id, F::Op) {
                Some("==" | "!=" | "===" | "!==" | "<" | ">" | "<=" | ">=" | "instanceof" | "in") => Ty::Prim("boolean"),
                Some("+") => {
                    let (a, b) = (inner(F::Left).widen(), inner(F::Right).widen());
                    match (&a, &b) {
                        (Ty::Prim("number"), Ty::Prim("number")) => Ty::Prim("number"),
                        (Ty::Prim("bigint"), Ty::Prim("bigint")) => Ty::Prim("bigint"),
                        (Ty::Prim("string"), _) | (_, Ty::Prim("string")) => Ty::Prim("string"),
                        _ => Ty::Unknown,
                    }
                }
                Some(op) => if op != ">>>" && matches!(inner(F::Left).widen(), Ty::Prim("bigint")) && matches!(inner(F::Right).widen(), Ty::Prim("bigint")) { Ty::Prim("bigint") } else { Ty::Prim("number") },
                None => Ty::Unknown,
            },
            K::Array => {
                let items: Vec<Ty> = s.all(id, F::Element).collect::<Vec<_>>().into_iter().map(|e| self.type_of(e).widen()).collect();
                if self.bindings().write_sites.contains_key(&id) || self.destructuring_receiver(id).is_some_and(|receiver| matches!(s.kind(receiver), Some(K::ArrayPattern | K::Array))) {
                    return Ty::Tuple(items.into_iter().collect());
                }
                if items.is_empty() {
                    return Ty::array(Ty::Never);
                }
                if items.iter().any(Ty::is_opaque) {
                    Ty::Unknown
                } else {
                    let mut unique: Vec<Ty> = Vec::new();
                    for t in items {
                        let printed = ck.print(&t);
                        if !unique.iter().any(|u| ck.print(u) == printed) {
                            unique.push(t);
                        }
                    }
                    Ty::array(if unique.len() == 1 { unique.pop().expect("one") } else { Ty::Union(unique) })
                }
            }
            K::Regex => ck.global_named("RegExp", Vec::new()),
            K::Object => self.object_literal(id),
            K::Arrow | K::Function | K::Method => {
                let mut sig = sig(&self.syntax, id);
                sig.is_async |= s.has_word(id, F::Modifier, "async");
                // An arrow's expression body gives its return type.
                let ret = match s.get(id, F::Body) {
                    Some(b) if sig.ret.is_none() && !s.is(b, K::Block) => {
                        let t = self.type_of(b);
                        if sig.is_async {
                            Some(ck.global_named("Promise", vec![ck.awaited(&t)]))
                        } else {
                            Some(t)
                        }
                    }
                    _ => None,
                };
                Ty::Fn(Arc::new(Callable { sigs: vec![Arc::new(sig)], env: Arc::new(self.env_at(id)), ret }))
            }
            K::TaggedTemplate => {
                let f = inner(F::Tag);
                ck.call(&f, &[], self.type_args(id))
            }
            _ => Ty::Unknown,
        }
    }

    /// `{ a: 1, b }`: its properties. Spreads and computed keys make it
    /// unknown.
    fn object_literal(&self, id: u32) -> Ty {
        let s = &self.syntax;
        let mut props: Vec<(Box<str>, Ty)> = Vec::new();
        for p in s.all(id, F::Property).collect::<Vec<_>>() {
            if s.is(p, K::Spread) {
                return if s.get(p, F::Arg).is_some_and(|arg| matches!(self.type_of(arg), Ty::Any)) { Ty::Any } else { Ty::Unknown };
            }
            let Some(key) = s.get(p, F::Key) else { return Ty::Unknown };
            let Some(name) = super::statics::property_name(s, p) else { return Ty::Unknown };
            let name = name.into_boxed_str();
            let t = match s.kind(p) {
                Some(K::Prop) => s.get(p, F::Value).map_or_else(|| self.type_of(key), |v| self.type_of(v)),
                Some(K::Method) => match s.word(p, F::Kind) {
                    Some("get") => s.get(p, F::ReturnType).map_or_else(|| self.own_return(&sig(s, p), self.ck().file).unwrap_or(Ty::Unknown), |annotation| self.type_of(annotation)),
                    Some("set") => s.all(p, F::Param).next().and_then(|param| s.get(param, F::Name)).map_or(Ty::Unknown, |name| self.type_of(name)),
                    _ => self.type_of(p),
                },
                _ => Ty::Unknown,
            };
            props.push((name, t));
        }
        Ty::Struct(props.into())
    }

    /// The operation's own type, before an earlier optional segment skips it.
    fn access_type(&self, id: u32) -> Ty {
        let s = &self.syntax;
        let ck = self.ck();
        match s.kind(id) {
            Some(K::Member) => {
                let name = s.get(id, F::Property).map_or("", |property| s.text(property));
                if let Some(object) = s.get(id, F::Object).filter(|&object| s.is(object, K::Ident) && s.text(object) == "globalThis") {
                    if self.decl_of(object).is_none() {
                        return ck.value_of(&ck.lookup(&Env::file(ck.file), name));
                    }
                }
                let object = s.get(id, F::Object).map_or(Ty::Unknown, |object| self.type_of(object));
                ck.member(&object, name)
            }
            Some(K::Index) => {
                let object = s.get(id, F::Object).map_or(Ty::Unknown, |object| self.type_of(object));
                let index = self.index_key_type(id);
                ck.indexed_type(&object, &index)
            }
            Some(K::Call) => {
                let Some(callee) = s.get(id, F::Callee) else { return Ty::Unknown };
                if s.is(callee, K::Super) { return Ty::Void }
                self.call_type(&self.type_of(callee), id)
            }
            _ => self.type_of(id),
        }
    }

    fn index_key_type(&self, id: u32) -> Ty {
        self.syntax.get(id, F::Index).map_or(Ty::Unknown, |index| {
            self.static_string(index).map_or_else(|| self.type_of(index), |name| Ty::Lit(format!("{name:?}").into(), "string"))
        })
    }

    pub(super) fn access_resolution(&self, id: u32) -> Option<super::types::Resolution> {
        let s = &self.syntax;
        let object = s.get(id, F::Object)?;
        if s.is(object, K::Ident) && s.text(object) == "globalThis" && self.decl_of(object).is_none() {
            return None;
        }
        let object_ty = self.constrained_type_of(object);
        let mut resolution = match s.kind(id)? {
            K::Member => self.ck().member_resolution(&object_ty, s.text(s.get(id, F::Property)?)),
            K::Index => self.ck().index_resolution(&object_ty, &self.constrain_type(self.index_key_type(id), id, &mut Vec::new())),
            _ => return None,
        };
        if !self.optional_chain_can_short_circuit(id) {
            resolution.ty = self.narrow(id, resolution.ty);
        }
        resolution.ty = self.constrain_type(resolution.ty, id, &mut Vec::new());
        Some(resolution)
    }

    pub(super) fn evaluated_type_of(&self, id: u32) -> Ty {
        let ty = if self.optional_chain_can_short_circuit(id) { self.access_type(id) } else { self.type_of(id) };
        self.constrain_type(ty, id, &mut Vec::new())
    }

    /// A call's result; this file's own unannotated functions are typed
    /// from their bodies.
    fn call_type(&self, f: &Ty, id: u32) -> Ty {
        // Argument types only matter for choosing an overload or inferring
        // type parameters; big literals are costly to type for nothing.
        let args = if self.ck().needs_args(f) { self.args(id) } else { vec![Ty::Unknown; self.syntax.all(id, F::Arg).count()] };
        let t = self.ck().call(f, &args, self.type_args(id));
        match f {
            Ty::Fn(c) if c.ret.is_none() => match &c.sigs[..] {
                [sig] if sig.ret.is_none() => self.own_return(sig, c.env.file).unwrap_or(t),
                _ => t,
            },
            _ => t,
        }
    }

    /// Parentheses end a chain; narrowed receivers cannot short-circuit it.
    pub(super) fn optional_chain_can_short_circuit(&self, id: u32) -> bool {
        let s = &self.syntax;
        let mut at = id;
        loop {
            let next = match s.kind(at) {
                Some(K::Member | K::Index) => s.get(at, F::Object),
                Some(K::Call) => s.get(at, F::Callee),
                Some(K::NonNull) => s.get(at, F::Expr),
                _ => None,
            };
            let Some(n) = next else { return false };
            if s.get(at, F::Optional).is_some() && may_be_nullish(&self.type_of(n)) {
                return true;
            }
            at = n;
        }
    }
}

fn may_be_nullish(ty: &Ty) -> bool {
    match ty {
        Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) | Ty::Null | Ty::Undefined | Ty::Void => true,
        Ty::Union(parts) => parts.iter().any(may_be_nullish),
        Ty::Inter(parts) => parts.iter().all(may_be_nullish),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{native_linter, with_native_file};
    use std::path::Path;

    #[test]
    fn bigint_literal_types_keep_signed_truthiness() {
        for (expression, truthy) in [("0n", false), ("2n", true), ("-2n", true), ("-0x0n", false)] {
            let source = format!("const result = /*@*/{expression};");
            with_native_file(&source, |file, nodes| {
                let ty = file.type_of(nodes[0]);
                let domain = file.typing().checker.value_domains(&ty);
                assert_eq!((domain.truthy, domain.falsy), (truthy, !truthy), "{expression}");
                assert!(file.type_flags(nodes[0]).unwrap_or_default().contains(&"literal"));
            });
        }
    }

    #[test]
    fn logical_truthiness_uses_literal_values_not_source_spellings() {
        for source in [
            "const result = /*@*/0x0 && 'unreachable';",
            "const result = /*@*/0b0 && 'unreachable';",
            "const result = /*@*/0o0 && 'unreachable';",
            "const result = /*@*/0.0 && 'unreachable';",
            "const result = /*@*/0e0 && 'unreachable';",
        ] {
            with_native_file(source, |file, nodes| {
                let flags = file.type_flags(nodes[0]).unwrap_or_default();
                assert!(flags.contains(&"number") && !flags.contains(&"string"), "{source}: {flags:?}");
            });
        }
        with_native_file("declare const zero: 0x0n; const result = /*@*/zero && 'unreachable';", |file, nodes| {
            let flags = file.type_flags(nodes[0]).unwrap_or_default();
            assert!(flags.contains(&"bigint") && !flags.contains(&"string"), "{flags:?}");
        });
        with_native_file("const result = /*@*/'\\\n' && 1;", |file, nodes| {
            let flags = file.type_flags(nodes[0]).unwrap_or_default();
            assert!(flags.contains(&"string") && !flags.contains(&"number"), "{flags:?}");
        });
    }

    #[test]
    fn dynamic_indexes_use_generic_inherited_and_domain_specific_signatures() {
        for (source, expected) in [
            ("declare const values: { [key: string]: number }; declare const key: string; const result = /*@*/values[key];", vec!["number"]),
            ("interface Base<T> { [key: string]: T } interface Values extends Base<number> {} declare const values: Values; declare const key: string; const result = /*@*/values[key];", vec!["number"]),
            ("interface Values { [key: string]: string | number; [key: number]: number } declare const values: Values; declare const key: number; const result = /*@*/values[key];", vec!["number"]),
            ("declare const values: { [key: symbol]: boolean }; declare const key: symbol; const result = /*@*/values[key];", vec!["boolean"]),
            ("declare const values: { [key: string]: number } | { [key: string]: string }; declare const key: string; const result = /*@*/values[key];", vec!["number", "string"]),
            ("declare const values: { named: number } | { [key: string]: string }; const result = /*@*/values['named'];", vec!["number", "string"]),
            ("declare const values: { [key: string]: number } & { named: string }; declare const key: string; const result = /*@*/values[key];", vec!["number"]),
            ("declare const values: string[]; const result = /*@*/values['length'];", vec!["number"]),
            ("declare const values: [string, number?]; declare const key: number; const result = /*@*/values[key];", vec!["string", "number", "undefined"]),
        ] {
            with_native_file(source, |file, nodes| {
                let flags = file.type_flags(nodes[0]).unwrap_or_default();
                let values: Vec<_> = flags.iter().filter(|flag| !matches!(**flag, "union" | "nullable" | "literal")).collect();
                assert_eq!(values.len(), expected.len(), "{source}: {flags:?}");
                for expected in expected {
                    assert!(flags.contains(&expected), "{source}: {flags:?} lacks {expected}");
                }
                assert!(!flags.contains(&"unknown"), "{source}: {flags:?}");
            });
        }
    }

    #[test]
    fn indexed_types_preserve_explicit_properties_and_missing_key_domains() {
        for source in [
            "interface Values { [key: string]: number; missing: Missing } declare const values: Values; const result = /*@*/values['missing'];",
            "declare const values: { [key: number]: string }; declare const key: string; const result = /*@*/values[key];",
            "declare const values: { [key: string]: number } | {}; declare const key: string; const result = /*@*/values[key];",
            "declare const values: [string, number]; const result = /*@*/values[-1];",
            "declare const values: [string, number]; const result = /*@*/values[0.5];",
        ] {
            with_native_file(source, |file, nodes| assert!(file.type_flags(nodes[0]).is_none(), "{source}"));
        }
        with_native_file("interface Values { [key: string]: number; named: string } declare const values: Values; const result = /*@*/values['named'];", |file, nodes| {
            let flags = file.type_flags(nodes[0]).unwrap_or_default();
            assert!(flags.contains(&"string") && !flags.contains(&"number"), "{flags:?}");
        });
    }

    #[test]
    fn optional_access_only_adds_undefined_when_a_receiver_can_short_circuit() {
        for (source, undefined, null) in [
            ("declare const value: { item: number } | null; const result = /*@*/value?.item;", true, false),
            ("declare const value: { item: number } | null; value !== null && /*@*/value?.item;", false, false),
            ("declare const value: { item: number } | null; value === null || /*@*/value?.item;", false, false),
            ("declare const value: { item: number | null } | null; value !== null && /*@*/value?.item;", false, true),
            ("declare const value: { item: { inner: number } | null } | null; value !== null && /*@*/value?.item?.inner;", true, false),
            ("declare const value: { item: { inner: number } | null } | null; value !== null && value?.item !== null && /*@*/value?.item?.inner;", false, false),
            ("declare const value: (() => number) | null; value !== null && /*@*/value?.();", false, false),
            ("let value: { item: number } | null; if (value !== null) { value = null; const result = /*@*/value?.item; }", true, false),
            ("declare const value: { item: number } | null; if (value !== null) { function later() { const result = /*@*/value?.item; } }", true, false),
        ] {
            with_native_file(source, |file, nodes| {
                let flags = file.type_flags(nodes[0]).unwrap_or_default();
                assert!(flags.contains(&"number"), "{source}: {flags:?}");
                assert_eq!(flags.contains(&"undefined"), undefined, "{source}: {flags:?}");
                assert_eq!(flags.contains(&"null"), null, "{source}: {flags:?}");
            });
        }
    }

    #[test]
    fn logical_types_exclude_unreachable_operands_but_keep_enum_fallbacks() {
        let linter = native_linter("in ts_logical rule reachable-string { when contains(default(type_flags, []), 'string')\nwarn 'string result' }");
        let code = "({}) || 'unreachable'; false && 'unreachable'; true && 'reachable'; ({}) ?? 'unreachable'; enum E { Zero, One } declare const value: E; value || 'reachable'; 1 || 'unreachable'; 0 ?? 'unreachable';";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].start, code.find("true").unwrap());
        assert_eq!(diagnostics[1].start, code.find("value ||").unwrap());
    }

    #[test]
    fn optional_callback_context_reaches_conditional_and_final_sequence_functions() {
        let linter = native_linter("in ts_parameter.name rule contextual-any { when contains(default(type_flags, []), 'any')\nwarn 'unsafe parameter' }");
        let code = "declare function receive(callback: ((e: any) => void) | undefined): void; declare const flag: boolean; receive(e => {}); receive(flag ? e => {} : e => {}); receive((e => {}, e => {}));";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 5);
    }

    #[test]
    fn unknown_type_facts_remain_absent_for_existing_rule_consumers() {
        let linter = native_linter("in ts_identifier rule known { when type != null\nwarn 'known' }");
        let code = "declare const narrow: number; declare const broad: unknown; narrow; broad;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }
}

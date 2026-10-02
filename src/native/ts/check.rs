//! Types of expressions in the file being linted: bottom-up from literals,
//! declarations and annotations, through the declared-type evaluator.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::modules::{DeclRef, ModuleAt};
use super::summary::{sig, type_syn, DeclKind, TypeSyn};
use super::types::{Callable, Checker, Env, Ty};

/// Type state of one linted file, created on the first type question.
pub struct Typing<'t> {
    pub checker: Checker<'t>,
    memo: RefCell<HashMap<u32, Ty>>,
    /// Nested `type_of` calls (`a + b + c + ...` nests one per operand).
    depth: Cell<u32>,
}

/// Deeper expressions than this are unknown (protects the stack).
const MAX_DEPTH: u32 = 120;

impl<'t> Typing<'t> {
    pub fn new(checker: Checker<'t>) -> Self {
        Typing { checker, memo: RefCell::new(HashMap::new()), depth: Cell::new(0) }
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
            match s.kind(n) {
                Some(K::Function) => return Ty::Unknown,
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
            return ck.value_of(&ck.lookup(&env, name));
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
            "param" => {
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

    /// An unannotated callback parameter, typed from the callee's
    /// parameter type (`list.map(x => ...)`).
    fn contextual_param(&self, param: u32) -> Ty {
        let s = &self.syntax;
        let Some(func) = s.parent(param) else { return Ty::Unknown };
        if !matches!(s.kind(func), Some(K::Arrow | K::Function)) || s.field(func) != Some(F::Arg) {
            return Ty::Unknown;
        }
        let Some(call) = s.parent(func) else { return Ty::Unknown };
        let index = s.all(func, F::Param).position(|p| p == param).unwrap_or(0);
        let args: Vec<u32> = s.all(call, F::Arg).collect();
        let Some(arg_index) = args.iter().position(|&a| a == func) else { return Ty::Unknown };
        let construct = s.is(call, K::New);
        let Some(callee) = s.get(call, F::Callee) else { return Ty::Unknown };
        let callee_ty = self.type_of(callee);
        let Some((sig, env)) = self.ck().signature_for(&callee_ty, args.len(), construct) else { return Ty::Unknown };
        let Some(p) = sig.params.get(arg_index).or_else(|| sig.params.last().filter(|p| p.rest)) else { return Ty::Unknown };
        let mut env = (*env).clone();
        for tp in &sig.type_params {
            env.tparams.push((tp.name.clone(), Ty::Param(tp.name.clone())));
        }
        match &p.ty {
            Some(TypeSyn::Fn(inner)) => match inner.params.get(index).and_then(|ip| ip.ty.as_ref()) {
                Some(t) => self.ck().eval(t, &env),
                None => Ty::Unknown,
            },
            _ => Ty::Unknown,
        }
    }

    fn args(&self, call: u32) -> Vec<Ty> {
        self.syntax.all(call, F::Arg).collect::<Vec<_>>().into_iter().map(|a| self.type_of(a)).collect()
    }

    fn type_args(&self, node: u32) -> Vec<Ty> {
        self.syntax.all(node, F::TypeArg).collect::<Vec<_>>().into_iter().map(|t| self.annotation(t)).collect()
    }

    /// For a node with type arguments (a call, `new`, a type reference or
    /// a class's `extends`): each type parameter's default and what the
    /// arguments would infer, printed (`None` where there is none).
    pub fn type_params(&self, id: u32) -> Option<super::facts::TypeParamFacts> {
        let s = &self.syntax;
        let ck = self.ck();
        let print = |t: Ty| (!t.is_unknown()).then(|| ck.print(&t));
        match s.kind(id)? {
            K::Call | K::New => {
                let construct = s.is(id, K::New);
                let callee = self.type_of(s.get(id, F::Callee)?);
                let args: Vec<u32> = s.all(id, F::Arg).collect();
                let (params, env, sig_params): (Vec<super::summary::TParam>, Env, Option<Arc<super::summary::Sig>>) = match (&callee, construct) {
                    (Ty::Class(sym), true) => {
                        let r = *sym.first()?;
                        let sm = ck.summary_of(r.file)?;
                        let decl = &sm.decls[r.decl as usize];
                        let sig = decl.sigs.iter().find(|sg| sg.params.len() >= args.len()).or(decl.sigs.first()).cloned();
                        (decl.type_params.clone(), ck.decl_env(r), sig)
                    }
                    _ => {
                        let (sig, env) = ck.signature_for(&callee, args.len(), construct)?;
                        (sig.type_params.clone(), (*env).clone(), Some(sig))
                    }
                };
                let defaults = self.defaults(&params, &env);
                // As typescript-eslint judges it: only parameters declared
                // `T` (or `T | ...`) count, with the argument's literal
                // type widened.
                let mut inferred: Vec<(Box<str>, Ty)> = Vec::new();
                if let Some(sig) = sig_params {
                    for (p, &a) in sig.params.iter().zip(&args) {
                        let direct = |t: &TypeSyn| matches!(t, TypeSyn::Ref(path, args) if path.len() == 1 && args.is_empty() && params.iter().any(|tp| *tp.name == *path[0]));
                        let named = match &p.ty {
                            Some(t @ TypeSyn::Ref(..)) if direct(t) => Some(t.clone()),
                            Some(TypeSyn::Union(items)) => items.iter().find(|t| direct(t)).cloned(),
                            _ => None,
                        };
                        // `this` has the polymorphic `this` type, never just
                        // the class.
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
                let inferred = params.iter().map(|p| inferred.iter().find(|(n, _)| *n == p.name).and_then(|(_, t)| print(t.clone().widen()))).collect();
                Some((defaults, inferred))
            }
            K::TypeRef => {
                let path = super::summary::path_of(s, s.get(id, F::Name)?);
                let env = self.env_at(id);
                let decls = ck.type_decls(&path, &env);
                let r = *decls.first()?;
                let sm = ck.summary_of(r.file)?;
                let params = sm.decls[r.decl as usize].type_params.clone();
                let defaults = self.defaults(&params, &ck.decl_env(r));
                Some((defaults, vec![None; params.len()]))
            }
            K::Class => {
                let base = self.type_of(s.get(id, F::Extends)?);
                let Ty::Class(sym) = base else { return None };
                let r = *sym.first()?;
                let sm = ck.summary_of(r.file)?;
                let params = sm.decls[r.decl as usize].type_params.clone();
                let defaults = self.defaults(&params, &ck.decl_env(r));
                Some((defaults, vec![None; params.len()]))
            }
            _ => None,
        }
    }

    fn defaults(&self, params: &[super::summary::TParam], env: &Env) -> Vec<Option<String>> {
        let ck = self.ck();
        // A default mentioning an earlier parameter stays generic (as the
        // checker sees it at its declaration), so it equals no argument.
        let mut env = env.clone();
        let mut out = Vec::new();
        for p in params {
            let t = p.default.as_ref().map(|d| ck.eval(d, &env));
            out.push(t.as_ref().filter(|t| !t.is_unknown()).map(|t| ck.print(t)));
            env.tparams.push((p.name.clone(), Ty::Param(p.name.clone())));
        }
        out
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
            K::Ident => self.ident_type(id),
            K::This => self.this_type(id),
            K::Number => Ty::Lit(s.text(id).into(), "number"),
            K::String => Ty::Lit(s.text(id).into(), "string"),
            K::Template => Ty::Prim("string"),
            K::Bool => Ty::Lit(s.text(id).into(), "boolean"),
            K::BigInt => Ty::Prim("bigint"),
            K::Null => Ty::Null,
            K::Paren | K::Satisfies => inner(F::Expr),
            K::NonNull => inner(F::Expr).non_null(),
            K::As | K::TypeAssertion => match s.get(id, F::Type) {
                Some(t) if s.text(t) == "const" => inner(F::Expr),
                Some(t) => self.annotation(t),
                None => Ty::Unknown,
            },
            K::Assign => match s.word(id, F::Op) {
                Some("=") => inner(F::Right),
                _ => Ty::Unknown,
            },
            K::Member => {
                let object = inner(F::Object);
                let name = s.get(id, F::Property).map_or("", |p| s.text(p));
                let t = ck.member(&object, name);
                if s.get(id, F::Optional).is_some() || self.in_optional_chain(id) {
                    t.or_undefined()
                } else {
                    t
                }
            }
            K::Index => {
                let object = inner(F::Object);
                match object {
                    Ty::Array(e) => *e,
                    Ty::Tuple(items) => match s.get(id, F::Index).map(|i| s.text(i).parse::<usize>()) {
                        Some(Ok(n)) => items.get(n).cloned().unwrap_or(Ty::Unknown),
                        _ => Ty::Unknown,
                    },
                    _ => Ty::Unknown,
                }
            }
            K::Call => {
                let Some(callee) = s.get(id, F::Callee) else { return Ty::Unknown };
                if s.is(callee, K::Super) {
                    return Ty::Void;
                }
                let f = self.type_of(callee);
                let t = ck.call(&f, &self.args(id), self.type_args(id));
                if s.get(id, F::Optional).is_some() || self.in_optional_chain(id) {
                    t.or_undefined()
                } else {
                    t
                }
            }
            K::New => {
                let c = inner(F::Callee);
                ck.construct(&c, &self.args(id), self.type_args(id))
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
            K::Unary => match s.word(id, F::Op) {
                Some("!" | "delete") => Ty::Prim("boolean"),
                Some("typeof") => Ty::Prim("string"),
                Some("void") => Ty::Undefined,
                Some("-" | "+" | "~") => Ty::Prim("number"),
                _ => Ty::Unknown,
            },
            K::Update => Ty::Prim("number"),
            K::Binary => match s.word(id, F::Op) {
                Some("==" | "!=" | "===" | "!==" | "<" | ">" | "<=" | ">=" | "instanceof" | "in") => Ty::Prim("boolean"),
                Some("+") => {
                    let (a, b) = (inner(F::Left).widen(), inner(F::Right).widen());
                    match (&a, &b) {
                        (Ty::Prim("number"), Ty::Prim("number")) => Ty::Prim("number"),
                        (Ty::Prim("string"), _) | (_, Ty::Prim("string")) => Ty::Prim("string"),
                        _ => Ty::Unknown,
                    }
                }
                Some(_) => Ty::Prim("number"),
                None => Ty::Unknown,
            },
            K::Array => {
                let items: Vec<Ty> = s.all(id, F::Element).collect::<Vec<_>>().into_iter().map(|e| self.type_of(e).widen()).collect();
                if items.is_empty() || items.iter().any(Ty::is_unknown) {
                    Ty::Unknown
                } else {
                    let mut unique: Vec<Ty> = Vec::new();
                    for t in items {
                        let printed = ck.print(&t);
                        if !unique.iter().any(|u| ck.print(u) == printed) {
                            unique.push(t);
                        }
                    }
                    Ty::Array(Box::new(if unique.len() == 1 { unique.pop().expect("one") } else { Ty::Union(unique) }))
                }
            }
            K::Regex => ck.global_named("RegExp", Vec::new()),
            K::Arrow | K::Function => {
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

    /// Inside `a?.b.c`: everything after a `?.` may be skipped.
    fn in_optional_chain(&self, id: u32) -> bool {
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
            if s.get(n, F::Optional).is_some() {
                return true;
            }
            at = n;
        }
    }
}

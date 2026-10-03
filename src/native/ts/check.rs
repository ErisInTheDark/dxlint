//! Types of expressions in the file being linted: bottom-up from literals,
//! declarations and annotations, through the declared-type evaluator.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::modules::{DeclRef, ModuleAt};
use super::summary::{sig, type_syn, DeclKind, Sig, TParam, TypeSyn};
use super::types::{Callable, Checker, Env, Ty};

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
        Ty::Union(items) | Ty::Inter(items) | Ty::Tuple(items) => items.iter().all(prints_exactly),
        Ty::Struct(props) => props.iter().all(|(_, t)| prints_exactly(t)),
        Ty::Array(e) => prints_exactly(e),
        _ => true,
    }
}

/// `any` (or a part zlint cannot read) anywhere inside.
fn mentions_any_or_unknown(t: &Ty) -> bool {
    match t {
        Ty::Any | Ty::Unknown => true,
        Ty::Named(_, args) => args.iter().any(mentions_any_or_unknown),
        Ty::Union(items) | Ty::Inter(items) | Ty::Tuple(items) => items.iter().any(mentions_any_or_unknown),
        Ty::Array(e) => mentions_any_or_unknown(e),
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
        // Callbacks pick no overload (and typing one here would recurse).
        let arg_types: Vec<Ty> = args.iter().map(|&a| if matches!(s.kind(a), Some(K::Arrow | K::Function)) { Ty::Unknown } else { self.type_of(a) }).collect();
        let Some((sig, env)) = self.ck().signature_for(&callee_ty, &arg_types, construct) else { return Ty::Unknown };
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
        if matches!(t, Ty::Unknown | Ty::Param(_)) {
            return None;
        }
        let mut out = Vec::new();
        self.flags_of(&t, &mut out);
        Some(out)
    }

    fn flags_of(&self, t: &Ty, out: &mut Vec<&'static str>) {
        let ck = self.ck();
        let mut add = |f: &'static str| {
            if !out.contains(&f) {
                out.push(f);
            }
        };
        match t {
            Ty::Unknown | Ty::Param(_) => add("unknown"),
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
                let name = s.get(id, F::Property).map_or("", |p| s.text(p));
                // `globalThis.x`: the global `x`.
                if let Some(o) = s.get(id, F::Object).filter(|&o| s.is(o, K::Ident) && s.text(o) == "globalThis") {
                    if self.decl_of(o).is_none() {
                        return ck.value_of(&ck.lookup(&Env::file(ck.file), name));
                    }
                }
                let object = inner(F::Object);
                let t = ck.member(&object, name);
                if s.get(id, F::Optional).is_some() || self.in_optional_chain(id) {
                    t.or_undefined()
                } else {
                    self.narrow(id, t)
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
                let t = self.call_type(&f, id);
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
            K::Logical => {
                let (left, right) = (inner(F::Left), inner(F::Right));
                // One known side still says something (`false | ?`).
                if left.is_unknown() && right.is_unknown() {
                    return Ty::Unknown;
                }
                // `x ?? []`: the empty array adds nothing to an array type.
                let empty = |t: &Ty| matches!(t, Ty::Array(e) if matches!(**e, Ty::Never));
                match s.word(id, F::Op) {
                    Some("??") => {
                        let left = left.non_null();
                        if empty(&right) && matches!(left, Ty::Array(_) | Ty::Tuple(_)) {
                            left
                        } else {
                            Ty::union(vec![left, right])
                        }
                    }
                    Some("||") => Ty::union(vec![super::flow::truthy(left, true), right]),
                    Some("&&") => Ty::union(vec![super::flow::truthy(left, false), right]),
                    _ => Ty::Unknown,
                }
            }
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
                if items.is_empty() {
                    return Ty::Array(Box::new(Ty::Never));
                }
                if items.iter().any(Ty::is_unknown) {
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
            K::Object => self.object_literal(id),
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

    /// `{ a: 1, b }`: its properties. Spreads and computed keys make it
    /// unknown.
    fn object_literal(&self, id: u32) -> Ty {
        let s = &self.syntax;
        let mut props: Vec<(Box<str>, Ty)> = Vec::new();
        for p in s.all(id, F::Prop).collect::<Vec<_>>() {
            let Some(key) = s.get(p, F::Key) else { return Ty::Unknown };
            let name: Box<str> = match s.kind(key) {
                Some(K::Ident | K::Number) => s.text(key).into(),
                Some(K::String) => s.text(key).trim_matches(|c| c == '"' || c == '\'').into(),
                _ => return Ty::Unknown,
            };
            let t = match s.kind(p) {
                Some(K::Prop) => s.get(p, F::Value).map_or_else(|| self.type_of(key), |v| self.type_of(v)),
                _ => Ty::Unknown,
            };
            props.push((name, t));
        }
        Ty::Struct(props.into())
    }

    /// A call's result; this file's own unannotated functions are typed
    /// from their bodies.
    fn call_type(&self, f: &Ty, id: u32) -> Ty {
        // Argument types only matter for choosing an overload or inferring
        // type parameters; big literals are costly to type for nothing.
        let args = if self.ck().needs_args(f) { self.args(id) } else { vec![Ty::Unknown; self.syntax.all(id, F::Arg).count()] };
        let t = self.ck().call(f, &args, self.type_args(id));
        match f {
            Ty::Fn(c) if t.is_unknown() && c.ret.is_none() => match &c.sigs[..] {
                [sig] => self.own_return(sig, c.env.file).unwrap_or(t),
                _ => t,
            },
            _ => t,
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

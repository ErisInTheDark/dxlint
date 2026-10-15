//! Types of expressions in the file being linted: bottom-up from literals,
//! declarations and annotations, through the declared-type evaluator.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::modules::{DeclRef, ModuleAt};
use super::summary::{sig, type_syn, DeclKind, Sig, TParam, TypeSyn};
use super::types::{Callable, Checker, Env, ResolvedPredicate, StructProperty, Ty};

mod contextual;
mod bindings;
mod literals;

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
        Ty::Struct(props) => props.iter().all(|property| prints_exactly(&property.ty)),
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
        let mut self_names = Vec::new();
        while let Some(n) = node {
            for tp in s.all(n, F::TypeParam) {
                if let Some(name) = s.get(tp, F::Name) {
                    let name: Box<str> = s.text(name).into();
                    if !self_names.contains(&name) { tparams.push((name.clone(), Ty::Param(name))) }
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
            if s.is(n, K::Class) {
                if env.this.is_none() {
                    if let Ty::Class(classes) = ck.class_value_at(s.tree.node(n).start, ck.file) {
                        let args = s.all(n, F::TypeParam).filter_map(|parameter| s.get(parameter, F::Name))
                            .map(|name| Ty::Param(s.text(name).into())).collect();
                        env.this = Some(Box::new(Ty::Named(classes, args)));
                    }
                }
                if let Some(i) = ck.summary.decls.iter().position(|decl|
                    decl.kind == DeclKind::Class && decl.start == s.tree.node(n).start) {
                    for scope in ck.decl_env(DeclRef { file: ck.file, decl: i as u32 }).namespaces {
                        if !namespaces.contains(&scope) { namespaces.push(scope) }
                    }
                    if let Some(name) = s.get(n, F::Name) { self_names.push(s.text(name).into()); }
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
        let hit = ck.summary.decls.iter().enumerate().find(|(_, d)| {
            if &*d.name != name || d.start != start { return false }
            let mut parent = d.parent;
            while let Some(scope) = parent {
                let enclosing = &ck.summary.decls[scope as usize];
                if enclosing.kind == DeclKind::Lexical { return false }
                parent = enclosing.parent;
            }
            true
        });
        let Some((i, d)) = hit else { return Vec::new() };
        // Every declaration merged with it.
        let env = ck.decl_env(DeclRef { file, decl: i as u32 });
        let mut all = ck.lookup(&env, &d.name);
        if !all.iter().any(|r| r.file == file && r.decl == i as u32) {
            all = vec![DeclRef { file, decl: i as u32 }];
        }
        all
    }

    fn new_target_type(&self, at: u32) -> Ty {
        if self.tokens(at) != ["new", ".", "target"] {
            return Ty::Unknown;
        }
        let s = &self.syntax;
        let mut node = s.parent(at);
        while let Some(n) = node {
            match s.kind(n) {
                Some(K::Function) => return self.type_of(n),
                Some(K::Method) => {
                    let constructor = !s.has_word(n, F::Modifier, "static")
                        && s.word(n, F::Kind).is_none()
                        && s.get(n, F::Key).is_some_and(|key| matches!(s.kind(key), Some(K::Ident | K::String)))
                        && super::statics::property_name(s, n).as_deref() == Some("constructor");
                    return s.parent(n).filter(|&class| constructor && s.is(class, K::Class))
                        .map_or(Ty::Unknown, |class| self.ck().class_value_at(s.tree.node(class).start, self.ck().file));
                }
                // Keys and decorators run outside their member's function scope.
                Some(K::Computed) => {
                    node = s.parent(n).and_then(|owner| s.parent(owner)).and_then(|container| s.parent(container));
                    continue;
                }
                Some(K::Decorator) => {
                    node = s.parent(n).and_then(|owner| if s.is(owner, K::Param) { s.parent(owner) } else { Some(owner) })
                        .and_then(|owner| s.parent(owner));
                    continue;
                }
                Some(K::ClassProp | K::StaticBlock | K::Namespace | K::Enum | K::Program) => return Ty::Unknown,
                _ => {}
            }
            node = s.parent(n);
        }
        Ty::Unknown
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
                    let mut classes: Vec<DeclRef> = match s.get(n, F::Name) {
                        Some(name) => self.summary_decls(s.text(name), n).into_iter()
                            .filter(|r| self.ck().summary_of(r.file).is_some_and(|sm| sm.decls[r.decl as usize].kind == DeclKind::Class)).collect(),
                        None => match self.ck().class_value_at(s.tree.node(n).start, self.ck().file) {
                            Ty::Class(classes) => classes.to_vec(),
                            _ => Vec::new(),
                        },
                    };
                    if classes.is_empty() && s.parent(n).is_some_and(|parent| !matches!(s.kind(parent), Some(K::Block | K::Program | K::Export))) {
                        if let Ty::Class(found) = self.ck().class_value_at(s.tree.node(n).start, self.ck().file) { classes = found.to_vec(); }
                    }
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

    fn super_type(&self, at: u32) -> Ty {
        let s = &self.syntax;
        let mut node = s.parent(at);
        let mut is_static = false;
        while let Some(owner) = node {
            match s.kind(owner) {
                Some(K::Function) => return Ty::Unknown,
                Some(K::Method) if !s.parent(owner).is_some_and(|parent| s.is(parent, K::Class)) => return Ty::Unknown,
                Some(K::Method | K::ClassProp | K::StaticBlock) =>
                    is_static = s.has_word(owner, F::Modifier, "static") || s.is(owner, K::StaticBlock),
                Some(K::Class) => {
                    let Some(base) = s.get(owner, F::Extends) else { return Ty::Unknown };
                    let base = self.type_of(base);
                    return if is_static { base } else { self.ck().construct(&base, &[], self.type_args(owner)) };
                }
                _ => {}
            }
            node = s.parent(owner);
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
        if self.bindings().arguments_owner(s, id).is_some() {
            let globals = ck.project.globals(ck.file);
            return globals.names.get("IArguments").filter(|decls| !decls.is_empty())
                .map_or(Ty::Unknown, |decls| Ty::Named(decls.clone().into(), Vec::new()));
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
            "class_name" => ck.class_value_at(s.tree.node(owner).start, ck.file),
            "import" => {
                let env = Env::file(ck.file);
                ck.value_of(&ck.lookup(&env, name))
            }
            "function" | "class" | "enum" | "namespace" => {
                let found = self.summary_decls(name, owner);
                if !found.is_empty() {
                    return ck.value_of(&found);
                }
                if kind == "class" {
                    return ck.class_value_at(s.tree.node(owner).start, ck.file);
                }
                if kind == "function" {
                    let sig = Arc::new(sig(&self.syntax, owner));
                    return Ty::Fn(Arc::new(Callable::new(vec![sig], Arc::new(self.env_at(owner)), None)));
                }
                Ty::Unknown
            }
            "var" | "let" | "const" | "using" => {
                self.binding_type(decl, kind)
            }
            "parameter" => self.binding_type(decl, kind),
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
                self.own_return(&sig.syntax, sig.env.file).unwrap_or_else(|| self.ck().sig_result(&sig.syntax, &sig.env, &[], Vec::new()))
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
                        inferred.push((path[0].clone(), self.type_of(a).literal_base()));
                    }
                }
            }
        }
        params.iter().map(|p| inferred.iter().find(|(n, _)| *n == p.name).map_or(Ty::Unknown, |(_, t)| t.clone().literal_base())).collect()
    }

    pub fn type_defaults(&self, id: u32) -> Option<Vec<Option<String>>> {
        let ck = self.ck();
        let print = |t: Ty| (!t.is_unknown()).then(|| ck.print(&t));
        let (params, env, _) = self.generic_target(id)?;
        Some(self.defaults(&params, &env).into_iter().map(print).collect())
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
            Ty::EnumMember(member) => {
                add("enum");
                self.flags_of(&member.base, out);
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
            K::Super => self.super_type(id),
            K::ObjectPattern | K::ArrayPattern => self.binding_type(id, "const"),
            K::ClassProp => {
                let Some(class) = s.parent(id).filter(|&parent| s.is(parent, K::Class)) else { return Ty::Unknown };
                let name = self.symbolic_property_key(id).map(|symbol| format!("[Symbol.{symbol}]"))
                    .or_else(|| s.get(id, F::Key).filter(|&key| s.is(key, K::PrivateName)).map(|key| s.text(key).to_string()))
                    .or_else(|| super::statics::property_name(s, id));
                let Some(name) = name else { return Ty::Unknown };
                let receiver = if s.has_word(id, F::Modifier, "static") {
                    ck.class_value_at(s.tree.node(class).start, ck.file)
                } else {
                    self.env_at(id).this.map_or(Ty::Unknown, |receiver| *receiver)
                };
                ck.member(&receiver, &name)
            }
            K::MetaProperty => self.new_target_type(id),
            K::Instantiation => self.ck().instantiate(&inner(F::Expr), s.all(id, F::TypeArg).map(|argument| self.annotation(argument)).collect()),
            K::Class if s.parent(id).is_some_and(|parent|
                !matches!(s.kind(parent), Some(K::Block | K::Program | K::Export))) =>
                self.ck().class_value_at(s.tree.node(id).start, self.ck().file),
            K::Number => Ty::Lit(s.text(id).into(), "number"),
            K::String => Ty::Lit(s.text(id).into(), "string"),
            K::Template => self.static_string(id).map_or(Ty::Prim("string"), |value| Ty::Lit(format!("{value:?}").into(), "string")),
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
            K::ImportCall => super::summary::import_specifier(s, id)
                .map_or(Ty::Unknown, |specifier| ck.import_type(&specifier, &self.env_at(id))),
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
            K::Binary => ck.binary_type(s.word(id, F::Op), || inner(F::Left), || inner(F::Right)),
            K::Array => self.array_literal(id),
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
                Ty::Fn(Arc::new(Callable::new(vec![Arc::new(sig)], Arc::new(self.env_at(id)), ret)))
            }
            K::TaggedTemplate => {
                let f = inner(F::Tag);
                ck.call(&f, &[], self.type_args(id))
            }
            _ => Ty::Unknown,
        }
    }

    /// Only direct references to a library Symbol constructor identify symbolic keys.
    pub(super) fn symbolic_property_key(&self, property: u32) -> Option<String> {
        let s = &self.syntax;
        let expression = if s.is(property, K::Index) {
            s.get(property, F::Index)?
        } else {
            let key = s.get(property, F::Key).filter(|&key| s.is(key, K::Computed))?;
            s.get(key, F::Expr)?
        };
        let expression = s.unparen(expression);
        if !s.is(expression, K::Member) { return None }
        let object = s.get(expression, F::Object).filter(|&object| s.is(object, K::Ident) && s.text(object) == "Symbol")?;
        if self.decl_of(object).is_some() { return None }
        let checker = self.ck();
        let symbol = checker.lookup(&self.env_at(object), "Symbol");
        let globals = checker.project.globals(checker.file);
        let declarations = globals.names.get("Symbol")?;
        if symbol.is_empty() || !symbol.iter().all(|decl| declarations.contains(decl) && checker.is_lib_decl(*decl)) { return None }
        let name = s.get(expression, F::Property).map(|property| s.text(property))?;
        self.symbol_constructor_key(&symbol, name).then(|| name.to_owned())
    }

    fn symbol_constructor_key(&self, symbol: &[DeclRef], name: &str) -> bool {
        let resolution = self.ck().member_resolution(&self.ck().value_of(symbol), name);
        if !resolution.declared { return false }
        if matches!(resolution.ty, Ty::Prim("symbol")) { return true }
        !resolution.declarations.is_empty() && resolution.declarations.iter().all(|declaration| {
            declaration.member.kind == super::summary::MemberKind::Prop
                && matches!(declaration.member.ty.as_ref(), Some(TypeSyn::Unique(inner))
                    if matches!(&**inner, TypeSyn::Kw(keyword) if &**keyword == "symbol"))
        })
    }

    pub(super) fn property_key_name(&self, property: u32) -> Option<String> {
        super::statics::property_name(&self.syntax, property)
            .or_else(|| self.symbolic_property_key(property).map(|name| format!("[Symbol.{name}]")))
    }

    /// Printed Symbol names share the string namespace, but not key identity.
    /// Symbol path selectors require real keys; ordinary property contexts allow two strings.
    pub(super) fn member_keys_match(&self, sender: &Ty, receiver: &Ty, name: &str, require_symbol: bool) -> bool {
        if !(name.starts_with("[Symbol.") && name.ends_with(']')) { return true }
        let key = |ty: &Ty| {
            let resolution = self.ck().member_resolution(ty, name);
            let mut keys = resolution.declarations.iter().map(|declaration| {
                if let Some(origin) = &declaration.origin {
                    if origin.file == self.ck().file {
                        return self.property_key_name(origin.node).map(|_| self.symbolic_property_key(origin.node).is_some());
                    }
                }
                if declaration.member.computed_key.is_some() {
                    self.ck().declaration_key_symbol(declaration).and_then(|name| {
                        let checker = self.ck();
                        let symbol = checker.lookup(declaration.environment.as_ref()?, "Symbol");
                        self.symbol_constructor_key(&symbol, name).then_some(true)
                    })
                } else { Some(false) }
            });
            let first = keys.next()??;
            keys.all(|key| key == Some(first)).then_some(first)
        };
        key(sender).zip(key(receiver)).is_some_and(|(sender, receiver)| sender == receiver && (!require_symbol || sender))
    }

    fn object_spread_properties(&self, source: &Ty) -> Option<Vec<StructProperty>> {
        if let Ty::Struct(properties) = source { return Some(properties.to_vec()) }
        if !matches!(source, Ty::Named(..) | Ty::Obj(..)) || !self.ck().index_signature_keys(source).is_empty() { return None }
        let mut names = Vec::new();
        self.ck().member_names(source, &mut names);
        Some(names.into_iter().filter_map(|name| {
            let resolution = self.ck().member_resolution(source, &name);
            resolution.declarations.iter().any(|declaration| declaration.member.kind == super::summary::MemberKind::Prop)
                .then_some(StructProperty { name, ty: resolution.ty, origin: None })
        }).collect())
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
                let object = s.get(id, F::Object).map_or(Ty::Unknown, |object| self.constrained_type_of(object));
                ck.member(&object, name)
            }
            Some(K::Index) => {
                let object = s.get(id, F::Object).map_or(Ty::Unknown, |object| self.constrained_type_of(object));
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

    /// A call's result; this file's own unannotated functions are typed
    /// from their bodies.
    fn call_type(&self, f: &Ty, id: u32) -> Ty {
        // Argument types only matter for choosing an overload or inferring
        // type parameters; big literals are costly to type for nothing.
        let args = if self.ck().needs_args(f) { self.args(id) } else { vec![Ty::Unknown; self.syntax.all(id, F::Arg).count()] };
        let t = self.ck().call(f, &args, self.type_args(id));
        match f {
            Ty::Fn(c) if c.ret.is_none() => match &c.sigs[..] {
                [sig] if sig.syntax.ret.is_none() => self.own_return(&sig.syntax, sig.env.file).unwrap_or(t),
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
    use crate::engine::tests::with_native_file;

    #[test]
    fn super_receivers_select_base_instance_or_constructor_through_arrows() {
        with_native_file(r#"
            class Base<T> { value: T; static value: boolean; }
            class Derived extends Base<string> {
                value: number;
                method() { const value = /*@*/super.value; const arrow = () => /*@*/super.value; }
                static method() { const value = /*@*/super.value; }
            }
        "#, |file, nodes| {
            for (node, expected) in nodes.iter().zip(["string", "string", "boolean"]) {
                assert!(matches!(file.type_of(*node), super::Ty::Prim(kind) if kind == expected),
                    "base receiver lost {expected}: {:?}", file.type_of(*node));
            }
        });
    }

    #[test]
    fn object_method_super_does_not_borrow_an_enclosing_class_base() {
        with_native_file(r#"
            class Base { value: string; }
            class Derived extends Base {
                method() {
                    return {
                        method() { return /*@*/super.value; },
                        arrow() { return () => /*@*/super.value; },
                    };
                }
            }
        "#, |file, nodes| {
            for &node in nodes {
                assert!(matches!(file.type_of(node), super::Ty::Unknown),
                    "object home borrowed the enclosing base: {:?}", file.type_of(node));
            }
        });
    }

    #[test]
    fn constant_template_types_keep_literals_without_narrowing_dynamic_templates() {
        with_native_file(r#"
            declare const dynamic: string;
            const first = /*@*/`ba${'zz'}`;
            const second = /*@*/`item${1}`;
            const third = /*@*/`ba${dynamic}`;
        "#, |file, nodes| {
            for (node, expected) in nodes.iter().zip(["\"bazz\"", "\"item1\""]) {
                assert!(matches!(file.type_of(*node), super::Ty::Lit(value, "string") if &*value == expected),
                    "constant template widened: {:?}", file.type_of(*node));
            }
            assert!(matches!(file.type_of(nodes[2]), super::Ty::Prim("string")), "dynamic template invented a literal");
        });
    }

    #[test]
    fn binding_pattern_nodes_share_contextual_and_iterated_binding_types() {
        with_native_file(r#"
            interface Item { value: string; child: { value: number }; }
            declare function consume(callback: (item: Item) => void): void;
            consume((/*@*/{ value, child: /*@*/{ value: nested } }) => {});
            declare const items: Item[];
            for (const /*@*/{ value } of items) {}
        "#, |file, nodes| {
            let checker = &file.typing().checker;
            for (node, expected) in nodes.iter().zip(["string", "number", "string"]) {
                let pattern = file.syntax.get(*node, super::F::Name).unwrap_or(*node);
                let value = checker.member(&file.type_of(pattern), "value");
                assert!(matches!(value, super::Ty::Prim(kind) if kind == expected),
                    "binding pattern lost {expected}: {value:?}");
            }
        });
    }

    #[test]
    fn new_target_uses_the_nearest_constructor_or_function_through_arrows() {
        let source = r#"
            class Outer {
                static marker: string;
                constructor() {
                    /*@*/ new.target.marker;
                    const arrow = () => { /*@*/ new /*keep*/ . target.marker; };
                    function local(): number {
                        /*@*/ new.target();
                        const nested = () => { /*@*/ new.target(); };
                        return 42;
                    }
                }
            }
            const Named = class Inner { static marker: number; constructor() { /*@*/ new.target.marker; } };
            const Anonymous = class { static marker: boolean; constructor() { /*@*/ new.target.marker; } };
        "#;
        crate::engine::tests::with_native_file(source, |file, marks| {
            assert_eq!(marks.len(), 6);
            for (mark, expected) in marks.iter().zip(["string", "string", "number", "number", "number", "boolean"]) {
                let expression = file.syntax.get(*mark, super::F::Expr).unwrap();
                assert!(matches!(file.type_of(expression), super::Ty::Prim(kind) if kind == expected),
                    "{expected}: {:?}", file.type_of(expression));
            }
        });
    }

    #[test]
    fn class_expressions_keep_constructor_static_and_lexical_self_types() {
        let source = r#"
            class C { marker: string }
            const K = class C { marker: number; self(): C { return this; } static marker: boolean };
            /*@*/ new K().self().marker;
            /*@*/ K.marker;
            /*@*/ new C().marker;
        "#;
        crate::engine::tests::with_native_file(source, |file, marks| {
            for (mark, expected) in marks.iter().zip(["number", "boolean", "string"]) {
                let expression = file.syntax.get(*mark, super::F::Expr).unwrap();
                assert!(matches!(file.type_of(expression), super::Ty::Prim(kind) if kind == expected),
                    "{expected}: {:?}", file.type_of(expression));
            }
        });
    }

    #[test]
    fn local_class_constructor_and_instance_contracts_share_identity() {
        for expression in ["", "const unrelated = class {};"] {
            let source = format!(r#"
                class Local {{ marker: string }}
                function f() {{
                    class Local {{ marker: number; self(): Local {{ return this; }} static marker: boolean }}
                    const constructor = /*@*/Local;
                    const instance = /*@*/new Local();
                    const returned = /*@*/new Local().self();
                    const field = /*@*/new Local().self().marker;
                    const staticField = /*@*/Local.marker;
                    {expression}
                }}
                const outer = /*@*/new Local().marker;
            "#);
            with_native_file(&source, |file, nodes| {
                assert!(matches!(file.type_of(nodes[0]), super::Ty::Class(_)), "local constructor unresolved");
                let instance = file.type_of(nodes[1]);
                assert!(!instance.is_opaque(), "local instance unresolved");
                assert!(instance.same_identity(&file.type_of(nodes[2])), "self return changed the class owner");
                for (&node, expected) in nodes[3..].iter().zip(["number", "boolean", "string"]) {
                    assert!(matches!(file.type_of(node), super::Ty::Prim(kind) if kind == expected),
                        "{expression}: expected {expected}, got {:?}", file.type_of(node));
                }
            });
        }
    }

    #[test]
    fn local_class_block_shadowing_does_not_leak() {
        let source = r#"
            function f() {
                class C { marker: number }
                {
                    class C { marker: boolean }
                    const inner = /*@*/new C().marker;
                }
                const outer = /*@*/new C().marker;
            }
            const missing = /*@*/new C().marker;
        "#;
        with_native_file(source, |file, nodes| {
            for (&node, expected) in nodes[..2].iter().zip(["boolean", "number"]) {
                assert!(matches!(file.type_of(node), super::Ty::Prim(kind) if kind == expected),
                    "expected {expected}, got {:?}", file.type_of(node));
            }
            assert!(!matches!(file.type_of(nodes[2]), super::Ty::Prim("number" | "boolean")),
                "a missing global acquired local class members: {:?}", file.type_of(nodes[2]));
        });
    }

    #[test]
    fn local_class_body_annotations_keep_outer_lexical_types() {
        let source = r#"
            type Value = string;
            function f() {
                type Value = number;
                class C {
                    method(value: Value): Value {
                        const reflected = /*@*/value;
                        return value;
                    }
                }
                const returned = /*@*/new C().method(1);
            }
        "#;
        with_native_file(source, |file, nodes| {
            for &node in nodes {
                assert!(matches!(file.type_of(node), super::Ty::Prim("number")),
                    "local class annotation used the wrong scope: {:?}", file.type_of(node));
            }
        });
    }

    #[test]
    fn object_property_docs_do_not_change_printing_or_assignability() {
        with_native_file("const tagged = /*@*/{ /** @deprecated */ value: { current: 1 } }; const ordinary = /*@*/{ value: { current: 1 } };", |file, nodes| {
            let checker = &file.typing().checker;
            let tagged = file.type_of(nodes[0]).widen();
            let ordinary = file.type_of(nodes[1]).widen();
            assert_eq!(checker.print(&tagged), checker.print(&ordinary));
            assert_eq!(checker.assignable(&tagged, &ordinary), Some(true));
            assert_eq!(checker.assignable(&ordinary, &tagged), Some(true));
        });
    }

    #[test]
    fn finite_object_spreads_keep_property_order_and_uncertainty() {
        for (source, expected) in [
            ("const source = { value: 'before' }; const result = /*@*/({ ...source, value: 2 }).value;", "number"),
            ("const source = { value: 'after' }; const result = /*@*/({ value: 2, ...source }).value;", "string"),
            ("declare const source: Missing; const result = /*@*/({ ...source, value: 2 });", "unknown"),
            ("declare const source: { [key: string]: number }; const result = /*@*/({ ...source, value: 2 });", "unknown"),
        ] {
            with_native_file(source, |file, nodes| {
                let ty = file.type_of(nodes[0]);
                match expected {
                    "number" => assert!(matches!(ty, super::Ty::Prim("number") | super::Ty::Lit(_, "number")), "{source}: {ty:?}"),
                    "string" => assert!(matches!(ty, super::Ty::Prim("string") | super::Ty::Lit(_, "string")), "{source}: {ty:?}"),
                    _ => assert!(matches!(ty, super::Ty::Unknown), "{source}: {ty:?}"),
                }
            });
        }
    }

    #[test]
    fn addition_uses_the_operand_runtime_domains() {
        for (source, expected) in [
            ("enum E { First, Second } const result = /*@*/E.First + E.Second;", "number"),
            ("enum E { First = 'a', Second = 'b' } const result = /*@*/E.First + E.Second;", "string"),
        ] {
            with_native_file(source, |file, nodes| {
                assert!(matches!(file.type_of(nodes[0]), super::Ty::Prim(kind) if kind == expected), "{source}");
            });
        }
    }

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
    fn constrained_receivers_preserve_member_and_index_results() {
        for (source, undefined) in [
            ("function f<T extends { item: number }>(value: T) { const result = /*@*/value.item; }", false),
            ("function f<T extends number[]>(value: T) { const result = /*@*/value[0]; }", false),
            ("function f<T extends { item: number }>(value: T | undefined) { const result = /*@*/value?.item; }", true),
            ("function f<T extends number[]>(value: T | undefined) { const result = /*@*/value?.[0]; }", true),
        ] {
            with_native_file(source, |file, nodes| {
                let flags = file.type_flags(nodes[0]).unwrap_or_default();
                assert!(flags.contains(&"number"), "{source}: {flags:?}");
                assert_eq!(flags.contains(&"undefined"), undefined, "{source}: {flags:?}");
            });
        }
        with_native_file("function f<T extends { item: number }>(value: T) { const result = /*@*/value; }", |file, nodes| {
            assert!(matches!(file.type_of(nodes[0]), super::Ty::Param(_)));
        });
    }

    #[test]
    fn lexical_this_annotations_preserve_the_class_member_owner() {
        for (source, number) in [
            ("class A { value = 0; method() { const that = {} as this & { extra: string }; const result = /*@*/that.value; } }", true),
            ("class A { value = 0; method() { function inner() { const that = {} as this; const result = /*@*/that.value; } } }", true),
            ("class A<T> { value!: T; method() { const that = {} as this; const result = /*@*/that.value; } }", false),
        ] {
            with_native_file(source, |file, nodes| {
                let object = file.syntax.get(nodes[0], super::F::Object).expect("member receiver");
                let checker = file.ck();
                let resolution = checker.member_resolution(&file.constrained_type_of(object), "value");
                assert!(resolution.declarations.iter().any(|declaration|
                    declaration.owner.is_some_and(|owner|
                        owner.file == checker.file && &*checker.summary.decls[owner.decl as usize].name == "A")), "{source}");
                if number {
                    assert!(file.type_flags(nodes[0]).unwrap_or_default().contains(&"number"), "{source}");
                }
            });
        }
    }

    #[test]
    fn class_property_types_keep_inferred_and_declared_literal_domains() {
        for (source, domain, literal) in [
            ("class A { /*@*/private value = 7; }", "number", false),
            ("declare const hello: 'hello'; class A { /*@*/private value = hello; }", "string", true),
            ("declare const hello: 'hello' | 10; class A { /*@*/private value = hello; }", "string", true),
            ("class A { /*@*/private static value = 7; }", "number", false),
            ("class A { /*@*/#value = 7; }", "number", false),
            ("class A { /*@*/private value: boolean = true; }", "boolean", false),
        ] {
            with_native_file(source, |file, nodes| {
                let flags = file.type_flags(nodes[0]).unwrap_or_default();
                assert!(flags.contains(&domain), "{source}: {flags:?}");
                assert_eq!(flags.contains(&"literal"), literal, "{source}: {flags:?}");
            });
        }
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
            ("declare const value: { item: number } | null; if (value !== null) { function later() { const result = /*@*/value?.item; } }", false, false),
        ] {
            with_native_file(source, |file, nodes| {
                let flags = file.type_flags(nodes[0]).unwrap_or_default();
                assert!(flags.contains(&"number"), "{source}: {flags:?}");
                assert_eq!(flags.contains(&"undefined"), undefined, "{source}: {flags:?}");
                assert_eq!(flags.contains(&"null"), null, "{source}: {flags:?}");
            });
        }
    }
}

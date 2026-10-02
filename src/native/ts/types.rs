//! A declared-type evaluator: types come from annotations, never from
//! inference beyond the obvious (an argument fixing a type parameter, an
//! arrow's expression body). Anything it does not model is `Unknown`, and
//! rules treat `Unknown` as "no fact", so they stay quiet rather than guess.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use super::modules::{DeclRef, FileId, Globals, ModRef, ModuleAt, Project};
use super::summary::{DeclKind, Member, MemberKind, Sig, Summary, TypeSyn};

/// All declarations merged under one name (`interface Promise` is spread
/// over several lib files; `class X` + `namespace X`).
pub type Sym = Arc<[DeclRef]>;

#[derive(Clone, Debug)]
pub enum Ty {
    Unknown,
    Any,
    Never,
    Void,
    Undefined,
    Null,
    Prim(&'static str),
    /// A literal type and its primitive.
    Lit(Box<str>, &'static str),
    /// A class instance, interface or enum type, with type arguments.
    Named(Sym, Vec<Ty>),
    Union(Vec<Ty>),
    Inter(Vec<Ty>),
    Array(Box<Ty>),
    Tuple(Vec<Ty>),
    Fn(Arc<Callable>),
    Obj(Arc<Vec<Member>>, Arc<Env>),
    /// A class as a value (its constructor and statics).
    Class(Sym),
    /// A namespace or enum as a value.
    Ns(Sym),
    Module(ModRefKey),
    /// An unresolved type parameter.
    Param(Box<str>),
}

/// A module as a value (`import * as m`).
#[derive(Clone, Copy, Debug)]
pub struct ModRefKey(pub ModRef);

/// Signatures with the environment their types are written in.
#[derive(Debug)]
pub struct Callable {
    pub sigs: Vec<Arc<Sig>>,
    pub env: Arc<Env>,
    /// The return type when it is known without the syntax (an arrow's
    /// expression body).
    pub ret: Option<Ty>,
}

/// Where type syntax is evaluated: a file, the module and namespaces it
/// sits in, bound type parameters and `this`.
#[derive(Clone, Debug)]
pub struct Env {
    pub file: FileId,
    pub module: ModuleAt,
    /// Enclosing namespace declarations, innermost first.
    pub namespaces: Vec<u32>,
    pub tparams: Vec<(Box<str>, Ty)>,
    pub this: Option<Box<Ty>>,
}

impl Env {
    pub fn file(file: FileId) -> Env {
        Env { file, module: ModuleAt::Top, namespaces: Vec::new(), tparams: Vec::new(), this: None }
    }
}

impl Ty {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }

    fn union(items: Vec<Ty>) -> Ty {
        let mut out: Vec<Ty> = Vec::new();
        for t in items {
            match t {
                Ty::Never => {}
                Ty::Union(inner) => out.extend(inner),
                Ty::Any => return Ty::Any,
                t => out.push(t),
            }
        }
        match out.len() {
            0 => Ty::Never,
            1 => out.pop().expect("one"),
            _ => Ty::Union(out),
        }
    }

    /// Without `null` and `undefined` (`x!`).
    pub fn non_null(self) -> Ty {
        match self {
            Ty::Null | Ty::Undefined => Ty::Never,
            Ty::Union(items) => Ty::union(items.into_iter().map(Ty::non_null).collect()),
            t => t,
        }
    }

    /// The type plus `undefined` (an optional chain that may stop).
    pub fn or_undefined(self) -> Ty {
        match self {
            Ty::Unknown => Ty::Unknown,
            t => Ty::union(vec![t, Ty::Undefined]),
        }
    }

    /// Literal types widened to their primitives (`let x = 0`).
    pub fn widen(self) -> Ty {
        match self {
            Ty::Lit(_, p) => Ty::Prim(p),
            Ty::Union(items) => Ty::union(items.into_iter().map(Ty::widen).collect()),
            t => t,
        }
    }
}

/// Evaluates types for one linted file. Caches live as long as the file's
/// lint; summaries are shared through the project.
pub struct Checker<'p> {
    pub project: &'p Project,
    pub file: FileId,
    pub summary: Summary,
    globals: Arc<Globals>,
    depth: Cell<u32>,
    values: RefCell<HashMap<DeclRef, Ty>>,
}

const MAX_DEPTH: u32 = 64;

impl<'p> Checker<'p> {
    pub fn new(project: &'p Project, file: FileId, summary: Summary) -> Self {
        let globals = project.globals(file);
        Checker { project, file, summary, globals, depth: Cell::new(0), values: RefCell::new(HashMap::new()) }
    }

    /// Runs `f` one level deeper, or gives up past the depth limit.
    fn deeper(&self, f: impl FnOnce() -> Ty) -> Ty {
        let d = self.depth.get();
        if d >= MAX_DEPTH {
            return Ty::Unknown;
        }
        self.depth.set(d + 1);
        let t = f();
        self.depth.set(d);
        t
    }

    pub fn summary_of(&self, file: FileId) -> Option<Summary> {
        if file == self.file {
            return Some(self.summary.clone());
        }
        self.project.summary(file)
    }

    /// The environment a declaration's own types are written in.
    pub fn decl_env(&self, d: DeclRef) -> Env {
        let mut env = Env::file(d.file);
        if let Some(s) = self.summary_of(d.file) {
            let decl = &s.decls[d.decl as usize];
            if let Some(m) = decl.module {
                env.module = ModuleAt::Ambient(m);
            }
            let mut p = decl.parent;
            while let Some(ns) = p {
                env.namespaces.push(ns);
                p = s.decls[ns as usize].parent;
            }
        }
        env
    }

    // ---- names -------------------------------------------------------------

    /// Declarations named `name` visible in `env`: namespaces, the module,
    /// then globals.
    pub fn lookup(&self, env: &Env, name: &str) -> Vec<DeclRef> {
        let Some(s) = self.summary_of(env.file) else { return Vec::new() };
        let refs = |ids: &[u32]| ids.iter().map(|&decl| DeclRef { file: env.file, decl }).collect::<Vec<_>>();
        for &ns in &env.namespaces {
            if let Some(scope) = &s.decls[ns as usize].scope {
                let found = scope.get(name);
                if !found.is_empty() {
                    return refs(found);
                }
            }
        }
        let (_, scope) = env.module.get(&s);
        let found = scope.get(name);
        if !found.is_empty() {
            return refs(found);
        }
        // Globals merge across every file (`namespace NodeJS` is spread
        // over many declaration files).
        let mut all = refs(s.globals.get(name));
        for r in self.globals.names.get(name).map_or(&[][..], Vec::as_slice) {
            if !all.contains(r) {
                all.push(*r);
            }
        }
        all
    }

    /// Follows imports to the declarations they name.
    fn follow(&self, refs: Vec<DeclRef>) -> Vec<DeclRef> {
        let mut out = Vec::new();
        for r in refs {
            let Some(s) = self.summary_of(r.file) else { continue };
            match &s.decls[r.decl as usize].kind {
                DeclKind::Import { from, name } if &**name != "*" => {
                    if let Some(m) = self.project.resolve(r.file, from) {
                        if self.depth.get() < MAX_DEPTH {
                            self.depth.set(self.depth.get() + 1);
                            let found = self.project.export(m, name);
                            out.extend(self.follow(found));
                            self.depth.set(self.depth.get() - 1);
                        }
                    }
                }
                _ => out.push(r),
            }
        }
        out
    }

    fn kind_of(&self, r: DeclRef) -> Option<DeclKind> {
        Some(self.summary_of(r.file)?.decls[r.decl as usize].kind.clone())
    }

    /// The members of a namespace (or `import * as m`) named `name`.
    fn namespace_member(&self, sym: &[DeclRef], name: &str) -> Vec<DeclRef> {
        let mut out = Vec::new();
        for r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            match &decl.kind {
                DeclKind::Namespace => {
                    if let Some(scope) = &decl.scope {
                        out.extend(scope.get(name).iter().map(|&decl| DeclRef { file: r.file, decl }));
                    }
                }
                DeclKind::Import { from, name: n } if &**n == "*" => {
                    if let Some(m) = self.project.resolve(r.file, from) {
                        out.extend(self.module_member(m, name));
                    }
                }
                _ => {}
            }
        }
        self.follow(out)
    }

    fn module_member(&self, m: ModRef, name: &str) -> Vec<DeclRef> {
        let found = self.project.export(m, name);
        if !found.is_empty() {
            return found;
        }
        // `export = ns`: members of that namespace.
        let eq = self.project.export(m, "=");
        if eq.is_empty() {
            return Vec::new();
        }
        self.namespace_member(&eq, name)
    }

    // ---- type syntax -------------------------------------------------------

    pub fn eval(&self, syn: &TypeSyn, env: &Env) -> Ty {
        self.deeper(|| self.eval_inner(syn, env))
    }

    fn eval_inner(&self, syn: &TypeSyn, env: &Env) -> Ty {
        match syn {
            TypeSyn::Kw(k) => match &**k {
                "any" => Ty::Any,
                "unknown" => Ty::Unknown,
                "never" => Ty::Never,
                "void" => Ty::Void,
                "undefined" => Ty::Undefined,
                "null" => Ty::Null,
                "string" => Ty::Prim("string"),
                "number" => Ty::Prim("number"),
                "boolean" => Ty::Prim("boolean"),
                "bigint" => Ty::Prim("bigint"),
                "symbol" => Ty::Prim("symbol"),
                "object" => Ty::Prim("object"),
                "this" => env.this.as_deref().cloned().unwrap_or(Ty::Unknown),
                _ => Ty::Unknown,
            },
            TypeSyn::Lit(text) => {
                let prim = if text.starts_with(['"', '\'', '`']) {
                    "string"
                } else if &**text == "true" || &**text == "false" {
                    "boolean"
                } else if text.ends_with('n') {
                    "bigint"
                } else {
                    "number"
                };
                Ty::Lit(text.clone(), prim)
            }
            TypeSyn::Ref(path, args) => {
                if path.len() == 1 {
                    if let Some((_, t)) = env.tparams.iter().rev().find(|(n, _)| **n == *path[0]) {
                        return t.clone();
                    }
                }
                let args: Vec<Ty> = args.iter().map(|a| self.eval(a, env)).collect();
                self.named_type(path, args, env)
            }
            TypeSyn::Union(items) => Ty::union(items.iter().map(|t| self.eval(t, env)).collect()),
            TypeSyn::Inter(items) => {
                let items: Vec<Ty> = items.iter().map(|t| self.eval(t, env)).collect();
                if items.iter().any(|t| matches!(t, Ty::Never)) {
                    Ty::Never
                } else {
                    Ty::Inter(items)
                }
            }
            TypeSyn::Array(t) => Ty::Array(Box::new(self.eval(t, env))),
            TypeSyn::Tuple(items) => Ty::Tuple(items.iter().map(|t| self.eval(t, env)).collect()),
            TypeSyn::Fn(sig) => Ty::Fn(Arc::new(Callable { sigs: vec![sig.clone()], env: Arc::new(env.clone()), ret: None })),
            TypeSyn::Obj(members) => Ty::Obj(members.clone(), Arc::new(env.clone())),
            TypeSyn::Query(path) => self.value_path(path, env),
            TypeSyn::Ctor(_) | TypeSyn::Other => Ty::Unknown,
        }
    }

    /// The type declarations a type name (`A.B`) refers to.
    pub fn type_decls(&self, path: &[Box<str>], env: &Env) -> Vec<DeclRef> {
        let Some((first, rest)) = path.split_first() else { return Vec::new() };
        let mut sym = self.follow(self.lookup(env, first));
        for seg in rest {
            sym = self.namespace_member(&sym, seg);
        }
        sym.into_iter()
            .filter(|&r| matches!(self.kind_of(r), Some(DeclKind::Class | DeclKind::Interface | DeclKind::Alias | DeclKind::Enum)))
            .collect()
    }

    /// A type name (`A.B`) with type arguments.
    fn named_type(&self, path: &[Box<str>], args: Vec<Ty>, env: &Env) -> Ty {
        let types = self.type_decls(path, env);
        let Some(&first_decl) = types.first() else { return Ty::Unknown };
        if self.kind_of(first_decl) == Some(DeclKind::Alias) {
            let Some(s) = self.summary_of(first_decl.file) else { return Ty::Unknown };
            let decl = &s.decls[first_decl.decl as usize];
            let mut alias_env = self.decl_env(first_decl);
            alias_env.tparams = self.bind_tparams(&decl.type_params, args, &alias_env);
            return match &decl.ty {
                Some(t) => self.eval(t, &alias_env),
                None => Ty::Unknown,
            };
        }
        if path.len() == 1 && matches!(&*path[0], "Array" | "ReadonlyArray") && args.len() == 1 {
            return Ty::Array(Box::new(args.into_iter().next().expect("one")));
        }
        Ty::Named(types.into(), args)
    }

    /// Type parameters bound to arguments, their defaults, or unknown.
    pub fn bind_tparams(&self, params: &[super::summary::TParam], args: Vec<Ty>, env: &Env) -> Vec<(Box<str>, Ty)> {
        let mut bound: Vec<(Box<str>, Ty)> = env.tparams.clone();
        let mut args = args.into_iter();
        for p in params {
            let t = match args.next() {
                Some(t) => t,
                None => {
                    let e = Env { tparams: bound.clone(), ..env.clone() };
                    p.default.as_ref().map_or(Ty::Unknown, |d| self.eval(d, &e))
                }
            };
            bound.push((p.name.clone(), t));
        }
        bound
    }

    // ---- values ------------------------------------------------------------

    /// The type of a value path (`a.b.c`) in `env`.
    pub fn value_path(&self, path: &[Box<str>], env: &Env) -> Ty {
        let Some((first, rest)) = path.split_first() else { return Ty::Unknown };
        let mut t = self.value_of(&self.lookup(env, first));
        for seg in rest {
            t = self.member(&t, seg);
        }
        t
    }

    /// The value a set of merged declarations names.
    pub fn value_of(&self, refs: &[DeclRef]) -> Ty {
        let refs = self.follow(refs.to_vec());
        let values: Vec<DeclRef> = refs
            .iter()
            .copied()
            .filter(|&r| !matches!(self.kind_of(r), Some(DeclKind::Interface | DeclKind::Alias) | None))
            .collect();
        let Some(&first) = values.first() else { return Ty::Unknown };
        if let Some(t) = self.values.borrow().get(&first) {
            return t.clone();
        }
        // Cycles (`const a = b; const b = a`) see unknown.
        self.values.borrow_mut().insert(first, Ty::Unknown);
        let t = self.deeper(|| self.value_of_decl(first, &values));
        self.values.borrow_mut().insert(first, t.clone());
        t
    }

    fn value_of_decl(&self, first: DeclRef, all: &[DeclRef]) -> Ty {
        let Some(s) = self.summary_of(first.file) else { return Ty::Unknown };
        let decl = &s.decls[first.decl as usize];
        let env = self.decl_env(first);
        match &decl.kind {
            DeclKind::Var => {
                if let Some(t) = &decl.ty {
                    return self.eval(t, &env);
                }
                decl.init.as_ref().map_or(Ty::Unknown, |i| self.init_type(i, &env))
            }
            DeclKind::Function => {
                let sigs: Vec<Arc<Sig>> = all
                    .iter()
                    .filter_map(|&r| {
                        let s = self.summary_of(r.file)?;
                        let d = &s.decls[r.decl as usize];
                        (d.kind == DeclKind::Function).then(|| d.sigs.clone())
                    })
                    .flatten()
                    .collect();
                Ty::Fn(Arc::new(Callable { sigs, env: Arc::new(env), ret: None }))
            }
            DeclKind::Class => Ty::Class(all.iter().copied().filter(|&r| self.kind_of(r) == Some(DeclKind::Class)).collect()),
            DeclKind::Enum | DeclKind::Namespace => Ty::Ns(all.to_vec().into()),
            DeclKind::Import { from, name } if &**name == "*" => match self.project.resolve(first.file, from) {
                Some(m) => {
                    let eq = self.project.export(m, "=");
                    if eq.is_empty() {
                        Ty::Module(ModRefKey(m))
                    } else {
                        self.value_of(&eq)
                    }
                }
                None => Ty::Unknown,
            },
            DeclKind::ImportAlias(path) => self.value_path(path, &env),
            _ => Ty::Unknown,
        }
    }

    /// The type of a simple initializer written in `env`.
    fn init_type(&self, init: &super::summary::Init, env: &Env) -> Ty {
        use super::summary::Init;
        match init {
            Init::New(path, targs) => {
                let targs = targs.iter().map(|t| self.eval(t, env)).collect::<Vec<_>>();
                let class = self.value_path(path, env);
                self.construct(&class, &[], targs)
            }
            Init::Call(callee, targs) => {
                let targs = targs.iter().map(|t| self.eval(t, env)).collect::<Vec<_>>();
                let f = self.init_type(callee, env);
                self.call(&f, &[], targs)
            }
            Init::Member(object, name) => {
                let o = self.init_type(object, env);
                self.member(&o, name)
            }
            Init::Await(inner) => {
                let t = self.init_type(inner, env);
                self.awaited(&t)
            }
            Init::Path(path) if path.first().is_some_and(|p| &**p == "this") => {
                let t = env.this.as_deref().cloned().unwrap_or(Ty::Unknown);
                path[1..].iter().fold(t, |t, seg| self.member(&t, seg))
            }
            Init::Path(path) => self.value_path(path, env),
            Init::Type(t) => self.eval(t, env).widen(),
            Init::Other => Ty::Unknown,
        }
    }

    /// A property's type: its annotation, else its initializer's.
    fn prop_type(&self, m: &Member, env: &Env) -> Ty {
        let t = match (&m.ty, &m.init) {
            (Some(t), _) => self.eval(t, env),
            (None, Some(i)) => self.init_type(i, env),
            (None, None) => Ty::Unknown,
        };
        if m.optional {
            t.or_undefined()
        } else {
            t
        }
    }

    // ---- members -----------------------------------------------------------

    /// The type of member `name` of a value of type `t`.
    pub fn member(&self, t: &Ty, name: &str) -> Ty {
        self.deeper(|| self.member_inner(t, name, t))
    }

    pub fn global_named(&self, name: &str, args: Vec<Ty>) -> Ty {
        let types: Vec<DeclRef> = self
            .globals
            .names
            .get(name)
            .map(|r| r.iter().copied().filter(|&d| matches!(self.kind_of(d), Some(DeclKind::Interface | DeclKind::Class))).collect())
            .unwrap_or_default();
        if types.is_empty() {
            Ty::Unknown
        } else {
            Ty::Named(types.into(), args)
        }
    }

    /// The interface primitives and arrays get their members from.
    pub fn apparent(&self, t: &Ty) -> Ty {
        match t {
            Ty::Prim("string") | Ty::Lit(_, "string") => self.global_named("String", vec![]),
            Ty::Prim("number") | Ty::Lit(_, "number") => self.global_named("Number", vec![]),
            Ty::Prim("boolean") | Ty::Lit(_, "boolean") => self.global_named("Boolean", vec![]),
            Ty::Prim("bigint") => self.global_named("BigInt", vec![]),
            Ty::Prim("symbol") => self.global_named("Symbol", vec![]),
            Ty::Array(e) => self.global_named("Array", vec![(**e).clone()]),
            Ty::Tuple(items) => self.global_named("Array", vec![Ty::union(items.clone())]),
            Ty::Fn(_) | Ty::Class(_) => self.global_named("Function", vec![]),
            other => other.clone(),
        }
    }

    /// Every step of a member lookup counts against the depth limit:
    /// heritage can loop (`interface A extends B`, `B extends A`).
    fn member_inner(&self, t: &Ty, name: &str, receiver: &Ty) -> Ty {
        self.deeper(|| self.member_core(t, name, receiver))
    }

    fn member_core(&self, t: &Ty, name: &str, receiver: &Ty) -> Ty {
        match t {
            Ty::Any => Ty::Any,
            Ty::Named(sym, args) => {
                // Enum members are the enum type.
                if sym.first().is_some_and(|&r| self.kind_of(r) == Some(DeclKind::Enum)) {
                    return self.member_inner(&Ty::Prim("number"), name, receiver);
                }
                self.instance_member(sym, args, name, receiver)
            }
            Ty::Union(items) => {
                let found: Vec<Ty> = items
                    .iter()
                    .filter(|i| !matches!(i, Ty::Undefined | Ty::Null))
                    .map(|i| self.member_inner(i, name, i))
                    .collect();
                if found.iter().any(Ty::is_unknown) {
                    Ty::Unknown
                } else {
                    Ty::union(found)
                }
            }
            Ty::Inter(items) => items.iter().map(|i| self.member_inner(i, name, receiver)).find(|t| !t.is_unknown()).unwrap_or(Ty::Unknown),
            Ty::Obj(members, env) => {
                let env = Env { this: Some(Box::new(receiver.clone())), ..(**env).clone() };
                self.member_from(members.iter(), name, &env, false).unwrap_or(Ty::Unknown)
            }
            Ty::Class(sym) => {
                if name == "prototype" {
                    return Ty::Named(sym.clone(), Vec::new());
                }
                self.static_member(sym, name).unwrap_or_else(|| self.member_inner(&self.global_named("Function", vec![]), name, receiver))
            }
            Ty::Ns(sym) => {
                if sym.first().is_some_and(|&r| self.kind_of(r) == Some(DeclKind::Enum)) {
                    let enum_sym: Sym = sym.iter().copied().filter(|&r| self.kind_of(r) == Some(DeclKind::Enum)).collect();
                    return Ty::Named(enum_sym, Vec::new());
                }
                let found = self.namespace_member(sym, name);
                self.value_of(&found)
            }
            Ty::Module(m) => {
                let found = self.module_member(m.0, name);
                self.value_of(&found)
            }
            Ty::Prim(_) | Ty::Lit(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_) => {
                let apparent = self.apparent(t);
                if apparent.is_unknown() {
                    return Ty::Unknown;
                }
                self.member_inner(&apparent, name, receiver)
            }
            _ => Ty::Unknown,
        }
    }

    fn static_member(&self, sym: &[DeclRef], name: &str) -> Option<Ty> {
        let d = self.depth.get();
        if d >= MAX_DEPTH {
            return None;
        }
        self.depth.set(d + 1);
        let found = self.static_member_core(sym, name);
        self.depth.set(d);
        found
    }

    fn static_member_core(&self, sym: &[DeclRef], name: &str) -> Option<Ty> {
        for &r in sym {
            let s = self.summary_of(r.file)?;
            let decl = &s.decls[r.decl as usize];
            let env = self.decl_env(r);
            if let Some(t) = self.member_from(decl.members.iter().filter(|m| m.is_static), name, &env, true) {
                return Some(t);
            }
            // Statics are inherited too.
            if let Some(TypeSyn::Ref(path, _)) = decl.extends.first().filter(|_| decl.kind == DeclKind::Class) {
                if let Ty::Class(base) = self.value_path(path, &env) {
                    if base.iter().any(|b| sym.contains(b)) {
                        continue;
                    }
                    if let Some(t) = self.static_member(&base, name) {
                        return Some(t);
                    }
                }
            }
        }
        // A class merged with a namespace.
        let ns = self.namespace_member(sym, name);
        (!ns.is_empty()).then(|| self.value_of(&ns))
    }

    fn instance_member(&self, sym: &[DeclRef], args: &[Ty], name: &str, receiver: &Ty) -> Ty {
        let mut sigs: Vec<Arc<Sig>> = Vec::new();
        let mut sig_env: Option<Env> = None;
        for &r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, args.to_vec(), &env);
            env.this = Some(Box::new(receiver.clone()));
            for m in decl.members.iter().filter(|m| !m.is_static && &*m.name == name) {
                match m.kind {
                    MemberKind::Method => {
                        sigs.extend(m.sigs.iter().cloned());
                        sig_env.get_or_insert_with(|| env.clone());
                    }
                    MemberKind::Prop => return self.prop_type(m, &env),
                    _ => {}
                }
            }
        }
        if let Some(env) = sig_env {
            return Ty::Fn(Arc::new(Callable { sigs, env: Arc::new(env), ret: None }));
        }
        // Inherited.
        for &r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, args.to_vec(), &env);
            let bases: Vec<&TypeSyn> = match decl.kind {
                // `implements` lists add no members.
                DeclKind::Class => decl.extends.first().into_iter().collect(),
                _ => decl.extends.iter().collect(),
            };
            for base in bases {
                let base_ty = self.eval(base, &env);
                if base_ty.is_unknown() {
                    continue;
                }
                let t = self.member_inner(&base_ty, name, receiver);
                if !t.is_unknown() {
                    return t;
                }
            }
        }
        Ty::Unknown
    }

    fn member_from<'m>(&self, members: impl Iterator<Item = &'m Member>, name: &str, env: &Env, _statics: bool) -> Option<Ty> {
        let mut sigs = Vec::new();
        for m in members.filter(|m| &*m.name == name) {
            match m.kind {
                MemberKind::Method => sigs.extend(m.sigs.iter().cloned()),
                MemberKind::Prop => return Some(self.prop_type(m, env)),
                _ => {}
            }
        }
        (!sigs.is_empty()).then(|| Ty::Fn(Arc::new(Callable { sigs, env: Arc::new(env.clone()), ret: None })))
    }

    /// Every member name of `t`, inherited ones included (for unions, the
    /// names of any constituent).
    pub fn member_names(&self, t: &Ty, out: &mut Vec<Box<str>>) {
        if self.depth.get() >= MAX_DEPTH {
            return;
        }
        self.depth.set(self.depth.get() + 1);
        match t {
            // Named types repeat a lot (and their heritage is deep): the
            // project remembers their member names.
            Ty::Named(sym, args) => {
                let key = (sym.to_vec(), args.iter().map(|a| self.print(a)).collect::<Vec<_>>());
                let names = match self.project.cached_members(&key) {
                    Some(names) => names,
                    None => {
                        let mut names = Vec::new();
                        self.named_member_names(sym, args, &mut names);
                        let names: Arc<[Box<str>]> = names.into();
                        self.project.cache_members(key, names.clone());
                        names
                    }
                };
                for n in names.iter() {
                    if !out.contains(n) {
                        out.push(n.clone());
                    }
                }
            }
            Ty::Union(items) | Ty::Inter(items) => items.iter().for_each(|i| self.member_names(i, out)),
            Ty::Obj(members, _) => out.extend(members.iter().map(|m| m.name.clone())),
            Ty::Prim(_) | Ty::Lit(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_) => {
                let a = self.apparent(t);
                if !matches!(a, Ty::Prim(_) | Ty::Lit(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_)) {
                    self.member_names(&a, out);
                }
            }
            _ => {}
        }
        self.depth.set(self.depth.get() - 1);
    }

    fn named_member_names(&self, sym: &[DeclRef], args: &[Ty], out: &mut Vec<Box<str>>) {
        for &r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            for m in decl.members.iter().filter(|m| !m.is_static) {
                if !out.contains(&m.name) {
                    out.push(m.name.clone());
                }
            }
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, args.to_vec(), &env);
            let bases: Vec<&TypeSyn> = match decl.kind {
                DeclKind::Class => decl.extends.first().into_iter().collect(),
                _ => decl.extends.iter().collect(),
            };
            for base in bases {
                let b = self.eval(base, &env);
                self.member_names(&b, out);
            }
        }
    }

    // ---- calls -------------------------------------------------------------

    /// Signatures `t` can be called with (or constructed with).
    fn signatures(&self, t: &Ty, construct: bool) -> Option<(Vec<Arc<Sig>>, Arc<Env>)> {
        match t {
            Ty::Fn(c) if !construct => Some((c.sigs.clone(), c.env.clone())),
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

    /// The first signature that accepts `n` arguments.
    fn pick(sigs: &[Arc<Sig>], n: usize) -> Option<&Arc<Sig>> {
        sigs.iter()
            .find(|s| {
                let required = s.params.iter().filter(|p| !p.optional && !p.rest).count();
                let rest = s.params.iter().any(|p| p.rest);
                n >= required && (rest || n <= s.params.len())
            })
            .or_else(|| sigs.first())
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
        let Some(sig) = Self::pick(&sigs, args.len()) else { return Ty::Unknown };
        self.sig_result(sig, &env, args, targs)
    }

    /// The signature `callee` resolves to for `n` arguments, with its env.
    pub fn signature_for(&self, f: &Ty, n: usize, construct: bool) -> Option<(Arc<Sig>, Arc<Env>)> {
        let (sigs, env) = match (f, construct) {
            (Ty::Class(sym), true) => {
                let r = *sym.first()?;
                let s = self.summary_of(r.file)?;
                let decl = &s.decls[r.decl as usize];
                (decl.sigs.clone(), Arc::new(self.decl_env(r)))
            }
            _ => self.signatures(f, construct)?,
        };
        Some((Self::pick(&sigs, n)?.clone(), env))
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
    pub fn infer(&self, syn: &TypeSyn, actual: &Ty, params: &[super::summary::TParam], out: &mut Vec<(Box<str>, Ty)>) {
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
                let mut inferred = Vec::new();
                if let Some(sig) = Self::pick(&decl.sigs, args.len()) {
                    for (p, a) in sig.params.iter().zip(args) {
                        if let Some(t) = &p.ty {
                            self.infer(t, a, &decl.type_params, &mut inferred);
                        }
                    }
                }
                let env = self.decl_env(r);
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
                Some((sigs, env)) => match Self::pick(&sigs, args.len()) {
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

    pub fn decl_name(&self, sym: &[DeclRef]) -> Option<Box<str>> {
        let r = sym.first()?;
        Some(self.summary_of(r.file)?.decls[r.decl as usize].name.clone())
    }

    // ---- printing -----------------------------------------------------------

    /// The type as TypeScript would print it, roughly; `?` for parts it
    /// does not know.
    pub fn print(&self, t: &Ty) -> String {
        if self.depth.get() >= MAX_DEPTH {
            return "?".into();
        }
        self.depth.set(self.depth.get() + 1);
        let s = match t {
            Ty::Unknown => "?".into(),
            Ty::Any => "any".into(),
            Ty::Never => "never".into(),
            Ty::Void => "void".into(),
            Ty::Undefined => "undefined".into(),
            Ty::Null => "null".into(),
            Ty::Prim(p) => (*p).into(),
            Ty::Lit(text, _) => text.to_string(),
            Ty::Named(sym, args) => {
                let name = self.qualified_name(sym);
                if args.is_empty() {
                    name
                } else {
                    format!("{name}<{}>", args.iter().map(|a| self.print(a)).collect::<Vec<_>>().join(", "))
                }
            }
            Ty::Union(items) => items.iter().map(|i| self.print(i)).collect::<Vec<_>>().join(" | "),
            Ty::Inter(items) => items.iter().map(|i| self.print(i)).collect::<Vec<_>>().join(" & "),
            Ty::Array(e) => {
                let inner = self.print(e);
                if matches!(**e, Ty::Union(_) | Ty::Inter(_) | Ty::Fn(_)) {
                    format!("({inner})[]")
                } else {
                    format!("{inner}[]")
                }
            }
            Ty::Tuple(items) => format!("[{}]", items.iter().map(|i| self.print(i)).collect::<Vec<_>>().join(", ")),
            Ty::Fn(_) => "Function".into(),
            Ty::Obj(..) => "object".into(),
            Ty::Class(sym) => format!("typeof {}", self.qualified_name(sym)),
            Ty::Ns(sym) => format!("typeof {}", self.qualified_name(sym)),
            Ty::Module(_) => "module".into(),
            Ty::Param(n) => n.to_string(),
        };
        self.depth.set(self.depth.get() - 1);
        s
    }

    /// `NodeJS.Timeout`: a declaration's name with its namespaces.
    fn qualified_name(&self, sym: &[DeclRef]) -> String {
        let Some(&r) = sym.first() else { return "?".into() };
        let Some(s) = self.summary_of(r.file) else { return "?".into() };
        let mut parts = vec![s.decls[r.decl as usize].name.to_string()];
        let mut p = s.decls[r.decl as usize].parent;
        while let Some(ns) = p {
            parts.push(s.decls[ns as usize].name.to_string());
            p = s.decls[ns as usize].parent;
        }
        parts.reverse();
        parts.join(".")
    }
}

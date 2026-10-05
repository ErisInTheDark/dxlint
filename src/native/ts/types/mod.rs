//! A declared-type evaluator: types come from annotations, never from
//! inference beyond the obvious (an argument fixing a type parameter, an
//! arrow's expression body). Anything it does not model is `Unknown`, and
//! rules treat `Unknown` as "no fact", so they stay quiet rather than guess.
//!
//! This module owns names, type syntax and values; `members`, `calls`,
//! `relate` (assignability) and `print` extend the same `Checker`.

mod calls;
mod domains;
mod members;
mod indexed;
mod iterability;
mod print;
mod relate;
mod utilities;
mod identity;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use super::modules::{DeclRef, FileId, Globals, ModRef, ModuleAt, Project};
use super::summary::{DeclKind, Member, Sig, Summary, TParam, TupleElement, TypeSyn};

pub use domains::{LiteralValue, ValueDomains};
pub use calls::ResolvedPredicate;
pub use members::Resolution;

/// All declarations merged under one name (`interface Promise` is spread
/// over several lib files; `class X` + `namespace X`).
pub type Sym = Arc<[DeclRef]>;

#[derive(Clone, Debug)]
pub enum Ty {
    /// Unsupported or unresolved computation, not TypeScript's `unknown`.
    Unknown,
    IntrinsicUnknown,
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
    Array(Array),
    Tuple(Tuple),
    Fn(Arc<Callable>),
    /// An object type written as type syntax (`{ a: string }`).
    Obj(Arc<Vec<Member>>, Arc<Env>),
    /// An object literal's type: its properties.
    Struct(Arc<[(Box<str>, Ty)]>),
    /// A class as a value (its constructor and statics).
    Class(Sym),
    /// A namespace or enum as a value.
    Ns(Sym),
    Module(ModRefKey),
    /// An unresolved type parameter.
    Param(Box<str>),
}

#[derive(Clone, Debug)]
pub struct Array {
    pub element: Box<Ty>,
    pub readonly: bool,
}

#[derive(Clone, Debug)]
pub struct Tuple {
    pub elements: Vec<TupleElement<Ty>>,
    pub readonly: bool,
}

impl Tuple {
    pub fn iter(&self) -> impl Iterator<Item = &Ty> { self.elements.iter().map(|element| &element.ty) }
    pub fn len(&self) -> usize { self.elements.len() }
    pub fn is_empty(&self) -> bool { self.elements.is_empty() }
    pub fn get(&self, index: usize) -> Option<&Ty> { self.elements.get(index).map(|element| &element.ty) }
    pub fn first(&self) -> Option<&Ty> { self.get(0) }

    pub fn at(&self, index: usize) -> Ty {
        for (offset, element) in self.elements.iter().enumerate() {
            if element.rest {
                let ty = match &element.ty {
                    Ty::Array(ty) => (*ty.element).clone(),
                    Ty::Tuple(tuple) => tuple.at(index.saturating_sub(offset)),
                    _ => Ty::Unknown,
                };
                if offset + 1 == self.elements.len() { return ty }
                if ty.is_opaque() { return ty }
                return Ty::union(std::iter::once(ty).chain(self.elements[offset + 1..].iter().map(|element| {
                    if element.optional { element.ty.clone().or_undefined() } else { element.ty.clone() }
                })).collect());
            }
            if offset == index {
                return if element.optional { element.ty.clone().or_undefined() } else { element.ty.clone() };
            }
        }
        Ty::Unknown
    }

    pub fn map_types(self, mut map: impl FnMut(Ty) -> Ty) -> Self {
        Self { readonly: self.readonly, elements: self.elements.into_iter().map(|element| TupleElement { ty: map(element.ty), ..element }).collect() }
    }

    pub fn length(&self) -> Ty {
        if self.elements.iter().any(|element| element.rest) { return Ty::Prim("number") }
        let required = self.required_length();
        Ty::union((required..=self.len()).map(|length| Ty::Lit(length.to_string().into(), "number")).collect())
    }

    pub fn required_length(&self) -> usize {
        self.elements.iter().filter(|element| !element.optional && !element.rest).count()
    }
}

impl FromIterator<Ty> for Tuple {
    fn from_iter<I: IntoIterator<Item = Ty>>(types: I) -> Self {
        Self { elements: types.into_iter().map(|ty| TupleElement { ty, optional: false, rest: false }).collect(), readonly: false }
    }
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
    pub fn kind(&self) -> &'static str {
        match self {
            Ty::Unknown => "unresolved",
            Ty::IntrinsicUnknown => "unknown",
            Ty::Any => "any",
            Ty::Never => "never",
            Ty::Void => "void",
            Ty::Undefined => "undefined",
            Ty::Null => "null",
            Ty::Prim(name) | Ty::Lit(_, name) => name,
            Ty::Param(_) => "parameter",
            Ty::Union(_) => "union",
            Ty::Inter(_) => "intersection",
            Ty::Array(_) => "array",
            Ty::Tuple(_) => "tuple",
            Ty::Fn(_) => "function",
            _ => "object",
        }
    }

    pub fn array(element: Ty) -> Ty {
        Ty::Array(Array { element: Box::new(element), readonly: false })
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown | Ty::IntrinsicUnknown)
    }

    pub fn is_opaque(&self) -> bool {
        matches!(self, Ty::Unknown)
    }

    pub fn union(items: Vec<Ty>) -> Ty {
        let mut out: Vec<Ty> = Vec::new();
        let mut unknown = false;
        for t in items {
            match t {
                Ty::Never => {}
                Ty::Union(inner) => out.extend(inner),
                Ty::Any => return Ty::Any,
                Ty::IntrinsicUnknown => unknown = true,
                t => out.push(t),
            }
        }
        if unknown { return Ty::IntrinsicUnknown }
        match out.len() {
            0 => Ty::Never,
            1 => out.pop().expect("one"),
            _ => Ty::Union(out),
        }
    }

    /// Without `null` and `undefined` (`x!`).
    pub fn may_be_nullish(&self) -> bool {
        match self {
            Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) | Ty::Null | Ty::Undefined | Ty::Void => true,
            Ty::Union(parts) => parts.iter().any(Ty::may_be_nullish),
            Ty::Inter(parts) => parts.iter().all(Ty::may_be_nullish),
            _ => false,
        }
    }

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
            Ty::IntrinsicUnknown => Ty::IntrinsicUnknown,
            t => Ty::union(vec![t, Ty::Undefined]),
        }
    }

    /// Literal types widened to their primitives (`let x = 0`).
    pub fn widen(self) -> Ty {
        match self {
            Ty::Lit(_, p) => Ty::Prim(p),
            Ty::Union(items) => Ty::union(items.into_iter().map(Ty::widen).collect()),
            Ty::Struct(props) => Ty::Struct(props.iter().map(|(n, t)| (n.clone(), t.clone().widen())).collect()),
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
    /// Nested structural comparisons in progress (see `relate`).
    structural_depth: Cell<u32>,
    values: RefCell<HashMap<DeclRef, Ty>>,
}

const MAX_DEPTH: u32 = 64;

impl<'p> Checker<'p> {
    pub fn new(project: &'p Project, file: FileId, summary: Summary) -> Self {
        let globals = project.globals(file);
        Checker { project, file, summary, globals, depth: Cell::new(0), structural_depth: Cell::new(0), values: RefCell::new(HashMap::new()) }
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
        // A script's top level is the global scope: its names merge with
        // every other file's below.
        if s.is_module || !matches!(env.module, ModuleAt::Top) {
            let (_, scope) = env.module.get(&s);
            let found = scope.get(name);
            if !found.is_empty() {
                return refs(found);
            }
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
                            let found = self.module_member(m, name);
                            // A default import of a module without one is
                            // the module itself (interop).
                            if found.is_empty() && &**name == "default" {
                                out.push(r);
                            }
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
    pub(crate) fn namespace_member(&self, sym: &[DeclRef], name: &str) -> Vec<DeclRef> {
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
        // `export = x`: its default import is `x` itself (interop), other
        // names are members of the namespace merged with it.
        let eq = self.project.export(m, "=");
        if eq.is_empty() {
            return Vec::new();
        }
        if name == "default" {
            return eq;
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
                "unknown" => Ty::IntrinsicUnknown,
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
                let evaluated: Vec<Ty> = args.iter().map(|a| self.eval(a, env)).collect();
                self.named_type(path, evaluated, env)
            }
            TypeSyn::Union(items) => Ty::union(items.iter().map(|t| self.eval(t, env)).collect()),
            TypeSyn::Inter(items) => {
                let items: Vec<Ty> = items.iter().map(|t| self.eval(t, env)).collect();
                if items.iter().any(|t| matches!(t, Ty::Never)) {
                    Ty::Never
                } else if items.iter().any(|t| matches!(t, Ty::Any)) {
                    Ty::Any
                } else {
                    let mut items: Vec<_> = items.into_iter().filter(|t| !matches!(t, Ty::IntrinsicUnknown)).collect();
                    match items.len() {
                        0 => Ty::IntrinsicUnknown,
                        1 => items.pop().expect("one"),
                        _ => Ty::Inter(items),
                    }
                }
            }
            TypeSyn::Array(t) => Ty::array(self.eval(t, env)),
            TypeSyn::Tuple(items) => Ty::Tuple(Tuple {
                elements: items.iter().map(|element| TupleElement { ty: self.eval(&element.ty, env), optional: element.optional, rest: element.rest }).collect(),
                readonly: false,
            }),
            TypeSyn::Readonly(inner) => match self.eval(inner, env) {
                Ty::Array(mut array) => { array.readonly = true; Ty::Array(array) },
                Ty::Tuple(mut tuple) => { tuple.readonly = true; Ty::Tuple(tuple) },
                _ => Ty::Unknown,
            },
            TypeSyn::Indexed(object, index) => {
                let object = self.eval(object, env);
                self.indexed_type(&object, &self.eval(index, env))
            },
            TypeSyn::Conditional { check, extends, consequent, alternate } => {
                let checked = self.eval(check, env);
                if let TypeSyn::Ref(path, args) = &**check {
                    if path.len() == 1 && args.is_empty() {
                        if let Some(index) = env.tparams.iter().rposition(|(name, _)| name == &path[0]) {
                            match &checked {
                                Ty::Never => return Ty::Never,
                                Ty::Union(parts) => return Ty::union(parts.iter().map(|part| {
                                    let mut env = env.clone();
                                    env.tparams[index].1 = part.clone();
                                    self.eval(syn, &env)
                                }).collect()),
                                _ => {}
                            }
                        }
                    }
                }
                let bound = self.eval(extends, env);
                if matches!(checked, Ty::Any) && !matches!(bound, Ty::Any | Ty::IntrinsicUnknown) {
                    return Ty::union(vec![self.eval(consequent, env), self.eval(alternate, env)]);
                }
                match self.assignable(&checked, &bound) {
                    Some(true) => self.eval(consequent, env),
                    Some(false) => self.eval(alternate, env),
                    None => Ty::Unknown,
                }
            },
            TypeSyn::Fn(sig) => Ty::Fn(Arc::new(Callable { sigs: vec![sig.clone()], env: Arc::new(env.clone()), ret: None })),
            TypeSyn::Obj(members) => Ty::Obj(members.clone(), Arc::new(env.clone())),
            TypeSyn::Query(path) => self.value_path(path, env),
            TypeSyn::Keyof(t) => self.keyof(&self.eval(t, env)),
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
        if let Some(ty) = self.utility_type(&types, &args, env) {
            return ty;
        }
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
        if path.len() == 1 && matches!(&*path[0], "Array" | "ReadonlyArray") && args.len() == 1 && self.is_builtin_symbol(&types, &path[0]) {
            return Ty::Array(Array { element: Box::new(args.into_iter().next().expect("one")), readonly: &*path[0] == "ReadonlyArray" });
        }
        Ty::Named(types.into(), args)
    }

    /// `keyof t` for resolved object properties; unions keep common names.
    /// Index signatures and unresolved generic keys stay unknown.
    fn keyof(&self, t: &Ty) -> Ty {
        let Some(names) = self.known_property_names(t, 0) else { return Ty::Unknown };
        Ty::union(
            names
                .into_iter()
                .map(|n| if n.starts_with('[') { Ty::Prim("symbol") } else { Ty::Lit(format!("\"{n}\"").into(), "string") })
                .collect(),
        )
    }

    /// Type parameters bound to arguments, their defaults, or unknown.
    pub fn bind_tparams(&self, params: &[TParam], args: Vec<Ty>, env: &Env) -> Vec<(Box<str>, Ty)> {
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
                // Overloads merged across files list the later file's first
                // (`setTimeout` from @types/node before lib.dom's).
                let mut files: Vec<FileId> = Vec::new();
                for r in all {
                    if !files.contains(&r.file) {
                        files.push(r.file);
                    }
                }
                let sigs: Vec<Arc<Sig>> = files
                    .iter()
                    .rev()
                    .flat_map(|&f| all.iter().filter(move |r| r.file == f))
                    .filter_map(|&r| {
                        let s = self.summary_of(r.file)?;
                        let d = &s.decls[r.decl as usize];
                        (d.kind == DeclKind::Function).then(|| d.sigs.clone())
                    })
                    .flatten()
                    .collect();
                let f = Ty::Fn(Arc::new(Callable { sigs, env: Arc::new(env), ret: None }));
                // A function merged with a namespace has its members too.
                let namespaces: Vec<DeclRef> = all.iter().copied().filter(|&r| self.kind_of(r) == Some(DeclKind::Namespace)).collect();
                if namespaces.is_empty() {
                    f
                } else {
                    Ty::Inter(vec![f, Ty::Ns(namespaces.into())])
                }
            }
            DeclKind::Class => Ty::Class(all.iter().copied().filter(|&r| self.kind_of(r) == Some(DeclKind::Class)).collect()),
            DeclKind::Enum | DeclKind::Namespace => Ty::Ns(all.to_vec().into()),
            DeclKind::Import { from, name } if &**name == "*" || &**name == "default" => match self.project.resolve(first.file, from) {
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
            Init::Literal(t) => self.eval(t, env),
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
}

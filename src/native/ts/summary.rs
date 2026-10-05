//! What one file declares, kept small: names, kinds, type annotations as
//! compact type syntax, members, generics, docs and exports. Other files'
//! summaries are all the type evaluator ever reads of them; syntax trees
//! are dropped as soon as a summary is built.
//!
//! [`FileSummary::fingerprint`] identifies what other files can see of a
//! file: the `.zlintcache` keeps a file's cached result while every summary
//! it read keeps its fingerprint.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::docs::leading_doc;
use super::kinds::{Field as F, Kind as K};
use super::view::View;

/// A type as written, before names are resolved.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum TypeSyn {
    /// `any`, `never`, `string`, `this`, ... (`boolean` for predicates).
    Kw(Box<str>),
    /// A literal type, as written (`"a"`, `1`, `true`).
    Lit(Box<str>),
    /// `A.B<C>`
    Ref(Box<[Box<str>]>, Vec<TypeSyn>),
    Union(Vec<TypeSyn>),
    Inter(Vec<TypeSyn>),
    Array(Box<TypeSyn>),
    Tuple(Vec<TupleElement>),
    Readonly(Box<TypeSyn>),
    Fn(Arc<Sig>),
    /// `new (...) => T`
    Ctor(Arc<Sig>),
    Obj(Arc<Vec<Member>>),
    /// `typeof a.b`
    Query(Box<[Box<str>]>),
    /// `keyof T`
    Keyof(Box<TypeSyn>),
    Indexed(Box<TypeSyn>, Box<TypeSyn>),
    Conditional { check: Box<TypeSyn>, extends: Box<TypeSyn>, consequent: Box<TypeSyn>, alternate: Box<TypeSyn> },
    /// Anything the evaluator does not model.
    Other,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct TupleElement<T = TypeSyn> {
    pub ty: T,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct TParam {
    pub name: Box<str>,
    pub constraint: Option<TypeSyn>,
    pub default: Option<TypeSyn>,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct Param {
    pub name: Box<str>,
    pub ty: Option<TypeSyn>,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sig {
    pub type_params: Vec<TParam>,
    pub params: Vec<Param>,
    /// `None`: not annotated.
    pub ret: Option<TypeSyn>,
    pub predicate: Option<Predicate>,
    pub is_async: bool,
    /// Where the declaring function starts, when it has a body: its file
    /// can type unannotated returns from it.
    pub body_at: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct Predicate {
    /// Runtime argument index; `None` denotes the receiver (`this`).
    pub parameter: Option<usize>,
    pub asserts: bool,
    /// Absent on truthiness assertions (`asserts value`).
    pub ty: Option<TypeSyn>,
}

/// Positions only mean something in the declaring file, which has its own
/// content hash: other files only see whether there is a body.
impl Hash for Sig {
    fn hash<H: Hasher>(&self, h: &mut H) {
        (&self.type_params, &self.params, &self.ret, &self.predicate, self.is_async, self.body_at.is_some()).hash(h);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberKind {
    Prop,
    Method,
    /// `()` call signatures and `new()` construct signatures of interfaces.
    Call,
    Construct,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

impl Visibility {
    fn of(v: &impl View, id: u32) -> Self {
        if v.has_word(id, F::Modifier, "private") { Self::Private }
        else if v.has_word(id, F::Modifier, "protected") { Self::Protected }
        else { Self::Public }
    }
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct Member {
    /// The key as written (`name`, `"str"`, `[Symbol.dispose]`).
    pub name: Box<str>,
    pub kind: MemberKind,
    pub visibility: Visibility,
    /// A type signature rather than an executable class/object member.
    pub signature: bool,
    /// A reference path used as a computed key; absent on ordinary/literal keys.
    pub computed_key: Option<Box<[Box<str>]>>,
    pub is_static: bool,
    pub optional: bool,
    /// Property type (or a getter's return type).
    pub ty: Option<TypeSyn>,
    /// Parameter type of an index signature; absent on named members.
    pub index_key: Option<TypeSyn>,
    pub sigs: Vec<Arc<Sig>>,
    /// An unannotated property's initializer.
    pub init: Option<Init>,
}

/// An initializer's shape, enough to type it from another file: `new`,
/// calls and member chains over names, annotations and literals.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Init {
    New(Box<[Box<str>]>, Vec<TypeSyn>),
    Call(Box<Init>, Vec<TypeSyn>),
    Member(Box<Init>, Box<str>),
    Await(Box<Init>),
    Path(Box<[Box<str>]>),
    /// A type the value widens to (`let x = 0`).
    Type(TypeSyn),
    /// A literal type kept as is (`const x = 0`).
    Literal(TypeSyn),
    Other,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum DeclKind {
    Class,
    Interface,
    Alias,
    Function,
    Var,
    Enum,
    Namespace,
    /// `import { name } from "from"` (`default`, or `*` for namespaces).
    Import { from: Box<str>, name: Box<str> },
    /// `import x = A.B`
    ImportAlias(Box<[Box<str>]>),
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub name: Box<str>,
    pub kind: DeclKind,
    pub type_params: Vec<TParam>,
    /// Class or interface inheritance, not class `implements`.
    pub extends: Vec<TypeSyn>,
    pub implements: Vec<TypeSyn>,
    pub members: Vec<Member>,
    /// Function overloads, or class constructors.
    pub sigs: Vec<Arc<Sig>>,
    /// Variable annotation or alias target.
    pub ty: Option<TypeSyn>,
    pub init: Option<Init>,
    pub doc: Option<Box<str>>,
    /// Namespace members: name -> decl indices.
    pub scope: Option<Scope>,
    /// Inside `declare` (or a declaration file).
    pub ambient: bool,
    /// Where it is declared (for local lookups).
    pub start: u32,
    /// The namespace it is declared in.
    pub parent: Option<u32>,
    /// The `declare module "x"` it is declared in.
    pub module: Option<usize>,
}

/// Everything but `start`, which only the declaring file reads.
impl Hash for Decl {
    fn hash<H: Hasher>(&self, h: &mut H) {
        (&self.name, &self.kind, &self.type_params, &self.extends, &self.members, &self.sigs, &self.ty, &self.init, &self.doc).hash(h);
        (&self.scope, self.ambient, self.parent, self.module).hash(h);
        self.implements.hash(h);
    }
}

#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub names: HashMap<Box<str>, Vec<u32>>,
}

/// Hashes a map in key order, so equal maps hash alike in every process.
fn hash_sorted<V: Hash, H: Hasher>(map: &HashMap<Box<str>, V>, h: &mut H) {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
    entries.hash(h);
}

impl Hash for Scope {
    fn hash<H: Hasher>(&self, h: &mut H) {
        hash_sorted(&self.names, h);
    }
}

impl Scope {
    pub fn get(&self, name: &str) -> &[u32] {
        self.names.get(name).map_or(&[], Vec::as_slice)
    }
}

#[derive(Debug, Clone, Hash)]
pub enum Export {
    /// Local declarations (several when merged).
    Decls(Vec<u32>),
    /// `export { name } from "from"`
    Reexport { from: Box<str>, name: Box<str> },
    /// `export * as name from "from"`
    Namespace(Box<str>),
}

#[derive(Debug, Default)]
pub struct Module {
    pub exports: HashMap<Box<str>, Export>,
    /// `export * from "x"`
    pub stars: Vec<Box<str>>,
    /// `export = name`
    pub equals: Option<Vec<u32>>,
}

impl Hash for Module {
    fn hash<H: Hasher>(&self, h: &mut H) {
        hash_sorted(&self.exports, h);
        (&self.stars, &self.equals).hash(h);
    }
}

#[derive(Debug, Default)]
pub struct FileSummary {
    pub decls: Vec<Decl>,
    /// Top-level names (imports included).
    pub top: Scope,
    pub module: Module,
    /// Declarations visible everywhere (script files, `declare global`).
    pub globals: Scope,
    /// `declare module "x" { ... }`
    pub ambient_modules: Vec<(Box<str>, Module, Scope)>,
    /// Imports or exports at the top level: a module, not a script.
    pub is_module: bool,
    /// `/// <reference types="x" />`, `lib="x"` and `path="x"`.
    pub ref_types: Vec<Box<str>>,
    pub ref_libs: Vec<Box<str>>,
    pub ref_paths: Vec<Box<str>>,
}

impl FileSummary {
    /// Identifies what other files can learn from this summary: equal
    /// fingerprints mean equal answers for them (positions aside).
    pub fn fingerprint(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&self.decls, &self.top, &self.module, &self.globals, &self.ambient_modules, self.is_module).hash(&mut h);
        (&self.ref_types, &self.ref_libs, &self.ref_paths).hash(&mut h);
        h.finish()
    }
}

// ---- building ------------------------------------------------------------------

pub fn path_of(v: &impl View, id: u32) -> Vec<Box<str>> {
    match v.kind(id) {
        Some(K::QualifiedName) => {
            let mut p = v.get(id, F::Left).map(|l| path_of(v, l)).unwrap_or_default();
            p.extend(v.get(id, F::Right).map(|r| v.text(r).into()));
            p
        }
        Some(K::Member) => {
            let mut p = v.get(id, F::Object).map(|o| path_of(v, o)).unwrap_or_default();
            if p.is_empty() {
                return p;
            }
            p.extend(v.get(id, F::Property).map(|r| v.text(r).into()));
            p
        }
        Some(K::Ident | K::This) => vec![v.text(id).into()],
        _ => Vec::new(),
    }
}

pub fn type_syn(v: &impl View, id: u32) -> TypeSyn {
    let all = |f| v.all(id, f).into_iter().map(|t| type_syn(v, t)).collect::<Vec<_>>();
    match v.kind(id) {
        Some(K::TypeKeyword) => TypeSyn::Kw(v.text(id).into()),
        Some(K::LiteralType) => TypeSyn::Lit(v.text(id).into()),
        Some(K::TypeRef) => {
            let path = v.get(id, F::Name).map(|n| path_of(v, n)).unwrap_or_default();
            TypeSyn::Ref(path.into(), all(F::TypeArg))
        }
        Some(K::Union) => TypeSyn::Union(all(F::Type)),
        Some(K::Intersection) => TypeSyn::Inter(all(F::Type)),
        Some(K::ArrayType) => TypeSyn::Array(Box::new(v.get(id, F::Type).map_or(TypeSyn::Other, |t| type_syn(v, t)))),
        Some(K::TupleType) => TypeSyn::Tuple(
            v.all(id, F::Element)
                .into_iter()
                .map(|e| TupleElement {
                    ty: v.get(e, F::Type).map_or(TypeSyn::Other, |t| type_syn(v, t)),
                    optional: v.get(e, F::Optional).is_some(),
                    rest: v.is(e, K::Rest),
                })
                .collect(),
        ),
        Some(K::FnType) => {
            let sig = Arc::new(sig(v, id));
            if v.get(id, F::Kind).is_some() {
                TypeSyn::Ctor(sig)
            } else {
                TypeSyn::Fn(sig)
            }
        }
        Some(K::TypeLit) => TypeSyn::Obj(Arc::new(v.all(id, F::Member).into_iter().filter_map(|m| member(v, m)).collect())),
        Some(K::TypeQuery) => TypeSyn::Query(v.get(id, F::Name).map(|n| path_of(v, n)).unwrap_or_default().into()),
        Some(K::ParenType) => v.get(id, F::Type).map_or(TypeSyn::Other, |t| type_syn(v, t)),
        Some(K::TypePredicate) => {
            if v.get(id, F::Modifier).is_some() {
                TypeSyn::Kw("void".into())
            } else {
                TypeSyn::Kw("boolean".into())
            }
        }
        Some(K::TypeOperator) if v.get(id, F::Op).is_some_and(|o| v.text(o) == "readonly") => {
            v.get(id, F::Type).map_or(TypeSyn::Other, |t| TypeSyn::Readonly(Box::new(type_syn(v, t))))
        }
        Some(K::TypeOperator) if v.get(id, F::Op).is_some_and(|o| v.text(o) == "keyof") => {
            v.get(id, F::Type).map_or(TypeSyn::Other, |t| TypeSyn::Keyof(Box::new(type_syn(v, t))))
        }
        Some(K::TemplateType) => TypeSyn::Kw("string".into()),
        Some(K::IndexedAccess) => TypeSyn::Indexed(
            Box::new(v.get(id, F::Object).map_or(TypeSyn::Other, |t| type_syn(v, t))),
            Box::new(v.get(id, F::Index).map_or(TypeSyn::Other, |t| type_syn(v, t))),
        ),
        Some(K::ConditionalType) => TypeSyn::Conditional {
            check: Box::new(v.get(id, F::Check).map_or(TypeSyn::Other, |t| type_syn(v, t))),
            extends: Box::new(v.get(id, F::Extends).map_or(TypeSyn::Other, |t| type_syn(v, t))),
            consequent: Box::new(v.get(id, F::Consequent).map_or(TypeSyn::Other, |t| type_syn(v, t))),
            alternate: Box::new(v.get(id, F::Alternate).map_or(TypeSyn::Other, |t| type_syn(v, t))),
        },
        _ => TypeSyn::Other,
    }
}

fn type_params(v: &impl View, id: u32) -> Vec<TParam> {
    v.all(id, F::TypeParam)
        .into_iter()
        .filter_map(|tp| {
            Some(TParam {
                name: v.text(v.get(tp, F::Name)?).into(),
                constraint: v.get(tp, F::Constraint).map(|c| type_syn(v, c)),
                default: v.get(tp, F::Default).map(|d| type_syn(v, d)),
            })
        })
        .collect()
}

pub fn sig(v: &impl View, id: u32) -> Sig {
    let params: Vec<_> = v
        .all(id, F::Param)
        .into_iter()
        .map(|p| {
            let name = v.get(p, F::Name);
            let rest = name.is_some_and(|n| v.is(n, K::Rest));
            Param {
                name: name.map_or("".into(), |n| v.text(n).trim_start_matches("...").into()),
                ty: v.get(p, F::Type).map(|t| type_syn(v, t)),
                optional: v.get(p, F::Optional).is_some() || v.get(p, F::Init).is_some(),
                rest,
            }
        })
        .collect();
    let predicate = v.get(id, F::ReturnType).filter(|&annotation| v.is(annotation, K::TypePredicate)).and_then(|annotation| {
        let name = super::statics::identifier_name(v.text(v.get(annotation, F::Name)?));
        let parameter = if name == "this" {
            None
        } else {
            Some(params.iter().filter(|param| &*param.name != "this").position(|param| super::statics::identifier_name(&param.name) == name)?)
        };
        Some(Predicate {
            parameter,
            asserts: v.has_word(annotation, F::Modifier, "asserts"),
            ty: v.get(annotation, F::Type).map(|ty| type_syn(v, ty)),
        })
    });
    Sig {
        type_params: type_params(v, id),
        params,
        ret: v.get(id, F::ReturnType).map(|r| type_syn(v, r)),
        predicate,
        is_async: v.has_word(id, F::Modifier, "async"),
        body_at: v.get(id, F::Body).map(|_| v.start(id)),
    }
}

fn key_name(v: &impl View, key: u32) -> Box<str> {
    let mut literal = match v.kind(key) {
        Some(K::Computed) => v.get(key, F::Expr).unwrap_or(key),
        _ => key,
    };
    while v.is(literal, K::Paren) {
        let Some(inner) = v.get(literal, F::Expr) else { break };
        literal = inner;
    }
    match v.kind(literal) {
        Some(K::String) => {
            if let Some(name) = super::regex::cook(v.text(literal)) { return name.into() }
        }
        Some(K::Template) if v.all(literal, F::Expr).is_empty() => {
            if let Some(name) = super::regex::cook(v.text(literal)) { return name.into() }
        }
        Some(K::Number) => {
            if let Some(number) = super::statics::number(v.text(literal)) { return super::statics::number_string(number).into() }
        }
        Some(K::Unary) => {
            if let (Some(op), Some(value)) = (v.get(literal, F::Op), v.get(literal, F::Arg).filter(|&arg| v.is(arg, K::Number))) {
                if let Some(number) = super::statics::number(v.text(value)) {
                    match v.text(op) {
                        "+" => return super::statics::number_string(number).into(),
                        "-" => return super::statics::number_string(-number).into(),
                        _ => {}
                    }
                }
            }
        }
        Some(K::Bool | K::Null) => return v.text(literal).into(),
        Some(K::Ident) if literal == key => return super::statics::identifier_name(v.text(key)).into(),
        _ => {}
    }
    match v.kind(key) {
        Some(K::String) => v.text(key).trim_matches(|c| c == '"' || c == '\'').into(),
        _ => v.text(key).into(),
    }
}

/// A class member or type member.
pub fn member(v: &impl View, id: u32) -> Option<Member> {
    let is_static = v.has_word(id, F::Modifier, "static");
    let optional = v.get(id, F::Optional).is_some();
    let visibility = Visibility::of(v, id);
    let signature = matches!(v.kind(id), Some(K::MethodSig | K::PropSig | K::CallSig | K::IndexSig));
    let computed_key = v.get(id, F::Key).filter(|&key| v.is(key, K::Computed))
        .and_then(|key| v.get(key, F::Expr)).map(|key| path_of(v, key))
        .filter(|path| !path.is_empty()).map(Vec::into_boxed_slice);
    let named = |kind, ty, sigs| -> Option<Member> {
        Some(Member { name: key_name(v, v.get(id, F::Key)?), kind, visibility, signature, computed_key: computed_key.clone(), is_static, optional, ty, index_key: None, sigs, init: None })
    };
    match v.kind(id)? {
        K::Method | K::MethodSig => {
            let s = Arc::new(sig(v, id));
            match v.get(id, F::Kind).map(|k| v.text(k)) {
                Some("get") => named(MemberKind::Prop, s.ret.clone(), Vec::new()),
                Some("set") => named(MemberKind::Prop, s.params.first().and_then(|p| p.ty.clone()), Vec::new()),
                _ => named(MemberKind::Method, None, vec![s]),
            }
        }
        K::ClassProp | K::PropSig => {
            // A property holding a function type is called like a method.
            let ty = v.get(id, F::Type).map(|t| type_syn(v, t));
            let mut m = named(MemberKind::Prop, ty.clone(), Vec::new())?;
            if ty.is_none() {
                m.init = v.get(id, F::Init).map(|i| init(v, i));
            }
            Some(m)
        }
        K::CallSig => {
            let kind = if v.get(id, F::Kind).is_some() { MemberKind::Construct } else { MemberKind::Call };
            Some(Member { name: "()".into(), kind, visibility, signature, computed_key: None, is_static, optional, ty: None, index_key: None, sigs: vec![Arc::new(sig(v, id))], init: None })
        }
        K::IndexSig => Some(Member {
            name: "[index]".into(),
            kind: MemberKind::Index,
            visibility,
            signature,
            computed_key: None,
            is_static,
            optional,
            ty: v.get(id, F::Type).map(|t| type_syn(v, t)),
            index_key: v.all(id, F::Param).first().and_then(|&p| v.get(p, F::Type)).map(|t| type_syn(v, t)),
            sigs: Vec::new(),
            init: None,
        }),
        _ => None,
    }
}

fn init(v: &impl View, id: u32) -> Init {
    let targs = |n| v.all(n, F::TypeArg).into_iter().map(|t| type_syn(v, t)).collect::<Vec<_>>();
    match v.kind(id) {
        Some(K::New) => v.get(id, F::Callee).map_or(Init::Other, |c| Init::New(path_of(v, c).into(), targs(id))),
        Some(K::Call) => v.get(id, F::Callee).map_or(Init::Other, |c| Init::Call(Box::new(init(v, c)), targs(id))),
        Some(K::Ident | K::This) => Init::Path(path_of(v, id).into()),
        Some(K::Member) => {
            let path = path_of(v, id);
            if !path.is_empty() {
                return Init::Path(path.into());
            }
            match (v.get(id, F::Object), v.get(id, F::Property)) {
                (Some(o), Some(p)) => Init::Member(Box::new(init(v, o)), v.text(p).into()),
                _ => Init::Other,
            }
        }
        Some(K::Paren | K::NonNull | K::Satisfies) => v.get(id, F::Expr).map_or(Init::Other, |e| init(v, e)),
        Some(K::Await) => v.get(id, F::Arg).map_or(Init::Other, |e| Init::Await(Box::new(init(v, e)))),
        Some(K::As | K::TypeAssertion) => {
            v.get(id, F::Type).map_or(Init::Other, |t| Init::Type(type_syn(v, t)))
        }
        Some(K::String | K::Template) => Init::Type(TypeSyn::Kw("string".into())),
        Some(K::Number) => Init::Type(TypeSyn::Kw("number".into())),
        Some(K::Bool) => Init::Type(TypeSyn::Kw("boolean".into())),
        Some(K::Arrow | K::Function) => Init::Type(TypeSyn::Fn(Arc::new(sig(v, id)))),
        _ => Init::Other,
    }
}

struct Builder<'v, V: View> {
    v: &'v V,
    out: FileSummary,
}

/// `dts`: a declaration file, where everything is ambient.
pub fn summarise(v: &impl View, root: u32, dts: bool) -> FileSummary {
    let mut b = Builder { v, out: FileSummary::default() };
    for line in v.src().lines().filter(|l| l.trim_start().starts_with("/// <reference")) {
        for attr in ["types", "lib", "path"] {
            if let Some(rest) = line.split_once(&format!("{attr}=\"")).map(|(_, r)| r) {
                if let Some(name) = rest.split('"').next() {
                    match attr {
                        "types" => b.out.ref_types.push(name.into()),
                        "lib" => b.out.ref_libs.push(name.into()),
                        _ => b.out.ref_paths.push(name.into()),
                    }
                }
            }
        }
    }
    let stmts = v.all(root, F::Body);
    b.out.is_module = stmts.iter().any(|&s| matches!(v.kind(s), Some(K::Import | K::Export | K::ExportAssign | K::ImportEquals)) && {
        // `import x = A.B` alone does not make a module.
        !v.is(s, K::ImportEquals) || v.get(s, F::Value).is_some_and(|x| v.is(x, K::Call))
    });
    let mut top = Scope::default();
    let mut module = Module::default();
    for s in stmts {
        b.statement(s, &mut top, Some(&mut module), dts);
    }
    if !b.out.is_module {
        for (name, ids) in &top.names {
            b.out.globals.names.entry(name.clone()).or_default().extend(ids);
        }
    }
    b.out.top = top;
    b.out.module = module;
    b.out
}

impl<V: View> Builder<'_, V> {
    fn push(&mut self, scope: &mut Scope, decl: Decl) -> u32 {
        let id = self.out.decls.len() as u32;
        scope.names.entry(decl.name.clone()).or_default().push(id);
        self.out.decls.push(decl);
        id
    }

    fn base(&self, id: u32, name: &str, kind: DeclKind, ambient: bool) -> Decl {
        Decl {
            name: name.into(),
            kind,
            type_params: type_params(self.v, id),
            extends: Vec::new(),
            implements: Vec::new(),
            members: Vec::new(),
            sigs: Vec::new(),
            ty: None,
            init: None,
            doc: leading_doc(self.v.src(), self.v.start(id) as usize).map(String::into_boxed_str),
            scope: None,
            ambient,
            start: self.v.start(id),
            parent: None,
            module: None,
        }
    }

    /// Declares what `s` declares into `scope`; returns the declared ids.
    fn statement(&mut self, s: u32, scope: &mut Scope, mut module: Option<&mut Module>, ambient: bool) -> Vec<u32> {
        let v = self.v;
        let ambient = ambient || v.has_word(s, F::Modifier, "declare");
        let mut ids = Vec::new();
        match v.kind(s) {
            Some(K::Var) => {
                for d in v.all(s, F::Decl) {
                    let Some(n) = v.get(d, F::Name).filter(|&n| v.is(n, K::Ident)) else { continue };
                    let mut decl = self.base(s, v.text(n), DeclKind::Var, ambient);
                    decl.type_params.clear();
                    decl.ty = v.get(d, F::Type).map(|t| type_syn(v, t));
                    decl.init = v.get(d, F::Init).map(|i| init(v, i));
                    // `const x = "a"` keeps its literal type.
                    let literal = v.get(d, F::Init).filter(|&i| matches!(v.kind(i), Some(K::String | K::Number | K::Bool)));
                    if let (Some(i), true) = (literal, v.get(s, F::Kind).is_some_and(|k| v.text(k) == "const")) {
                        decl.init = Some(Init::Literal(TypeSyn::Lit(v.text(i).into())));
                    }
                    ids.push(self.push(scope, decl));
                }
            }
            Some(K::Function) => {
                let Some(n) = v.get(s, F::Name) else { return ids };
                let name = v.text(n);
                let sig = Arc::new(sig(v, s));
                // Overloads share one declaration.
                if let Some(&prev) = scope.get(name).iter().find(|&&i| self.out.decls[i as usize].kind == DeclKind::Function) {
                    let d = &mut self.out.decls[prev as usize];
                    d.sigs.push(sig);
                    if d.doc.is_none() {
                        d.doc = leading_doc(v.src(), v.start(s) as usize).map(String::into_boxed_str);
                    }
                    ids.push(prev);
                } else {
                    let mut decl = self.base(s, name, DeclKind::Function, ambient);
                    decl.type_params.clear();
                    decl.sigs.push(sig);
                    ids.push(self.push(scope, decl));
                }
            }
            Some(K::Class) => {
                let name = v.get(s, F::Name).map_or("default", |n| v.text(n));
                let mut decl = self.base(s, name, DeclKind::Class, ambient);
                if let Some(e) = v.get(s, F::Extends) {
                    let args = v.all(s, F::TypeArg).into_iter().map(|t| type_syn(v, t)).collect();
                    decl.extends.push(TypeSyn::Ref(path_of(v, e).into(), args));
                }
                decl.implements.extend(v.all(s, F::Implements).into_iter().map(|t| type_syn(v, t)));
                for m in v.all(s, F::Member) {
                    if v.is(m, K::Method) && v.get(m, F::Key).is_some_and(|k| v.text(k) == "constructor") {
                        decl.sigs.push(Arc::new(sig(v, m)));
                        // Parameter properties: `constructor(private x: T)`.
                        for p in v.all(m, F::Param) {
                            if v.get(p, F::Modifier).is_some() {
                                if let Some(n) = v.get(p, F::Name).filter(|&n| v.is(n, K::Ident)) {
                                    decl.members.push(Member {
                                        name: v.text(n).into(),
                                        kind: MemberKind::Prop,
                                        visibility: Visibility::of(v, p),
                                        signature: false,
                                        computed_key: None,
                                        is_static: false,
                                        optional: false,
                                        ty: v.get(p, F::Type).map(|t| type_syn(v, t)),
                                        index_key: None,
                                        sigs: Vec::new(),
                                        init: None,
                                    });
                                }
                            }
                        }
                    } else if let Some(mem) = member(v, m) {
                        decl.members.push(mem);
                    }
                }
                ids.push(self.push(scope, decl));
            }
            Some(K::Interface) => {
                let Some(n) = v.get(s, F::Name) else { return ids };
                let mut decl = self.base(s, v.text(n), DeclKind::Interface, ambient);
                decl.extends = v.all(s, F::Extends).into_iter().map(|t| type_syn(v, t)).collect();
                decl.members = v.all(s, F::Member).into_iter().filter_map(|m| member(v, m)).collect();
                ids.push(self.push(scope, decl));
            }
            Some(K::TypeAlias) => {
                let Some(n) = v.get(s, F::Name) else { return ids };
                let mut decl = self.base(s, v.text(n), DeclKind::Alias, ambient);
                decl.ty = v.get(s, F::Type).map(|t| type_syn(v, t));
                ids.push(self.push(scope, decl));
            }
            Some(K::Enum) => {
                let Some(n) = v.get(s, F::Name) else { return ids };
                let mut decl = self.base(s, v.text(n), DeclKind::Enum, ambient);
                decl.members = v
                    .all(s, F::Member)
                    .into_iter()
                    .filter_map(|m| {
                        let name = v.get(m, F::Name)?;
                        // String members make a string enum.
                        let ty = v.get(m, F::Init).filter(|&i| v.is(i, K::String)).map(|i| TypeSyn::Lit(v.text(i).into()));
                        Some(Member {
                            name: key_name(v, name),
                            kind: MemberKind::Prop,
                            visibility: Visibility::Public,
                            signature: false,
                            computed_key: None,
                            is_static: true,
                            optional: false,
                            ty,
                            index_key: None,
                            sigs: Vec::new(),
                            init: None,
                        })
                    })
                    .collect();
                ids.push(self.push(scope, decl));
            }
            Some(K::Namespace) => {
                let kind = v.get(s, F::Kind).map(|k| v.text(k)).unwrap_or("namespace");
                let name_node = v.get(s, F::Name);
                let body = v.get(s, F::Body);
                if kind == "global" {
                    if let Some(b) = body {
                        let mut g = std::mem::take(&mut self.out.globals);
                        for st in v.all(b, F::Body) {
                            self.statement(st, &mut g, None, true);
                        }
                        self.out.globals = g;
                    }
                    return ids;
                }
                if let Some(n) = name_node.filter(|&n| v.is(n, K::String)) {
                    // `declare module "x" { ... }`
                    let spec: Box<str> = v.text(n).trim_matches(|c| c == '"' || c == '\'').into();
                    let mut inner = Scope::default();
                    let mut m = Module::default();
                    let first = self.out.decls.len();
                    if let Some(b) = body {
                        for st in v.all(b, F::Body) {
                            let declared = self.statement(st, &mut inner, Some(&mut m), true);
                            // Ambient module bodies export everything.
                            for id in declared {
                                let name = self.out.decls[id as usize].name.clone();
                                if let Export::Decls(list) = m.exports.entry(name).or_insert_with(|| Export::Decls(Vec::new())) {
                                    if !list.contains(&id) {
                                        list.push(id);
                                    }
                                }
                            }
                        }
                    }
                    let index = self.out.ambient_modules.len();
                    for d in &mut self.out.decls[first..] {
                        d.module.get_or_insert(index);
                    }
                    self.out.ambient_modules.push((spec, m, inner));
                    return ids;
                }
                let Some(n) = name_node else { return ids };
                // `namespace A.B { }` declares A containing B.
                let path = path_of(v, n);
                let mut inner = Scope::default();
                let first = self.out.decls.len();
                if let Some(b) = body {
                    for st in v.all(b, F::Body) {
                        self.statement(st, &mut inner, None, ambient);
                    }
                }
                let members_end = self.out.decls.len();
                // Members whose namespace is the innermost one.
                let mut children: Vec<u32> = (first as u32..members_end as u32).filter(|&i| self.out.decls[i as usize].parent.is_none()).collect();
                for (depth, name) in path.iter().enumerate().rev() {
                    let mut decl = self.base(s, name, DeclKind::Namespace, ambient);
                    decl.type_params.clear();
                    decl.scope = Some(inner);
                    // Namespaces merge.
                    let merged = if depth == 0 {
                        scope.get(name).iter().copied().find(|&i| self.out.decls[i as usize].kind == DeclKind::Namespace)
                    } else {
                        None
                    };
                    let id = match merged {
                        Some(prev) => {
                            let add = decl.scope.take().unwrap_or_default();
                            if let Some(sc) = self.out.decls[prev as usize].scope.as_mut() {
                                for (k, list) in add.names {
                                    sc.names.entry(k).or_default().extend(list);
                                }
                            }
                            prev
                        }
                        None if depth == 0 => self.push(scope, decl),
                        None => {
                            let id = self.out.decls.len() as u32;
                            self.out.decls.push(decl);
                            id
                        }
                    };
                    for c in children {
                        self.out.decls[c as usize].parent = Some(id);
                    }
                    children = vec![id];
                    if depth == 0 {
                        ids.push(id);
                        break;
                    }
                    let mut outer = Scope::default();
                    outer.names.entry(name.clone()).or_default().push(id);
                    inner = outer;
                }
            }
            Some(K::Import) => {
                let from: Box<str> = v.get(s, F::From).map_or("".into(), |f| v.text(f).trim_matches(|c| c == '"' || c == '\'').into());
                let mut add = |b: &mut Self, local: u32, name: &str| {
                    let mut decl = b.base(s, v.text(local), DeclKind::Import { from: from.clone(), name: name.into() }, ambient);
                    decl.type_params.clear();
                    decl.doc = None;
                    b.push(scope, decl);
                };
                if let Some(d) = v.get(s, F::DefaultImport) {
                    add(self, d, "default");
                }
                if let Some(n) = v.get(s, F::Namespace) {
                    add(self, n, "*");
                }
                for spec in v.all(s, F::Specifier) {
                    let Some(imported) = v.get(spec, F::Imported) else { continue };
                    let local = v.get(spec, F::Local).unwrap_or(imported);
                    let name = key_name(v, imported);
                    add(self, local, &name);
                }
            }
            Some(K::ImportEquals) => {
                let Some(n) = v.get(s, F::Name) else { return ids };
                let value = v.get(s, F::Value);
                let kind = match value {
                    Some(c) if v.is(c, K::Call) => {
                        let arg = v.all(c, F::Arg).first().map(|&a| v.text(a).trim_matches(|c| c == '"' || c == '\'').into());
                        DeclKind::Import { from: arg.unwrap_or_else(|| "".into()), name: "*".into() }
                    }
                    Some(p) => DeclKind::ImportAlias(path_of(v, p).into()),
                    None => return ids,
                };
                let mut decl = self.base(s, v.text(n), kind, ambient);
                decl.type_params.clear();
                ids.push(self.push(scope, decl));
            }
            Some(K::Export) => {
                let doc = leading_doc(v.src(), v.start(s) as usize).map(String::into_boxed_str);
                let default = v.get(s, F::Kind).is_some_and(|k| v.text(k) == "default");
                let from = v.get(s, F::From).map(|f| -> Box<str> { v.text(f).trim_matches(|c| c == '"' || c == '\'').into() });
                if let Some(decl) = v.get(s, F::Decl) {
                    let declared = self.statement(decl, scope, None, ambient);
                    for &id in &declared {
                        if self.out.decls[id as usize].doc.is_none() {
                            self.out.decls[id as usize].doc = doc.clone();
                        }
                        let name = self.out.decls[id as usize].name.clone();
                        if let Some(m) = module.as_deref_mut() {
                            add_export(m, &name, id);
                            if default {
                                add_export(m, "default", id);
                            }
                        }
                    }
                    ids.extend(declared);
                } else if let Some(e) = v.get(s, F::Expr) {
                    if let Some(m) = module.as_deref_mut() {
                        if v.is(e, K::Ident) {
                            let local: Vec<u32> = scope.get(v.text(e)).to_vec();
                            // Declared later: resolved below by name.
                            m.exports.insert("default".into(), Export::Reexport { from: "".into(), name: v.text(e).into() });
                            if !local.is_empty() {
                                m.exports.insert("default".into(), Export::Decls(local));
                            }
                        } else {
                            let mut decl = self.base(s, "default", DeclKind::Var, ambient);
                            decl.type_params.clear();
                            decl.init = Some(init(v, e));
                            let id = self.out.decls.len() as u32;
                            self.out.decls.push(decl);
                            m.exports.insert("default".into(), Export::Decls(vec![id]));
                        }
                    }
                } else if let Some(m) = module.as_deref_mut() {
                    let specs = v.all(s, F::Specifier);
                    match &from {
                        Some(from) if specs.is_empty() => match v.get(s, F::Namespace) {
                            Some(ns) => {
                                m.exports.insert(key_name(v, ns), Export::Namespace(from.clone()));
                            }
                            None => m.stars.push(from.clone()),
                        },
                        _ => {}
                    }
                    for spec in specs {
                        let Some(local) = v.get(spec, F::Local) else { continue };
                        let exported = key_name(v, v.get(spec, F::Exported).unwrap_or(local));
                        let local = key_name(v, local);
                        // Local names are resolved once every declaration is known.
                        let from = from.clone().unwrap_or_else(|| "".into());
                        m.exports.insert(exported, Export::Reexport { from, name: local });
                    }
                }
            }
            Some(K::ExportAssign) => {
                if let (Some(m), Some(e)) = (module, v.get(s, F::Expr)) {
                    let path = path_of(v, e);
                    if let Some(first) = path.first() {
                        m.equals = Some(scope.get(first).to_vec());
                        if m.equals.as_ref().is_some_and(Vec::is_empty) {
                            m.exports.insert("=".into(), Export::Reexport { from: "".into(), name: first.clone() });
                        }
                    }
                }
            }
            _ => {}
        }
        ids
    }
}

fn add_export(m: &mut Module, name: &str, id: u32) {
    match m.exports.entry(name.into()).or_insert_with(|| Export::Decls(Vec::new())) {
        Export::Decls(list) => list.push(id),
        other => *other = Export::Decls(vec![id]),
    }
}

impl FileSummary {
    /// The declarations behind export `name` that are local to this file
    /// (`export { a as b }` and late `export default a` resolved by name).
    pub fn local_export<'a>(&'a self, module: &'a Module, scope: &'a Scope, name: &str) -> Option<LocalExport<'a>> {
        match module.exports.get(name) {
            Some(Export::Decls(ids)) => Some(LocalExport::Decls(ids.clone())),
            Some(Export::Reexport { from, name: inner }) if from.is_empty() => {
                let ids = scope.get(inner);
                if ids.is_empty() {
                    None
                } else {
                    Some(LocalExport::Decls(ids.to_vec()))
                }
            }
            Some(Export::Reexport { from, name: inner }) => Some(LocalExport::Reexport(from, inner)),
            Some(Export::Namespace(from)) => Some(LocalExport::Namespace(from)),
            None => None,
        }
    }
}

pub enum LocalExport<'a> {
    Decls(Vec<u32>),
    Reexport(&'a str, &'a str),
    Namespace(&'a str),
}

/// Shared, immutable summaries.
pub type Summary = Arc<FileSummary>;

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::view::Raw;

    fn fingerprint(src: &str) -> u64 {
        let parsed = super::super::parse(src, false).unwrap();
        summarise(&Raw { src, nodes: &parsed.nodes }, parsed.root, false).fingerprint()
    }

    #[test]
    fn member_visibility_changes_invalidate_dependents() {
        for (before, after) in [
            ("class C { private value: number; }", "class C { protected value: number; }"),
            ("class C { protected value: number; }", "class C { public value: number; }"),
            ("class C { private method(): void; }", "class C { public method(): void; }"),
            ("class C { constructor(private value: number) {} }", "class C { constructor(public value: number) {} }"),
        ] {
            assert_ne!(fingerprint(before), fingerprint(after), "{before} -> {after}");
        }
    }

    #[test]
    fn computed_reference_keys_do_not_share_literal_key_identity() {
        assert_ne!(
            fingerprint("interface T { '[Symbol.toPrimitive]'(): string; }"),
            fingerprint("interface T { [Symbol.toPrimitive](): string; }"),
        );
    }

    #[test]
    fn imported_type_modifiers_invalidate_dependents() {
        for (before, after) in [
            ("type T = number[];", "type T = readonly number[];"),
            ("type T = [number];", "type T = [number?];"),
            ("type T = [number[]];", "type T = [...number[]];"),
        ] {
            assert_ne!(fingerprint(before), fingerprint(after), "{before} -> {after}");
        }
    }

    #[test]
    fn deferred_type_changes_invalidate_dependents() {
        for (before, after) in [
            ("type T = O['a'];", "type T = O['b'];"),
            ("type T<X> = X extends string ? number : boolean;", "type T<X> = X extends string ? number : string;"),
        ] {
            assert_ne!(fingerprint(before), fingerprint(after), "{before} -> {after}");
        }
    }

    #[test]
    fn predicate_contract_changes_invalidate_dependents() {
        for (before, after) in [
            ("declare function guard(x: unknown): x is string;", "declare function guard(x: unknown): x is number;"),
            ("declare function guard(x: unknown, y: unknown): x is string;", "declare function guard(x: unknown, y: unknown): y is string;"),
            ("declare function guard(x: unknown): asserts x;", "declare function guard(x: unknown): asserts x is string;"),
        ] {
            assert_ne!(fingerprint(before), fingerprint(after), "{before} -> {after}");
        }
    }

    #[test]
    fn changing_class_inheritance_invalidates_dependents() {
        assert_ne!(fingerprint("class C extends B {}"), fingerprint("class C implements B {}"));
    }

    #[test]
    fn equivalent_literal_keys_share_member_identity() {
        assert_eq!(fingerprint("interface T { 'a': number }"), fingerprint("interface T { ['\\u0061']: number }"));
        assert_eq!(fingerprint("interface T { 16: number }"), fingerprint("interface T { [0x10]: number }"));
    }
}

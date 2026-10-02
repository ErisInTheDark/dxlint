//! Scopes and name resolution for one file. Values and types live in
//! separate namespaces (TypeScript's declaration spaces), so `const x` does
//! not hide an imported type `x`. Every declaring identifier gets use
//! counts, split by whether the use needs the runtime value: `typeof x` and
//! computed keys inside types do not.

use std::collections::HashMap;

use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;

const VALUE: u8 = 1;
const TYPE: u8 = 2;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Binding {
    /// What declares it: `var`, `let`, `const`, `using`, `function`,
    /// `class`, `param`, `catch`, `import`, `type`, `interface`, `enum`,
    /// `namespace`, `type_param`, or `function_name`/`class_name` for the
    /// names of function and class expressions.
    pub kind: &'static str,
    /// Declared by an `export`ed statement.
    pub exported: bool,
    /// Inside `declare` (or a `declare`d namespace): no runtime code.
    pub ambient: bool,
    /// References of any kind (reads, type uses, `typeof`), writes aside.
    pub uses: u32,
    /// References that need the runtime value.
    pub value_uses: u32,
    /// Uses from inside the declaration itself (a function calling itself).
    pub self_uses: u32,
    pub writes: u32,
}

#[derive(Default)]
pub struct Bindings {
    /// Declaring identifier -> its uses.
    pub decls: HashMap<u32, Binding>,
    /// Referencing identifier -> the identifier that declares it.
    pub refs: HashMap<u32, u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    Value,
    Type,
    /// A value read that only matters to the type checker.
    TypeOnly,
    Write,
    ReadWrite,
    /// The left of `A.B` in a type or `import x = A.B`: any meaning.
    Namespace,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Value,
    Type,
    TypeOnly,
}

struct Scope<'t> {
    parent: Option<usize>,
    function: bool,
    names: HashMap<&'t str, Vec<(u32, u8)>>,
}

struct Binder<'s, 't> {
    s: &'s Syntax<'t>,
    scopes: Vec<Scope<'t>>,
    refs: Vec<(u32, usize, Use)>,
    decls: HashMap<u32, Binding>,
    /// Declaring identifier -> the function it names (uses inside it are
    /// self uses).
    owners: HashMap<u32, u32>,
    /// Later declarations of a merged name -> the first.
    merged: HashMap<u32, u32>,
    /// What the next declarations are: their kind and declaring node.
    current: (&'static str, u32),
}

pub fn bind(s: &Syntax) -> Bindings {
    let mut b =
        Binder { s, scopes: Vec::new(), refs: Vec::new(), decls: HashMap::new(), owners: HashMap::new(), merged: HashMap::new(), current: ("", 0) };
    let module = b.scope(None, true);
    for st in s.all(s.root(), F::Body).collect::<Vec<_>>() {
        b.walk(st, module, Mode::Value);
    }
    b.resolve()
}

impl<'t> Binder<'_, 't> {
    fn scope(&mut self, parent: Option<usize>, function: bool) -> usize {
        self.scopes.push(Scope { parent, function, names: HashMap::new() });
        self.scopes.len() - 1
    }

    fn function_scope(&self, mut scope: usize) -> usize {
        while !self.scopes[scope].function {
            match self.scopes[scope].parent {
                Some(p) => scope = p,
                None => break,
            }
        }
        scope
    }

    /// Sets what the following declarations are.
    fn as_(&mut self, kind: &'static str, owner: u32) {
        self.current = (kind, owner);
    }

    fn declare(&mut self, scope: usize, ident: u32, space: u8) {
        let s = self.s;
        if !s.is(ident, K::Ident) {
            return;
        }
        let name = s.text(ident);
        self.scopes[scope].names.entry(name).or_default().push((ident, space));
        let (kind, owner) = self.current;
        // The statement: a declarator's `var`, or the declaration itself.
        let stmt = if s.is(owner, K::Declarator) { s.parent(owner).unwrap_or(owner) } else { owner };
        let exported = s.parent(stmt).is_some_and(|p| s.is(p, K::Export));
        let mut ambient = false;
        let mut at = Some(stmt);
        while let Some(n) = at {
            if s.has_word(n, F::Modifier, "declare") {
                ambient = true;
                break;
            }
            at = s.parent(n);
        }
        // Merged declarations (`interface X` + `const X`, overloads) are one
        // binding: the first.
        let first = self.scopes[scope].names[name].first().map(|&(d, _)| d).filter(|&d| d != ident);
        if let Some(first) = first {
            self.merged.insert(ident, first);
            if let Some(b) = self.decls.get_mut(&first) {
                b.exported |= exported;
                b.ambient &= ambient;
            }
            return;
        }
        self.decls.insert(ident, Binding { kind, exported, ambient, ..Binding::default() });
        // Uses inside the function a name declares are not uses (as ESLint
        // counts them): a function's own body, or a function initializer.
        let own = if s.is(owner, K::Function) {
            Some(owner)
        } else if s.is(owner, K::Declarator) {
            s.get(owner, F::Init).filter(|&i| matches!(s.kind(i), Some(K::Function | K::Arrow)))
        } else {
            None
        };
        if let Some(own) = own {
            self.owners.insert(ident, own);
        }
    }

    fn reference(&mut self, ident: u32, scope: usize, use_: Use) {
        if self.s.is(ident, K::Ident) {
            self.refs.push((ident, scope, use_));
        }
    }

    /// Declares every name a binding pattern introduces.
    fn declare_pattern(&mut self, pat: u32, scope: usize, space: u8) {
        let s = self.s;
        match s.kind(pat) {
            Some(K::Ident) => self.declare(scope, pat, space),
            Some(K::ObjectPattern) => {
                for p in s.all(pat, F::Prop).collect::<Vec<_>>() {
                    match s.get(p, F::Value) {
                        Some(v) if !(s.is(v, K::AssignPattern) && s.get(v, F::Left).is_none()) => self.declare_pattern(v, scope, space),
                        _ => {
                            if let Some(k) = s.get(p, F::Key) {
                                self.declare(scope, k, space);
                            }
                        }
                    }
                }
                if let Some(r) = s.get(pat, F::Rest) {
                    self.declare_pattern(r, scope, space);
                }
            }
            Some(K::ArrayPattern) => {
                for e in s.all(pat, F::Element).collect::<Vec<_>>() {
                    self.declare_pattern(e, scope, space);
                }
            }
            Some(K::AssignPattern) => {
                if let Some(l) = s.get(pat, F::Left) {
                    self.declare_pattern(l, scope, space);
                }
            }
            Some(K::Rest) => {
                if let Some(a) = s.get(pat, F::Arg) {
                    self.declare_pattern(a, scope, space);
                }
            }
            _ => {}
        }
    }

    /// The expressions inside a binding pattern: defaults, computed keys and
    /// annotations.
    fn pattern_exprs(&mut self, pat: u32, scope: usize) {
        let s = self.s;
        match s.kind(pat) {
            Some(K::ObjectPattern) => {
                for p in s.all(pat, F::Prop).collect::<Vec<_>>() {
                    if let Some(k) = s.get(p, F::Key).filter(|&k| s.is(k, K::Computed)) {
                        self.walk(k, scope, Mode::Value);
                    }
                    if let Some(v) = s.get(p, F::Value) {
                        self.pattern_exprs(v, scope);
                    }
                }
                if let Some(r) = s.get(pat, F::Rest) {
                    self.pattern_exprs(r, scope);
                }
            }
            Some(K::ArrayPattern) => {
                for e in s.all(pat, F::Element).collect::<Vec<_>>() {
                    self.pattern_exprs(e, scope);
                }
            }
            Some(K::AssignPattern) => {
                if let Some(l) = s.get(pat, F::Left) {
                    self.pattern_exprs(l, scope);
                }
                if let Some(r) = s.get(pat, F::Right) {
                    self.walk(r, scope, Mode::Value);
                }
            }
            Some(K::Rest) => {
                if let Some(a) = s.get(pat, F::Arg) {
                    self.pattern_exprs(a, scope);
                }
            }
            _ => {}
        }
        if let Some(t) = s.get(pat, F::Type) {
            self.walk(t, scope, Mode::Type);
        }
    }

    fn type_params(&mut self, owner: u32, scope: usize) {
        let s = self.s;
        let params: Vec<u32> = s.all(owner, F::TypeParam).collect();
        for &tp in &params {
            if let Some(n) = s.get(tp, F::Name) {
                self.as_("type_param", tp);
                self.declare(scope, n, TYPE);
            }
        }
        for tp in params {
            self.walk_fields(tp, scope, Mode::Type, &[F::Constraint, F::Default]);
        }
    }

    fn walk_fields(&mut self, id: u32, scope: usize, mode: Mode, fields: &[F]) {
        for (f, c) in self.s.kids(id).collect::<Vec<_>>() {
            if f.is_some_and(|f| fields.contains(&f)) {
                self.walk(c, scope, mode);
            }
        }
    }

    /// Is this function or class a declaration (rather than an expression)?
    fn is_declaration(&self, id: u32) -> bool {
        let s = self.s;
        matches!(s.field(id), Some(F::Body | F::Decl))
            && s.parent(id).and_then(|p| s.kind(p)).is_some_and(|k| matches!(k, K::Program | K::Block | K::Case | K::Export | K::Labeled))
    }

    /// Type parameters, parameters, return type and body of anything callable.
    fn function_parts(&mut self, id: u32, scope: usize) {
        let s = self.s;
        self.type_params(id, scope);
        for d in s.all(id, F::Decorator).collect::<Vec<_>>() {
            self.walk(d, scope, Mode::Value);
        }
        for p in s.all(id, F::Param).collect::<Vec<_>>() {
            for d in s.all(p, F::Decorator).collect::<Vec<_>>() {
                self.walk(d, scope, Mode::Value);
            }
            if let Some(n) = s.get(p, F::Name) {
                self.as_("param", p);
                self.declare_pattern(n, scope, VALUE);
                self.pattern_exprs(n, scope);
            }
            self.walk_fields(p, scope, Mode::Type, &[F::Type]);
            self.walk_fields(p, scope, Mode::Value, &[F::Init]);
        }
        self.walk_fields(id, scope, Mode::Type, &[F::ReturnType]);
        if let Some(body) = s.get(id, F::Body) {
            if s.is(body, K::Block) {
                for st in s.all(body, F::Body).collect::<Vec<_>>() {
                    self.walk(st, scope, Mode::Value);
                }
            } else {
                self.walk(body, scope, Mode::Value);
            }
        }
    }

    fn key(&mut self, owner: u32, scope: usize, mode: Mode) {
        if let Some(k) = self.s.get(owner, F::Key).filter(|&k| self.s.is(k, K::Computed)) {
            let mode = if mode == Mode::Type { Mode::TypeOnly } else { mode };
            self.walk(k, scope, mode);
        }
    }

    /// The target of `=`: names written, member objects read.
    fn assign_target(&mut self, id: u32, scope: usize) {
        let s = self.s;
        match s.kind(id) {
            Some(K::Ident) => self.reference(id, scope, Use::Write),
            Some(K::Array) => {
                for e in s.all(id, F::Element).collect::<Vec<_>>() {
                    self.assign_target(e, scope);
                }
            }
            Some(K::Object) => {
                for p in s.all(id, F::Prop).collect::<Vec<_>>() {
                    match (s.kind(p), s.get(p, F::Value)) {
                        (Some(K::Prop), Some(v)) if !(s.is(v, K::AssignPattern) && s.get(v, F::Left).is_none()) => {
                            self.key(p, scope, Mode::Value);
                            self.assign_target(v, scope);
                        }
                        (Some(K::Prop), v) => {
                            if let Some(k) = s.get(p, F::Key) {
                                self.reference(k, scope, Use::Write);
                            }
                            if let Some(r) = v.and_then(|v| s.get(v, F::Right)) {
                                self.walk(r, scope, Mode::Value);
                            }
                        }
                        _ => self.assign_target(p, scope),
                    }
                }
            }
            Some(K::Spread | K::Rest) => {
                if let Some(a) = s.get(id, F::Arg) {
                    self.assign_target(a, scope);
                }
            }
            Some(K::AssignPattern) => {
                if let Some(l) = s.get(id, F::Left) {
                    self.assign_target(l, scope);
                }
                self.walk_fields(id, scope, Mode::Value, &[F::Right]);
            }
            Some(K::Paren | K::NonNull | K::As | K::Satisfies) => {
                if let Some(e) = s.get(id, F::Expr) {
                    self.assign_target(e, scope);
                }
                self.walk_fields(id, scope, Mode::Type, &[F::Type]);
            }
            _ => self.walk(id, scope, Mode::Value),
        }
    }

    /// The leftmost identifier of `A.B.C` (a qualified name or member chain).
    fn leftmost(&self, mut id: u32) -> u32 {
        loop {
            match self.s.kind(id) {
                Some(K::QualifiedName) => id = self.s.get(id, F::Left).unwrap_or(id),
                Some(K::Member) => id = self.s.get(id, F::Object).unwrap_or(id),
                _ => return id,
            }
        }
    }

    fn walk(&mut self, id: u32, scope: usize, mode: Mode) {
        let s = self.s;
        let Some(kind) = s.kind(id) else { return };
        match kind {
            K::Ident => match mode {
                Mode::Value => self.reference(id, scope, Use::Value),
                Mode::TypeOnly => self.reference(id, scope, Use::TypeOnly),
                Mode::Type => {}
            },
            K::Var => {
                let word = s.word(id, F::Kind).unwrap_or("var");
                let target = if word == "var" { self.function_scope(scope) } else { scope };
                let kind = match word {
                    "let" => "let",
                    "const" => "const",
                    "var" => "var",
                    _ => "using",
                };
                for d in s.all(id, F::Decl).collect::<Vec<_>>() {
                    if let Some(n) = s.get(d, F::Name) {
                        self.as_(kind, d);
                        self.declare_pattern(n, target, VALUE);
                        self.pattern_exprs(n, scope);
                    }
                    self.walk_fields(d, scope, Mode::Type, &[F::Type]);
                    self.walk_fields(d, scope, Mode::Value, &[F::Init]);
                }
            }
            K::Function => {
                let decl = self.is_declaration(id);
                let name = s.get(id, F::Name);
                if decl {
                    if let Some(n) = name {
                        self.as_("function", id);
                        self.declare(scope, n, VALUE);
                    }
                }
                let inner = self.scope(Some(scope), true);
                if !decl {
                    if let Some(n) = name {
                        self.as_("function_name", id);
                        self.declare(inner, n, VALUE);
                    }
                }
                self.function_parts(id, inner);
            }
            K::Arrow | K::FnType | K::CallSig => {
                let inner = self.scope(Some(scope), true);
                self.function_parts(id, inner);
            }
            K::Method | K::MethodSig => {
                for d in s.all(id, F::Decorator).collect::<Vec<_>>() {
                    self.walk(d, scope, Mode::Value);
                }
                let key_mode = if kind == K::MethodSig { Mode::Type } else { Mode::Value };
                self.key(id, scope, key_mode);
                let inner = self.scope(Some(scope), true);
                self.function_parts(id, inner);
            }
            K::Class => {
                let decl = self.is_declaration(id);
                let name = s.get(id, F::Name);
                if decl {
                    if let Some(n) = name {
                        self.as_("class", id);
                        self.declare(scope, n, VALUE | TYPE);
                    }
                }
                for d in s.all(id, F::Decorator).collect::<Vec<_>>() {
                    self.walk(d, scope, Mode::Value);
                }
                let inner = self.scope(Some(scope), false);
                if !decl {
                    if let Some(n) = name {
                        self.as_("class_name", id);
                        self.declare(inner, n, VALUE | TYPE);
                    }
                }
                self.type_params(id, inner);
                self.walk_fields(id, inner, Mode::Value, &[F::Extends]);
                self.walk_fields(id, inner, Mode::Type, &[F::TypeArg, F::Implements]);
                for m in s.all(id, F::Member).collect::<Vec<_>>() {
                    self.walk(m, inner, Mode::Value);
                }
            }
            K::ClassProp => {
                for d in s.all(id, F::Decorator).collect::<Vec<_>>() {
                    self.walk(d, scope, Mode::Value);
                }
                self.key(id, scope, Mode::Value);
                self.walk_fields(id, scope, Mode::Type, &[F::Type]);
                self.walk_fields(id, scope, Mode::Value, &[F::Init]);
            }
            K::Block | K::StaticBlock => {
                let inner = self.scope(Some(scope), kind == K::StaticBlock);
                let body: Vec<u32> = if kind == K::Block {
                    s.all(id, F::Body).collect()
                } else {
                    s.get(id, F::Body).map(|b| s.all(b, F::Body).collect()).unwrap_or_default()
                };
                for st in body {
                    self.walk(st, inner, Mode::Value);
                }
            }
            K::For | K::ForIn | K::ForOf => {
                let inner = self.scope(Some(scope), false);
                for (f, c) in s.kids(id).collect::<Vec<_>>() {
                    match f {
                        Some(F::Left) if !s.is(c, K::Var) => self.assign_target(c, inner),
                        Some(F::Modifier) => {}
                        _ => self.walk(c, inner, Mode::Value),
                    }
                }
            }
            K::Switch => {
                self.walk_fields(id, scope, Mode::Value, &[F::Discriminant]);
                let inner = self.scope(Some(scope), false);
                for c in s.all(id, F::Case).collect::<Vec<_>>() {
                    for (_, x) in s.kids(c).collect::<Vec<_>>() {
                        self.walk(x, inner, Mode::Value);
                    }
                }
            }
            K::Try => {
                self.walk_fields(id, scope, Mode::Value, &[F::Block, F::Finalizer]);
                if let Some(h) = s.get(id, F::Handler) {
                    let inner = self.scope(Some(scope), false);
                    if let Some(p) = s.get(id, F::Param) {
                        self.as_("catch", p);
                        self.declare_pattern(p, inner, VALUE);
                        self.pattern_exprs(p, inner);
                    }
                    for st in s.all(h, F::Body).collect::<Vec<_>>() {
                        self.walk(st, inner, Mode::Value);
                    }
                }
            }
            K::Interface | K::TypeAlias => {
                if let Some(n) = s.get(id, F::Name) {
                    self.as_(if kind == K::Interface { "interface" } else { "type" }, id);
                    self.declare(scope, n, TYPE);
                }
                let inner = self.scope(Some(scope), false);
                self.type_params(id, inner);
                for (f, c) in s.kids(id).collect::<Vec<_>>() {
                    if matches!(f, Some(F::Extends | F::Member | F::Type)) {
                        self.walk(c, inner, Mode::Type);
                    }
                }
            }
            K::Enum => {
                if let Some(n) = s.get(id, F::Name) {
                    self.as_("enum", id);
                    self.declare(scope, n, VALUE | TYPE);
                }
                for m in s.all(id, F::Member).collect::<Vec<_>>() {
                    self.walk_fields(m, scope, Mode::Value, &[F::Init]);
                }
            }
            K::Namespace => {
                if let Some(n) = s.get(id, F::Name) {
                    let n = self.leftmost(n);
                    self.as_("namespace", id);
                    self.declare(scope, n, VALUE | TYPE);
                }
                if let Some(b) = s.get(id, F::Body) {
                    let inner = self.scope(Some(scope), true);
                    for st in s.all(b, F::Body).collect::<Vec<_>>() {
                        self.walk(st, inner, Mode::Value);
                    }
                }
            }
            K::Import => {
                self.as_("import", id);
                for (f, c) in s.kids(id).collect::<Vec<_>>() {
                    match f {
                        Some(F::DefaultImport | F::Namespace) => self.declare(scope, c, VALUE | TYPE),
                        Some(F::Specifier) => {
                            if let Some(local) = s.get(c, F::Local).or_else(|| s.get(c, F::Imported)) {
                                self.declare(scope, local, VALUE | TYPE);
                            }
                        }
                        _ => {}
                    }
                }
            }
            K::ImportEquals => {
                if let Some(n) = s.get(id, F::Name) {
                    self.as_("import", id);
                    self.declare(scope, n, VALUE | TYPE);
                }
                if let Some(v) = s.get(id, F::Value) {
                    if matches!(s.kind(v), Some(K::Ident | K::Member)) {
                        let l = self.leftmost(v);
                        self.reference(l, scope, Use::Namespace);
                    } else {
                        self.walk(v, scope, Mode::Value);
                    }
                }
            }
            K::Export => {
                let reexport = s.get(id, F::From).is_some();
                for (f, c) in s.kids(id).collect::<Vec<_>>() {
                    match f {
                        Some(F::Decl | F::Expr) => self.walk(c, scope, Mode::Value),
                        Some(F::Specifier) if !reexport => {
                            if let Some(l) = s.get(c, F::Local) {
                                self.reference(l, scope, Use::Value);
                            }
                        }
                        _ => {}
                    }
                }
            }
            K::Labeled => self.walk_fields(id, scope, Mode::Value, &[F::Body]),
            K::Break | K::Continue | K::MetaProperty | K::QualifiedName => {}
            K::Prop => {
                let value = s.get(id, F::Value);
                match (s.get(id, F::Key), value) {
                    (Some(k), None) if s.is(k, K::Ident) => self.reference(k, scope, Use::Value),
                    _ => self.key(id, scope, Mode::Value),
                }
                if let Some(v) = value {
                    self.walk(v, scope, Mode::Value);
                }
            }
            K::Member => self.walk_fields(id, scope, mode, &[F::Object]),
            K::Assign => {
                let compound = s.word(id, F::Op) != Some("=");
                if let Some(l) = s.get(id, F::Left) {
                    if compound && s.is(l, K::Ident) {
                        self.reference(l, scope, Use::ReadWrite);
                    } else if compound {
                        self.walk(l, scope, Mode::Value);
                    } else {
                        self.assign_target(l, scope);
                    }
                }
                self.walk_fields(id, scope, Mode::Value, &[F::Right]);
            }
            K::Update => match s.get(id, F::Arg) {
                Some(a) if s.is(a, K::Ident) => self.reference(a, scope, Use::ReadWrite),
                Some(a) => self.walk(a, scope, Mode::Value),
                None => {}
            },
            K::TypeRef => {
                if let Some(n) = s.get(id, F::Name) {
                    if s.is(n, K::Ident) {
                        self.reference(n, scope, Use::Type);
                    } else {
                        let l = self.leftmost(n);
                        self.reference(l, scope, Use::Namespace);
                    }
                }
                self.walk_fields(id, scope, Mode::Type, &[F::TypeArg]);
            }
            K::TypeQuery => {
                if let Some(n) = s.get(id, F::Name) {
                    let l = self.leftmost(n);
                    self.reference(l, scope, Use::TypeOnly);
                }
                self.walk_fields(id, scope, Mode::Type, &[F::TypeArg]);
            }
            K::ImportType => self.walk_fields(id, scope, Mode::Type, &[F::TypeArg]),
            K::TypePredicate | K::TupleMember => self.walk_fields(id, scope, Mode::Type, &[F::Type]),
            K::Infer => {
                if let Some(tp) = s.get(id, F::TypeParam) {
                    if let Some(n) = s.get(tp, F::Name) {
                        self.as_("type_param", tp);
                        self.declare(scope, n, TYPE);
                    }
                    self.walk_fields(tp, scope, Mode::Type, &[F::Constraint]);
                }
            }
            K::ConditionalType => {
                let inner = self.scope(Some(scope), false);
                self.walk_fields(id, inner, Mode::Type, &[F::Check, F::Extends, F::Consequent]);
                self.walk_fields(id, scope, Mode::Type, &[F::Alternate]);
            }
            K::Mapped => {
                let inner = self.scope(Some(scope), false);
                if let Some(tp) = s.get(id, F::TypeParam) {
                    if let Some(n) = s.get(tp, F::Name) {
                        self.as_("type_param", tp);
                        self.declare(inner, n, TYPE);
                    }
                    self.walk_fields(tp, scope, Mode::Type, &[F::Constraint]);
                }
                self.walk_fields(id, inner, Mode::Type, &[F::Name, F::Type]);
            }
            K::PropSig => {
                self.key(id, scope, Mode::Type);
                self.walk_fields(id, scope, Mode::Type, &[F::Type]);
            }
            K::Computed => self.walk_fields(id, scope, mode, &[F::Expr]),
            _ => {
                let type_kind = kind as u16 >= K::TypeRef as u16;
                for (f, c) in s.kids(id).collect::<Vec<_>>() {
                    let Some(f) = f else { continue };
                    if matches!(f, F::Key | F::Name | F::Label | F::Property | F::Imported | F::Exported | F::Op | F::Kind | F::Modifier | F::Optional | F::Quasi) {
                        continue;
                    }
                    let child_mode = if type_kind
                        || matches!(f, F::Type | F::ReturnType | F::TypeArg | F::TypeParam | F::Implements | F::Constraint | F::Default | F::Check)
                    {
                        Mode::Type
                    } else {
                        mode
                    };
                    self.walk(c, scope, child_mode);
                }
            }
        }
    }

    fn resolve(self) -> Bindings {
        let Binder { s, scopes, refs, mut decls, owners, merged, .. } = self;
        let mut out_refs = HashMap::with_capacity(refs.len());
        for (ident, scope, use_) in refs {
            let name = s.text(ident);
            let want = match use_ {
                Use::Type => TYPE,
                Use::Namespace => VALUE | TYPE,
                _ => VALUE,
            };
            let mut at = Some(scope);
            let mut found = None;
            while let Some(sc) = at {
                if let Some(list) = scopes[sc].names.get(name) {
                    if let Some(&(decl, _)) = list.iter().find(|(_, space)| space & want != 0) {
                        found = Some(decl);
                        break;
                    }
                }
                at = scopes[sc].parent;
            }
            let Some(decl) = found.map(|d| merged.get(&d).copied().unwrap_or(d)) else { continue };
            if decl == ident {
                continue;
            }
            let inside = owners.get(&decl).is_some_and(|&o| {
                let (n, r) = (s.tree.node(o), s.tree.node(ident));
                n.start <= r.start && r.end <= n.end
            });
            let b = decls.entry(decl).or_default();
            if inside && use_ != Use::Write {
                b.self_uses += 1;
            }
            match use_ {
                Use::Value | Use::Namespace => {
                    b.uses += 1;
                    b.value_uses += u32::from(use_ == Use::Value);
                }
                Use::Type | Use::TypeOnly => b.uses += 1,
                Use::Write => b.writes += 1,
                Use::ReadWrite => {
                    b.uses += 1;
                    b.value_uses += 1;
                    b.writes += 1;
                }
            }
            out_refs.insert(ident, decl);
        }
        Bindings { decls, refs: out_refs }
    }
}

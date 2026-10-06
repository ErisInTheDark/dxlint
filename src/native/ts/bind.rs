//! Scopes and name resolution for one file. Values and types live in
//! separate namespaces (TypeScript's declaration spaces), so `const x` does
//! not hide an imported type `x`. Every declaring identifier gets use
//! counts, split by whether the use needs the runtime value: `typeof x` and
//! computed keys inside types do not.

// Keyed by node ids and names, built for every file: the fast hasher.
use crate::fx_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::cell::OnceCell;

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
    /// Value references that resolve to nothing in the file (globals).
    pub unresolved: HashSet<u32>,
    /// Referencing identifier -> how it uses the name: `read`, `write`
    /// (`x = 1`, destructuring, `for (x of ...)`), `read_write` (`x += 1`,
    /// `x++`) or `type` (type positions, `typeof x` in a type).
    pub access: HashMap<u32, &'static str>,
    /// Written identifier or member (`x`, `a.b`, `a[b]`), or array or
    /// object literal used as a destructuring target -> the node that writes
    /// it: the assignment, update, `delete` or `for` statement.
    pub write_sites: HashMap<u32, u32>,
    /// Declaring identifier -> the kinds of every declaration merged into
    /// its binding (`class` and `interface` of one name).
    pub declaration_kinds: HashMap<u32, Vec<&'static str>>,
    /// Class member declaration (property, method, parameter property) ->
    /// accesses through `this` (or `x.#name`) inside its class.
    pub members: HashMap<u32, MemberUses>,
    /// Function-like node -> what its own body does ([`USES_THIS`], ...).
    /// Arrow functions count toward the function around them.
    pub functions: HashMap<u32, u8>,
    pub scopes: Vec<ScopeInfo>,
    /// Declaring identifier (merged ones too) -> the scope it declares in.
    pub decl_scope: HashMap<u32, usize>,
    /// Referencing identifier -> the scope it appears in.
    pub ref_scope: HashMap<u32, usize>,
    /// Later declaring identifier of a merged name (`var a; var a;`,
    /// overloads, `class X` + `interface X`) -> the first, which owns the
    /// binding.
    pub merged: HashMap<u32, u32>,
    index: OnceCell<BindingIndex>,
}

#[derive(Default)]
struct BindingIndex {
    declarations: HashMap<u32, Vec<u32>>,
    references: HashMap<u32, Vec<u32>>,
}

impl Bindings {
    fn index(&self, s: &Syntax) -> &BindingIndex {
        self.index.get_or_init(|| {
            let mut index = BindingIndex::default();
            for &d in self.decls.keys() {
                index.declarations.insert(d, vec![d]);
            }
            for (&later, &first) in &self.merged {
                index.declarations.entry(first).or_default().push(later);
            }
            for (&reference, &decl) in &self.refs {
                index.references.entry(decl).or_default().push(reference);
            }
            for nodes in index.declarations.values_mut().chain(index.references.values_mut()) {
                nodes.sort_by_key(|&id| s.tree.node(id).start);
            }
            index
        })
    }

    /// Canonical bindings and all their declaring identifiers, in source order.
    pub fn declarations(&self, s: &Syntax) -> &HashMap<u32, Vec<u32>> {
        &self.index(s).declarations
    }

    /// Real references of a canonical binding; initializer writes are not references.
    pub fn references_of(&self, s: &Syntax, declaration: u32) -> &[u32] {
        self.index(s).references.get(&declaration).map(Vec::as_slice).unwrap_or_default()
    }

    /// The declaration `name` means at `at` (a value, or a type when
    /// `value` is false), searching the scopes around it outward; `None`
    /// for a global or an unknown name. For names no reference spells out:
    /// `{}.hasOwnProperty` asks whether `Object` is the global one.
    pub fn visible(&self, s: &Syntax, at: u32, name: &str, value: bool) -> Option<u32> {
        let mut scope = None;
        let mut node = Some(at);
        while let (None, Some(n)) = (scope, node) {
            scope = self.scopes.iter().rposition(|sc| sc.node == n);
            node = s.parent(n);
        }
        let mut scope = scope.or((!self.scopes.is_empty()).then_some(0));
        let type_only = |d: u32| {
            let kinds = self.declaration_kinds.get(&d).map(Vec::as_slice).unwrap_or_default();
            let kind = self.decls.get(&d).map_or("", |b| b.kind);
            std::iter::once(kind).chain(kinds.iter().copied()).all(|k| matches!(k, "type" | "interface" | "type_param"))
        };
        while let Some(sc) = scope {
            let found = self.decl_scope.iter()
                .filter(|&(&d, &ds)| ds == sc && s.text(d) == name && (!value || !type_only(self.merged.get(&d).copied().unwrap_or(d))))
                .map(|(&d, _)| self.merged.get(&d).copied().unwrap_or(d))
                .min_by_key(|&d| s.tree.node(d).start);
            if found.is_some() {
                return found;
            }
            scope = self.scopes[sc].parent;
        }
        None
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct MemberUses {
    pub reads: u32,
    /// Writes outside the constructor (`this.x = `, `this.x++`, `delete`).
    pub writes: u32,
    /// Writes inside the constructor.
    pub ctor_writes: u32,
}

pub const USES_THIS: u8 = 1;
pub const USES_ARGUMENTS: u8 = 2;
pub const HAS_YIELD: u8 = 4;

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
    /// `export { x }`, `export default x`, `export = x`: any meaning, and
    /// a value use when it is a value.
    Export,
    /// `export type { x }`, `export { type x }`: any meaning, never a
    /// value use.
    TypeExport,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Value,
    Type,
    TypeOnly,
}

/// A scope: indexes into [`Bindings::scopes`]; the first is the file's.
#[derive(Clone, Copy, Debug)]
pub struct ScopeInfo {
    pub parent: Option<usize>,
    /// Where `var` lands: the file, a function (or method, arrow), a static
    /// block or a namespace body.
    pub function: bool,
    /// What opens it: the program, a function-like, block, class, `for`,
    /// `switch`, catch clause, namespace body or type construct.
    pub node: u32,
}

struct Scope<'t> {
    info: ScopeInfo,
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
    /// Class -> its members by name (`x`, `#x`) -> declaring node.
    class_members: HashMap<u32, HashMap<&'t str, u32>>,
    members: HashMap<u32, MemberUses>,
    functions: HashMap<u32, u8>,
    write_sites: HashMap<u32, u32>,
    /// The assignment (or update, `for`) whose target is being walked.
    site: u32,
    /// First declaration of a name -> the kinds of all its declarations.
    merged_kinds: HashMap<u32, Vec<&'static str>>,
    decl_scope: HashMap<u32, usize>,
    /// Inside a conditional type's `extends`: where `infer X` declares.
    infer_scope: Option<usize>,
    ecma_version: u16,
}

pub fn bind(s: &Syntax, root_function: bool, ecma_version: u16) -> Bindings {
    let mut b = Binder {
        s,
        scopes: Vec::new(),
        refs: Vec::new(),
        decls: HashMap::default(),
        owners: HashMap::default(),
        merged: HashMap::default(),
        current: ("", 0),
        class_members: HashMap::default(),
        members: HashMap::default(),
        functions: HashMap::default(),
        write_sites: HashMap::default(),
        site: 0,
        merged_kinds: HashMap::default(),
        decl_scope: HashMap::default(),
        infer_scope: None,
        ecma_version,
    };
    let global = b.scope(None, true, s.root());
    let module = if root_function { b.scope(Some(global), true, s.root()) } else { global };
    for st in s.all(s.root(), F::Body).collect::<Vec<_>>() {
        b.walk(st, module, Mode::Value);
    }
    b.resolve()
}

/// The function whose own `this`/`arguments` a node at `id` sees: the
/// nearest non-arrow function, method or class body.
fn this_owner(s: &Syntax, id: u32) -> Option<u32> {
    let mut child = id;
    while let Some(n) = s.parent(child) {
        // A computed key (`[this.x]() {}`) runs where the class does.
        let key = s.field(child) == Some(F::Key);
        match s.kind(n) {
            Some(K::Method | K::ClassProp) if !key => return Some(n),
            Some(K::Function | K::StaticBlock) => return Some(n),
            _ => child = n,
        }
    }
    None
}

impl<'t> Binder<'_, 't> {
    fn scope(&mut self, parent: Option<usize>, function: bool, node: u32) -> usize {
        self.scopes.push(Scope { info: ScopeInfo { parent, function, node }, names: HashMap::default() });
        self.scopes.len() - 1
    }

    fn function_scope(&self, mut scope: usize) -> usize {
        while !self.scopes[scope].info.function {
            match self.scopes[scope].info.parent {
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
        self.decl_scope.insert(ident, scope);
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
            self.merged_kinds.entry(first).or_default().push(kind);
            if let Some(b) = self.decls.get_mut(&first) {
                b.exported |= exported;
                b.ambient &= ambient;
            }
            return;
        }
        self.merged_kinds.insert(ident, vec![kind]);
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
                for p in s.all(pat, F::Property).collect::<Vec<_>>() {
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
                for p in s.all(pat, F::Property).collect::<Vec<_>>() {
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
                self.as_("type_parameter", tp);
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
            // A parameter's decorators run outside the function.
            let outer = self.scopes[scope].info.parent.unwrap_or(scope);
            for d in s.all(p, F::Decorator).collect::<Vec<_>>() {
                self.walk(d, outer, Mode::Value);
            }
            if let Some(n) = s.get(p, F::Name) {
                self.as_("parameter", p);
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
            Some(K::Ident) => {
                self.write_sites.insert(id, self.site);
                self.reference(id, scope, Use::Write)
            }
            Some(K::Member | K::Index) => {
                self.write_sites.insert(id, self.site);
                self.walk(id, scope, Mode::Value)
            }
            Some(K::Array) => {
                self.write_sites.insert(id, self.site);
                for e in s.all(id, F::Element).collect::<Vec<_>>() {
                    self.assign_target(e, scope);
                }
            }
            Some(K::Object) => {
                self.write_sites.insert(id, self.site);
                for p in s.all(id, F::Property).collect::<Vec<_>>() {
                    match (s.kind(p), s.get(p, F::Value)) {
                        (Some(K::Prop), Some(v)) if !(s.is(v, K::AssignPattern) && s.get(v, F::Left).is_none()) => {
                            self.key(p, scope, Mode::Value);
                            self.assign_target(v, scope);
                        }
                        (Some(K::Prop), v) => {
                            if let Some(k) = s.get(p, F::Key) {
                                if s.is(k, K::Ident) {
                                    self.write_sites.insert(k, self.site);
                                }
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
            // `[x = 1] = y`: a default, written as an assignment.
            Some(K::AssignPattern) | Some(K::Assign) if s.word(id, F::Op).is_none_or(|op| op == "=") => {
                if let Some(l) = s.get(id, F::Left) {
                    self.assign_target(l, scope);
                }
                let site = self.site;
                self.walk_fields(id, scope, Mode::Value, &[F::Right]);
                self.site = site;
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

    /// Registers a class's members: properties, methods and accessors by
    /// key, and constructor parameter properties.
    fn class_members(&mut self, class: u32) {
        let s = self.s;
        let mut names: HashMap<&'t str, u32> = HashMap::default();
        for m in s.all(class, F::Member).filter(|&m| matches!(s.kind(m), Some(K::ClassProp | K::Method))) {
            let Some(k) = s.get(m, F::Key).filter(|&k| matches!(s.kind(k), Some(K::Ident | K::PrivateName))) else { continue };
            if s.text(k) == "constructor" {
                for p in s.all(m, F::Param) {
                    let property = s.all(p, F::Modifier).any(|w| matches!(s.text(w), "private" | "protected" | "public" | "readonly"));
                    if let Some(n) = s.get(p, F::Name).filter(|&n| property && s.is(n, K::Ident)) {
                        names.entry(s.text(n)).or_insert(p);
                    }
                }
                continue;
            }
            // Getter and setter pairs are one member: the first.
            names.entry(s.text(k)).or_insert(m);
        }
        self.class_members.insert(class, names);
    }

    /// Whether `class` declares member `name`; with `ts_private`, only a
    /// `private` one.
    fn declares(&self, class: u32, name: &str, ts_private: bool) -> bool {
        self.class_members.get(&class).and_then(|m| m.get(name)).is_some_and(|&d| !ts_private || self.s.all(d, F::Modifier).any(|w| self.s.text(w) == "private"))
    }

    /// `this.x`, `obj.#x`, or `other.x` for a `private x` of an enclosing
    /// class (only that class can reach it, so another instance's): a read
    /// or write of that member.
    fn member_access(&mut self, id: u32) {
        let s = self.s;
        let (Some(object), Some(prop)) = (s.get(id, F::Object), s.get(id, F::Property)) else { return };
        let name = s.text(prop);
        let hash = s.is(prop, K::PrivateName);
        // Resolved by the enclosing class that declares it, not by `this`.
        let private = !s.is(object, K::This);
        if private && !hash && !self.enclosing_declares(id, name) {
            return;
        }
        // The class: `this`'s (through arrows), or for `#x` the nearest
        // enclosing class declaring it.
        let mut at = s.parent(id);
        let mut in_ctor = false;
        let mut owner_fn = None;
        // A callback inside the constructor runs later: not a constructor
        // write.
        let mut crossed_function = false;
        let class = loop {
            let Some(n) = at else { return };
            match s.kind(n) {
                Some(K::Function) if !private => return,
                Some(K::Function | K::Arrow) => crossed_function = true,
                Some(K::Method | K::ClassProp | K::StaticBlock) if owner_fn.is_none() => {
                    owner_fn = Some(n);
                    in_ctor = !crossed_function && s.is(n, K::Method) && s.get(n, F::Key).is_some_and(|k| s.text(k) == "constructor");
                    if let Some(p) = s.parent(n).filter(|&p| s.is(p, K::Class)) {
                        if !private || self.declares(p, name, !hash) {
                            break p;
                        }
                    } else if !private {
                        return;
                    }
                }
                Some(K::Class) if private && self.declares(n, name, !hash) => break n,
                _ => {}
            }
            at = s.parent(n);
        };
        let Some(&decl) = self.class_members.get(&class).and_then(|m| m.get(name)) else { return };
        let (read, write) = self.access_kind(id);
        let uses = self.members.entry(decl).or_default();
        uses.reads += u32::from(read);
        if write {
            if in_ctor {
                uses.ctor_writes += 1;
            } else {
                uses.writes += 1;
            }
        }
    }

    /// Some enclosing class declares a `private` member `name`.
    fn enclosing_declares(&self, id: u32, name: &str) -> bool {
        let mut at = self.s.parent(id);
        while let Some(n) = at {
            if self.s.is(n, K::Class) && self.declares(n, name, true) {
                return true;
            }
            at = self.s.parent(n);
        }
        false
    }

    /// Whether an expression is read, written, or both where it stands.
    fn access_kind(&self, id: u32) -> (bool, bool) {
        let s = self.s;
        let Some(p) = s.parent(id) else { return (true, false) };
        let statement = |n: u32| s.parent(n).is_some_and(|g| s.is(g, K::ExprStmt));
        match s.kind(p) {
            Some(K::Assign) if s.field(id) == Some(F::Left) => {
                let compound = s.word(p, F::Op) != Some("=");
                (compound && !statement(p), true)
            }
            Some(K::Update) => (!statement(p), true),
            Some(K::Unary) if s.word(p, F::Op) == Some("delete") => (false, true),
            Some(K::Paren | K::NonNull) => self.access_kind(p),
            // `this.x.y = v` / `this.x[k] = v` modify what `x` holds: as
            // typescript-eslint's prefer-readonly counts it, a write too.
            Some(K::Member | K::Index) if s.field(id) == Some(F::Object) => (true, self.access_kind(p).1),
            // Destructuring targets.
            Some(K::Array | K::Object | K::Prop | K::Spread | K::AssignPattern) if self.in_pattern(p) => (false, true),
            Some(K::ForIn | K::ForOf) if s.field(id) == Some(F::Left) => (false, true),
            _ => (true, false),
        }
    }

    /// Inside the target of a destructuring assignment.
    fn in_pattern(&self, mut id: u32) -> bool {
        let s = self.s;
        while let Some(p) = s.parent(id) {
            match s.kind(p) {
                Some(K::Array | K::Object | K::Prop | K::Spread | K::AssignPattern) => id = p,
                Some(K::Assign) => return s.field(id) == Some(F::Left),
                Some(K::ForIn | K::ForOf) => return s.field(id) == Some(F::Left),
                _ => return false,
            }
        }
        false
    }

    /// `id` without the parentheses, `!`, `as` and `satisfies` around it.
    fn unwrapped(&self, mut id: u32) -> u32 {
        while matches!(self.s.kind(id), Some(K::Paren | K::NonNull | K::As | K::Satisfies | K::TypeAssertion)) {
            match self.s.get(id, F::Expr) {
                Some(e) => id = e,
                None => break,
            }
        }
        id
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
                        let target = if self.ecma_version < 2015 { self.function_scope(scope) } else { scope };
                        self.declare(target, n, VALUE);
                    }
                }
                let inner = self.scope(Some(scope), true, id);
                if !decl {
                    if let Some(n) = name {
                        self.as_("function_name", id);
                        self.declare(inner, n, VALUE);
                    }
                }
                self.function_parts(id, inner);
            }
            K::Arrow | K::FnType | K::CallSig => {
                let inner = self.scope(Some(scope), true, id);
                self.function_parts(id, inner);
            }
            K::Method | K::MethodSig => {
                for d in s.all(id, F::Decorator).collect::<Vec<_>>() {
                    self.walk(d, scope, Mode::Value);
                }
                let key_mode = if kind == K::MethodSig { Mode::Type } else { Mode::Value };
                self.key(id, scope, key_mode);
                let inner = self.scope(Some(scope), true, id);
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
                let inner = self.scope(Some(scope), false, id);
                if !decl {
                    if let Some(n) = name {
                        self.as_("class_name", id);
                        self.declare(inner, n, VALUE | TYPE);
                    }
                }
                self.type_params(id, inner);
                self.walk_fields(id, inner, Mode::Value, &[F::Extends]);
                self.walk_fields(id, inner, Mode::Type, &[F::TypeArg, F::Implements]);
                self.class_members(id);
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
                let inner = self.scope(Some(scope), kind == K::StaticBlock, id);
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
                let inner = self.scope(Some(scope), false, id);
                for (f, c) in s.kids(id).collect::<Vec<_>>() {
                    match f {
                        Some(F::Left) if !s.is(c, K::Var) => {
                            self.site = id;
                            self.assign_target(c, inner)
                        }
                        Some(F::Modifier) => {}
                        _ => self.walk(c, inner, Mode::Value),
                    }
                }
            }
            K::Switch => {
                self.walk_fields(id, scope, Mode::Value, &[F::Discriminant]);
                let inner = self.scope(Some(scope), false, id);
                for c in s.all(id, F::Case).collect::<Vec<_>>() {
                    for (_, x) in s.kids(c).collect::<Vec<_>>() {
                        self.walk(x, inner, Mode::Value);
                    }
                }
            }
            K::Try => {
                self.walk_fields(id, scope, Mode::Value, &[F::Block, F::Finalizer]);
                if let Some(h) = s.get(id, F::Handler) {
                    let inner = self.scope(Some(scope), false, h);
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
                let inner = self.scope(Some(scope), false, id);
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
                    let inner = self.scope(Some(scope), true, b);
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
                        // `export default X` and `export { X }` export any
                        // meaning of `X` (a type too).
                        Some(F::Expr) if s.is(s.unparen(c), K::Ident) => self.reference(s.unparen(c), scope, Use::Export),
                        Some(F::Decl | F::Expr) => self.walk(c, scope, Mode::Value),
                        Some(F::Specifier) if !reexport => {
                            if let Some(l) = s.get(c, F::Local) {
                                let type_only = s.word(id, F::Kind) == Some("type") || s.word(c, F::Kind) == Some("type");
                                self.reference(l, scope, if type_only { Use::TypeExport } else { Use::Export });
                            }
                        }
                        _ => {}
                    }
                }
            }
            K::ExportAssign => match s.get(id, F::Expr).map(|e| s.unparen(e)) {
                Some(e) if s.is(e, K::Ident) => self.reference(e, scope, Use::Export),
                _ => self.walk_fields(id, scope, Mode::Value, &[F::Expr]),
            },
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
            K::Member => {
                self.member_access(id);
                self.walk_fields(id, scope, mode, &[F::Object]);
            }
            K::This => {
                if let Some(f) = this_owner(s, id) {
                    *self.functions.entry(f).or_default() |= USES_THIS;
                }
            }
            K::Yield => {
                if let Some(f) = this_owner(s, id) {
                    *self.functions.entry(f).or_default() |= HAS_YIELD;
                }
                self.walk_fields(id, scope, mode, &[F::Arg]);
            }
            K::Assign => {
                let compound = s.word(id, F::Op) != Some("=");
                if let Some(l) = s.get(id, F::Left) {
                    let target = self.unwrapped(l);
                    if compound && matches!(s.kind(target), Some(K::Ident | K::Member | K::Index)) {
                        self.write_sites.insert(target, id);
                    }
                    if compound && s.is(target, K::Ident) {
                        self.reference(target, scope, Use::ReadWrite);
                    } else if compound {
                        self.walk(l, scope, Mode::Value);
                    } else {
                        self.site = id;
                        self.assign_target(l, scope);
                    }
                }
                self.walk_fields(id, scope, Mode::Value, &[F::Right]);
            }
            K::Update => match s.get(id, F::Arg).map(|a| (a, self.unwrapped(a))) {
                Some((_, a)) if s.is(a, K::Ident) => {
                    self.write_sites.insert(a, id);
                    self.reference(a, scope, Use::ReadWrite)
                }
                Some((a, target)) => {
                    if matches!(s.kind(target), Some(K::Member | K::Index)) {
                        self.write_sites.insert(target, id);
                    }
                    self.walk(a, scope, Mode::Value)
                }
                None => {}
            },
            K::Unary if s.word(id, F::Op) == Some("delete") => {
                if let Some(a) = s.get(id, F::Arg) {
                    let target = self.unwrapped(a);
                    if matches!(s.kind(target), Some(K::Member | K::Index)) {
                        self.write_sites.insert(target, id);
                    }
                    self.walk(a, scope, mode);
                }
            }
            K::TypeRef => {
                if let Some(n) = s.get(id, F::Name) {
                    // `foo.Foo` looks `foo` up as a namespace, which a
                    // plain value (a parameter `foo`) does not hide.
                    let named = if s.is(n, K::Ident) { n } else { self.leftmost(n) };
                    self.reference(named, scope, Use::Type);
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
            // `x is T` names parameter `x` (only the checker needs it).
            K::TypePredicate => {
                if let Some(n) = s.get(id, F::Name).filter(|&n| s.is(n, K::Ident)) {
                    self.reference(n, scope, Use::TypeOnly);
                }
                self.walk_fields(id, scope, Mode::Type, &[F::Type]);
            }
            K::TupleMember => self.walk_fields(id, scope, Mode::Type, &[F::Type]),
            K::Infer => {
                if let Some(tp) = s.get(id, F::TypeParam) {
                    if let Some(n) = s.get(tp, F::Name) {
                        self.as_("type_parameter", tp);
                        self.declare(self.infer_scope.unwrap_or(scope), n, TYPE);
                    }
                    self.walk_fields(tp, scope, Mode::Type, &[F::Constraint]);
                }
            }
            // `infer X` anywhere in `extends` (even inside a function type)
            // declares `X` for the true branch.
            K::ConditionalType => {
                let inner = self.scope(Some(scope), false, id);
                self.walk_fields(id, inner, Mode::Type, &[F::Check]);
                let outer_infer = self.infer_scope.replace(inner);
                self.walk_fields(id, inner, Mode::Type, &[F::Extends]);
                self.infer_scope = outer_infer;
                self.walk_fields(id, inner, Mode::Type, &[F::Consequent]);
                self.walk_fields(id, scope, Mode::Type, &[F::Alternate]);
            }
            K::Mapped => {
                let inner = self.scope(Some(scope), false, id);
                if let Some(tp) = s.get(id, F::TypeParam) {
                    if let Some(n) = s.get(tp, F::Name) {
                        self.as_("type_parameter", tp);
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
            // The opening name is a use (components); the closing one not.
            K::JsxElement => {
                self.walk_fields(id, scope, Mode::Value, &[F::Name, F::Property, F::Body]);
                self.walk_fields(id, scope, Mode::Type, &[F::TypeArg]);
            }
            _ => {
                let type_kind = kind as u16 >= K::TypeRef as u16;
                for (f, c) in s.kids(id).collect::<Vec<_>>() {
                    let Some(f) = f else { continue };
                    // A member's `.property` is a name, not a use; object
                    // literal and pattern properties are code.
                    let member_name = f == F::Property && kind == K::Member;
                    // A tagged template's `quasi` is a whole template.
                    let text = f == F::Quasi && !s.is(c, K::Template);
                    if member_name || text || matches!(f, F::Key | F::Name | F::Label | F::Imported | F::Exported | F::Op | F::Kind | F::Modifier | F::Optional) {
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
        let Binder { s, scopes, refs, mut decls, owners, merged, members, mut functions, write_sites, merged_kinds, decl_scope, .. } = self;
        let mut out_refs = HashMap::with_capacity_and_hasher(refs.len(), Default::default());
        let mut unresolved = HashSet::default();
        let mut access = HashMap::with_capacity_and_hasher(refs.len(), Default::default());
        let mut ref_scope = HashMap::with_capacity_and_hasher(refs.len(), Default::default());
        for (ident, scope, use_) in refs {
            ref_scope.insert(ident, scope);
            let name = s.text(ident);
            access.insert(
                ident,
                match use_ {
                    Use::Value | Use::Namespace | Use::Export => "read",
                    Use::Write => "write",
                    Use::ReadWrite => "read_write",
                    Use::Type | Use::TypeOnly | Use::TypeExport => "type",
                },
            );
            let want = match use_ {
                Use::Type => TYPE,
                Use::Namespace | Use::Export | Use::TypeExport => VALUE | TYPE,
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
                at = scopes[sc].info.parent;
            }
            let Some(decl) = found.map(|d| merged.get(&d).copied().unwrap_or(d)) else {
                if !matches!(use_, Use::Type | Use::TypeOnly | Use::TypeExport) {
                    unresolved.insert(ident);
                    if name == "arguments" {
                        if let Some(f) = this_owner(s, ident) {
                            *functions.entry(f).or_default() |= USES_ARGUMENTS;
                        }
                    }
                }
                continue;
            };
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
                Use::Value | Use::Namespace | Use::Export => {
                    b.uses += 1;
                    b.value_uses += u32::from(use_ != Use::Namespace);
                }
                Use::Type | Use::TypeOnly | Use::TypeExport => b.uses += 1,
                Use::Write => b.writes += 1,
                Use::ReadWrite => {
                    b.uses += 1;
                    b.value_uses += 1;
                    b.writes += 1;
                }
            }
            out_refs.insert(ident, decl);
        }
        let declaration_kinds = merged.iter().map(|(&d, first)| (d, merged_kinds.get(first).cloned().unwrap_or_default())).chain(merged_kinds.clone()).collect();
        let scopes = scopes.into_iter().map(|sc| sc.info).collect();
        Bindings { decls, refs: out_refs, unresolved, access, write_sites, declaration_kinds, members, functions, scopes, decl_scope, ref_scope, merged, index: OnceCell::new() }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    #[test]
    fn names_resolve_at_any_node_through_enclosing_scopes() {
        let text = "let Object = 1; type T = 1; function f() { const T = 2; /*@*/a; { /*@*/b; } } /*@*/c;";
        crate::engine::tests::with_native_file(text, |file, marks| {
            let b = file.bindings();
            let s = &file.syntax;
            let name = |d: Option<u32>| d.map(|d| s.tree.node(d).start);
            let outer_object = Some(4);
            assert_eq!(name(b.visible(s, marks[0], "Object", true)), outer_object);
            assert_eq!(name(b.visible(s, marks[1], "T", true)), Some(49), "the inner const, not the outer type");
            assert_eq!(name(b.visible(s, marks[2], "T", true)), None, "only a type outside");
            assert_eq!(name(b.visible(s, marks[2], "T", false)), Some(21));
            assert_eq!(b.visible(s, marks[2], "Math", true), None);
        })
    }

    #[test]
    fn computed_class_keys_use_the_outer_this() {
        let l = native_linter("rule no-extra-bind");
        let lint = |text: &str| l.lint(Path::new("/root/a.js"), text).len();
        assert_eq!(lint("var f = function () { class A { [this.x]() {} } }.bind(b);"), 0);
        assert_eq!(lint("var f = function () { class A { [k] = this.x } }.bind(b);"), 1);
    }

    #[test]
    fn legacy_block_functions_resolve_in_the_enclosing_function_scope() {
        for (edition, undefined, early) in [(3, 0, 1), (5, 0, 1), (2015, 1, 0)] {
            let linter = native_linter(&format!(
                "language .js {{ set sourceType \"script\"\nset ecmaVersion {edition}\n}}\nrule no-undef\nrule no-use-before-define"));
            let diagnostics = linter.lint(Path::new("/root/a.js"), "'use strict'; a(); { function a() {} }");
            let count = |rule| diagnostics.iter().filter(|diagnostic| diagnostic.rule == rule).count();
            assert_eq!(count("no-undef"), undefined, "{edition}: {diagnostics:#?}");
            assert_eq!(count("no-use-before-define"), early, "{edition}: {diagnostics:#?}");
        }
    }

    #[test]
    fn legacy_block_function_bindings_merge_locally_without_escaping_functions() {
        let linter = native_linter(r#"language .js { set sourceType "script"
set ecmaVersion 5
set globalReturn true
}
rule no-undef
rule no-use-before-define
rule no-redeclare
rule no-implicit-globals
"#);
        let source = "function outer() { a(); { function a() {} } { function a() {} } a(); } a();";
        let diagnostics = linter.lint(Path::new("/root/a.js"), source);
        let count = |rule| diagnostics.iter().filter(|diagnostic| diagnostic.rule == rule).count();
        assert_eq!(count("no-undef"), 1, "{diagnostics:#?}");
        assert_eq!(count("no-use-before-define"), 1, "{diagnostics:#?}");
        assert_eq!(count("no-redeclare"), 1, "{diagnostics:#?}");
        assert_eq!(count("no-implicit-globals"), 0, "{diagnostics:#?}");
    }
}

//! Which class members are read, as typescript-eslint's class scope analyzer
//! (and eslint's no-unused-private-class-members) count it: through `this`
//! (rebound by functions, not arrows), `#name`, the class's own name,
//! `const self = this`, parameters typed as the class and `this: Foo`
//! parameters, and destructuring from `this`.

use std::cell::OnceCell;

use crate::fx_hash::FxHashMap as HashMap;

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;

/// Meaningful reads for unused-member policy; direct writes for type queries.
struct Member {
    /// What reports point at: the key, or a parameter property's name.
    name_node: u32,
    key: MemberName,
    /// `private` (TypeScript), or a `#name`.
    private: bool,
    hash: bool,
    accessor: bool,
    used: bool,
    reads: u32,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct MemberUses {
    pub reads: u32,
    /// Direct reassignments outside the constructor.
    pub writes: u32,
    pub ctor_writes: u32,
    /// Constructor reassignments through a receiver other than direct `this`.
    pub ctor_indirect_writes: u32,
}

struct ClassMembers {
    members: Vec<Member>,
    by_node: HashMap<u32, usize>,
    classes: Vec<Class>,
    class_at: HashMap<u32, usize>,
    writes: Vec<Write>,
    uses: OnceCell<Vec<MemberUses>>,
}

struct Class {
    name: Option<String>,
    instance: HashMap<MemberName, usize>,
    statics: HashMap<MemberName, usize>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum MemberName {
    Text(String),
    Computed(u32),
}

impl MemberName {
    fn declaration(s: &Syntax, node: u32) -> Option<Self> {
        let key = s.get(node, F::Key)?;
        key_name(s, key).map(Self::Text).or_else(|| s.is(key, K::Computed).then_some(Self::Computed(node)))
    }

    fn resolve(&self, file: &NativeFile) -> Option<MemberKey> {
        match self {
            Self::Text(name) => Some(MemberKey::Text(name.clone())),
            Self::Computed(node) => file.symbolic_property_key(*node).map(MemberKey::Symbol),
        }
    }
}

struct ResolvedMembers {
    instance: HashMap<MemberKey, usize>,
    statics: HashMap<MemberKey, usize>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum MemberKey {
    Text(String),
    Symbol(String),
}

impl MemberKey {
    fn access(file: &NativeFile, node: u32) -> Option<Self> {
        if let Some(symbol) = file.symbolic_property_key(node) { return Some(Self::Symbol(symbol)) }
        let s = &file.syntax;
        let property = s.get(node, F::Property).or_else(|| s.get(node, F::Index))?;
        if s.is(node, K::Member) { Some(Self::Text(s.text(property).to_string())) }
        else { key_name(s, property).or_else(|| super::statics::literal_text(s, s.unparen(property))).map(Self::Text) }
    }

    fn printed(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Text(name) => std::borrow::Cow::Borrowed(name),
            Self::Symbol(name) => std::borrow::Cow::Owned(format!("[Symbol.{name}]")),
        }
    }
}

/// A scope where `this` means something: a class (its instance side, in
/// field initializers), a method or static block (`this_static`), a
/// function (nothing, unless `this: Foo`).
#[derive(Clone, Copy)]
struct Scope {
    class: Option<usize>,
    this: Option<usize>,
    this_static: bool,
    constructor: Option<usize>,
}

struct Write {
    node: u32,
    scope: Scope,
}

struct Analyzer<'a, 't> {
    file: &'a NativeFile<'t>,
    s: &'a Syntax<'t>,
    classes: Vec<Class>,
    class_at: HashMap<u32, usize>,
    members: Vec<Member>,
    scopes: Vec<Scope>,
    writes: Vec<Write>,
}

/// Every member of every class, with its uses.
fn analyze(file: &NativeFile) -> ClassMembers {
    let s = &file.syntax;
    let mut a = Analyzer {
        file, s, classes: Vec::new(), class_at: HashMap::default(), members: Vec::new(),
        scopes: vec![Scope { class: None, this: None, this_static: false, constructor: None }],
        writes: Vec::new(),
    };
    for node in s.tree.order.iter().copied().filter(|&node| s.is(node, K::Class)) {
        let index = a.classes.len();
        a.classes.push(Class {
            name: s.get(node, F::Name).map(|name| s.text(name).to_string()),
            instance: HashMap::default(), statics: HashMap::default(),
        });
        a.class_at.insert(s.tree.node(node).start, index);
        a.collect_members(node, index);
    }
    a.visit(s.root());
    // Only what the maps still hold: a later declaration of a name (a
    // getter after its setter) replaces the earlier one.
    let by_node = a.classes.iter().flat_map(|class| class.instance.values().chain(class.statics.values()))
        .map(|&index| (a.members[index].name_node, index)).collect();
    ClassMembers { members: a.members, by_node, classes: a.classes, class_at: a.class_at, writes: a.writes, uses: OnceCell::new() }
}

/// A key's name and lookup key: `#x` for private names, the static name
/// otherwise (computed keys only for literals).
fn key_name(s: &Syntax, key: u32) -> Option<String> {
    match s.kind(key)? {
        K::Ident | K::PrivateName => Some(s.text(key).to_string()),
        K::Computed => {
            let e = s.unparen(s.get(key, F::Expr)?);
            match s.kind(e)? {
                K::Template if s.all(e, F::Expr).next().is_none() => {
                    let t = s.text(e);
                    Some(t[1..t.len().saturating_sub(1)].to_string())
                }
                _ => super::statics::literal_text(s, e),
            }
        }
        _ => super::statics::literal_text(s, key),
    }
}

impl Analyzer<'_, '_> {
    fn current(&self) -> Scope {
        *self.scopes.last().expect("root scope")
    }

    fn declare(&mut self, class: usize, name_node: u32, key: MemberName, private: bool, accessor: bool, is_static: bool) {
        let hash = matches!(&key, MemberName::Text(name) if name.starts_with('#'));
        let idx = self.members.len();
        self.members.push(Member { name_node, key: key.clone(), private, hash, accessor, used: false, reads: 0 });
        let c = &mut self.classes[class];
        let map = if is_static { &mut c.statics } else { &mut c.instance };
        // A later declaration of the name replaces the earlier one.
        map.insert(key, idx);
    }

    fn collect_members(&mut self, class_node: u32, class: usize) {
        let s = self.s;
        for m in s.all(class_node, F::Member).collect::<Vec<_>>() {
            let is_static = s.has_word(m, F::Modifier, "static");
            let private = s.has_word(m, F::Modifier, "private");
            match s.kind(m) {
                Some(K::Method) => {
                    let Some(key) = s.get(m, F::Key) else { continue };
                    if s.text(key) == "constructor" && s.is(key, K::Ident) {
                        for p in s.all(m, F::Param).collect::<Vec<_>>() {
                            let property = s.all(p, F::Modifier).any(|w| matches!(s.text(w), "private" | "protected" | "public" | "readonly" | "override"));
                            let Some(n) = s.get(p, F::Name).filter(|&n| property && s.is(n, K::Ident)) else { continue };
                            let private = s.has_word(p, F::Modifier, "private");
                            self.declare(class, n, MemberName::Text(s.text(n).to_string()), private, false, false);
                        }
                        continue;
                    }
                    let Some(name) = MemberName::declaration(s, m) else { continue };
                    let accessor = matches!(s.word(m, F::Kind), Some("get" | "set"));
                    self.declare(class, key, name, private, accessor, is_static);
                }
                Some(K::ClassProp) => {
                    let Some(key) = s.get(m, F::Key) else { continue };
                    let Some(name) = MemberName::declaration(s, m) else { continue };
                    let accessor = s.has_word(m, F::Modifier, "accessor");
                    self.declare(class, key, name, private, accessor, is_static);
                }
                _ => {}
            }
        }
    }

    fn visit(&mut self, node: u32) {
        let s = self.s;
        match s.kind(node) {
            Some(K::Class) => {
                let idx = self.class_at[&s.tree.node(node).start];
                self.scopes.push(Scope { class: Some(idx), this: Some(idx), this_static: false, constructor: None });
                self.visit_kids(node);
                self.scopes.pop();
                return;
            }
            Some(K::Method) => {
                let class = self.current().class.filter(|_| s.parent(node).is_some_and(|p| s.is(p, K::Class)));
                let scope = Scope {
                    class: None, this: class, this_static: s.has_word(node, F::Modifier, "static"),
                    constructor: class.filter(|_| s.get(node, F::Key).is_some_and(|key| s.is(key, K::Ident) && s.text(key) == "constructor")),
                };
                // The key is evaluated in the class.
                if let Some(k) = s.get(node, F::Key) {
                    self.visit(k);
                }
                self.scopes.push(scope);
                for (f, c) in s.kids(node).collect::<Vec<_>>() {
                    if f != Some(F::Key) {
                        self.visit(c);
                    }
                }
                self.scopes.pop();
                return;
            }
            Some(K::StaticBlock) => {
                let class = self.current().class;
                self.scopes.push(Scope { class: None, this: class, this_static: true, constructor: None });
                self.visit_kids(node);
                self.scopes.pop();
                return;
            }
            Some(K::Function) => {
                let cur = self.current();
                // A property's function value is a method.
                let as_method = s.parent(node).filter(|&p| s.is(p, K::ClassProp) && s.field(node) == Some(F::Init));
                let scope = match as_method {
                    Some(p) => Scope { class: None, this: cur.class, this_static: s.has_word(p, F::Modifier, "static"), constructor: None },
                    None => Scope { class: None, this: self.this_parameter(node), this_static: false, constructor: None },
                };
                self.scopes.push(scope);
                self.visit_kids(node);
                self.scopes.pop();
                return;
            }
            Some(K::Arrow) => {
                self.scopes.push(Scope { class: None, constructor: None, ..self.current() });
                self.visit_kids(node);
                self.scopes.pop();
                return;
            }
            Some(K::Member | K::Index) => {
                self.visit_kids(node);
                self.member_access(node);
                return;
            }
            Some(K::Assign) => {
                self.visit_kids(node);
                if s.word(node, F::Op) == Some("=") {
                    self.this_destructuring(s.get(node, F::Left), s.get(node, F::Right));
                }
                return;
            }
            Some(K::AssignPattern) => {
                self.visit_kids(node);
                self.this_destructuring(s.get(node, F::Left), s.get(node, F::Right));
                return;
            }
            Some(K::Declarator | K::Param) => {
                self.visit_kids(node);
                self.this_destructuring(s.get(node, F::Name), s.get(node, F::Init));
                return;
            }
            _ => {}
        }
        self.visit_kids(node);
    }

    fn visit_kids(&mut self, node: u32) {
        for (_, c) in self.s.kids(node).collect::<Vec<_>>() {
            self.visit(c);
        }
    }

    /// `function (this: Foo)`: `this` is an instance of class `Foo`.
    fn this_parameter(&self, func: u32) -> Option<usize> {
        let s = self.s;
        let first = s.all(func, F::Param).next()?;
        let name = s.get(first, F::Name)?;
        if s.text(name) != "this" {
            return None;
        }
        let t = s.get(first, F::Type).filter(|&t| s.is(t, K::TypeRef))?;
        let type_name = s.get(t, F::Name).filter(|&n| s.is(n, K::Ident))?;
        self.class_named(s.text(type_name))
    }

    /// The nearest class (in scope) with that name.
    fn class_named(&self, name: &str) -> Option<usize> {
        self.scopes.iter().rev().filter_map(|sc| sc.class).find(|&c| self.classes[c].name.as_deref() == Some(name))
    }

    /// `const { a } = this`: each key is a use of the member.
    fn this_destructuring(&mut self, pattern: Option<u32>, value: Option<u32>) {
        let s = self.s;
        let (Some(pattern), Some(value)) = (pattern, value) else { return };
        if !s.is(s.unparen(value), K::This) || !matches!(s.kind(pattern), Some(K::ObjectPattern | K::Object)) {
            return;
        }
        let cur = self.current();
        let Some(class) = cur.this else { return };
        for p in s.all(pattern, F::Property).collect::<Vec<_>>() {
            let Some(key) = s.get(p, F::Key).filter(|&k| s.is(k, K::Ident)) else { continue };
            let map = if cur.this_static { &self.classes[class].statics } else { &self.classes[class].instance };
            if let Some(&m) = map.get(&MemberName::Text(s.text(key).to_string())) {
                self.members[m].used = true;
                self.members[m].reads += 1;
            }
        }
    }

    /// `object.name`: a use of a member when `object` is known to be the
    /// class or one of its instances.
    fn member_access(&mut self, node: u32) {
        let s = self.s;
        if self.file.bindings().write_sites.contains_key(&node) {
            self.writes.push(Write { node, scope: self.current() });
        }
        let Some(prop) = s.get(node, F::Property).or_else(|| s.get(node, F::Index)) else { return };
        if s.is(prop, K::PrivateName) {
            let name = MemberName::Text(s.text(prop).to_string());
            // The nearest class (through every scope) declaring it.
            let found = self.scopes.iter().rev().filter_map(|sc| sc.this).find_map(|c| self.classes[c].instance.get(&name).or_else(|| self.classes[c].statics.get(&name)).copied());
            if let Some(m) = found {
                self.count(node, m);
            }
            return;
        }
        let name = if s.is(node, K::Member) { Some(s.text(prop).to_string()) }
            else { key_name(s, prop).or_else(|| super::statics::literal_text(s, s.unparen(prop))) };
        let Some(name) = name.map(MemberName::Text) else { return };
        if let Some((class, is_static)) = self.object_class(node) {
            let map = if is_static { &self.classes[class].statics } else { &self.classes[class].instance };
            if let Some(&m) = map.get(&name) {
                self.count(node, m);
            }
        }
    }

    /// What `object.x` reaches: `(class, static side)`.
    fn object_class(&self, node: u32) -> Option<(usize, bool)> {
        let s = self.s;
        let object = s.get(node, F::Object)?;
        let cur = self.current();
        match s.kind(object)? {
            K::This => cur.this.map(|c| (c, cur.this_static)),
            K::Ident => {
                if let Some(c) = self.class_named(s.text(object)) {
                    return Some((c, true));
                }
                let decl = *self.file.bindings().refs.get(&object)?;
                let parent = s.parent(decl)?;
                if s.is(parent, K::Declarator) && s.field(decl) == Some(F::Name) {
                    // `const self = this`, never reassigned.
                    let init = s.get(parent, F::Init).map(|i| s.unparen(i))?;
                    if s.is(init, K::This) && self.file.binding(decl).is_some_and(|b| b.writes == 0) {
                        return cur.this.map(|c| (c, cur.this_static));
                    }
                    return None;
                }
                // A parameter typed as the class (`Foo`, `typeof Foo`).
                let t = s.get(parent, F::Type)?;
                match s.kind(t)? {
                    K::TypeRef => {
                        let n = s.get(t, F::Name).filter(|&n| s.is(n, K::Ident))?;
                        self.class_named(s.text(n)).map(|c| (c, false))
                    }
                    K::TypeQuery => {
                        let n = s.get(t, F::Name).filter(|&n| s.is(n, K::Ident))?;
                        self.class_named(s.text(n)).map(|c| (c, true))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// eslint's `countReference`: a write-only use, or a read.
    fn count(&mut self, access: u32, member: usize) {
        let s = self.s;
        let up = |mut n: u32| {
            let mut p = s.parent(n);
            while let Some(x) = p.filter(|&x| s.is(x, K::Paren)) {
                n = x;
                p = s.parent(x);
            }
            (n, p)
        };
        let (node, parent) = up(access);
        let write = match parent.map(|p| (p, s.kind(p))) {
            Some((p, Some(K::Assign))) if s.field(node) == Some(F::Left) => s.word(p, F::Op) == Some("=") || s.parent(p).is_some_and(|g| s.is(g, K::ExprStmt)),
            Some((_, Some(K::ForIn | K::ForOf | K::AssignPattern))) => s.field(node) == Some(F::Left),
            Some((p, Some(K::Update))) => s.parent(p).is_some_and(|g| s.is(g, K::ExprStmt)),
            // `({ x: this.#a } = o)`, `[this.#a] = o`, `[...this.#a] = o`.
            Some((p, Some(K::Prop))) => s.field(node) == Some(F::Value) && s.parent(p).is_some_and(|o| self.file.write_site(o).is_some()),
            Some((p, Some(K::Array))) => self.file.write_site(p).is_some(),
            Some((p, Some(K::Spread))) => s.parent(p).is_some_and(|o| self.file.write_site(o).is_some()),
            Some((_, Some(K::ArrayPattern | K::Rest))) => true,
            _ => false,
        };
        if write {
            self.members[member].used |= self.members[member].accessor;
        } else {
            self.members[member].used = true;
            self.members[member].reads += 1;
        }
    }
}

impl ClassMembers {
    fn uses(&self, file: &NativeFile) -> &[MemberUses] {
        self.uses.get_or_init(|| {
            let resolve = |side: &HashMap<MemberName, usize>| {
                let mut indices: Vec<_> = side.values().copied().collect();
                indices.sort_unstable();
                indices.into_iter().filter_map(|index|
                    self.members[index].key.resolve(file).map(|key| (key, index))).collect()
            };
            let classes: Vec<_> = self.classes.iter().map(|class| ResolvedMembers {
                instance: resolve(&class.instance), statics: resolve(&class.statics),
            }).collect();
            let mut uses: Vec<_> = self.members.iter().map(|member|
                MemberUses { reads: member.reads, ..MemberUses::default() }).collect();
            let s = &file.syntax;
            for write in &self.writes {
                let Some(name) = MemberKey::access(file, write.node) else { continue };
                let Some(object) = s.get(write.node, F::Object).map(|object| s.unparen(object)) else { continue };
                let mut targets = Vec::new();
                if s.is(object, K::This) {
                    if let Some(class) = write.scope.this {
                        targets.push((class, write.scope.this_static));
                    }
                } else {
                    let checker = &file.typing().checker;
                    let receiver = file.constrained_type_of(object);
                    for declaration in checker.member_resolution(&receiver, &name.printed()).declarations {
                        let Some(owner) = declaration.owner.filter(|owner| owner.file == checker.file) else { continue };
                        let Some(summary) = checker.summary_of(owner.file) else { continue };
                        let Some(&class) = self.class_at.get(&summary.decls[owner.decl as usize].start) else { continue };
                        let target = (class, declaration.member.is_static);
                        if !targets.contains(&target) { targets.push(target); }
                    }
                }
                for (class, is_static) in targets {
                    let side = if is_static { &classes[class].statics } else { &classes[class].instance };
                    let Some(&member) = side.get(&name) else { continue };
                    let constructor = !is_static && write.scope.constructor == Some(class);
                    if constructor {
                        uses[member].ctor_writes += 1;
                        if !s.is(object, K::This) { uses[member].ctor_indirect_writes += 1; }
                    } else { uses[member].writes += 1; }
                }
            }
            uses
        })
    }
}

impl NativeFile<'_> {
    /// A destructuring target (or written node): what writes it.
    fn write_site(&self, node: u32) -> Option<u32> {
        self.bindings().write_sites.get(&node).copied()
    }
}

pub fn member_uses(file: &NativeFile, node: u32) -> Option<MemberUses> {
    let s = &file.syntax;
    let declaration = if s.is(node, K::Ident) && s.parent(node).is_some_and(|parent| s.is(parent, K::Param)) {
        s.parent(node)?
    } else { node };
    let name = match s.kind(declaration)? {
        K::ClassProp | K::Method => s.get(declaration, F::Key)?,
        K::Param => s.get(declaration, F::Name)?,
        _ => return None,
    };
    let members = file.cached(analyze);
    Some(members.by_node.get(&name).map_or(MemberUses::default(), |&index| members.uses(file)[index]))
}

pub fn unused_member(file: &NativeFile, node: u32) -> Option<&'static str> {
    let members = file.cached(analyze);
    let member = &members.members[*members.by_node.get(&node)?];
    if member.used || matches!(member.key, MemberName::Computed(_)) { return None }
    Some(if member.hash { "hash" } else if member.private { "private" } else { "public" })
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use crate::native::ts::facts::Facts;
    use crate::native::ts::kinds::Field;

    #[test]
    fn member_writes_track_reassignment_of_the_resolved_receiver() {
        for (source, writes, constructor_writes, indirect_writes) in [
            ("class A { /*@*/private value = 0; change() { this['value'] = 1; } }", 1, 0, 0),
            ("class A { /*@*/private value = { inner: 0 }; change() { this.value.inner = 1; } }", 0, 0, 0),
            ("class A { /*@*/private value = 0; change(other: B) { other.value = 1; } } class B { value = 0; }", 0, 0, 0),
            ("class A { /*@*/private static value = 0; change() { A.value = 1; } }", 1, 0, 0),
            ("class A { /*@*/private value = 0; constructor() { this['value'] = 1; } }", 0, 1, 0),
            ("class A { /*@*/private value = 0; constructor() { (this).value = 1; } }", 0, 1, 0),
            ("class A { /*@*/private value = 0; constructor() { const change = () => { this.value = 1; }; } }", 1, 0, 0),
            ("class A { /*@*/private value = 0; constructor() { this.value = 1; this.value += 1; } }", 0, 2, 0),
            ("class A { /*@*/private value: number; constructor() { const that = {} as this & { extra: string }; that.value = 1; } }", 0, 1, 1),
            ("class A { /*@*/private value: number; constructor() { const that = this; that.value = 1; } }", 0, 1, 1),
        ] {
            with_native_file(source, |file, nodes| {
                let uses = file.member_uses(nodes[0]).expect("class property");
                assert_eq!((uses.writes, uses.ctor_writes, uses.ctor_indirect_writes), (writes, constructor_writes, indirect_writes), "{source}");
            });
        }
    }

    #[test]
    fn meaningful_member_reads_stay_distinct_from_reassignment() {
        for (source, unused, writes) in [
            ("class A { /*@*/#value = 0; change() { this.#value = 1; } }", true, 1),
            ("class A { /*@*/#value = 0; change() { this.#value++; } }", true, 1),
            ("class A { /*@*/#value = 0; change() { return this.#value++; } }", false, 1),
            ("class A { /*@*/#value = { inner: 0 }; change() { this.#value.inner = 1; } }", false, 0),
        ] {
            with_native_file(source, |file, nodes| {
                let key = file.syntax.get(nodes[0], Field::Key).expect("member key");
                assert_eq!(file.unused_member(key).is_some(), unused, "{source}");
                assert_eq!(file.member_uses(nodes[0]).expect("class property").writes, writes, "{source}");
            });
        }
    }
}

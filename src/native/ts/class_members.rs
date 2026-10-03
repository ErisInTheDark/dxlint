//! Which class members are read, as typescript-eslint's class scope analyzer
//! (and eslint's no-unused-private-class-members) count it: through `this`
//! (rebound by functions, not arrows), `#name`, the class's own name,
//! `const self = this`, parameters typed as the class and `this: Foo`
//! parameters, and destructuring from `this`.

use std::collections::HashMap;

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;

/// A declared member and how it is used.
#[derive(Debug, Clone)]
pub struct Member {
    /// What reports point at: the key, or a parameter property's name.
    pub name_node: u32,
    /// As written: `#x`, `x`.
    pub name: String,
    /// `private` (TypeScript), or a `#name`.
    pub private: bool,
    pub hash: bool,
    accessor: bool,
    reads: u32,
    writes: u32,
}

impl Member {
    pub fn used(&self) -> bool {
        self.reads > 0 || self.writes > 0 && self.accessor
    }
}

struct Class {
    name: Option<String>,
    instance: HashMap<String, usize>,
    statics: HashMap<String, usize>,
}

/// A scope where `this` means something: a class (its instance side, in
/// field initializers), a method or static block (`this_static`), a
/// function (nothing, unless `this: Foo`).
#[derive(Clone, Copy)]
struct Scope {
    class: Option<usize>,
    this: Option<usize>,
    this_static: bool,
}

struct Analyzer<'a, 't> {
    file: &'a NativeFile<'t>,
    s: &'a Syntax<'t>,
    classes: Vec<Class>,
    members: Vec<Member>,
    scopes: Vec<Scope>,
}

/// Every member of every class, with its uses.
pub fn analyze(file: &NativeFile) -> Vec<Member> {
    let s = &file.syntax;
    let mut a = Analyzer { file, s, classes: Vec::new(), members: Vec::new(), scopes: vec![Scope { class: None, this: None, this_static: false }] };
    a.visit(s.root());
    // Only what the maps still hold: a later declaration of a name (a
    // getter after its setter) replaces the earlier one.
    let live: std::collections::HashSet<usize> = a.classes.iter().flat_map(|c| c.instance.values().chain(c.statics.values()).copied()).collect();
    a.members.into_iter().enumerate().filter(|(i, _)| live.contains(i)).map(|(_, m)| m).collect()
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

    fn declare(&mut self, class: usize, name_node: u32, name: String, private: bool, accessor: bool, is_static: bool) {
        let hash = name.starts_with('#');
        let idx = self.members.len();
        self.members.push(Member { name_node, name: name.clone(), private, hash, accessor, reads: 0, writes: 0 });
        let c = &mut self.classes[class];
        let map = if is_static { &mut c.statics } else { &mut c.instance };
        // A later declaration of the name replaces the earlier one.
        map.insert(name, idx);
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
                            self.declare(class, n, s.text(n).to_string(), private, false, false);
                        }
                        continue;
                    }
                    let Some(name) = key_name(s, key) else { continue };
                    let accessor = matches!(s.word(m, F::Kind), Some("get" | "set"));
                    self.declare(class, key, name, private, accessor, is_static);
                }
                Some(K::ClassProp) => {
                    let Some(key) = s.get(m, F::Key) else { continue };
                    let Some(name) = key_name(s, key) else { continue };
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
                let idx = self.classes.len();
                let name = s.get(node, F::Name).map(|n| s.text(n).to_string());
                self.classes.push(Class { name, instance: HashMap::new(), statics: HashMap::new() });
                self.collect_members(node, idx);
                self.scopes.push(Scope { class: Some(idx), this: Some(idx), this_static: false });
                self.visit_kids(node);
                self.scopes.pop();
                return;
            }
            Some(K::Method) => {
                let class = self.current().class.filter(|_| s.parent(node).is_some_and(|p| s.is(p, K::Class)));
                let scope = Scope { class: None, this: class, this_static: s.has_word(node, F::Modifier, "static") };
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
                self.scopes.push(Scope { class: None, this: class, this_static: true });
                self.visit_kids(node);
                self.scopes.pop();
                return;
            }
            Some(K::Function) => {
                let cur = self.current();
                // A property's function value is a method.
                let as_method = s.parent(node).filter(|&p| s.is(p, K::ClassProp) && s.field(node) == Some(F::Init));
                let scope = match as_method {
                    Some(p) => Scope { class: None, this: cur.class, this_static: s.has_word(p, F::Modifier, "static") },
                    None => Scope { class: None, this: self.this_parameter(node), this_static: false },
                };
                self.scopes.push(scope);
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
            if let Some(&m) = map.get(s.text(key)) {
                self.members[m].reads += 1;
            }
        }
    }

    /// `object.name`: a use of a member when `object` is known to be the
    /// class or one of its instances.
    fn member_access(&mut self, node: u32) {
        let s = self.s;
        let Some(prop) = s.get(node, F::Property).or_else(|| s.get(node, F::Index)) else { return };
        if s.is(prop, K::PrivateName) {
            let name = s.text(prop);
            // The nearest class (through every scope) declaring it.
            let found = self.scopes.iter().rev().filter_map(|sc| sc.this).find_map(|c| self.classes[c].instance.get(name).or_else(|| self.classes[c].statics.get(name)).copied());
            if let Some(m) = found {
                self.count(node, m);
            }
            return;
        }
        let name = if s.is(node, K::Member) { Some(s.text(prop).to_string()) } else { key_name(s, prop).or_else(|| super::statics::literal_text(s, s.unparen(prop))) };
        let Some(name) = name else { return };
        let Some((class, is_static)) = self.object_class(node) else { return };
        let map = if is_static { &self.classes[class].statics } else { &self.classes[class].instance };
        if let Some(&m) = map.get(&name) {
            self.count(node, m);
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
            self.members[member].writes += 1;
        } else {
            self.members[member].reads += 1;
        }
    }
}

impl NativeFile<'_> {
    /// A destructuring target (or written node): what writes it.
    fn write_site(&self, node: u32) -> Option<u32> {
        self.bindings().write_sites.get(&node).copied()
    }

    /// Members nothing reads, by their name node: `(name, private, hash)`.
    pub fn unused_members(&self) -> &HashMap<u32, (String, bool, bool)> {
        self.unused_members.get_or_init(|| analyze(self).into_iter().filter(|m| !m.used()).map(|m| (m.name_node, (m.name, m.private, m.hash))).collect())
    }
}

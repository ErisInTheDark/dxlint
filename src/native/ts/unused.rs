//! no-unused-vars as eslint (`.js`) and typescript-eslint (`.ts`) decide
//! it: for every variable, whether it counts as used, where the rule points
//! and what its options look at. The rule (`std/ts/scopes`) applies the
//! options.

use std::collections::HashSet;

use crate::expr::Value;
use crate::fx_hash::FxHashMap as HashMap;

use super::bind::Bindings;
use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;

/// One variable, on its first declaring identifier.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Var {
    /// Used, as the file's dialect counts it (typescript-eslint also counts
    /// exported and implicitly used variables).
    pub used: bool,
    /// Exported (`export const`): never reported, yet not used (eslint).
    pub exported: bool,
    /// Never reported: a typescript-eslint import used only as a type.
    pub silent: bool,
    /// Which ignore pattern applies: `variable`, `parameter` or `catch`.
    pub def: &'static str,
    /// Its declaration is an array pattern element.
    pub array_element: bool,
    /// Some write to it is an array destructuring target (with
    /// `array_element`: `destructuredArrayIgnorePattern` applies).
    pub array_target: bool,
    /// eslint skips setter parameters.
    pub setter: bool,
    /// A plain parameter followed by one that is referenced ("after-used"
    /// keeps it).
    pub before_used_param: bool,
    pub rest_sibling: bool,
    /// A class with a `static {}` block.
    pub static_block: bool,
    pub using: bool,
    /// In the global scope of a script (`vars: "local"` skips it).
    pub global: bool,
    /// Written somewhere (`assigned a value` rather than `defined`).
    pub assigned: bool,
    /// Only used in types (typescript-eslint's "only used as a type").
    pub only_type: bool,
    /// Where an unused report points: the last write in its own function,
    /// else its declaration.
    pub offset: u32,
    /// Where a "marked as ignored but is used" report points.
    pub ignored_offset: u32,
}

pub struct Unused {
    pub vars: HashMap<u32, Var>,
}

/// One reference: a real one, or the write a declaration's initializer or
/// default makes (eslint-scope's implicit write references).
#[derive(Clone, Copy)]
struct Ref {
    id: u32,
    read: bool,
    write: bool,
    /// A type position reference (typescript-eslint).
    typ: bool,
}

pub fn analyze(file: &NativeFile) -> Unused {
    let s = &file.syntax;
    let b = file.bindings();
    let ext = file.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let ts = [".ts", ".tsx", ".mts", ".cts"].iter().any(|e| ext.ends_with(e));
    let dts = ts && (ext.contains(".d.") || ext.ends_with(".d.ts"));
    let script = file.source_type() == "script";
    let a = Analysis { s, b, ts };

    let idents = b.declarations(s);
    let exported_comment = if script { exported_names(file) } else { HashSet::new() };
    let marked = if ts { a.marked_used(idents, dts) } else { HashSet::new() };

    // typescript-eslint's default JSX pragma: JSX uses `React`.
    let jsx = ts && (0..s.tree.nodes.len() as u32).any(|n| matches!(s.kind(n), Some(K::JsxElement | K::JsxFragment)));

    let mut vars = HashMap::default();
    for (&d, binding) in &b.decls {
        // Function and class expression names; `namespace a.b` (no
        // variable for typescript-eslint).
        if matches!(binding.kind, "function_name" | "class_name") || s.parent(d).is_some_and(|p| s.is(p, K::QualifiedName)) {
            continue;
        }
        let ids = &idents[&d];
        let mut refs: Vec<Ref> = b.references_of(s, d).iter().map(|&r| {
            let access = b.access.get(&r).copied().unwrap_or("read");
            Ref {
                id: r,
                read: matches!(access, "read" | "read_write" | "type"),
                write: matches!(access, "write" | "read_write"),
                typ: access == "type",
            }
        }).collect();
        let real_count = refs.len();
        refs.extend(ids.iter().filter(|&&i| a.initialized(i)).map(|&i| Ref { id: i, read: false, write: true, typ: false }));
        if refs.len() != real_count {
            refs.sort_by_key(|r| a.start(r.id));
        }
        let decl_nodes = ids.iter().map(|&i| a.decl_node(i));
        let type_decl = |n: u32| matches!(s.kind(n), Some(K::Interface | K::TypeAlias));
        let has_value = decl_nodes.clone().any(|n| !type_decl(n));
        let has_variable = decl_nodes.clone().any(|n| s.is(n, K::Var));
        // A type and a value of one name: exporting the type does not
        // export the value (typescript-eslint).
        let ts_exported = decl_nodes.clone().any(|n| !(has_value && type_decl(n)) && s.parent(n).is_some_and(|p| s.is(p, K::Export)));
        let pragma = jsx && s.text(d) == "React" && b.decl_scope.get(&d) == Some(&0);
        // A class's own references are its inner name's (eslint-scope's
        // class scope), not the declaration's.
        let class = ids.iter().filter_map(|&i| s.parent(i).filter(|&p| s.is(p, K::Class) && s.field(i) == Some(F::Name))).collect::<Vec<_>>();
        let own = refs.iter().copied().filter(|r| !class.iter().any(|&c| a.inside(r.id, c)));
        let global = script && b.decl_scope.get(&d) == Some(&0);
        let marked_used = marked.contains(&d) || pragma || (global && exported_comment.contains(s.text(d)));
        let only_type = ts && own.clone().any(|r| a.type_only(has_variable, &r));
        let imported_as_type = ids.iter().all(|&i| a.type_import(i));
        let used = marked_used || (ts && ts_exported) || a.is_used(ids, has_variable, own.clone(), imported_as_type);
        let scope = a.variable_scope(d);
        let last_write = own.clone().rev().find(|r| r.write && a.variable_scope(r.id) == scope).map(|r| a.start(r.id));
        let offset = last_write.unwrap_or(a.start(d));
        let def = match binding.kind {
            "parameter" => "parameter",
            "catch" => "catch",
            _ => "variable",
        };
        vars.insert(
            d,
            Var {
                used,
                exported: !ts && binding.exported,
                silent: only_type && binding.kind == "import",
                def,
                array_element: s.parent(d).is_some_and(|p| s.is(p, K::ArrayPattern)),
                array_target: own.clone().any(|r| r.write && s.parent(r.id).is_some_and(|p| s.is(p, K::Array))),
                setter: def == "parameter" && a.function_of(d).is_some_and(|f| s.word(f, F::Kind) == Some("set")),
                before_used_param: def == "parameter" && a.before_used_param(d, &marked),
                rest_sibling: a.rest_sibling(d) || own.clone().any(|r| a.rest_sibling(r.id)),
                static_block: binding.kind == "class" && class.iter().any(|&c| s.all(c, F::Member).any(|m| s.is(m, K::StaticBlock))),
                using: binding.kind == "using",
                global,
                assigned: own.clone().any(|r| r.write),
                only_type,
                offset,
                ignored_offset: if ts { offset } else { a.start(d) },
            },
        );
    }
    Unused { vars }
}

/// Fact `unused_var`: on a variable's first declaring identifier.
pub fn var_fact(file: &NativeFile, node: u32) -> Value<'static> {
    let u = file.cached(analyze);
    let Some(v) = u.vars.get(&node) else { return Value::Null };
    let b = Value::Bool;
    Value::record(vec![
        ("used".into(), b(v.used)),
        ("exported".into(), b(v.exported)),
        ("silent".into(), b(v.silent)),
        ("def".into(), Value::from(v.def)),
        ("array_element".into(), b(v.array_element)),
        ("array_target".into(), b(v.array_target)),
        ("setter".into(), b(v.setter)),
        ("before_used_param".into(), b(v.before_used_param)),
        ("rest_sibling".into(), b(v.rest_sibling)),
        ("static_block".into(), b(v.static_block)),
        ("using".into(), b(v.using)),
        ("global".into(), b(v.global)),
        ("action".into(), Value::from(if v.assigned { "assigned a value" } else { "defined" })),
        ("only_type".into(), b(v.only_type)),
        ("offset".into(), Value::from(v.offset as usize)),
        ("ignored_offset".into(), Value::from(v.ignored_offset as usize)),
    ])
}

/// Fact `unused_globals`: on the program.
pub fn globals_fact(file: &NativeFile, node: u32) -> Value<'static> {
    if node != file.syntax.root() {
        return Value::Null;
    }
    Value::list(unused_globals(file).into_iter().map(|(name, at)| Value::record(vec![("name".into(), Value::from(name)), ("offset".into(), Value::from(at as usize))])).collect())
}

/// Names of `/* exported a, b */` comments.
pub(super) fn exported_names(file: &NativeFile) -> HashSet<String> {
    let src = file.syntax.src;
    let mut out = HashSet::new();
    for c in file.comments().iter().filter(|c| c.block) {
        let body = c.body(src).trim_start();
        if let Some(rest) = body.strip_prefix("exported").filter(|r| r.starts_with(char::is_whitespace)) {
            out.extend(rest.split(|ch: char| ch.is_whitespace() || ch == ',').filter(|w| !w.is_empty()).map(str::to_string));
        }
    }
    out
}

/// `/* global */` names nothing reads (and, in a script, nothing at the
/// top level declares).
fn unused_globals(file: &NativeFile) -> Vec<(String, u32)> {
    let s = &file.syntax;
    let globals = file.globals();
    if globals.mentions.is_empty() {
        return Vec::new();
    }
    let b = file.bindings();
    let script = file.source_type() == "script";
    let read: HashSet<&str> = b.unresolved.iter().filter(|r| b.access.get(r).is_some_and(|&a| a != "write")).map(|&r| s.text(r)).collect();
    let top: HashSet<&str> = if script { b.decl_scope.iter().filter(|(_, &sc)| sc == 0).map(|(&d, _)| s.text(d)).collect() } else { HashSet::new() };
    globals
        .mentions
        .iter()
        .filter(|(name, _)| globals.setting(name) != Some("off") && !read.contains(name.as_str()) && !top.contains(name.as_str()))
        .cloned()
        .collect()
}

struct Analysis<'a, 't> {
    s: &'a Syntax<'t>,
    b: &'a Bindings,
    ts: bool,
}

impl Analysis<'_, '_> {
    fn start(&self, id: u32) -> u32 {
        self.s.tree.node(id).start
    }

    fn inside(&self, inner: u32, outer: u32) -> bool {
        let (i, o) = (self.s.tree.node(inner), self.s.tree.node(outer));
        o.start <= i.start && i.end <= o.end
    }

    /// The ESTree parent: parentheses are not nodes there.
    fn up(&self, id: u32) -> Option<u32> {
        let mut p = self.s.parent(id)?;
        while self.s.is(p, K::Paren) {
            p = self.s.parent(p)?;
        }
        Some(p)
    }

    /// Declaring identifiers that an initializer or default writes:
    /// `let a = 1`, `for (const a of b)`, `function f(a = 1)`, `[a = 1]`.
    fn initialized(&self, ident: u32) -> bool {
        let s = self.s;
        let mut n = ident;
        while let Some(p) = s.parent(n) {
            match s.kind(p) {
                Some(K::ArrayPattern | K::ObjectPattern | K::Rest) => {}
                Some(K::PatternProp) if s.field(n) == Some(F::Key) => {
                    if s.get(p, F::Value).is_some_and(|v| s.is(v, K::AssignPattern)) {
                        return true;
                    }
                }
                Some(K::PatternProp) => {}
                Some(K::AssignPattern) => return s.field(n) == Some(F::Left),
                Some(K::Declarator) => {
                    let in_loop_head = s.parent(p).filter(|&v| s.field(v) == Some(F::Left)).and_then(|v| s.parent(v)).is_some_and(|l| matches!(s.kind(l), Some(K::ForIn | K::ForOf)));
                    return s.field(n) == Some(F::Name) && (s.get(p, F::Init).is_some() || in_loop_head);
                }
                Some(K::Param) => return s.field(n) == Some(F::Name) && s.get(p, F::Init).is_some(),
                _ => return false,
            }
            n = p;
        }
        false
    }

    fn variable_scope(&self, id: u32) -> u32 {
        variable_scope(self.s, id)
    }

    fn is_function(&self, id: u32) -> bool {
        matches!(self.s.kind(id), Some(K::Function | K::Arrow | K::Method))
    }

    fn in_loop(&self, id: u32) -> bool {
        let mut at = self.s.parent(id);
        while let Some(n) = at {
            if self.is_function(n) {
                return false;
            }
            if matches!(self.s.kind(n), Some(K::For | K::ForIn | K::ForOf | K::While | K::DoWhile)) {
                return true;
            }
            at = self.s.parent(n);
        }
        false
    }

    /// The function a parameter's identifier belongs to.
    fn function_of(&self, ident: u32) -> Option<u32> {
        let mut at = self.s.parent(ident);
        while let Some(n) = at {
            if self.s.is(n, K::Param) {
                return self.s.parent(n);
            }
            at = self.s.parent(n);
        }
        None
    }

    /// The expression's value is thrown away: a statement, or a sequence
    /// part other than a used last one.
    fn unused_expression(&self, id: u32) -> bool {
        let Some(p) = self.up(id) else { return false };
        match self.s.kind(p) {
            Some(K::ExprStmt) => true,
            Some(K::Sequence) => {
                let last = self.s.all(p, F::Expr).last().map(|l| self.s.unparen(l));
                last != Some(id) || self.unused_expression(p)
            }
            _ => false,
        }
    }

    /// `a = <rhs>` as a statement, in the variable's own function and not
    /// in a loop: the RHS, where reads of `a` only feed `a` itself.
    fn rhs(&self, r: &Ref, var_scope: u32, prev: Option<u32>) -> Option<u32> {
        if let Some(prev) = prev.filter(|&p| self.inside(r.id, p)) {
            return Some(prev);
        }
        let s = self.s;
        let p = self.up(r.id)?;
        let later = self.variable_scope(r.id) != var_scope || self.in_loop(r.id);
        (s.is(p, K::Assign) && s.get(p, F::Left).map(|l| s.unparen(l)) == Some(r.id) && self.unused_expression(p) && !later).then(|| s.get(p, F::Right)).flatten()
    }

    /// A read that only updates the variable: `a++`, `a += 1` as
    /// statements, `a` in the RHS of `a = a + 1`.
    fn read_for_itself(&self, r: &Ref, rhs: Option<u32>) -> bool {
        let s = self.s;
        if !r.read {
            return false;
        }
        let Some(p) = self.up(r.id) else { return false };
        let self_update = match s.kind(p) {
            Some(K::Assign) => s.get(p, F::Left).map(|l| s.unparen(l)) == Some(r.id) && !matches!(s.word(p, F::Op), Some("&&=" | "||=" | "??=")) && self.unused_expression(p),
            Some(K::Update) => self.unused_expression(p),
            _ => false,
        };
        self_update || rhs.is_some_and(|rhs| self.inside(r.id, rhs) && !self.inside_storable_function(r.id, rhs))
    }

    /// Inside a function in the RHS that is kept for later (stored,
    /// passed): its reads happen when it is called.
    fn inside_storable_function(&self, id: u32, rhs: u32) -> bool {
        let s = self.s;
        let mut at = s.parent(id);
        let func = loop {
            match at {
                Some(n) if self.is_function(n) => break n,
                Some(n) => at = s.parent(n),
                None => return false,
            }
        };
        if !self.inside(func, rhs) {
            return false;
        }
        let mut node = func;
        let mut parent = self.up(func);
        while let Some(p) = parent.filter(|&p| self.inside(p, rhs)) {
            match s.kind(p) {
                Some(K::Sequence) => {
                    if s.all(p, F::Expr).last().map(|l| s.unparen(l)) != Some(node) {
                        return false;
                    }
                }
                Some(K::Call | K::New) => return s.get(p, F::Callee).map(|c| s.unparen(c)) != Some(node),
                Some(K::Assign | K::TaggedTemplate | K::Yield) => return true,
                Some(k) if is_statement(s, p, k) => return true,
                _ => {}
            }
            node = p;
            parent = self.up(p);
        }
        false
    }

    /// eslint's exception: `for (x in o) return;` uses `x`.
    fn for_in_return(&self, r: &Ref) -> bool {
        let s = self.s;
        let Some(mut target) = self.up(r.id) else { return false };
        if s.is(target, K::Declarator) {
            match s.parent(target).and_then(|v| s.parent(v)) {
                Some(l) => target = l,
                None => return false,
            }
        }
        if !matches!(s.kind(target), Some(K::ForIn | K::ForOf)) {
            return false;
        }
        let body = s.get(target, F::Body);
        let first = match body {
            Some(b) if s.is(b, K::Block) => s.all(b, F::Body).next(),
            other => other,
        };
        first.is_some_and(|f| s.is(f, K::Return))
    }

    /// typescript-eslint: a reference that only matters to types (in
    /// `typeof x` in a type; any type use of a `var`/`let`/`const`).
    fn type_only(&self, has_variable: bool, r: &Ref) -> bool {
        let predicate = self.s.parent(r.id).is_some_and(|p| self.s.is(p, K::TypePredicate));
        r.typ && (has_variable || predicate || self.in_type_query(r.id))
    }

    /// The declaration a declaring identifier belongs to: the `var`/`let`
    /// statement, function, class, import, parameter...
    fn decl_node(&self, ident: u32) -> u32 {
        let s = self.s;
        let mut n = ident;
        while let Some(p) = s.parent(n) {
            match s.kind(p) {
                Some(K::ArrayPattern | K::ObjectPattern | K::PatternProp | K::AssignPattern | K::Rest) => n = p,
                Some(K::Declarator) => return s.parent(p).unwrap_or(p),
                _ => return p,
            }
        }
        n
    }

    fn in_type_query(&self, id: u32) -> bool {
        let s = self.s;
        let mut n = id;
        while let Some(p) = s.parent(n) {
            match s.kind(p) {
                Some(K::QualifiedName | K::Member) => n = p,
                Some(K::TypeQuery) => return true,
                _ => return false,
            }
        }
        false
    }

    /// `import type { a }`, `import { type a }`.
    fn type_import(&self, ident: u32) -> bool {
        let s = self.s;
        let Some(p) = s.parent(ident) else { return false };
        let import = if s.is(p, K::ImportSpec) {
            if s.word(p, F::Kind) == Some("type") {
                return true;
            }
            s.parent(p)
        } else {
            Some(p)
        };
        import.is_some_and(|i| s.is(i, K::Import) && s.word(i, F::Kind) == Some("type"))
    }

    /// Nodes inside which references to the variable are its own: a
    /// function's body, and (typescript-eslint) a type, namespace or enum
    /// declaration's.
    fn own_nodes(&self, ids: &[u32]) -> Vec<u32> {
        let s = self.s;
        let mut out = Vec::new();
        for &i in ids {
            let Some(p) = s.parent(i) else { continue };
            match s.kind(p) {
                Some(K::Function) if s.field(i) == Some(F::Name) => out.push(p),
                Some(K::Declarator) if s.field(i) == Some(F::Name) => {
                    if let Some(init) = s.get(p, F::Init).map(|n| s.unparen(n)).filter(|&n| matches!(s.kind(n), Some(K::Function | K::Arrow))) {
                        out.push(init);
                    }
                }
                Some(K::Interface | K::TypeAlias | K::Namespace | K::Enum) if self.ts => out.push(p),
                _ => {}
            }
        }
        out
    }

    fn is_used(&self, ids: &[u32], has_variable: bool, mut refs: impl Iterator<Item = Ref>, imported_as_type: bool) -> bool {
        let own = self.own_nodes(ids);
        let var_scope = self.variable_scope(ids[0]);
        let mut rhs = None;
        refs.any(|r| {
            if !self.ts && self.for_in_return(&r) {
                return true;
            }
            let for_itself = self.read_for_itself(&r, rhs);
            rhs = self.rhs(&r, var_scope, rhs);
            let type_only = self.ts && !imported_as_type && self.type_only(has_variable, &r);
            let own_ref = own.iter().any(|&o| self.inside(r.id, o));
            r.read && !for_itself && !type_only && !own_ref
        })
    }

    /// A plain parameter (`function f(a)`: no pattern, default or rest)
    /// with a later parameter of the same function that is referenced.
    fn before_used_param(&self, d: u32, marked: &HashSet<u32>) -> bool {
        let s = self.s;
        let Some(param) = s.parent(d).filter(|&p| s.is(p, K::Param) && s.field(d) == Some(F::Name)) else { return false };
        if s.get(param, F::Init).is_some() || s.get(param, F::Modifier).is_some() {
            return false;
        }
        let Some(func) = s.parent(param).filter(|&f| self.is_function(f)) else { return false };
        let mut names = Vec::new();
        for p in s.all(func, F::Param) {
            if let Some(n) = s.get(p, F::Name) {
                pattern_names(s, n, &mut names);
            }
        }
        let first = |i: u32| self.b.merged.get(&i).copied().unwrap_or(i);
        names.iter().skip_while(|&&n| n != d).skip(1).any(|&n| {
            let first = first(n);
            !self.b.references_of(s, first).is_empty()
                || self.b.declarations(s).get(&first).is_some_and(|ids| ids.iter().any(|&i| self.initialized(i)))
                || marked.contains(&first)
        })
    }

    /// `{ a, ...rest }`: a sibling of a rest property, in a pattern or a
    /// destructuring target.
    fn rest_sibling(&self, id: u32) -> bool {
        let s = self.s;
        let Some(p) = s.parent(id) else { return false };
        let defaulted = |prop: u32| s.get(prop, F::Value).is_some_and(|v| s.is(v, K::AssignPattern));
        match s.kind(p) {
            Some(K::PatternProp) if !(s.field(id) == Some(F::Key) && defaulted(p)) => s.parent(p).is_some_and(|o| s.get(o, F::Rest).is_some()),
            Some(K::Prop) if !(s.field(id) == Some(F::Key) && defaulted(p)) => {
                s.parent(p).filter(|&o| s.is(o, K::Object)).and_then(|o| s.all(o, F::Property).last()).is_some_and(|l| s.is(l, K::Spread))
            }
            _ => false,
        }
    }

    /// typescript-eslint's implicitly used variables: setter and signature
    /// parameters, parameter properties, mapped type keys, `declare global`,
    /// `for (x in o) return;`, and ambient declarations.
    fn marked_used(&self, idents: &HashMap<u32, Vec<u32>>, dts: bool) -> HashSet<u32> {
        let s = self.s;
        let mut out = HashSet::new();
        for (&d, ids) in idents {
            let kind = self.b.decls.get(&d).map_or("", |b| b.kind);
            let marked = ids.iter().any(|&i| match kind {
                "parameter" => self.function_of(i).is_some_and(|f| {
                    let sig = matches!(s.kind(f), Some(K::FnType | K::CallSig | K::MethodSig)) || (matches!(s.kind(f), Some(K::Function | K::Method)) && s.get(f, F::Body).is_none());
                    let property = s.parent(i).is_some_and(|p| s.is(p, K::Param) && s.get(p, F::Modifier).is_some());
                    sig || property || s.word(f, F::Kind) == Some("set")
                }),
                "type_parameter" => s.parent(i).and_then(|tp| s.parent(tp)).is_some_and(|m| s.is(m, K::Mapped)),
                "namespace" => s.text(i) == "global",
                _ => false,
            }) || ids.iter().any(|&i| self.ambient_export(i, dts));
            if marked {
                out.insert(d);
            }
        }
        for l in (0..s.tree.nodes.len() as u32).filter(|&n| matches!(s.kind(n), Some(K::ForIn | K::ForOf))) {
            let body = s.get(l, F::Body);
            let only = match body {
                Some(b) if s.is(b, K::Block) => {
                    let mut it = s.all(b, F::Body);
                    it.next().filter(|_| it.next().is_none())
                }
                other => other,
            };
            if !only.is_some_and(|o| s.is(o, K::Return)) {
                continue;
            }
            let Some(left) = s.get(l, F::Left) else { continue };
            let target = if s.is(left, K::Var) {
                s.get(left, F::Decl).and_then(|d| s.get(d, F::Name)).filter(|&n| s.is(n, K::Ident))
            } else {
                Some(s.unparen(left)).filter(|&n| s.is(n, K::Ident)).and_then(|n| self.b.refs.get(&n).copied())
            };
            if let Some(t) = target {
                out.insert(self.b.merged.get(&t).copied().unwrap_or(t));
            }
        }
        out
    }

    /// A declaration typescript-eslint treats as exported: at the top of a
    /// `.d.ts` file, or directly in a `declare namespace` (or a namespace
    /// inside one, or any namespace of a `.d.ts`), unless that body has an
    /// `export =` or `export default`.
    fn ambient_export(&self, ident: u32, dts: bool) -> bool {
        let s = self.s;
        let stmt = self.decl_node(ident);
        if !matches!(s.kind(stmt), Some(K::Interface | K::TypeAlias | K::Class | K::Function | K::Enum | K::Namespace | K::Var)) {
            return false;
        }
        let stmt = s.parent(stmt).filter(|&e| s.is(e, K::Export)).unwrap_or(stmt);
        let Some(container) = s.parent(stmt) else { return false };
        // `export =`, `export default x`, `export {}`, `export * from`: the
        // body says what it exports.
        let overriding = |body: u32| {
            s.all(body, F::Body).any(|st| {
                s.is(st, K::ExportAssign)
                    || s.is(st, K::Export)
                        && if s.has_word(st, F::Kind, "default") {
                            s.get(st, F::Expr).is_some_and(|e| s.is(s.unparen(e), K::Ident))
                        } else {
                            s.get(st, F::Decl).is_none()
                        }
            })
        };
        if s.is(container, K::Program) {
            return dts && !overriding(container);
        }
        let Some(ns) = s.parent(container).filter(|&n| s.is(n, K::Namespace)) else { return false };
        if overriding(container) {
            return false;
        }
        let declared = |n: u32| s.has_word(n, F::Modifier, "declare");
        if dts || declared(ns) {
            return true;
        }
        let mut at = s.parent(ns);
        while let Some(n) = at {
            if s.is(n, K::Namespace) && declared(n) {
                return true;
            }
            at = s.parent(n);
        }
        false
    }
}

/// Kinds eslint's `/(Statement|Declaration)$/` matches: statements, and
/// function and class declarations.
fn is_statement(s: &Syntax, id: u32, kind: K) -> bool {
    match kind {
        K::Function | K::Class => matches!(s.field(id), Some(F::Body | F::Decl)),
        K::ExprStmt | K::Var | K::If | K::For | K::ForIn | K::ForOf | K::While | K::DoWhile | K::Return | K::Throw | K::Break | K::Continue | K::Switch | K::Try | K::Block | K::Labeled | K::Empty | K::Debugger | K::With | K::Import | K::Export | K::Interface | K::TypeAlias | K::Enum | K::Namespace => true,
        _ => false,
    }
}

/// eslint's variable scope: the function (or static block, field
/// initializer, program) whose `var`s would hold the node; also where its
/// code path starts.
pub(super) fn variable_scope(s: &Syntax, id: u32) -> u32 {
    let mut child = id;
    let mut at = s.parent(id);
    while let Some(n) = at {
        match s.kind(n) {
            // A declaration's name belongs to the scope around it.
            Some(K::Function | K::Class) if s.field(child) == Some(F::Name) => {}
            Some(K::Function | K::Arrow | K::Method | K::StaticBlock | K::Program) => return n,
            Some(K::ClassProp) if s.field(child) == Some(F::Init) => return n,
            _ => {}
        }
        child = n;
        at = s.parent(n);
    }
    child
}

/// The identifiers a binding pattern declares, in order.
pub(super) fn pattern_names(s: &Syntax, pat: u32, out: &mut Vec<u32>) {
    match s.kind(pat) {
        Some(K::Ident) => out.push(pat),
        Some(K::ObjectPattern) => {
            for p in s.all(pat, F::Property) {
                match s.get(p, F::Value).filter(|&v| !(s.is(v, K::AssignPattern) && s.get(v, F::Left).is_none())) {
                    Some(v) => pattern_names(s, v, out),
                    None => {
                        if let Some(k) = s.get(p, F::Key) {
                            pattern_names(s, k, out);
                        }
                    }
                }
            }
            if let Some(r) = s.get(pat, F::Rest) {
                pattern_names(s, r, out);
            }
        }
        Some(K::ArrayPattern) => s.all(pat, F::Element).for_each(|e| pattern_names(s, e, out)),
        Some(K::AssignPattern) => {
            if let Some(l) = s.get(pat, F::Left) {
                pattern_names(s, l, out);
            }
        }
        Some(K::Rest) => {
            if let Some(a) = s.get(pat, F::Arg) {
                pattern_names(s, a, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn global_queries_do_not_run_variable_usage_analysis() {
        for text in ["const unused = 1;", "/* global external */ const unused = 1;"] {
            crate::engine::tests::with_native_file(text, |file, _| {
                super::globals_fact(file, file.syntax.root());
                let initialized = std::cell::Cell::new(false);
                file.cached(|_| {
                    initialized.set(true);
                    super::Unused { vars: Default::default() }
                });
                assert!(initialized.get(), "global queries must leave variable analysis lazy");
            });
        }
    }

    #[test]
    fn later_parameter_defaults_count_as_writes_for_after_used() {
        for (text, expected) in [
            ("function f(first, second) {}", false),
            ("function f(first, second = 1) {}", true),
            ("function f(first, [second = 1]) {}", true),
            ("function f(first, ...second) {}", false),
            ("function f(first, second) { return second; }", true),
        ] {
            crate::engine::tests::with_native_file(text, |file, _| {
                let declaration = *file.bindings().decls.keys().find(|&&d| file.syntax.text(d) == "first").expect("first parameter");
                let result = super::analyze(file);
                assert_eq!(result.vars[&declaration].before_used_param, expected, "{text}");
            });
        }
    }
}

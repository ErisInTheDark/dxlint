//! no-useless-assignment: assignments whose value no path reads before it
//! is overwritten or its function ends. A forward dataflow over each code
//! path ([`super::paths::walk`]): every variable carries the assignments
//! still waiting for a read; a read uses them, a write replaces them. With
//! eslint's exceptions: variables nothing reads (no-unused-vars' job), read
//! from other functions, exported, or assigned inside a `try` block.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::expr::Value;

use super::bind::Bindings;
use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::paths::{walk, Domain};
use super::syntax::Syntax;
use super::unused::{exported_names, pattern_names, variable_scope};

/// Assigning identifiers whose value is never read.
struct DeadStores(HashSet<u32>);

/// Fact `useless_assignment`: on an identifier being assigned.
pub fn fact(file: &NativeFile, node: u32) -> Value<'static> {
    Value::Bool(file.cached(analyze).0.contains(&node))
}

fn analyze(file: &NativeFile) -> DeadStores {
    let s = &file.syntax;
    let b = file.bindings();
    let exported_comment = if file.source_type() == "script" { exported_names(file) } else { HashSet::new() };
    // Which variables can be followed: all their reads in their own code
    // path, at least one of them.
    let mut reads: HashMap<u32, bool> = HashMap::new();
    for (&r, &v) in &b.refs {
        let export = s.parent(r).is_some_and(|p| s.is(p, K::ExportSpec));
        let access = b.access.get(&r).copied().unwrap_or("read");
        if export {
            reads.insert(v, false);
        } else if matches!(access, "read" | "read_write") {
            let own = variable_scope(s, r) == variable_scope(s, v);
            let ok = reads.get(&v).copied().unwrap_or(true) && own;
            reads.insert(v, ok);
        }
    }
    let tracked: HashSet<u32> = b
        .decls
        .iter()
        .filter(|(d, binding)| {
            reads.get(*d) == Some(&true) && !binding.exported && !(b.decl_scope.get(*d) == Some(&0) && exported_comment.contains(s.text(**d)))
        })
        .map(|(&d, _)| d)
        .collect();
    let paths: BTreeSet<u32> = tracked.iter().map(|&d| variable_scope(s, d)).collect();
    let mut d = Stores { b, tracked: &tracked, used: HashSet::new(), assigns: HashSet::new() };
    for cp in paths {
        let body = if s.is(cp, K::Program) { Some(cp) } else { s.get(cp, F::Body) };
        if let Some(body) = body {
            walk(s, &mut d, body, BTreeMap::new());
        }
    }
    DeadStores(d.assigns.into_iter().filter(|&a| !d.used.contains(&a) && !in_try_block(s, a)).collect())
}

/// Inside a `try` block of its own function.
fn in_try_block(s: &Syntax, id: u32) -> bool {
    let mut child = id;
    let mut at = s.parent(id);
    while let Some(n) = at {
        match s.kind(n) {
            Some(K::Try) if s.field(child) == Some(F::Block) => return true,
            Some(K::Function | K::Arrow | K::Method | K::StaticBlock | K::Program) => return false,
            _ => {}
        }
        child = n;
        at = s.parent(n);
    }
    false
}

/// Variable -> its assignments not read yet.
type Pending = BTreeMap<u32, BTreeSet<u32>>;

struct Stores<'a> {
    b: &'a Bindings,
    tracked: &'a HashSet<u32>,
    used: HashSet<u32>,
    assigns: HashSet<u32>,
}

impl Stores<'_> {
    fn first(&self, ident: u32) -> u32 {
        self.b.merged.get(&ident).copied().unwrap_or(ident)
    }

    fn read(&mut self, var: u32, st: &Pending) {
        if let Some(p) = st.get(&var) {
            self.used.extend(p.iter().copied());
        }
    }

    fn write(&mut self, var: u32, ident: u32, st: &mut Pending) {
        st.insert(var, BTreeSet::from([ident]));
        self.assigns.insert(ident);
    }

    /// Reads inside a declaration's pattern (defaults), after its names are
    /// bound: `let { a, b = a } = o`.
    fn pattern_reads(&mut self, s: &Syntax, node: u32, st: &Pending) {
        for (_, c) in s.kids(node).collect::<Vec<_>>() {
            if let Some(&v) = self.b.refs.get(&c).filter(|_| self.b.access.get(&c).is_some_and(|&a| a != "write" && a != "type")) {
                self.read(v, st);
            }
            self.pattern_reads(s, c, st);
        }
    }
}

impl Domain for Stores<'_> {
    type S = Pending;
    const FINALLY_FROM_FIRST_THROW: bool = true;

    fn join(&self, a: &Pending, b: &Pending) -> Pending {
        let mut out = a.clone();
        for (v, set) in b {
            out.entry(*v).or_default().extend(set.iter().copied());
        }
        out
    }

    fn step(&mut self, s: &Syntax, node: u32, st: &mut Pending) {
        match s.kind(node) {
            Some(K::Ident) => {
                let Some(&v) = self.b.refs.get(&node).filter(|v| self.tracked.contains(v)) else { return };
                match self.b.access.get(&node).copied() {
                    Some("read") => self.read(v, st),
                    Some("write") => self.write(v, node, st),
                    Some("read_write") => {
                        self.read(v, st);
                        self.write(v, node, st);
                    }
                    _ => {}
                }
            }
            // `({ a } = o)`, `({ a = 1 } = o)`: the walker skips property
            // keys, so the shorthand target is written here.
            Some(K::Prop) => {
                let Some(k) = s.get(node, F::Key).filter(|&k| s.is(k, K::Ident)) else { return };
                if self.b.access.get(&k) == Some(&"write") {
                    if let Some(&v) = self.b.refs.get(&k).filter(|v| self.tracked.contains(v)) {
                        self.write(v, k, st);
                    }
                }
            }
            Some(K::Declarator) if s.get(node, F::Init).is_some() => {
                let Some(name) = s.get(node, F::Name) else { return };
                let mut names = Vec::new();
                pattern_names(s, name, &mut names);
                for i in names {
                    let v = self.first(i);
                    if self.tracked.contains(&v) {
                        self.write(v, i, st);
                    }
                }
                self.pattern_reads(s, name, st);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    fn useless(text: &str) -> Vec<usize> {
        native_linter("rule no-useless-assignment").lint(Path::new("/root/a.js"), text).into_iter().map(|d| d.start).collect()
    }

    #[test]
    fn values_no_path_reads() {
        // Overwritten before any read.
        assert_eq!(useless("function f() { let a = 1; a = 2; return a; }"), [19]);
        // Read on one branch is enough; a loop reads the next round's value.
        assert!(useless("function f(c) { let a = 1; if (c) a = 2; return a; }").is_empty());
        assert!(useless("function f(n) { let i = 0; while (i < n) { g(i); i = i + 1; } }").is_empty());
        // The last write of a value read only before it.
        assert_eq!(useless("function f() { let a = 1; g(a); a = 2; }"), [32]);
        // eslint's exceptions: read from another function, inside `try`.
        assert!(useless("function f() { let a = 1; g(() => a); a = 2; }").is_empty());
        assert!(useless("function f() { let a = 1; try { g(); a = 2; a = 3; } catch {} g(a); }").is_empty());
    }
}

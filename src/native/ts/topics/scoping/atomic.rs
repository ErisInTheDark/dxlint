//! Reads become stale across suspension; refresh and joins follow code paths.

use std::collections::{HashMap, HashSet};
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::paths::{self, Domain};
use crate::native::ts::syntax::Syntax;
use super::bindings::execution_scope;

#[derive(Clone, PartialEq, Eq, Default)]
struct Reads {
    fresh: HashSet<u32>,
    stale: HashSet<u32>,
}

struct AtomicResults(HashMap<u32, HashSet<(u32, bool)>>);

pub(super) fn updates(file: &NativeFile, node: u32) -> Value<'static> {
    let results = file.cached(analyze);
    let s = &file.syntax;
    let Some(entries) = results.0.get(&node) else { return Value::Null };
    let mut entries: Vec<_> = entries.iter().copied().collect();
    entries.sort_by_key(|&(id, _)| s.tree.node(id).start);
    Value::list(entries.into_iter().map(|(id, property)| Value::record(vec![
        ("name".into(), Value::from(s.text(id).to_string())),
        ("property".into(), Value::Bool(property)),
        ("value".into(), Value::from(s.get(node, F::Left).map(|n| s.text(n)).unwrap_or("").to_string())),
    ])).collect())
}

fn analyze(file: &NativeFile) -> AtomicResults {
    let b = file.bindings();
    let mut references = b.refs.clone();
    let mut globals = HashMap::new();
    let mut unresolved: Vec<_> = b.unresolved.iter().copied().collect();
    unresolved.sort_by_key(|&r| file.syntax.tree.node(r).start);
    for r in unresolved {
        if matches!(file.globals().setting(file.syntax.text(r)), Some("readonly" | "writable")) {
            let declaration = *globals.entry(file.syntax.text(r)).or_insert(r);
            references.insert(r, declaration);
        }
    }
    let mut escaped = HashSet::new();
    for (&reference, &declaration) in &references {
        if !b.decl_scope.contains_key(&declaration)
            || execution_scope(file, reference, false) != execution_scope(file, declaration, true) {
            escaped.insert(declaration);
        }
    }
    let mut out = AtomicResults(HashMap::new());
    let s = &file.syntax;
    let mut stack = vec![s.root()];
    while let Some(n) = stack.pop() {
        stack.extend(s.kids(n).map(|(_, child)| child));
        if matches!(s.kind(n), Some(K::Function | K::Arrow | K::Method))
            && (s.has_word(n, F::Modifier, "async") || s.has_word(n, F::Modifier, "*")) {
            if let Some(body) = s.get(n, F::Body) {
                let mut domain = Atomic { file, references: &references, escaped: &escaped, out: &mut out };
                paths::walk(s, &mut domain, body, Reads::default());
            }
        }
    }
    out
}

struct Atomic<'a, 'f> {
    file: &'a NativeFile<'f>,
    references: &'a HashMap<u32, u32>,
    escaped: &'a HashSet<u32>,
    out: &'a mut AtomicResults,
}

impl Domain for Atomic<'_, '_> {
    type S = Reads;

    fn join(&self, a: &Reads, b: &Reads) -> Reads {
        Reads { fresh: a.fresh.union(&b.fresh).copied().collect(), stale: a.stale.union(&b.stale).copied().collect() }
    }

    /// A compound assignment reads its target before its value runs.
    fn before_expr(&mut self, s: &Syntax, node: u32, state: &mut Reads) {
        if !s.is(node, K::Assign) || s.word(node, F::Op) == Some("=") { return }
        let Some(left) = s.get(node, F::Left) else { return };
        let mut stack = vec![left];
        while let Some(reference) = stack.pop() {
            stack.extend(s.kids(reference).map(|(_, child)| child));
            let Some(&declaration) = self.references.get(&reference) else { continue };
            if assignment_target(s, reference).is_some_and(|(owner, _)| owner == node) {
                state.fresh.insert(declaration);
                state.stale.remove(&declaration);
            }
        }
    }

    fn step(&mut self, s: &Syntax, node: u32, state: &mut Reads) {
        let b = self.file.bindings();
        if let Some(&declaration) = self.references.get(&node) {
            let read = matches!(b.access.get(&node), Some(&"read" | &"read_write"));
            if read && assignment_target(s, node).is_none() {
                state.fresh.insert(declaration);
                state.stale.remove(&declaration);
            }
        }
        if matches!(s.kind(node), Some(K::Await | K::Yield)) {
            state.stale.extend(state.fresh.drain());
        }
        if let Some(assignment) = s.parent(node).filter(|&n| s.is(n, K::Assign) && s.get(n, F::Right) == Some(node)) {
            let Some(left) = s.get(assignment, F::Left) else { return };
            let mut stack = vec![left];
            while let Some(reference) = stack.pop() {
                stack.extend(s.kids(reference).map(|(_, child)| child));
                let Some(&declaration) = self.references.get(&reference) else { continue };
                let Some((owner, property)) = assignment_target(s, reference) else { continue };
                let parameter = super::bindings::declaration_kind(s, declaration) == "parameter";
                if owner == assignment && state.stale.contains(&declaration) && (self.escaped.contains(&declaration) || property && parameter) {
                    self.out.0.entry(assignment).or_default().insert((declaration, property));
                }
            }
        }
    }
}

fn assignment_target(s: &Syntax, reference: u32) -> Option<(u32, bool)> {
    let mut child = reference;
    let mut property = false;
    loop {
        let parent = s.parent(child)?;
        match s.kind(parent)? {
            K::Paren => {}
            K::Member | K::Index if s.field(child) == Some(F::Object) => property = true,
            K::Assign if s.field(child) == Some(F::Left) => return Some((parent, property)),
            _ => return None,
        }
        child = parent;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn compound_reads_precede_suspension_and_refresh_old_reads() {
        let l = native_linter("rule require-atomic-updates");
        let path = Path::new("/root/a.mjs");
        assert_eq!(l.lint(path, "let count; async function f() { count += await amount; }").len(), 1);
        assert!(l.lint(path, "let count; async function f() { count += 1; await work(); count -= 1; }").is_empty());
        assert!(l.lint(path, "async function f() { let count = 0; count += await amount; }").is_empty());
        assert_eq!(l.lint(path, "async function f(obj) { if (obj.ready) { await work(); obj.ready = false; } }").len(), 1);
    }
}

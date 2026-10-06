//! Eligibility for turning a binding's unique writer into a const declaration.

use std::collections::HashMap;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

struct Candidate { offset: u32, group: u32, before: bool }
struct ConstBindings(HashMap<u32, Candidate>, HashMap<u32, (bool, bool)>);

pub(super) fn binding(file: &NativeFile, node: u32) -> Value<'static> {
    let result = file.cached(analyze);
    let Some(candidate) = result.0.get(&node) else { return Value::Null };
    let (all, early) = result.1.get(&candidate.group).copied().unwrap_or((true, false));
    Value::record(vec![
        ("offset".into(), Value::from(file.syntax.tree.node(candidate.offset).start as usize)),
        ("before".into(), Value::Bool(candidate.before)),
        ("all".into(), Value::Bool(all)),
        ("all_before".into(), Value::Bool(early)),
    ])
}

fn analyze(file: &NativeFile) -> ConstBindings {
    let s = &file.syntax;
    let b = file.bindings();
    let exported = crate::native::ts::unused::exported_names(file);
    let mut candidates = HashMap::new();
    let mut groups: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&id, &scope) in &b.decl_scope {
        if super::bindings::declaration_kind(s, id) != "let" || b.merged.contains_key(&id) { continue; }
        if scope == 0 && file.source_type() == "script" && exported.contains(s.text(id)) { continue; }
        let mut declarator = id;
        while let Some(parent) = s.parent(declarator) {
            declarator = parent;
            if s.is(declarator, K::Declarator) { break; }
        }
        if !s.is(declarator, K::Declarator) { continue; }
        let variable = s.parent(declarator).unwrap_or(declarator);
        if s.parent(variable).is_some_and(|p| s.is(p, K::For) && s.field(variable) == Some(F::Init)) { continue; }
        let initialized = s.get(declarator, F::Init).is_some()
            || s.parent(variable).is_some_and(|p| matches!(s.kind(p), Some(K::ForIn | K::ForOf)) && s.field(variable) == Some(F::Left));
        let refs = b.references_of(s, id);
        let mut writes = 0;
        let mut unique_writer = id;
        if initialized { groups.entry(declarator).or_default().push(id); }
        for writer in refs.iter().copied().filter(|r| matches!(b.access.get(r), Some(&"write" | &"read_write"))) {
            writes += 1;
            unique_writer = writer;
            if let Some(host) = writer_host(s, writer) { groups.entry(host).or_default().push(id); }
        }
        let writer = match (initialized, writes) {
            (true, 0) => id,
            (false, 1) => unique_writer,
            _ => continue,
        };
        let before = refs.iter().any(|&r| s.tree.node(r).start < s.tree.node(writer).start && b.access.get(&r) == Some(&"read"));
        let Some(host) = (if initialized { Some(declarator) } else { writer_host(s, writer) }) else { continue };
        if !initialized {
            if b.ref_scope.get(&writer) != Some(&scope) { continue; }
            let mut expression = host;
            while let Some(p) = s.parent(expression).filter(|&n| s.is(n, K::Paren)) { expression = p; }
            let valid = s.parent(expression).filter(|&n| s.is(n, K::ExprStmt)).and_then(|n| s.parent(n))
                .is_some_and(|n| matches!(s.kind(n), Some(K::Program | K::Block | K::Case | K::StaticBlock)));
            if !valid || invalid_assignment_pattern(file, host, scope) { continue; }
        }
        candidates.insert(id, Candidate { offset: if before { id } else { writer }, group: host, before });
    }
    let groups = groups.into_iter().map(|(host, ids)| {
        let all = ids.iter().all(|id| candidates.contains_key(id));
        let early = ids.iter().any(|id| candidates.get(id).is_some_and(|c| c.before));
        (host, (all, early))
    }).collect();
    ConstBindings(candidates, groups)
}

fn writer_host(s: &Syntax, writer: u32) -> Option<u32> {
    let mut child = writer;
    loop {
        let parent = s.parent(child)?;
        match s.kind(parent)? {
            K::Assign if s.field(child) == Some(F::Left) => return Some(parent),
            K::Declarator if s.field(child) == Some(F::Name) => return Some(parent),
            K::AssignPattern if s.field(child) == Some(F::Left) => {}
            K::Array | K::ArrayPattern | K::Object | K::ObjectPattern | K::Prop | K::PatternProp | K::Rest | K::Spread | K::Paren => {}
            _ => return None,
        }
        child = parent;
    }
}

fn invalid_assignment_pattern(file: &NativeFile, host: u32, scope: usize) -> bool {
    let s = &file.syntax;
    let b = file.bindings();
    let Some(left) = s.get(host, F::Left) else { return false };
    let direct: Vec<_> = match s.kind(left) {
        Some(K::Array | K::ArrayPattern) => s.all(left, F::Element).collect(),
        Some(K::Object | K::ObjectPattern) => s.all(left, F::Property).filter_map(|p| s.get(p, F::Value).or_else(|| s.get(p, F::Key))).collect(),
        _ => Vec::new(),
    };
    if direct.iter().any(|n| s.is(*n, K::Ident) && b.refs.get(n).is_some_and(|d| {
        b.decl_scope.get(d) != Some(&scope) || super::bindings::declaration_kind(s, *d) == "parameter"
    })) { return true; }
    let mut stack = vec![left];
    while let Some(n) = stack.pop() {
        if matches!(s.kind(n), Some(K::Member | K::Index)) { return true; }
        match s.kind(n) {
            Some(K::AssignPattern) => stack.extend(s.get(n, F::Left)),
            Some(K::Prop | K::PatternProp) => stack.extend(s.get(n, F::Value).or_else(|| s.get(n, F::Key))),
            _ => stack.extend(s.kids(n).map(|(_, c)| c)),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn unique_writer_must_be_in_the_declaring_scope_and_declaration_position() {
        let l = native_linter("rule prefer-const");
        let path = Path::new("/root/a.mjs");
        assert_eq!(l.lint(path, "let value; value = 1;").len(), 1);
        assert!(l.lint(path, "let value; if (flag) value = 1;").is_empty());
        assert!(l.lint(path, "let value; { value = 1; }").is_empty());
        assert_eq!(l.lint(path, "let { first, second } = input; second = 2;").len(), 1);
    }
}

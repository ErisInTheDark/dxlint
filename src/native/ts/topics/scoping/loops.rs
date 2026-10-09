//! Loop iteration captures, using the binder's canonical declarations.

use std::collections::{HashMap, HashSet};
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use super::bindings::execution_scope;

struct References {
    ordered: Vec<(u32, u32)>,
    writes: HashMap<u32, Vec<u32>>,
}

pub(super) fn captures(file: &NativeFile, function: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(loop_node) = containing_loop(file, function) else { return Value::Null };
    if skipped_iife(file, function) { return Value::Null; }
    let refs = file.cached(|f| {
        let b = f.bindings();
        let mut ordered: Vec<_> = b.refs.iter().map(|(&r, &d)| (r, d)).collect();
        ordered.sort_by_key(|&(r, _)| f.syntax.tree.node(r).start);
        let mut writes: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(r, d) in &ordered {
            if matches!(b.access.get(&r), Some(&"write" | &"read_write")) {
                writes.entry(d).or_default().push(r);
            }
        }
        for &id in b.decl_scope.keys() {
            let mut owner = id;
            while let Some(parent) = f.syntax.parent(owner) {
                owner = parent;
                if f.syntax.is(owner, K::Declarator) {
                    let iteration = f.syntax.parent(owner).and_then(|n| f.syntax.parent(n))
                        .is_some_and(|n| matches!(f.syntax.kind(n), Some(K::ForIn | K::ForOf)));
                    if f.syntax.get(owner, F::Init).is_some() || iteration {
                        writes.entry(b.merged.get(&id).copied().unwrap_or(id)).or_default().push(id);
                    }
                    break;
                }
                if matches!(f.syntax.kind(owner), Some(K::Function | K::Arrow | K::Method | K::Program)) { break; }
            }
        }
        References { ordered, writes }
    });
    let span = s.tree.node(function);
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for &(reference, declaration) in &refs.ordered {
        let r = s.tree.node(reference);
        if r.start < span.start || r.end > span.end { continue; }
        let d = s.tree.node(declaration);
        if span.start <= d.start && d.end <= span.end { continue; }
        let kind = super::bindings::declaration_kind(s, declaration);
        if matches!(kind, "const" | "using" | "await using") { continue; }
        let mut variable = declaration;
        while let Some(parent) = s.parent(variable) {
            variable = parent;
            if matches!(s.kind(variable), Some(K::Var | K::Function | K::Arrow | K::Method | K::Program)) { break; }
        }
        let v = s.tree.node(variable);
        let l = s.tree.node(loop_node);
        if kind == "let" && v.start > l.start && v.end < l.end { continue; }
        let mut top = loop_node;
        let border = if kind == "let" { v.end } else { 0 };
        while let Some(outer) = containing_loop(file, top) {
            if s.tree.node(outer).start < border { break; }
            top = outer;
        }
        let unsafe_write = refs.writes.get(&declaration).is_some_and(|writes| writes.iter().any(|&write| {
            s.tree.node(write).start >= s.tree.node(top).start
                || execution_scope(file, write, false) != execution_scope(file, declaration, true)
        }));
        let name = s.text(reference);
        if unsafe_write && seen.insert(name.to_string()) { names.push(Value::from(name.to_owned())); }
    }
    Value::list(names)
}

fn containing_loop(file: &NativeFile, node: u32) -> Option<u32> {
    let s = &file.syntax;
    let mut child = node;
    while let Some(parent) = s.parent(child) {
        match s.kind(parent) {
            Some(K::While | K::DoWhile) => return Some(parent),
            Some(K::For) if s.field(child) != Some(F::Init) => return Some(parent),
            Some(K::ForIn | K::ForOf) if s.field(child) != Some(F::Right) => return Some(parent),
            Some(K::Function | K::Arrow | K::Method) if !skipped_iife(file, parent) => return None,
            _ => {}
        }
        child = parent;
    }
    None
}

fn skipped_iife(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    if !matches!(s.kind(node), Some(K::Function | K::Arrow))
        || s.has_word(node, F::Modifier, "async") || s.has_word(node, F::Modifier, "*") { return false; }
    let mut callee = node;
    while let Some(parent) = s.parent(callee).filter(|&n| s.is(n, K::Paren)) { callee = parent; }
    let immediate = s.parent(callee).is_some_and(|n| s.is(n, K::Call) && s.get(n, F::Callee) == Some(callee));
    let recursive = s.get(node, F::Name).and_then(|id| file.bindings().decls.get(&id))
        .is_some_and(|b| b.uses > 0 || b.self_uses > 0);
    immediate && !recursive
}

struct UnmodifiedConditions([HashSet<u32>; 2]);

pub(super) fn unmodified_condition(file: &NativeFile, node: u32) -> Value<'static> {
    let result = file.cached(|f| UnmodifiedConditions([
        condition_references(f, false), condition_references(f, true),
    ]));
    Value::record(vec![
        ("grouped".into(), Value::Bool(result.0[0].contains(&node))),
        ("independent".into(), Value::Bool(result.0[1].contains(&node))),
    ])
}

fn condition_references(file: &NativeFile, independent: bool) -> HashSet<u32> {
    let s = &file.syntax;
    let b = file.bindings();
    let mut modifiers: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&r, &d) in &b.refs {
        if matches!(b.access.get(&r), Some(&"write" | &"read_write")) {
            modifiers.entry(d).or_default().push(r);
        }
    }
    for &id in b.decl_scope.keys() {
        if super::bindings::declaration_kind(s, id) != "var" { continue; }
        let mut n = id;
        while let Some(parent) = s.parent(n) {
            n = parent;
            if s.is(n, K::Declarator) {
                if s.get(n, F::Init).is_some() {
                    modifiers.entry(b.merged.get(&id).copied().unwrap_or(id)).or_default().push(id);
                }
                break;
            }
            if matches!(s.kind(n), Some(K::Program | K::Function | K::Arrow | K::Method)) { break; }
        }
    }
    let mut groups: HashMap<u32, Vec<(u32, bool)>> = HashMap::new();
    let mut result = HashSet::new();
    for (&reference, &declaration) in &b.refs {
        let mut child = reference;
        let mut group = None;
        let loop_node = loop {
            let Some(parent) = s.parent(child) else { break None };
            match s.kind(parent) {
                Some(K::While | K::DoWhile | K::For) => {
                    break (s.field(child) == Some(F::Test)).then_some(parent);
                }
                Some(K::Binary) => {
                    if dynamic_expression(file, parent) { break None; }
                    group = Some(parent);
                }
                Some(K::Conditional) if !independent => {
                    if dynamic_expression(file, parent) { break None; }
                    group = Some(parent);
                }
                Some(K::Call | K::Member | K::Index | K::New | K::Yield | K::Function | K::Arrow | K::Method
                    | K::Class | K::Block | K::ExprStmt | K::Var | K::Declarator | K::Return | K::If
                    | K::Switch | K::ForIn | K::ForOf | K::Program) => break None,
                _ => {}
            }
            child = parent;
        };
        let Some(loop_node) = loop_node else { continue };
        let in_loop = |n| {
            let r = s.tree.node(n);
            let l = s.tree.node(loop_node);
            l.start <= r.start && r.end <= l.end
                && !s.get(loop_node, F::Init).is_some_and(|init| {
                    let i = s.tree.node(init);
                    i.start <= r.start && r.end <= i.end
                })
        };
        let modified = modifiers.get(&declaration).is_some_and(|writes| writes.iter().any(|&write| {
            if in_loop(write) { return true; }
            let mut owner = write;
            while let Some(parent) = s.parent(owner) {
                owner = parent;
                if s.is(owner, K::Function) {
                    let declaration = s.parent(owner).is_some_and(|p| matches!(s.kind(p), Some(K::Program | K::Block | K::Export)));
                    if declaration {
                        if let Some(name) = s.get(owner, F::Name) {
                            let canonical = b.merged.get(&name).copied().unwrap_or(name);
                            return b.refs.iter().any(|(&r, &d)| d == canonical && in_loop(r));
                        }
                    }
                }
            }
            false
        }));
        if let Some(group) = group {
            groups.entry(group).or_default().push((reference, modified));
        } else if !modified { result.insert(reference); }
    }
    for conditions in groups.values() {
        if conditions.iter().all(|&(_, modified)| !modified) {
            result.extend(conditions.iter().map(|&(r, _)| r));
        }
    }
    result
}

fn dynamic_expression(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        match s.kind(n) {
            Some(K::Call | K::Member | K::Index | K::New | K::TaggedTemplate | K::Yield) => return true,
            Some(K::Function | K::Arrow | K::Class) => continue,
            _ => stack.extend(s.kids(n).map(|(_, c)| c)),
        }
    }
    false
}

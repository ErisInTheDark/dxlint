//! Redundant constructor writes after parameter-property initialisation.

use std::collections::{HashMap, HashSet};
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;
use crate::native::ts::statics::literal_text;

struct RedundantAssignments(HashSet<u32>);

pub(super) fn unnecessary_parameter_assignment(file: &NativeFile, node: u32) -> Value<'static> {
    Value::Bool(file.cached(collect).0.contains(&node))
}

fn nearest(s: &Syntax, node: u32, accepts: impl Fn(u32) -> bool) -> Option<u32> {
    let mut at = s.parent(node);
    while let Some(n) = at {
        if accepts(n) { return Some(n); }
        at = s.parent(n);
    }
    None
}

fn function(s: &Syntax, node: u32) -> Option<u32> {
    nearest(s, node, |n| matches!(s.kind(n), Some(K::Method | K::Function | K::Arrow)))
}

fn arrow_call(s: &Syntax, node: u32) -> Option<u32> {
    if !s.is(node, K::Arrow) { return None; }
    let mut at = node;
    while let Some(parent) = s.parent(at).filter(|&p| s.is(p, K::Paren)) { at = parent; }
    s.parent(at).filter(|&p| s.is(p, K::Call))
}

fn this_name(s: &Syntax, node: u32) -> Option<String> {
    if !s.get(node, F::Object).is_some_and(|n| s.is(n, K::This)) { return None; }
    if s.is(node, K::Member) { return s.word(node, F::Property).map(str::to_string); }
    if s.is(node, K::Index) { return literal_text(s, s.unparen(s.get(node, F::Index)?)); }
    None
}

fn identifier(s: &Syntax, mut node: u32) -> Option<u32> {
    loop {
        node = s.unparen(node);
        if s.is(node, K::Ident) { return Some(node); }
        if !matches!(s.kind(node), Some(K::As | K::NonNull)) { return None; }
        node = s.get(node, F::Expr)?;
    }
}

fn collect(file: &NativeFile) -> RedundantAssignments {
    let s = &file.syntax;
    let mut classes: HashMap<u32, Vec<u32>> = HashMap::new();
    for node in 0..s.tree.nodes.len() as u32 {
        if s.is(node, K::Assign) {
            if let Some(class) = nearest(s, node, |n| s.is(n, K::Class)) {
                classes.entry(class).or_default().push(node);
            }
        }
    }
    let mut result = HashSet::new();
    for (_, mut assignments) in classes {
        assignments.sort_by_key(|&n| s.tree.node(n).start);
        let mut before_constructor = HashSet::new();
        let mut prior_writes = HashSet::new();
        let mut candidates = Vec::new();
        for assignment in assignments {
            let Some(left) = s.get(assignment, F::Left) else { continue };
            let mut owner = function(s, assignment);
            if let Some(call) = owner.and_then(|n| arrow_call(s, n)) { owner = function(s, call); }
            if let Some(constructor) = owner.filter(|&n| s.is(n, K::Method) && s.word(n, F::Key) == Some("constructor")) {
                if s.is(left, K::Ident) {
                    if file.decl_of(left).and_then(|n| file.binding(n)).is_some_and(|b| b.kind == "parameter") { prior_writes.insert(s.text(left).to_string()); }
                    continue;
                }
                let Some(name) = this_name(s, left) else { continue };
                if !matches!(s.word(assignment, F::Op), Some("=" | "??=" | "&&=" | "||=")) {
                    prior_writes.insert(name);
                    continue;
                }
                let Some(right) = s.get(assignment, F::Right).and_then(|n| identifier(s, n)) else { continue };
                if s.text(right) != name || file.decl_of(right).and_then(|n| file.binding(n)).is_none_or(|b| b.kind != "parameter") { continue; }
                let property = s.all(constructor, F::Param).any(|p| {
                    s.word(p, F::Name) == Some(name.as_str()) && ["public", "protected", "private", "readonly"].iter().any(|m| s.has_word(p, F::Modifier, m))
                });
                if property && !prior_writes.contains(&name) { candidates.push((assignment, name)); }
            } else if let Some(property) = nearest(s, assignment, |n| s.is(n, K::ClassProp)) {
                let enclosing = function(s, assignment);
                let immediate = enclosing.and_then(|n| arrow_call(s, n)).is_some_and(|call| s.get(property, F::Init).is_some_and(|n| s.unparen(n) == call));
                if enclosing.is_none() || immediate {
                    if let Some(name) = this_name(s, left) { before_constructor.insert(name); }
                }
            }
        }
        result.extend(candidates.into_iter().filter(|(_, name)| !before_constructor.contains(name)).map(|(node, _)| node));
    }
    RedundantAssignments(result)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn parameter_and_field_writes_preserve_constructor_assignments() {
        let linter = native_linter("rule no-unnecessary-parameter-property-assignment");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "class A { constructor(public x: number) { this.x = x; } }").len(), 1);
        for text in [
            "class A { constructor(public x: number) { x = 2; this.x = x; } }",
            "class A { y = (this.x = 2); constructor(public x: number) { this.x = x; } }",
            "class A { constructor(public x: number) { (() => { this.x = x; }); } }",
        ] {
            assert!(linter.lint(Path::new("/root/file.ts"), text).is_empty(), "{text}");
        }
    }
}

//! Import declaration metadata and mergeability, cached at the file owner.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics::compare_names as compare;
use crate::expr::Value;
use std::cmp::Ordering;

struct Declaration {
    node: u32,
    module: String,
    export: bool,
    type_only: bool,
    category: &'static str,
    syntax: &'static str,
    local: String,
    members: Vec<(u32, String)>,
}

struct Declarations(Vec<Declaration>);

fn declarations(file: &NativeFile) -> Declarations {
    let s = &file.syntax;
    let mut result = Vec::new();
    for node in 0..s.tree.nodes.len() as u32 {
        if !matches!(s.kind(node), Some(K::Import | K::Export)) { continue; }
        let Some(from) = s.get(node, F::From) else { continue };
        let module = file.static_name(from).unwrap_or_default();
        let export = s.is(node, K::Export);
        let members: Vec<_> = s.all(node, F::Specifier).map(|n| {
            let id = s.get(n, F::Local).or_else(|| s.get(n, F::Imported)).unwrap_or(n);
            (n, s.text(id).to_string())
        }).collect();
        let namespace = s.get(node, F::Namespace);
        let default = s.get(node, F::DefaultImport);
        let count = members.len() + usize::from(namespace.is_some()) + usize::from(default.is_some());
        let category = if !members.is_empty() { "named" } else if namespace.is_some() { "namespace" }
            else if default.is_some() { "default" } else if export { "all" } else { "side" };
        let syntax = if count == 0 { "none" } else if namespace.is_some() && default.is_none() { "all" }
            else if count == 1 { "single" } else { "multiple" };
        let local = default.or(namespace).map(|n| s.text(n).to_string()).or_else(|| members.first().map(|(_, name)| name.clone())).unwrap_or_default();
        result.push(Declaration { node, module, export, type_only: s.word(node, F::Kind) == Some("type"), category, syntax, local, members });
    }
    result.sort_by_key(|d| s.tree.node(d.node).start);
    Declarations(result)
}

fn mergeable(a: &Declaration, b: &Declaration) -> bool {
    if a.type_only && b.type_only && matches!((a.category, b.category), ("default", "named") | ("named", "default")) { return false; }
    !matches!((a.category, b.category), ("all", "named" | "namespace" | "default") | ("named" | "namespace" | "default", "all") | ("namespace", "named") | ("named", "namespace"))
}

pub fn duplicates(file: &NativeFile, node: u32) -> Value<'static> {
    let declarations = file.cached(declarations);
    let Some(index) = declarations.0.iter().position(|d| d.node == node) else { return Value::Null };
    let own = &declarations.0[index];
    if own.module.trim().is_empty() { return Value::Null; }
    let mut hits = Vec::new();
    for separate in [false, true] {
        for export in [false, true] {
            if declarations.0[..index].iter().any(|previous| previous.export == export && previous.module.trim() == own.module.trim()
                && (!separate || previous.type_only == own.type_only) && mergeable(own, previous)) {
                hits.push(Value::Record(vec![
                    ("allowSeparateTypeImports".into(), Value::Bool(separate)),
                    ("export".into(), Value::Bool(own.export)),
                    ("previous_export".into(), Value::Bool(export)),
                    ("module".into(), Value::from(own.module.trim().to_string())),
                ]));
            }
        }
    }
    Value::List(hits)
}

pub fn order(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let declarations = file.cached(declarations);
    let Some(index) = declarations.0.iter().position(|d| d.node == node) else { return Value::Null };
    let own = &declarations.0[index];
    let previous = declarations.0[..index].iter().rev().find(|d| !d.export);
    let mut members = Vec::new();
    for insensitive in [false, true] {
        if let Some(pair) = own.members.windows(2).find(|pair| compare(&pair[0].1, &pair[1].1, insensitive, false) == Ordering::Greater) {
            members.push(Value::Record(vec![
                ("ignoreCase".into(), Value::Bool(insensitive)),
                ("name".into(), Value::from(pair[1].1.clone())),
                ("offset".into(), Value::from(s.tree.node(pair[1].0).start as usize)),
            ]));
        }
    }
    Value::Record(vec![
        ("syntax".into(), Value::from(own.syntax)),
        ("previous_syntax".into(), previous.map_or(Value::Null, |d| Value::from(d.syntax))),
        ("separated".into(), Value::Bool(previous.is_some_and(|d| s.src[s.tree.node(d.node).end as usize..s.tree.node(node).start as usize].bytes().filter(|&b| b == b'\n').count() > 1))),
        ("sensitive".into(), Value::Bool(previous.is_some_and(|d| compare(&own.local, &d.local, false, false) == Ordering::Less))),
        ("insensitive".into(), Value::Bool(previous.is_some_and(|d| compare(&own.local, &d.local, true, false) == Ordering::Less))),
        ("members".into(), Value::List(members)),
    ])
}

pub fn restriction(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut declaration = node;
    while !matches!(s.kind(declaration), Some(K::Import | K::Export | K::ImportEquals)) {
        let Some(parent) = s.parent(declaration) else { return Value::Null };
        declaration = parent;
    }
    let from = s.get(declaration, F::From).or_else(|| s.get(declaration, F::Value).and_then(|n| s.all(n, F::Arg).next()));
    let Some(source) = from.and_then(|n| file.static_name(n)) else { return Value::Null };
    let source = source.trim().to_string();
    let whole = node == declaration;
    let star = s.is(declaration, K::Export) && s.all(declaration, F::Specifier).next().is_none() && s.get(declaration, F::Decl).is_none();
    let name = if star || s.field(node) == Some(F::Namespace) {
        "*".to_string()
    } else if s.field(node) == Some(F::DefaultImport) {
        "default".to_string()
    } else if matches!(s.kind(node), Some(K::ImportSpec | K::ExportSpec)) {
        let name = s.get(node, F::Imported).or_else(|| s.get(node, F::Local)).unwrap_or(node);
        file.static_name(name).unwrap_or_else(|| s.text(name).to_string())
    } else { String::new() };
    let mut offset = s.tree.node(node).start as usize;
    if name == "*" {
        offset = s.text(declaration).find('*').map_or(offset, |i| s.tree.node(declaration).start as usize + i);
    }
    let type_only = s.word(node, F::Kind) == Some("type") || s.word(declaration, F::Kind) == Some("type");
    let all_type = s.word(declaration, F::Kind) == Some("type") || s.get(declaration, F::DefaultImport).is_none() && s.get(declaration, F::Namespace).is_none()
        && s.all(declaration, F::Specifier).next().is_some() && s.all(declaration, F::Specifier).all(|n| s.word(n, F::Kind) == Some("type"));
    let parents = source.match_indices('/').filter(|(i, _)| *i > 0).map(|(i, _)| Value::from(source[..i].to_string())).collect();
    Value::Record(vec![
        ("source".into(), Value::from(source)),
        ("name".into(), Value::from(name)),
        ("item".into(), Value::Bool(!whole || star)),
        ("type".into(), Value::Bool(type_only)),
        ("all_type".into(), Value::Bool(all_type)),
        ("offset".into(), Value::from(offset)),
        ("parents".into(), Value::List(parents)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn import_patterns_respect_negation_and_excluded_parents() {
        let l = native_linter("rule no-restricted-imports:patterns { set options [{patterns: ['foo/*', '!foo/bar']}] }");
        assert!(l.lint(Path::new("/root/file.ts"), "import x from 'foo/bar';").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "import x from 'foo/baz';").len(), 1);
        let l = native_linter("rule no-restricted-imports:patterns { set options [{patterns: ['foo', '!foo/bar']}] }");
        assert_eq!(l.lint(Path::new("/root/file.ts"), "import x from 'foo/bar';").len(), 1);
    }
}

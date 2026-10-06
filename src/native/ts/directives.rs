//! String directive prologues and lexical strictness.

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use crate::expr::Value;
use crate::fx_hash::FxHashMap;

struct Prologue {
    end: u32,
    strict: Vec<u32>,
}

struct Directives(FxHashMap<u32, Prologue>);

fn body(file: &NativeFile, scope: u32) -> Option<u32> {
    let s = &file.syntax;
    if s.is(scope, K::Program) {
        Some(scope)
    } else if matches!(s.kind(scope), Some(K::Function | K::Method | K::Arrow | K::Namespace)) {
        s.get(scope, F::Body).filter(|&node| s.is(node, K::Block))
    } else {
        None
    }
}

fn string_statement(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    s.is(node, K::ExprStmt) && s.get(node, F::Expr).is_some_and(|expression| s.is(expression, K::String))
}

fn collect(file: &NativeFile) -> Directives {
    let s = &file.syntax;
    let mut scopes = FxHashMap::default();
    for scope in 0..s.tree.nodes.len() as u32 {
        let Some(body) = body(file, scope) else { continue };
        let mut prologue = Prologue { end: 0, strict: Vec::new() };
        for statement in s.all(body, F::Body).take_while(|&node| string_statement(file, node)) {
            prologue.end = s.tree.node(statement).end;
            if use_strict(file, statement) {
                prologue.strict.push(statement);
            }
        }
        scopes.insert(scope, prologue);
    }
    Directives(scopes)
}

fn use_strict(file: &NativeFile, statement: u32) -> bool {
    let s = &file.syntax;
    s.get(statement, F::Expr).is_some_and(|expression| matches!(s.text(expression), "'use strict'" | "\"use strict\""))
}

pub(super) fn strict_code(file: &NativeFile, node: u32) -> bool {
    if file.source_type() == "module" || file.environment.implied_strict && file.environment.ecma_version >= 5 {
        return true;
    }
    let s = &file.syntax;
    let directives = file.cached(collect);
    let mut cursor = Some(node);
    while let Some(scope) = cursor {
        if matches!(s.kind(scope), Some(K::Class | K::StaticBlock)) {
            return true;
        }
        if file.environment.ecma_version >= 5 && !s.is(scope, K::Namespace)
            && directives.0.get(&scope).is_some_and(|prologue| !prologue.strict.is_empty()) {
            return true;
        }
        cursor = s.parent(scope);
    }
    false
}

pub(super) fn statement(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let scope = s.parent(node).and_then(|parent| {
        if s.is(parent, K::Program) { Some(parent) }
        else if s.is(parent, K::Block) { s.parent(parent) }
        else { None }
    });
    Value::Bool(string_statement(file, node) && scope.is_some_and(|scope|
        file.cached(collect).0.get(&scope).is_some_and(|prologue| s.tree.node(node).end <= prologue.end)))
}

pub(super) fn scope(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let directives = file.cached(collect);
    let own = directives.0.get(&node).into_iter().flat_map(|prologue| &prologue.strict)
        .map(|&statement| Value::Node(statement, s.text(statement).to_owned().into())).collect();
    let mut parent_function = false;
    let mut class_body = false;
    let mut child = node;
    while let Some(parent) = s.parent(child) {
        parent_function |= matches!(s.kind(parent), Some(K::Function | K::Method | K::Arrow));
        class_body |= s.is(parent, K::Class) && s.field(child) == Some(F::Member);
        child = parent;
    }
    let simple = s.all(node, F::Param).all(|param| s.get(param, F::Name).is_some_and(|name| s.is(name, K::Ident))
        && s.get(param, F::Init).is_none());
    Value::record(vec![
        ("directives".into(), Value::list(own)),
        ("inherited".into(), Value::Bool(s.parent(node).map_or_else(
            || file.source_type() == "module" || file.environment.implied_strict && file.environment.ecma_version >= 5,
            |parent| strict_code(file, parent)))),
        ("parent_function".into(), Value::Bool(parent_function)),
        ("class_body".into(), Value::Bool(class_body)),
        ("simple_parameters".into(), Value::Bool(simple)),
    ])
}

//! Function declaration and parameter policies.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use crate::expr::Value;

struct UnsafeUndefined(std::collections::HashSet<u32>);

fn unsafe_undefined(file: &NativeFile) -> UnsafeUndefined {
    let s = &file.syntax;
    let b = file.bindings();
    let mut unsafe_bindings = std::collections::HashSet::new();
    for &node in b.decls.keys().chain(b.merged.keys()) {
        if s.text(node) != "undefined" { continue }
        let owner = b.merged.get(&node).copied().unwrap_or(node);
        let mut at = s.parent(node);
        let mut uninitialised = false;
        while let Some(n) = at {
            if s.is(n, K::Declarator) {
                uninitialised = s.get(n, F::Init).is_none();
                break;
            }
            if matches!(s.kind(n), Some(K::Param | K::Function | K::Arrow | K::Method | K::Try | K::Class | K::Import)) { break }
            at = s.parent(n);
        }
        if !uninitialised || b.decls.get(&owner).is_some_and(|binding| binding.writes > 0) {
            unsafe_bindings.insert(owner);
        }
    }
    UnsafeUndefined(unsafe_bindings)
}

pub fn restricted_binding(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let b = file.bindings();
    let owner = b.merged.get(&node).copied().unwrap_or(node);
    if !b.decls.contains_key(&owner) || !matches!(s.text(node), "NaN" | "Infinity" | "undefined" | "eval" | "arguments" | "globalThis") {
        return Value::Bool(false);
    }
    Value::Bool(s.text(node) != "undefined" || file.cached(unsafe_undefined).0.contains(&owner))
}

pub fn style(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(parent) = s.parent(node) else { return Value::Null };
    let declaration = s.is(node, K::Function) && matches!(s.field(node), Some(F::Body | F::Decl | F::Consequent | F::Alternate));
    let holder = if declaration { node } else {
        let mut outer = node;
        while let Some(p) = s.parent(outer).filter(|&p| s.is(p, K::Paren)) { outer = p; }
        let Some(p) = s.parent(outer).filter(|&p| s.is(p, K::Declarator)) else { return Value::Null };
        p
    };
    let export = if declaration { Some(parent) } else { s.parent(holder).and_then(|p| s.parent(p)) };
    let exported = export.is_some_and(|p| s.is(p, K::Export));
    let default_export = export.is_some_and(|p| s.is(p, K::Export) && s.has_word(p, F::Kind, "default"));
    let overloaded = declaration && s.get(node, F::Name).is_some_and(|name| {
        let owner = if exported || s.is(parent, K::Case) { s.parent(parent).unwrap_or(parent) } else { parent };
        let siblings: Vec<_> = s.kids(owner).flat_map(|(_, sibling)| {
            if s.is(sibling, K::Case) { s.all(sibling, F::Body).collect::<Vec<_>>() } else { vec![sibling] }
        }).collect();
        siblings.into_iter().any(|mut sibling| {
            if s.is(sibling, K::Export) { sibling = s.get(sibling, F::Decl).unwrap_or(sibling); }
            s.is(sibling, K::Function) && s.get(sibling, F::Body).is_none() && s.word(sibling, F::Name) == Some(s.text(name))
        })
    });
    let arrow = s.is(node, K::Arrow);
    let mut captures = false;
    if arrow {
        let mut stack: Vec<_> = s.kids(node).map(|(_, child)| child).collect();
        while let Some(n) = stack.pop() {
            match s.kind(n) {
                Some(K::Function | K::Arrow | K::Method) => continue,
                Some(K::This | K::Super) => { captures = true; break }
                _ => stack.extend(s.kids(n).map(|(_, child)| child)),
            }
        }
    }
    Value::record(vec![
        ("declaration".into(), Value::Bool(declaration)),
        ("exported".into(), Value::Bool(exported && !default_export)),
        ("exempt".into(), Value::Bool(default_export || overloaded || s.get(node, F::Body).is_none())),
        ("arrow".into(), Value::Bool(arrow)),
        ("captures".into(), Value::Bool(captures)),
        ("annotated".into(), Value::Bool(s.get(holder, F::Type).is_some())),
        ("offset".into(), Value::Num(f64::from(s.tree.node(holder).start))),
    ])
}

pub(super) fn default_this(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let anonymous = s.get(node, F::Name).is_none();
    let capital = |n| s.is(n, K::Ident) && s.text(n).starts_with(char::is_uppercase);
    if s.get(node, F::Name).is_some_and(capital)
        || s.all(node, F::Param).any(|n| s.word(n, F::Name) == Some("this")) { return false }
    let nullish = |n| {
        let n = s.unparen(n);
        s.is(n, K::Null) || s.is(n, K::Ident) && s.text(n) == "undefined" || s.is(n, K::Unary) && s.word(n, F::Op) == Some("void")
    };
    let mut current = node;
    while let Some(parent) = s.parent(current) {
        match s.kind(parent) {
            Some(K::Paren | K::Logical | K::Conditional) => current = parent,
            Some(K::Prop | K::ClassProp) => return !matches!(s.field(current), Some(F::Value | F::Init)),
            Some(K::Assign | K::AssignPattern) => return s.get(parent, F::Left).is_none_or(|n|
                !(matches!(s.kind(s.unparen(n)), Some(K::Member | K::Index)) || anonymous && capital(n))),
            Some(K::Declarator) => return !anonymous || s.get(parent, F::Name).is_none_or(|n| !capital(n)),
            Some(K::Param) if s.field(current) == Some(F::Init) => return !anonymous || s.get(parent, F::Name).is_none_or(|n| !capital(n)),
            Some(K::Member | K::Index) => {
                let bound = matches!(super::super::super::statics::property_name(s, parent).as_deref(), Some("bind" | "call" | "apply"))
                    && s.get(parent, F::Object) == Some(current)
                    && s.parent(parent).filter(|&n| s.is(n, K::Call) && s.get(n, F::Callee) == Some(parent))
                        .and_then(|call| s.all(call, F::Arg).next()).is_some_and(|arg| !nullish(arg));
                return !bound;
            }
            Some(K::Call) => {
                let args: Vec<_> = s.all(parent, F::Arg).collect();
                let Some(callee) = s.get(parent, F::Callee).map(|n| s.unparen(n)) else { return true };
                let name = super::super::super::statics::property_name(s, callee);
                let owner = s.get(callee, F::Object).map(|n| s.unparen(n));
                let callback = match name.as_deref() {
                    Some("apply") if owner.is_some_and(|n| s.is(n, K::Ident) && s.text(n) == "Reflect") => Some((0, 1, 3)),
                    Some("from" | "fromAsync") if owner.is_some_and(|n| s.is(n, K::Ident) && s.text(n) == "Array") => Some((1, 2, 3)),
                    Some("every" | "filter" | "find" | "findIndex" | "findLast" | "findLastIndex" | "flatMap" | "forEach" | "map" | "some") => Some((0, 1, 2)),
                    _ => None,
                };
                return callback.is_none_or(|(index, receiver, count)| args.len() != count || args[index] != current || nullish(args[receiver]));
            }
            _ => return true,
        }
    }
    true
}

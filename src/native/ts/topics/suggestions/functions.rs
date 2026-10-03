//! Function declaration and parameter policies.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use crate::expr::Value;

pub fn default_parameter(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(parent) = s.parent(node).filter(|&p| matches!(s.kind(p), Some(K::Function | K::Arrow | K::Method))) else { return Value::Bool(false) };
    let optional = |n| s.get(n, F::Init).is_some() || s.get(n, F::Optional).is_some();
    let rest = |n| s.get(n, F::Name).is_some_and(|name| s.is(name, K::Rest)) || s.text(n).trim_start().starts_with("...");
    Value::Bool(optional(node) && s.all(parent, F::Param).skip_while(|&p| p != node).skip(1).any(|p| !optional(p) && !rest(p)))
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
    Value::Record(vec![
        ("declaration".into(), Value::Bool(declaration)),
        ("exported".into(), Value::Bool(exported && !default_export)),
        ("exempt".into(), Value::Bool(default_export || overloaded || s.get(node, F::Body).is_none())),
        ("arrow".into(), Value::Bool(arrow)),
        ("captures".into(), Value::Bool(captures)),
        ("annotated".into(), Value::Bool(s.get(holder, F::Type).is_some())),
        ("offset".into(), Value::Num(f64::from(s.tree.node(holder).start))),
    ])
}

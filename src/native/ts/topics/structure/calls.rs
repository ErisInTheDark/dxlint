//! Immediate function invocations and call-chain layout.

use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field as F, Kind as K}, lexer::{Lexer, Tok}, statics, syntax::Syntax};

pub(super) fn iife_function(s: &Syntax, node: u32) -> Option<(u32, bool)> {
    if !s.is(node, K::Call) {
        return None;
    }
    let callee = s.unparen(s.get(node, F::Callee)?);
    if s.is(callee, K::Function) {
        return Some((callee, false));
    }
    if matches!(s.kind(callee), Some(K::Member | K::Index)) {
        let object = s.unparen(s.get(callee, F::Object)?);
        if s.is(object, K::Function) && statics::property_name(s, callee).is_some_and(|name| matches!(name.as_str(), "call" | "apply")) {
            return Some((object, true));
        }
    }
    None
}

fn wrapped(s: &Syntax, node: u32) -> bool {
    let Some(parent) = s.parent(node) else { return false };
    if s.is(parent, K::Paren) {
        return true;
    }
    if matches!((s.kind(parent), s.field(node)),
        (Some(K::If | K::While | K::DoWhile), Some(F::Test))
        | (Some(K::Switch), Some(F::Discriminant))
        | (Some(K::With), Some(F::Object))) {
        return true;
    }
    if matches!(s.kind(parent), Some(K::Call | K::New)) && s.field(node) == Some(F::Arg) && s.all(parent, F::Arg).count() == 1 {
        let tail = &s.src[s.tree.node(node).end as usize..s.tree.node(parent).end as usize];
        return Lexer::new(tail).next_token().tok == Tok::P(")");
    }
    false
}

pub(super) fn layout(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some((function, prototype)) = iife_function(s, node) else { return Value::Null };
    Value::Record(vec![
        ("prototype".into(), Value::Bool(prototype)),
        ("call_wrapped".into(), Value::Bool(wrapped(s, node))),
        ("function_wrapped".into(), Value::Bool(wrapped(s, function))),
    ])
}

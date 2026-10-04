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
    if matches!(s.kind(parent), Some(K::Call | K::New | K::ImportCall)) && s.field(node) == Some(F::Arg) && s.all(parent, F::Arg).count() == 1 {
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

pub(super) fn chain(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, F::Callee).map(|n| s.unparen(n)) else { return Value::Null };
    if !matches!(s.kind(callee), Some(K::Member | K::Index)) {
        return Value::Null;
    }
    let (Some(object), Some(property)) = (s.get(callee, F::Object), s.get(callee, F::Property).or_else(|| s.get(callee, F::Index))) else { return Value::Null };
    let object = s.unparen(object);
    let mut parent = object;
    let mut depth = 1;
    while let Some(parent_callee) = s.get(parent, F::Callee) {
        depth += 1;
        let Some(next) = s.get(s.unparen(parent_callee), F::Object) else { break };
        parent = s.unparen(next);
    }
    let start = s.tree.node(object).end as usize;
    let end = s.tree.node(property).start as usize;
    let gap = &s.src[start..end];
    let mut lexer = Lexer::new(gap);
    let offset = loop {
        let token = lexer.next_token();
        if token.tok != Tok::P(")") {
            break start + token.start as usize;
        }
    };
    let computed = s.is(callee, K::Index);
    let optional = s.get(callee, F::Optional).is_some();
    let text = s.text(s.unparen(property));
    let first = text.split(['\n', '\r', '\u{2028}', '\u{2029}']).next().expect("split has a first segment");
    let prefix = match (optional, computed) { (true, true) => "?.[", (true, false) => "?.", (false, true) => "[", (false, false) => "." };
    let suffix = if computed && first.len() == text.len() { "]" } else { "" };
    Value::Record(vec![
        ("depth".into(), Value::from(depth)),
        ("same_line".into(), Value::Bool(!gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']))),
        ("offset".into(), Value::from(offset)),
        ("callee".into(), Value::from(format!("{prefix}{first}{suffix}"))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn construct_parens_count_but_argument_separators_do_not() {
        for (source, outside, inside) in [
            ("import(function (){}());", 0, 1),
            ("consume(function (){}());", 0, 1),
            ("consume(function (){}(), other);", 1, 1),
            ("consume(function (){}(),);", 1, 1),
            ("consume((function (){})());", 0, 0),
        ] {
            for (mode, count) in [("outside", outside), ("inside", inside)] {
                let lint = native_linter(&format!("rule wrap-iife {{ set mode \"{mode}\" }}"));
                assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{mode}: {source}");
            }
        }
    }
}

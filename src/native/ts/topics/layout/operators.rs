//! Spacing sites owned by expression operators.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::{gap_has_space, lex};

pub(super) fn star_spacing_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let is_yield = syntax.is(node, K::Yield);
    let star = if is_yield {
        syntax.get(node, F::Op)
    } else if matches!(syntax.kind(node), Some(K::Function | K::Method)) {
        syntax.all(node, F::Modifier).find(|&modifier| syntax.text(modifier) == "*")
    } else {
        None
    };
    let Some(star) = star else { return Value::list(Vec::new()) };
    let lexed = file.cached(lex);
    let at = lexed.0.partition_point(|token| token.start < syntax.tree.node(star).start);
    let Some(token) = lexed.0.get(at) else { return Value::list(Vec::new()) };
    let method = syntax.is(node, K::Method);
    let kind = if is_yield {
        "yield"
    } else if method {
        if syntax.parent(node).is_some_and(|parent| syntax.is(parent, K::Object)) { "shorthand" } else { "method" }
    } else if syntax.get(node, F::Name).is_some() {
        "named"
    } else {
        "anonymous"
    };
    let mut sites = Vec::new();
    for (side, adjacent) in [
        ("before", at.checked_sub(1).and_then(|i| lexed.0.get(i))),
        ("after", lexed.0.get(at + 1)),
    ] {
        if method && side == "before" && token.start == syntax.tree.node(node).start {
            continue;
        }
        let Some(adjacent) = adjacent else { continue };
        let (mut start, mut end) = if side == "before" {
            (adjacent.end, token.start)
        } else {
            (token.end, adjacent.start)
        };
        if !is_yield {
            if side == "before" {
                if let Some(comment) = file.comments().iter().rev().find(|comment| start <= comment.start && comment.end <= end) {
                    start = comment.end;
                }
            } else if let Some(comment) = file.comments().iter().find(|comment| start <= comment.start && comment.end <= end) {
                end = comment.start;
            }
        }
        sites.push(Value::record(vec![
            ("kind".into(), Value::from(kind)),
            ("side".into(), Value::from(side)),
            ("offset".into(), Value::from(token.start as usize)),
            ("start".into(), Value::from(start as usize)),
            ("end".into(), Value::from(end as usize)),
            ("separated".into(), Value::Bool(start != end)),
            ("spaced".into(), Value::Bool(gap_has_space(&syntax.src[start as usize..end as usize]))),
        ]));
    }
    Value::list(sites)
}

pub(super) fn unary_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let kind = syntax.kind(node);
    if !matches!(kind, Some(K::Unary | K::Update | K::New | K::Await | K::Yield | K::NonNull)) {
        return Value::Null;
    }
    let span = syntax.tree.node(node);
    let lexed = file.cached(lex);
    let first = lexed.0.partition_point(|token| token.start < span.start);
    let last = lexed.0.partition_point(|token| token.end <= span.end);
    if last < first + 2 {
        return Value::Null;
    }
    let prefix = match kind {
        Some(K::NonNull) => false,
        Some(K::Unary | K::Update) => syntax.get(node, F::Op).is_none_or(|op| syntax.tree.node(op).start == span.start),
        _ => true,
    };
    let (left, right) = if prefix {
        (lexed.0[first], lexed.0[first + 1])
    } else {
        (lexed.0[last - 2], lexed.0[last - 1])
    };
    if kind == Some(K::Yield) && right.tok == Tok::P("*") {
        return Value::Null;
    }
    let operator_token = if prefix { left } else { right };
    let operator = &syntax.src[operator_token.start as usize..operator_token.end as usize];
    let word = operator_token.tok == Tok::Ident;
    let removable = if word {
        !matches!(right.tok, Tok::Ident | Tok::Num | Tok::BigInt)
    } else {
        !matches!((left.tok, right.tok), (Tok::P("+"), Tok::P("+" | "++")) | (Tok::P("-"), Tok::P("-" | "--")))
    };
    let first_bang = operator == "!" && prefix && syntax.get(node, F::Arg).is_some_and(|arg| syntax.is(arg, K::Unary) && syntax.get(arg, F::Op).is_some_and(|op| syntax.text(op) == "!"));
    let start = left.end as usize;
    let end = right.start as usize;
    Value::record(vec![
        ("operator".into(), Value::from(operator.to_owned())),
        ("word".into(), Value::Bool(word)),
        ("prefix".into(), Value::Bool(prefix)),
        ("start".into(), Value::from(start)),
        ("end".into(), Value::from(end)),
        ("separated".into(), Value::Bool(start != end)),
        ("removable".into(), Value::Bool(removable)),
        ("first_bang".into(), Value::Bool(first_bang)),
        ("override_key".into(), Value::from(if kind == Some(K::NonNull) { "ts-non-null" } else { operator }.to_owned())),
    ])
}

pub(super) fn spread_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let rest = match syntax.kind(node) {
        Some(K::Rest) => true,
        Some(K::Spread) => false,
        _ => return Value::Null,
    };
    let lexed = file.cached(lex);
    let at = lexed.0.partition_point(|token| token.start < syntax.tree.node(node).start);
    let Some(operator) = lexed.0.get(at).filter(|token| token.tok == Tok::P("...")) else { return Value::Null };
    let Some(next) = lexed.0.get(at + 1) else { return Value::Null };
    let start = operator.end as usize;
    let end = file.comments().iter().find(|comment| operator.end <= comment.start && comment.end <= next.start).map_or(next.start, |comment| comment.start) as usize;
    let property = syntax.parent(node).is_some_and(|parent| matches!(syntax.kind(parent), Some(K::Object | K::ObjectPattern)));
    let kind = match (rest, property) {
        (true, true) => "rest property",
        (true, false) => "rest",
        (false, true) => "spread property",
        (false, false) => "spread",
    };
    Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("start".into(), Value::from(start)),
        ("end".into(), Value::from(end)),
        ("spaced".into(), Value::Bool(gap_has_space(&syntax.src[start..end]))),
    ])
}

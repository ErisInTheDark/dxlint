//! Spacing sites owned by expression operators.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::lex;


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

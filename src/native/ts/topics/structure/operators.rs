//! Operator precedence and parent-child mixtures.

use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field as F, Kind as K}, lexer::{Lexer, Tok}, syntax::Syntax};

pub(super) fn new_args(s: &Syntax, node: u32) -> bool {
    s.get(node, F::Callee).is_some_and(|callee| s.tree.node(callee).end < s.tree.node(node).end)
}

pub(super) fn precedence(s: &Syntax, node: u32) -> u8 {
    match s.kind(node) {
        Some(K::Sequence) => 0,
        Some(K::Assign | K::Arrow) => 1,
        Some(K::Yield) => 2,
        Some(K::Conditional | K::ConditionalType | K::FnType) => 3,
        Some(K::Union) => 4,
        Some(K::Intersection) => 5,
        Some(K::Binary | K::Logical) => match s.word(node, F::Op).unwrap_or("") {
            "||" | "??" => 4, "&&" => 5, "|" => 6, "^" => 7, "&" => 8,
            "==" | "!=" | "===" | "!==" => 9,
            "<" | ">" | "<=" | ">=" | "in" | "instanceof" => 10,
            "<<" | ">>" | ">>>" => 11, "+" | "-" => 12,
            "*" | "/" | "%" => 13, "**" => 14, _ => 21,
        },
        Some(K::Unary | K::As | K::Satisfies | K::TypeAssertion | K::TypeOperator) => 15,
        Some(K::Await) => 16,
        Some(K::Update) => 17,
        Some(K::New) if !new_args(s, node) => 18,
        Some(K::Call | K::New | K::TaggedTemplate | K::ImportCall) => 19,
        Some(K::Member | K::Index | K::NonNull | K::ArrayType) => 20,
        _ => 21,
    }
}

fn operator<'s>(s: &Syntax<'s>, node: u32) -> Option<(&'s str, usize)> {
    if let Some(op) = s.get(node, F::Op) {
        return Some((s.text(op), s.tree.node(op).start as usize));
    }
    if s.is(node, K::Conditional) {
        let test = s.get(node, F::Test)?;
        let start = s.tree.node(test).end as usize;
        let mut lexer = Lexer::new(&s.src[start..]);
        loop {
            let token = lexer.next_token();
            match token.tok {
                Tok::P(")") => {}
                Tok::P("?") => return Some(("?:", start + token.start as usize)),
                _ => return None,
            }
        }
    }
    None
}

pub(super) fn mixed(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !matches!(s.kind(node), Some(K::Binary | K::Logical)) {
        return Value::Null;
    }
    let Some(parent) = s.parent(node).filter(|&parent| matches!(s.kind(parent), Some(K::Binary | K::Logical | K::Conditional))) else { return Value::Null };
    let (Some(child_op), Some(parent_op)) = (operator(s, node), operator(s, parent)) else { return Value::Null };
    if child_op.0 == parent_op.0 {
        return Value::Null;
    }
    let (left, right) = if matches!(s.field(node), Some(F::Left | F::Test)) { (child_op, parent_op) } else { (parent_op, child_op) };
    Value::Record(vec![
        ("left".into(), Value::from(left.0.to_owned())),
        ("right".into(), Value::from(right.0.to_owned())),
        ("same_precedence".into(), Value::Bool(precedence(s, node) == precedence(s, parent))),
        ("offsets".into(), Value::List(vec![Value::from(left.1), Value::from(right.1)])),
    ])
}

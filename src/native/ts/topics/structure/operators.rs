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

fn linebreak_site(file: &NativeFile, from: usize, right: u32, wanted: &str, exact: Option<u32>, alias: bool) -> Option<Value<'static>> {
    let s = &file.syntax;
    let end = s.tree.node(right).start as usize;
    if from > end { return None }
    let mut lexer = Lexer::new(&s.src[from..end]);
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token();
        if token.tok == Tok::Eof { break }
        tokens.push((from + token.start as usize, from + token.end as usize));
    }
    let (start, stop) = if let Some(op) = exact {
        let bounds = s.tree.node(op);
        (bounds.start as usize, bounds.end as usize)
    } else { *tokens.iter().rev().find(|&&(start, end)| &s.src[start..end] == wanted)? };
    let before = tokens.iter().rev().find(|&&(_, end)| end <= start).map_or(from, |&(_, end)| end);
    let after = tokens.iter().find(|&&(at, _)| at >= stop).map_or(end, |&(at, _)| at);
    let broken = |from: usize, to: usize| s.src[from..to].contains(['\n', '\r', '\u{2028}', '\u{2029}']);
    let leading_union = alias && matches!(Lexer::new(&s.src[after..]).next_token().tok, Tok::P("|" | "&"));
    Some(Value::Record(vec![
        ("offset".into(), Value::from(start)),
        ("operator".into(), Value::from(wanted.to_owned())),
        ("before".into(), Value::Bool(broken(before, start))),
        ("after".into(), Value::Bool(broken(stop, after))),
        ("leading_union".into(), Value::Bool(leading_union)),
    ]))
}

pub(super) fn linebreaks(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut out = Vec::new();
    let mut add = |left: u32, right: u32, op: &str, exact: Option<u32>, alias: bool| {
        if let Some(site) = linebreak_site(file, s.tree.node(left).end as usize, right, op, exact, alias) { out.push(site); }
    };
    match s.kind(node) {
        Some(K::Binary | K::Logical | K::Assign) => {
            if let (Some(left), Some(right), Some(op)) = (s.get(node, F::Left), s.get(node, F::Right), s.get(node, F::Op)) {
                add(left, right, s.text(op), Some(op), false);
            }
        }
        Some(K::Conditional | K::ConditionalType) => {
            if let (Some(left), Some(consequent), Some(alternate)) = (
                s.get(node, F::Test).or_else(|| s.get(node, F::Extends)), s.get(node, F::Consequent), s.get(node, F::Alternate),
            ) {
                add(left, consequent, "?", None, false);
                add(consequent, alternate, ":", None, false);
            }
        }
        Some(K::Union | K::Intersection) => {
            let types: Vec<_> = s.all(node, F::Type).collect();
            for pair in types.windows(2) { add(pair[0], pair[1], if s.is(node, K::Union) { "|" } else { "&" }, None, false); }
        }
        Some(K::Declarator | K::ClassProp | K::EnumMember | K::TypeParam | K::TypeAlias | K::ImportEquals) => {
            let right = match s.kind(node) {
                Some(K::TypeParam) => s.get(node, F::Default),
                Some(K::TypeAlias) => s.get(node, F::Type),
                Some(K::ImportEquals) => s.get(node, F::Value),
                _ => s.get(node, F::Init),
            };
            if let Some(right) = right {
                if let Some(site) = linebreak_site(file, s.tree.node(node).start as usize, right, "=", None, s.is(node, K::TypeAlias)) { out.push(site); }
            }
        }
        _ => {}
    }
    Value::List(out)
}

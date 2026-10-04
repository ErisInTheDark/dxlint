//! Assignment shorthand and comparison orientation.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics;
use crate::expr::Value;

pub fn arithmetic_assignment(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.word(node, F::Op) != Some("=") { return Value::Null }
    let Some((left, right)) = s.get(node, F::Left).zip(s.get(node, F::Right)) else { return Value::Null };
    let right = s.unparen(right);
    if !s.is(right, K::Binary) { return Value::Null }
    let Some(op) = s.word(right, F::Op).filter(|op| matches!(*op, "+" | "-" | "*" | "/" | "%" | "**" | "<<" | ">>" | ">>>" | "&" | "|" | "^")) else { return Value::Null };
    let forward = s.get(right, F::Left).is_some_and(|n| file.same_reference_keyed(left, n, true));
    let reverse = matches!(op, "*" | "&" | "^" | "|") && s.get(right, F::Right).is_some_and(|n| file.same_reference_keyed(left, n, true));
    if forward || reverse { Value::from(format!("{op}=")) } else { Value::Null }
}

fn reference(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    if file.chain_top(node) { return false }
    matches!(s.kind(node), Some(K::Member | K::Index))
        || s.is(node, K::Ident) && s.text(node) != "undefined"
}

fn nullish(file: &NativeFile, node: u32) -> Option<bool> {
    let s = &file.syntax;
    let node = s.unparen(node);
    if s.is(node, K::Null) { return Some(false) }
    if s.is(node, K::Unary) && s.word(node, F::Op) == Some("void")
        && s.get(node, F::Arg).is_some_and(|n| s.is(s.unparen(n), K::Number) && statics::number(s.text(s.unparen(n))) == Some(0.0))
        || s.is(node, K::Ident) && s.text(node) == "undefined" && file.resolved(node) == Some("global") {
        return Some(true)
    }
    None
}

fn nullish_comparison(file: &NativeFile, node: u32, op: &str) -> Option<(u32, bool)> {
    let s = &file.syntax;
    let node = s.unparen(node);
    if !s.is(node, K::Binary) || s.word(node, F::Op) != Some(op) { return None }
    let (left, right) = s.get(node, F::Left).zip(s.get(node, F::Right))?;
    if reference(file, left) { nullish(file, right).map(|kind| (left, kind)) }
    else if reference(file, right) { nullish(file, left).map(|kind| (right, kind)) }
    else { None }
}

fn existence(file: &NativeFile, node: u32) -> Option<(u32, &'static str)> {
    let s = &file.syntax;
    let node = s.unparen(node);
    let negated = s.is(node, K::Unary) && s.word(node, F::Op) == Some("!");
    let base = if negated { s.unparen(s.get(node, F::Arg)?) } else { node };
    if reference(file, base) { return Some((base, if negated { "||" } else { "&&" })) }
    if s.is(base, K::Unary) && s.word(base, F::Op) == Some("!") {
        let arg = s.unparen(s.get(base, F::Arg)?);
        if reference(file, arg) { return Some((arg, "&&")) }
    }
    if s.is(base, K::Call) && super::expressions::global_callee(file, base) == Value::from("Boolean") {
        let args: Vec<_> = s.all(base, F::Arg).collect();
        if args.len() == 1 && reference(file, args[0]) { return Some((args[0], if negated { "||" } else { "&&" })) }
    }
    if let Some((reference, _)) = nullish_comparison(file, node, "==") { return Some((reference, "??")) }
    if s.is(node, K::Logical) && s.word(node, F::Op) == Some("||") {
        let (left, right) = s.get(node, F::Left).zip(s.get(node, F::Right))?;
        let ((a, ak), (b, bk)) = nullish_comparison(file, left, "===")
            .zip(nullish_comparison(file, right, "==="))?;
        if ak != bk && file.same_reference(a, b) { return Some((a, "??")) }
    }
    None
}

pub fn logical_assignment(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let found = (|| {
        match s.kind(node)? {
            K::Assign if matches!(s.word(node, F::Op), Some("&&=" | "||=" | "??=")) => {
                let op = s.word(node, F::Op)?;
                Some(("shorthand", op.trim_end_matches('=').to_owned()))
            }
            K::Assign if s.word(node, F::Op) == Some("=") => {
                let right = s.unparen(s.get(node, F::Right)?);
                if !s.is(right, K::Logical) { return None }
                let op = s.word(right, F::Op)?;
                let mut left = s.get(right, F::Left)?;
                while s.is(left, K::Logical) && s.word(left, F::Op) == Some(op) {
                    left = s.get(left, F::Left)?;
                }
                file.same_reference(s.get(node, F::Left)?, left).then_some(("assignment", op.to_owned()))
            }
            K::Logical => {
                let left = s.get(node, F::Left)?;
                let right = s.unparen(s.get(node, F::Right)?);
                (reference(file, left) && s.is(right, K::Assign) && s.word(right, F::Op) == Some("=")
                    && file.same_reference(left, s.get(right, F::Left)?))
                    .then_some(("logical", s.word(node, F::Op)?.to_owned()))
            }
            K::If if s.get(node, F::Alternate).is_none() => {
                let mut body = s.get(node, F::Consequent)?;
                if s.is(body, K::Block) {
                    if s.all(body, F::Body).count() != 1 { return None }
                    body = s.all(body, F::Body).next()?;
                }
                if !s.is(body, K::ExprStmt) { return None }
                let assignment = s.unparen(s.get(body, F::Expr)?);
                if !s.is(assignment, K::Assign) || s.word(assignment, F::Op) != Some("=") { return None }
                let (test, op) = existence(file, s.get(node, F::Test)?)?;
                file.same_reference(test, s.get(assignment, F::Left)?).then_some(("if", op.to_owned()))
            }
            _ => None,
        }
    })();
    let Some((kind, op)) = found else { return Value::Null };
    Value::Record(vec![("kind".into(), Value::from(kind)), ("operator".into(), Value::from(format!("{op}=")))])
}

fn literal(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    matches!(s.kind(node), Some(K::String | K::Number | K::BigInt | K::Bool | K::Null | K::Regex))
        || s.is(node, K::Template) && s.all(node, F::Expr).next().is_none()
        || s.is(node, K::Unary) && s.word(node, F::Op) == Some("-")
            && s.get(node, F::Arg).is_some_and(|n| matches!(s.kind(s.unparen(n)), Some(K::Number | K::BigInt)))
}

fn ordered(file: &NativeFile, left: u32, right: u32) -> bool {
    if !literal(file, left) && !literal(file, right) { return false }
    if !literal(file, left) || !literal(file, right) { return true }
    let s = &file.syntax;
    let Some((a, b)) = statics::literal_value(s, s.unparen(left)).zip(statics::literal_value(s, s.unparen(right))) else { return false };
    if a.0 == "string" && b.0 == "string" { return a.1.encode_utf16().cmp(b.1.encode_utf16()).is_le() }
    let number = |(kind, value): (&str, String)| match kind {
        "null" => Some(0.0),
        "boolean" => Some(if value == "true" { 1.0 } else { 0.0 }),
        "string" if value.trim().is_empty() => Some(0.0),
        _ => statics::number(value.trim()),
    };
    number(a).zip(number(b)).is_some_and(|(a, b)| a <= b)
}

fn range(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let Some(parent) = s.parent(node) else { return false };
    let parent = if s.is(parent, K::Paren) { s.parent(parent).unwrap_or(parent) } else { parent };
    if !s.is(parent, K::Logical) { return false }
    let wrapped = s.parent(parent).is_some_and(|n| s.is(n, K::Paren)
        || s.field(parent) == Some(F::Test) && matches!(s.kind(n), Some(K::If | K::While | K::DoWhile | K::For)));
    if !wrapped { return false }
    let Some((a, b)) = s.get(parent, F::Left).zip(s.get(parent, F::Right)).map(|(a, b)| (s.unparen(a), s.unparen(b))) else { return false };
    if ![a, b].into_iter().all(|n| s.is(n, K::Binary) && matches!(s.word(n, F::Op), Some("<" | "<="))) { return false }
    let Some((al, ar, bl, br)) = s.get(a, F::Left).zip(s.get(a, F::Right))
        .zip(s.get(b, F::Left).zip(s.get(b, F::Right))).map(|((al, ar), (bl, br))| (al, ar, bl, br)) else { return false };
    match s.word(parent, F::Op) {
        Some("&&") => file.same_reference(ar, bl) && ordered(file, al, br),
        Some("||") => file.same_reference(al, br) && ordered(file, ar, bl),
        _ => false,
    }
}

pub fn comparison_orientation(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some((left, right)) = s.get(node, F::Left).zip(s.get(node, F::Right)) else { return Value::Null };
    let Some(op) = s.word(node, F::Op).filter(|op| matches!(*op, "==" | "===" | "!=" | "!==" | "<" | ">" | "<=" | ">=")) else { return Value::Null };
    Value::Record(vec![
        ("left".into(), Value::Bool(literal(file, left))),
        ("right".into(), Value::Bool(literal(file, right))),
        ("equality".into(), Value::Bool(matches!(op, "==" | "==="))),
        ("range".into(), Value::Bool(range(file, node))),
    ])
}

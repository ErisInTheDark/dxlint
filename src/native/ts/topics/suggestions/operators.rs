//! Logical assignment shorthand policies.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics;
use crate::expr::Value;

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
    Value::record(vec![("kind".into(), Value::from(kind)), ("operator".into(), Value::from(format!("{op}=")))])
}

//! Stable-reference replacements and truthiness context for nullish coalescing.

use super::*;
use crate::native::ts::statics;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn finding(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let (subject, truthiness, kind) = match s.kind(node) {
        Some(K::Logical | K::Assign) if matches!(s.word(node, F::Op), Some("||" | "||=")) => {
            let Some(left) = s.get(node, F::Left) else { return Value::Null };
            (left, true, if s.is(node, K::Assign) { "assignment" } else { "logical" })
        }
        Some(K::Conditional) => {
            let (Some(test), Some(yes), Some(no)) = (s.get(node, F::Test), s.get(node, F::Consequent), s.get(node, F::Alternate)) else { return Value::Null };
            let test = s.unparen(test);
            let negative = s.word(test, F::Op).is_some_and(|op| matches!(op, "!" | "==" | "===" | "||"));
            let Some((subject, truthiness)) = replacement(file, test, if negative { no } else { yes }) else { return Value::Null };
            (subject, truthiness, "ternary")
        }
        Some(K::If) if s.get(node, F::Alternate).is_none() => {
            let (Some(test), Some(mut body)) = (s.get(node, F::Test), s.get(node, F::Consequent)) else { return Value::Null };
            if s.is(body, K::Block) {
                let stmts: Vec<_> = s.all(body, F::Body).collect();
                if stmts.len() != 1 { return Value::Null }
                body = stmts[0];
            }
            let Some(assign) = s.get(body, F::Expr).map(|id| s.unparen(id)).filter(|&id| s.is(body, K::ExprStmt) && s.is(id, K::Assign)) else { return Value::Null };
            let Some(left) = s.get(assign, F::Left) else { return Value::Null };
            let test = s.unparen(test);
            if !matches!(s.word(test, F::Op), Some("!" | "==" | "===" | "||")) { return Value::Null }
            let Some((subject, truthiness)) = replacement(file, test, left) else { return Value::Null };
            (subject, truthiness, "if")
        }
        _ => return Value::Null,
    };
    let flags = file.type_flags(subject).unwrap_or_default();
    let offset = if matches!(kind, "logical" | "assignment") { s.get(node, F::Op).unwrap_or(node) } else { node };
    Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("flags".into(), Value::list(flags.into_iter().map(Value::from).collect())),
        ("truthiness".into(), Value::Bool(truthiness)),
        ("unknown".into(), Value::Bool(matches!(file.type_of(subject), Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown))),
        ("conditional".into(), Value::Bool(context(file, node, false))),
        ("boolean".into(), Value::Bool(context(file, node, true))),
        ("mixed".into(), Value::Bool(kind == "logical" && mixed(file, node, &mut Vec::new()))),
        ("offset".into(), Value::from(s.tree.node(offset).start as usize)),
    ])
}

fn stable(file: &NativeFile<'_>, a: u32, b: u32) -> bool {
    let s = &file.syntax;
    let (a, b) = (s.unparen(a), s.unparen(b));
    match (s.kind(a), s.kind(b)) {
        (Some(K::Ident | K::This | K::Super), Some(K::Ident | K::This | K::Super)) => s.kind(a) == s.kind(b) && s.text(a) == s.text(b),
        (Some(K::Member | K::Index), Some(K::Member | K::Index)) => {
            let same_object = s.get(a, F::Object).zip(s.get(b, F::Object)).is_some_and(|(x, y)| stable(file, x, y));
            if let Some(name) = statics::property_name(s, a) {
                same_object && statics::property_name(s, b).as_deref() == Some(&name)
            } else {
                s.get(a, F::Index).zip(s.get(b, F::Index)).is_some_and(|(x, y)| same_object && file.equal_tokens(x, y))
            }
        }
        _ => false,
    }
}

fn nullish(file: &NativeFile<'_>, node: u32) -> Option<&'static str> {
    let s = &file.syntax;
    let node = s.unparen(node);
    if s.is(node, K::Null) { Some("null") } else if s.is(node, K::Ident) && s.text(node) == "undefined" { Some("undefined") } else { None }
}

fn comparisons<'a>(file: &'a NativeFile<'_>, test: u32) -> Option<(&'a str, Vec<u32>)> {
    let s = &file.syntax;
    let test = s.unparen(test);
    let op = s.word(test, F::Op)?;
    if s.is(test, K::Binary) && matches!(op, "==" | "===" | "!=" | "!==") {
        return Some((op, vec![s.get(test, F::Left)?, s.get(test, F::Right)?]));
    }
    if s.is(test, K::Logical) && matches!(op, "&&" | "||") {
        let a = s.unparen(s.get(test, F::Left)?);
        let b = s.unparen(s.get(test, F::Right)?);
        let (ao, an) = comparisons(file, a)?;
        let (bo, bn) = comparisons(file, b)?;
        if an.len() != 2 || bn.len() != 2 || an.iter().all(|&n| nullish(file, n).is_some()) || bn.iter().all(|&n| nullish(file, n).is_some()) { return None }
        return match (op, ao, bo) {
            ("||", "==" | "===", "==" | "===") => Some((if ao == "==" || bo == "==" { "==" } else { "===" }, [an, bn].concat())),
            ("&&", "!=" | "!==", "!=" | "!==") => Some((if ao == "!=" || bo == "!=" { "!=" } else { "!==" }, [an, bn].concat())),
            _ => None,
        };
    }
    None
}

fn replacement(file: &NativeFile<'_>, test: u32, branch: u32) -> Option<(u32, bool)> {
    let s = &file.syntax;
    if let Some((op, nodes)) = comparisons(file, test) {
        let mut subject = None;
        let (mut checks_null, mut checks_undefined) = (false, false);
        for node in nodes {
            match nullish(file, node) {
                Some("null") => checks_null = true,
                Some("undefined") => checks_undefined = true,
                _ if stable(file, node, branch) => { subject.get_or_insert(node); }
                _ => return None,
            };
        }
        let subject = subject?;
        if !checks_null && !checks_undefined { return None }
        if checks_null && checks_undefined || matches!(op, "==" | "!=") { return Some((subject, false)) }
        let ty = file.type_of(subject);
        if matches!(ty, Ty::Unknown | Ty::IntrinsicUnknown | Ty::Any) { return None }
        let uncovered = if checks_null { "undefined" } else { "null" };
        if has_nullish(&ty, uncovered) { None } else { Some((subject, false)) }
    } else {
        let subject = if s.is(test, K::Unary) && s.word(test, F::Op) == Some("!") { s.get(test, F::Arg)? } else { test };
        stable(file, subject, branch).then_some((subject, true))
    }
}

fn has_nullish(ty: &Ty, name: &str) -> bool {
    match ty {
        Ty::Null => name == "null",
        Ty::Undefined | Ty::Void => name == "undefined",
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(|ty| has_nullish(ty, name)),
        _ => false,
    }
}

fn context(file: &NativeFile<'_>, mut node: u32, boolean: bool) -> bool {
    let s = &file.syntax;
    let mut direct_ternary = s.is(node, K::Conditional);
    loop {
        let Some(parent) = s.parent(node) else { return false };
        match s.kind(parent) {
            Some(K::Paren) => {}
            Some(K::Logical) => direct_ternary = false,
            Some(K::Conditional) if s.get(parent, F::Test) != Some(node) => direct_ternary = false,
            Some(K::Sequence) if s.all(parent, F::Expr).last() == Some(node) => direct_ternary = false,
            Some(K::Unary) if !boolean && s.word(parent, F::Op) == Some("!") => {}
            Some(K::Call) if boolean => {
                if direct_ternary { return false }
                return s.get(parent, F::Callee).is_some_and(|callee| file.global_value(callee).as_deref() == Some("Boolean"));
            }
            Some(K::Conditional | K::If | K::While | K::DoWhile | K::For) if !boolean => return s.get(parent, F::Test) == Some(node),
            _ => return false,
        }
        node = parent;
    }
}

fn mixed(file: &NativeFile<'_>, node: u32, visited: &mut Vec<u32>) -> bool {
    let s = &file.syntax;
    if visited.contains(&node) { return false }
    visited.push(node);
    if s.is(node, K::Paren) {
        return s.get(node, F::Expr).is_some_and(|n| mixed(file, n, visited)) || s.parent(node).is_some_and(|n| mixed(file, n, visited));
    }
    if !s.is(node, K::Logical) { return false }
    match s.word(node, F::Op) {
        Some("&&") => true,
        Some("||") => [s.parent(node), s.get(node, F::Left), s.get(node, F::Right)].into_iter().flatten().any(|n| mixed(file, n, visited)),
        _ => false,
    }
}

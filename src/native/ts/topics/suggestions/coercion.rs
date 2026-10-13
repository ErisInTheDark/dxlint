//! Shorthand coercion classification and explicit recommendations.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics;
use crate::expr::Value;

fn numeric(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    s.is(node, K::Number) || s.is(node, K::Call) && s.get(node, F::Callee)
        .is_some_and(|c| s.is(c, K::Ident) && matches!(s.text(c), "Number" | "parseInt" | "parseFloat"))
}

fn string(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    matches!(s.kind(node), Some(K::String | K::Template)) || s.is(node, K::Call) && s.get(node, F::Callee)
        .is_some_and(|c| s.is(c, K::Ident) && s.text(c) == "String")
}

fn empty(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    matches!(s.kind(node), Some(K::String | K::Template)) && statics::literal_text(s, node).as_deref() == Some("")
}

fn operand(file: &NativeFile, node: u32) -> String {
    let s = &file.syntax;
    let node = s.unparen(node);
    if s.is(node, K::Sequence) { format!("({})", s.text(node)) } else { s.text(node).to_string() }
}

fn classify(file: &NativeFile, node: u32) -> Option<(&'static str, &'static str, String)> {
    let s = &file.syntax;
    let op = s.word(node, F::Op);
    match s.kind(node)? {
        K::Unary => {
            let arg = s.unparen(s.get(node, F::Arg)?);
            if matches!(op, Some("!" | "-")) && s.is(arg, K::Unary) && s.word(arg, F::Op) == op {
                let inner = s.get(arg, F::Arg)?;
                if op == Some("!") { return Some(("boolean", "!!", format!("Boolean({})", operand(file, inner)))) }
                if !numeric(file, inner) { return Some(("number", "- -", format!("Number({})", operand(file, inner)))) }
            }
            if op == Some("+") && !numeric(file, arg) {
                return Some(("number", "+", format!("Number({})", operand(file, arg))));
            }
            if op == Some("~") && s.is(arg, K::Call) {
                let raw_callee = s.get(arg, F::Callee)?;
                let callee = s.unparen(raw_callee);
                if matches!(statics::property_name(s, callee).as_deref(), Some("indexOf" | "lastIndexOf")) {
                    let optional = s.has_word(arg, F::Optional, "?.") || !s.is(raw_callee, K::Paren) && s.has_word(callee, F::Optional, "?.");
                    return Some(("boolean", "~", format!("{} {}", s.text(arg), if optional { ">= 0" } else { "!== -1" })));
                }
            }
        }
        K::Binary | K::Assign => {
            let left = s.unparen(s.get(node, F::Left)?);
            let right = s.unparen(s.get(node, F::Right)?);
            if op == Some("*") {
                let one = |n| s.is(n, K::Number) && statics::number(s.text(n)) == Some(1.0);
                let fraction = one(right) && s.parent(node).is_some_and(|p| s.is(p, K::Binary) && s.word(p, F::Op) == Some("/") && s.field(node) == Some(F::Left));
                if (one(left) || one(right)) && !fraction {
                    let value = [right, left].into_iter().find(|&n| !s.is(n, K::Binary) && !numeric(file, n))?;
                    return Some(("number", "*", format!("Number({})", operand(file, value))));
                }
            }
            if op == Some("-") && s.is(right, K::Number) && statics::number(s.text(right)) == Some(0.0) && !numeric(file, left) {
                return Some(("number", "-", format!("Number({})", operand(file, left))));
            }
            if op == Some("+") && (empty(file, left) && !string(file, right) || empty(file, right) && !string(file, left)) {
                let value = if empty(file, left) { right } else { left };
                return Some(("string", "+", format!("String({})", operand(file, value))));
            }
            if op == Some("+=") && empty(file, right) {
                let text = s.text(left);
                return Some(("string", "+", format!("{text} = String({text})")));
            }
        }
        K::Template => {
            if s.parent(node).is_some_and(|p| s.is(p, K::TaggedTemplate)) { return None }
            let exprs: Vec<_> = s.all(node, F::Expr).collect();
            if exprs.len() == 1 && !string(file, exprs[0]) {
                let span = s.tree.node(exprs[0]);
                let template = s.tree.node(node);
                let prefix = &s.src[template.start as usize + 1..span.start as usize];
                let suffix = &s.src[span.end as usize..template.end as usize - 1];
                let empty_quasi = |text: &str| crate::expr::js::strings::decode_literal(&format!("`{text}`")).as_deref() == Some("");
                if prefix.trim_end().strip_suffix("${").is_some_and(empty_quasi)
                    && suffix.trim_start().strip_prefix('}').is_some_and(empty_quasi) {
                    return Some(("template", "", format!("String({})", operand(file, exprs[0]))));
                }
            }
        }
        _ => {}
    }
    None
}

pub fn coercion(file: &NativeFile, node: u32) -> Value<'static> {
    classify(file, node).map_or(Value::Null, |(kind, op, recommendation)| Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("operator".into(), Value::from(op)),
        ("recommendation".into(), Value::from(recommendation)),
    ]))
}

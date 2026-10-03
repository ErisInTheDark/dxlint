//! Expression identity and literal preferences.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics;
use crate::expr::Value;

struct WrittenGlobals(std::collections::HashSet<String>);

fn written_globals(file: &NativeFile) -> WrittenGlobals {
    WrittenGlobals(file.bindings().unresolved.iter().filter(|&&n| matches!(file.access(n), Some("write" | "read_write")))
        .map(|&n| file.syntax.text(n).to_string()).collect())
}

pub fn global_callee(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, F::Callee).map(|n| s.unparen(n)) else { return Value::Null };
    if s.is(callee, K::Ident) && file.resolved(callee) == Some("global") && file.global_setting(callee) != Some("off") {
        Value::from(s.text(callee).to_string())
    } else {
        Value::Null
    }
}

fn method_path(file: &NativeFile, node: u32, depth: usize) -> Option<String> {
    if depth > 16 { return None }
    let s = &file.syntax;
    let node = s.unparen(node);
    if matches!(s.kind(node), Some(K::Member | K::Index)) {
        let object = s.get(node, F::Object)?;
        let name = if s.is(node, K::Index) {
            file.static_string(s.get(node, F::Index)?)
        } else {
            statics::property_name(s, node)
        }?;
        let global = file.global_value(object)?;
        if file.cached(written_globals).0.contains(&global) { return None }
        return Some(format!("{global}.{name}"));
    }
    if s.is(node, K::Ident) {
        let decl = *file.bindings().refs.get(&node)?;
        let binding = file.binding(decl)?;
        if binding.writes != 0 { return None }
        let declarator = s.parent(decl).filter(|&n| s.is(n, K::Declarator))?;
        return method_path(file, s.get(declarator, F::Init)?, depth + 1);
    }
    None
}

pub fn global_method_callee(file: &NativeFile, node: u32) -> Value<'static> {
    file.syntax.get(node, F::Callee).and_then(|callee| method_path(file, callee, 0))
        .map_or(Value::Null, Value::from)
}

pub fn parse_int_callee(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, F::Callee).map(|n| s.unparen(n)) else { return Value::Null };
    let direct = s.is(callee, K::Ident) && s.text(callee) == "parseInt" && file.resolved(callee) == Some("global") && file.global_setting(callee) != Some("off");
    let member = matches!(s.kind(callee), Some(K::Member | K::Index))
        && statics::property_name(s, callee).as_deref() == Some("parseInt")
        && s.get(callee, F::Object).map(|n| s.unparen(n))
            .is_some_and(|n| s.is(n, K::Ident) && s.text(n) == "Number" && file.resolved(n) == Some("global") && file.global_setting(n) != Some("off"));
    if direct || member { Value::from(s.text(callee).to_string()) } else { Value::Null }
}

pub fn radix_error(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let args: Vec<_> = s.all(node, F::Arg).map(|n| s.unparen(n)).collect();
    if args.iter().take(2).any(|&n| s.is(n, K::Spread)) { return Value::Null }
    let error = match args.as_slice() {
        [] => Some("Missing parameters."),
        [_] => Some("Missing radix parameter."),
        [_, radix, ..] => {
            let undefined = s.is(*radix, K::Ident) && s.text(*radix) == "undefined" && file.resolved(*radix) == Some("global");
            let literal = matches!(s.kind(*radix), Some(K::Number | K::String | K::Bool | K::Null | K::Regex | K::BigInt))
                || s.is(*radix, K::Unary) && matches!(s.word(*radix, F::Op), Some("+" | "-"))
                    && s.get(*radix, F::Arg).is_some_and(|n| s.is(n, K::Number));
            let valid = statics::literal_value(s, *radix).is_some_and(|(kind, value)| kind == "number"
                && value.parse::<f64>().is_ok_and(|n| (2.0..=36.0).contains(&n) && n.fract() == 0.0));
            (undefined || literal && !valid).then_some("Invalid radix parameter, must be an integer between 2 and 36.")
        }
    };
    error.map_or(Value::Null, Value::from)
}

fn could_be_error(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    let recurse = |field| s.get(node, field).is_some_and(|n| could_be_error(file, n));
    match s.kind(node) {
        Some(K::Ident | K::Call | K::New | K::Member | K::Index | K::TaggedTemplate | K::Yield | K::Await) => true,
        Some(K::Assign) if matches!(s.word(node, F::Op), Some("=" | "&&=")) => recurse(F::Right),
        Some(K::Assign) if matches!(s.word(node, F::Op), Some("||=" | "??=")) => recurse(F::Left) || recurse(F::Right),
        Some(K::Logical) if s.word(node, F::Op) == Some("&&") => recurse(F::Right),
        Some(K::Logical) => recurse(F::Left) || recurse(F::Right),
        Some(K::Conditional) => recurse(F::Consequent) || recurse(F::Alternate),
        Some(K::Sequence) => s.all(node, F::Expr).last().is_some_and(|n| could_be_error(file, n)),
        _ => false,
    }
}

pub fn thrown_error(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(arg) = s.get(node, F::Arg).map(|n| s.unparen(n)) else { return Value::Null };
    let message = if !could_be_error(file, arg) { Some("Expected an error object to be thrown.") }
        else if s.is(arg, K::Ident) && s.text(arg) == "undefined" && file.resolved(arg) == Some("global") && file.global_setting(arg) != Some("off") {
            Some("Do not throw undefined.")
        } else { None };
    message.map_or(Value::Null, Value::from)
}

fn module_name(file: &NativeFile, node: u32) -> Option<String> {
    let s = &file.syntax;
    let node = s.unparen(node);
    match s.kind(node)? {
        K::Ident => super::super::super::regex::cook(&format!("\"{}\"", s.text(node))),
        K::String => statics::literal_text(s, node),
        _ => None,
    }
}

pub fn useless_rename(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let pair = match s.kind(node) {
        Some(K::ImportSpec) => s.get(node, F::Imported).zip(s.get(node, F::Local)),
        Some(K::ExportSpec) => s.get(node, F::Local).zip(s.get(node, F::Exported)),
        Some(K::PatternProp) => s.get(node, F::Key).zip(s.get(node, F::Value)),
        Some(K::Prop) if s.parent(node).is_some_and(|parent| file.write_site(parent).is_some()) => s.get(node, F::Key).zip(s.get(node, F::Value)),
        _ => None,
    };
    let Some((key, value)) = pair else { return Value::Null };
    let mut value = s.unparen(value);
    if matches!(s.kind(value), Some(K::AssignPattern | K::Assign)) {
        let Some(left) = s.get(value, F::Left) else { return Value::Null };
        value = s.unparen(left);
    }
    let Some(name) = module_name(file, key) else { return Value::Null };
    if module_name(file, value).as_deref() == Some(&name) { Value::from(name) } else { Value::Null }
}

pub fn useless_concat(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.is(node, K::Binary) || s.word(node, F::Op) != Some("+") { return Value::Null }
    let (Some(mut left), Some(mut right)) = (s.get(node, F::Left), s.get(node, F::Right)) else { return Value::Null };
    left = s.unparen(left);
    right = s.unparen(right);
    while s.is(left, K::Binary) && s.word(left, F::Op) == Some("+") {
        let Some(next) = s.get(left, F::Right) else { return Value::Null };
        left = s.unparen(next);
    }
    while s.is(right, K::Binary) && s.word(right, F::Op) == Some("+") {
        let Some(next) = s.get(right, F::Left) else { return Value::Null };
        right = s.unparen(next);
    }
    let strings = matches!(s.kind(left), Some(K::String | K::Template)) && matches!(s.kind(right), Some(K::String | K::Template));
    let between = &s.src[s.tree.node(left).end as usize..s.tree.node(right).start as usize];
    if strings && !between.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        s.get(node, F::Op).map_or(Value::Null, |op| Value::Num(f64::from(s.tree.node(op).start)))
    } else {
        Value::Null
    }
}

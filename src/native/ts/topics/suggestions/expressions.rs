//! Expression identity and literal preferences.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics;
use crate::expr::Value;

struct WrittenGlobals(std::collections::HashSet<String>);

pub fn redundant_conditional(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some((yes, no)) = s.get(node, F::Consequent).zip(s.get(node, F::Alternate)).map(|(a, b)| (s.unparen(a), s.unparen(b))) else { return Value::Null };
    if s.is(yes, K::Bool) && s.is(no, K::Bool) { return Value::from("boolean") }
    let Some(test) = s.get(node, F::Test).map(|n| s.unparen(n)) else { return Value::Null };
    if s.is(test, K::Ident) && s.is(yes, K::Ident) && s.text(test) == s.text(yes) { Value::from("default") } else { Value::Null }
}

pub fn decimal_escapes(file: &NativeFile, node: u32) -> Value<'static> {
    let text = file.syntax.text(node);
    let bytes = text.as_bytes();
    let mut findings = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            if matches!(bytes[i + 1], b'8' | b'9') {
                findings.push(Value::Record(vec![
                    ("text".into(), Value::from(text[i..i + 2].to_owned())),
                    ("offset".into(), Value::Num(f64::from(file.syntax.tree.node(node).start) + i as f64)),
                ]));
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    Value::List(findings)
}

pub fn mixed_concat(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let addition = |n| s.is(n, K::Binary) && s.word(n, F::Op) == Some("+");
    if !addition(node) { return Value::Bool(false) }
    let mut outer = node;
    while s.parent(outer).is_some_and(|p| s.is(p, K::Paren)) {
        outer = s.parent(outer).unwrap_or(outer);
    }
    if s.parent(outer).is_some_and(addition) { return Value::Bool(false) }
    let mut string = false;
    let mut other = false;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        let n = s.unparen(n);
        if addition(n) {
            stack.extend(s.get(n, F::Left));
            stack.extend(s.get(n, F::Right));
        } else if matches!(s.kind(n), Some(K::String | K::Template)) {
            string = true;
        } else {
            other = true;
        }
    }
    Value::Bool(string && other)
}

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

pub fn useless_call(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, F::Callee).map(|n| s.unparen(n)).filter(|&n| s.is(n, K::Member)) else { return Value::Null };
    let Some(name) = s.word(callee, F::Property) else { return Value::Null };
    let args: Vec<_> = s.all(node, F::Arg).collect();
    if !(name == "call" && !args.is_empty() || name == "apply" && args.len() == 2 && s.is(s.unparen(args[1]), K::Array)) { return Value::Null }
    let Some(applied) = s.get(callee, F::Object).map(|n| s.unparen(n)) else { return Value::Null };
    if unchanged_receiver(file, applied, args[0]) { Value::from(name.to_owned()) } else { Value::Null }
}

fn unchanged_receiver(file: &NativeFile, applied: u32, argument: u32) -> bool {
    let s = &file.syntax;
    let argument = s.unparen(argument);
    if matches!(s.kind(applied), Some(K::Member | K::Index)) {
        s.get(applied, F::Object).is_some_and(|owner| file.equal_tokens(owner, argument))
    } else {
        s.is(argument, K::Null) || s.is(argument, K::Ident) && s.text(argument) == "undefined"
            || s.is(argument, K::Unary) && s.word(argument, F::Op) == Some("void")
    }
}

pub fn variadic_apply(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, F::Callee).map(|n| s.unparen(n))
        .filter(|&n| statics::property_name(s, n).as_deref() == Some("apply")) else { return Value::Bool(false) };
    let args: Vec<_> = s.all(node, F::Arg).collect();
    if args.len() != 2 || matches!(s.kind(s.unparen(args[1])), Some(K::Array | K::Spread)) { return Value::Bool(false) }
    Value::Bool(s.get(callee, F::Object).map(|n| s.unparen(n)).is_some_and(|applied| unchanged_receiver(file, applied, args[0])))
}

pub fn extended_native(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let prototype = if s.is(node, K::Assign) {
        s.get(node, F::Left).map(|n| s.unparen(n)).filter(|&n| matches!(s.kind(n), Some(K::Member | K::Index)))
            .and_then(|n| s.get(n, F::Object))
    } else if s.is(node, K::Call) {
        let Some(callee) = s.get(node, F::Callee).map(|n| s.unparen(n)) else { return Value::Null };
        if !matches!(statics::property_name(s, callee).as_deref(), Some("defineProperty" | "defineProperties"))
            || !s.get(callee, F::Object).is_some_and(|n| s.is(s.unparen(n), K::Ident) && s.text(s.unparen(n)) == "Object") { return Value::Null }
        s.all(node, F::Arg).next()
    } else { return Value::Null };
    let Some(prototype) = prototype.map(|n| s.unparen(n)).filter(|&n| statics::property_name(s, n).as_deref() == Some("prototype")) else { return Value::Null };
    let Some(object) = s.get(prototype, F::Object).map(|n| s.unparen(n)).filter(|&n| s.is(n, K::Ident)) else { return Value::Null };
    let name = s.text(object);
    if !super::super::super::globals::is_builtin(name) || !name.starts_with(char::is_uppercase)
        || file.resolved(object) != Some("global") || file.global_setting(object) == Some("off") { return Value::Null }
    Value::from(name.to_owned())
}

pub fn multiline_string(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    Value::Bool(s.text(node).contains(['\r', '\n', '\u{2028}', '\u{2029}'])
        && !s.parent(node).is_some_and(|p| matches!(s.kind(p), Some(K::JsxAttribute | K::JsxExpression | K::JsxElement))))
}

pub fn eval_usage(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let (direct, optional, offset) = if s.is(node, K::Ident) && s.text(node) == "eval" {
        let mut outer = node;
        while let Some(parent) = s.parent(outer).filter(|&n| s.is(n, K::Paren)) { outer = parent; }
        let call = s.parent(outer).filter(|&n| s.is(n, K::Call) && s.get(n, F::Callee) == Some(outer));
        if call.is_none() && (file.resolved(node) != Some("global") || file.global_setting(node) == Some("off")) { return Value::Null }
        (call.is_some(), call.is_some_and(|n| s.get(n, F::Optional).is_some()), s.tree.node(node).start)
    } else if matches!(s.kind(node), Some(K::Member | K::Index)) && statics::property_name(s, node).as_deref() == Some("eval") {
        let Some(mut object) = s.get(node, F::Object).map(|n| s.unparen(n)) else { return Value::Null };
        let global = if s.is(object, K::This) {
            let mut child = object;
            let mut current = s.parent(child);
            loop {
                let Some(n) = current else { break false };
                match s.kind(n) {
                    Some(K::Program) => break file.source_type() != "module",
                    Some(K::Function) => break !super::statements::strict(file, n) && super::functions::default_this(file, n),
                    Some(K::Method) if s.field(child) != Some(F::Key) => break false,
                    Some(K::ClassProp) if s.field(child) == Some(F::Init) => break false,
                    Some(K::StaticBlock) => break false,
                    _ => {}
                }
                child = n;
                current = s.parent(n);
            }
        } else {
            let mut name = None;
            while matches!(s.kind(object), Some(K::Member | K::Index)) {
                let Some(property) = statics::property_name(s, object) else { break };
                if !matches!(property.as_str(), "global" | "window" | "globalThis") || name.as_ref().is_some_and(|n| n != &property) { break }
                name = Some(property);
                let Some(next) = s.get(object, F::Object).map(|n| s.unparen(n)) else { break };
                object = next;
            }
            s.is(object, K::Ident) && matches!(s.text(object), "global" | "window" | "globalThis")
                && name.as_ref().is_none_or(|name| name == s.text(object))
                && file.resolved(object) == Some("global") && file.global_setting(object).is_some_and(|setting| setting != "off")
        };
        if !global { return Value::Null }
        let Some(property) = s.get(node, if s.is(node, K::Index) { F::Index } else { F::Property }) else { return Value::Null };
        (false, false, s.tree.node(property).start)
    } else { return Value::Null };
    Value::Record(vec![
        ("direct".into(), Value::Bool(direct)),
        ("optional".into(), Value::Bool(optional)),
        ("offset".into(), Value::Num(f64::from(offset))),
    ])
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

/// Whether an expression can produce an error object, following possible value branches.
pub(crate) fn could_be_error(file: &NativeFile, node: u32) -> bool {
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

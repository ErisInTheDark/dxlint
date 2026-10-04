//! Regular expression construction and flag policies.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use crate::expr::Value;

fn escape_finding(character: char, offset: usize, regex: bool) -> Value<'static> {
    Value::record(vec![
        ("character".into(), Value::from(character.to_string())),
        ("offset".into(), Value::Num(offset as f64)),
        ("regex".into(), Value::Bool(regex)),
    ])
}

pub fn useless_escapes(file: &NativeFile, node: u32) -> Value<'static> {
    use super::super::super::regex::parse::{parse, Kind};
    let s = &file.syntax;
    let mut findings = vec![];
    if s.is(node, K::Regex) {
        let text = s.text(node);
        let Some(close) = text.rfind('/') else { return Value::list(findings) };
        let pattern = &text[1..close];
        let flags = &text[close + 1..];
        let sets = flags.contains('v');
        let Ok(ast) = parse(pattern, flags.contains('u'), sets) else { return Value::list(findings) };
        let offsets: Vec<_> = pattern.char_indices().map(|(i, _)| i).chain([pattern.len()]).collect();
        for (id, n) in ast.nodes.iter().enumerate() {
            let Kind::Character(value) = n.kind else { continue };
            let raw = ast.raw(id);
            let Some(character) = char::from_u32(value) else { continue };
            if raw != format!("\\{character}") { continue }
            let class = ast.path(id).into_iter().skip(1).find(|&p| matches!(ast.nodes[p].kind, Kind::Class { .. }));
            let allowed = if class.is_none() { "\\bcdDfnpPrsStvwWxu0123456789]^/.$*+?[{}|()Bk" }
                else if sets { "\\bcdDfnpPrsStvwWxu0123456789]q/[{}|()-" }
                else { "\\bcdDfnpPrsStvwWxu0123456789]" };
            if allowed.contains(character) { continue }
            if let Some(class) = class {
                let c = &ast.nodes[class];
                if character == '^' && c.start + 1 == n.start { continue }
                if !sets && character == '-' && c.start + 1 != n.start && n.end != c.end - 1 { continue }
                if sets && "!#$%&*+,.:;<=>?@^`~".contains(character) {
                    if ast.chars.get(n.end) == Some(&character) { continue }
                    if n.start > 0 && ast.chars.get(n.start - 1) == Some(&character) {
                        let negate = matches!(c.kind, Kind::Class { negate: true, .. });
                        if character != '^' || !negate || c.start + 1 < n.start - 1 { continue }
                    }
                }
            }
            findings.push(escape_finding(character, s.tree.node(node).start as usize + 1 + offsets[n.start], true));
        }
    } else {
        let template = s.is(node, K::Template);
        if s.parent(node).is_some_and(|p| s.is(p, K::TaggedTemplate) && s.field(node) == Some(F::Quasi)
            || matches!(s.kind(p), Some(K::JsxAttribute | K::JsxElement | K::JsxFragment))) { return Value::list(findings) }
        let parts: Vec<_> = if template { s.all(node, F::Quasi).collect() } else { vec![node] };
        for part in parts {
            let text = s.text(part);
            let quote = if template { '`' } else { text.chars().next().unwrap_or('"') };
            let chars: Vec<_> = text.char_indices().collect();
            let mut i = 0;
            while i + 1 < chars.len() {
                if chars[i].1 != '\\' { i += 1; continue }
                let character = chars[i + 1].1;
                let mut necessary = character.is_ascii_digit() || "\\nrvtbfux\r\n\u{2028}\u{2029}".contains(character) || character == quote;
                if template && character == '$' { necessary = chars.get(i + 2).is_some_and(|(_, c)| *c == '{') }
                if template && character == '{' { necessary = i > 0 && chars[i - 1].1 == '$' }
                if !necessary { findings.push(escape_finding(character, s.tree.node(part).start as usize + chars[i].0, false)) }
                i += 2;
            }
        }
    }
    Value::list(findings)
}

pub fn unnamed_captures(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let pattern_flags = if s.is(node, K::Regex) {
        let text = s.text(node);
        text.rfind('/').map(|close| (text[1..close].to_owned(), text[close + 1..].to_owned()))
    } else {
        s.get(node, F::Callee).filter(|&n| file.global_value(n).as_deref() == Some("RegExp")).and_then(|_| {
            let mut args = s.all(node, F::Arg);
            let pattern = file.static_string(args.next()?)?;
            let flags = args.next().and_then(|n| file.static_string(n)).unwrap_or_default();
            Some((pattern, flags))
        })
    };
    let Some((pattern, flags)) = pattern_flags else { return Value::list(vec![]) };
    let Ok(ast) = super::super::super::regex::parse::parse(&pattern, flags.contains('u'), flags.contains('v')) else { return Value::list(vec![]) };
    Value::list(ast.nodes.iter().enumerate()
        .filter(|(_, n)| matches!(n.kind, super::super::super::regex::parse::Kind::CapturingGroup(None)))
        .map(|(i, _)| Value::from(ast.raw(i))).collect())
}

pub fn literal_preference(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, F::Callee) else { return Value::Null };
    if file.global_value(callee).as_deref() != Some("RegExp") { return Value::Null }
    let is_static = |node| {
        let node = s.unparen(node);
        match s.kind(node) {
            Some(K::String) => true,
            Some(K::Template) => s.all(node, F::Expr).next().is_none(),
            Some(K::TaggedTemplate) => s.get(node, F::Tag).map(|n| s.unparen(n)).is_some_and(|tag| {
                super::super::super::statics::property_name(s, tag).as_deref() == Some("raw")
                    && s.get(tag, F::Object).map(|n| s.unparen(n)).is_some_and(|object|
                        s.is(object, K::Ident) && s.text(object) == "String"
                            && file.resolved(object) == Some("global") && file.global_setting(object) != Some("off"))
                    && s.get(node, F::Quasi).is_some_and(|template| s.all(template, F::Expr).next().is_none())
            }),
            _ => false,
        }
    };
    let args: Vec<_> = s.all(node, F::Arg).map(|n| s.unparen(n)).collect();
    if !(1..=2).contains(&args.len()) { return Value::Null }
    if args.iter().copied().all(is_static) {
        return Value::record(vec![
            ("wrapped".into(), Value::Bool(false)),
            ("message".into(), Value::from("Use a regular expression literal instead of the 'RegExp' constructor.")),
        ]);
    }
    if s.is(args[0], K::Regex) && (args.len() == 1 || is_static(args[1])) {
        return Value::record(vec![
            ("wrapped".into(), Value::Bool(true)),
            ("message".into(), Value::from(if args.len() == 1 {
                "Regular expression literal is unnecessarily wrapped within a 'RegExp' constructor."
            } else { "Use regular expression literal with flags instead of the 'RegExp' constructor." })),
        ]);
    }
    Value::Null
}

pub fn unicode_flags(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let flags = if s.is(node, K::Regex) {
        let text = s.text(node);
        let Some(close) = text.rfind('/') else { return Value::Null };
        text[close + 1..].to_owned()
    } else {
        let Some(callee) = s.get(node, F::Callee) else { return Value::Null };
        if file.global_value(callee).as_deref() != Some("RegExp") { return Value::Null }
        let mut args = s.all(node, F::Arg);
        if args.next().is_some_and(|n| s.is(s.unparen(n), K::Spread)) { return Value::Null }
        match args.next() {
            Some(flags) => {
                let Some(flags) = file.static_string(flags) else { return Value::Null };
                flags
            }
            None => String::new(),
        }
    };
    Value::record(vec![("flags".into(), Value::from(flags))])
}

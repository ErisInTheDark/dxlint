//! Regular expression construction and flag policies.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use crate::expr::Value;

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
        return Value::Record(vec![
            ("wrapped".into(), Value::Bool(false)),
            ("message".into(), Value::from("Use a regular expression literal instead of the 'RegExp' constructor.")),
        ]);
    }
    if s.is(args[0], K::Regex) && (args.len() == 1 || is_static(args[1])) {
        return Value::Record(vec![
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
    Value::Record(vec![("flags".into(), Value::from(flags))])
}

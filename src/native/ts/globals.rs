//! Edition built-ins, configured globals and authentic source declarations.

use std::collections::HashMap;

use super::comments::Comment;
use super::environment::Environment;
use crate::expr::Value;

// eslint v10.12.0, conf/globals.js: additions by edition.
const BUILTINS: &[(u16, &[&str])] = &[
    (3, &[
        "Array", "Boolean", "constructor", "Date", "decodeURI", "decodeURIComponent", "encodeURI", "encodeURIComponent",
        "Error", "escape", "eval", "EvalError", "Function", "hasOwnProperty", "Infinity", "isFinite", "isNaN",
        "isPrototypeOf", "Math", "NaN", "Number", "Object", "parseFloat", "parseInt", "propertyIsEnumerable", "RangeError",
        "ReferenceError", "RegExp", "String", "SyntaxError", "toLocaleString", "toString", "TypeError", "undefined",
        "unescape", "URIError", "valueOf",
    ]),
    (5, &["JSON"]),
    (2015, &[
        "ArrayBuffer", "DataView", "Float32Array", "Float64Array", "Int16Array", "Int32Array", "Int8Array", "Intl", "Map",
        "Promise", "Proxy", "Reflect", "Set", "Symbol", "Uint16Array", "Uint32Array", "Uint8Array", "Uint8ClampedArray",
        "WeakMap", "WeakSet",
    ]),
    (2017, &["Atomics", "SharedArrayBuffer"]),
    (2020, &["BigInt", "BigInt64Array", "BigUint64Array", "globalThis"]),
    (2021, &["AggregateError", "FinalizationRegistry", "WeakRef"]),
    (2025, &["Float16Array", "Iterator"]),
    (2026, &["AsyncDisposableStack", "DisposableStack", "SuppressedError", "Temporal"]),
];

const COMMONJS: &[(&str, &str)] = &[("exports", "writable"), ("global", "readonly"), ("module", "readonly"), ("require", "readonly")];

/// How a global is configured: `readonly`, `writable` or `off`.
pub struct Globals {
    ecma_version: u16,
    commonjs: bool,
    configured: HashMap<String, &'static str>,
    /// From `/* global */` comments (the last setting of a name wins).
    declared: HashMap<String, &'static str>,
    /// Comment-declared names with the offset of their first mention, in
    /// order.
    pub mentions: Vec<(String, u32)>,
}

impl Globals {
    pub fn new(src: &str, comments: &[Comment], environment: &Environment, source_type: &str) -> Self {
        let mut configured = HashMap::new();
        for value in &environment.globals {
            let Value::Record(entries) = value else { continue };
            for (name, value) in entries.iter() {
                let setting = match value.text().as_ref() {
                    "readonly" => "readonly", "writable" => "writable", "off" => "off", _ => continue,
                };
                configured.insert(name.to_string(), setting);
            }
        }
        let mut declared = HashMap::new();
        let mut mentions: Vec<(String, u32)> = Vec::new();
        for c in comments.iter().filter(|c| c.block) {
            let body = c.body(src).trim_start();
            let body_at = body.as_ptr() as usize - src.as_ptr() as usize;
            let Some(rest): Option<&str> = body.strip_prefix("globals").or_else(|| body.strip_prefix("global")) else { continue };
            if !rest.starts_with(char::is_whitespace) {
                continue;
            }
            // `a, b: writable` or `a b:true`: names, each with an optional value.
            let normal = rest.replace(char::is_whitespace, " ");
            let parts = normal.split(',').flat_map(|p| {
                // `a: true b` splits at spaces outside a `name: value` pair.
                let mut p = p.to_string();
                while p.contains(" :") || p.contains(": ") {
                    p = p.replace(" :", ":").replace(": ", ":");
                }
                p.split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>()
            });
            for item in parts {
                let (name, value) = item.split_once(':').map_or((item.as_str(), ""), |(n, v)| (n, v));
                let setting = match value.trim() {
                    "true" | "writable" | "writeable" => "writable",
                    "off" => "off",
                    _ => "readonly",
                };
                if !name.is_empty() {
                    declared.insert(name.to_string(), setting);
                    if !mentions.iter().any(|(n, _)| n == name) {
                        let rest_at = body_at + body.len() - rest.len();
                        let at = name_offset(rest, name).map_or(c.start, |i| (rest_at + i) as u32);
                        mentions.push((name.to_string(), at));
                    }
                }
            }
        }
        Globals { declared, mentions, configured, ecma_version: environment.ecma_version, commonjs: source_type == "commonjs" }
    }

    /// The setting of global `name`, or `None` when nothing declares it.
    pub fn setting(&self, name: &str) -> Option<&'static str> {
        if let Some(&s) = self.declared.get(name) {
            return Some(s);
        }
        self.implicit_setting(name)
    }

    fn implicit_setting(&self, name: &str) -> Option<&'static str> {
        if let Some(&setting) = self.configured.get(name) {
            return Some(setting);
        }
        if self.commonjs {
            if let Some((_, setting)) = COMMONJS.iter().find(|(global, _)| *global == name) {
                return Some(setting);
            }
        }
        BUILTINS.iter().find(|(_, names)| names.contains(&name))
            .map(|(edition, _)| if *edition <= self.ecma_version { "readonly" } else { "off" })
    }

    pub fn is_builtin(&self, name: &str) -> bool {
        BUILTINS.iter().any(|(edition, names)| *edition <= self.ecma_version && names.contains(&name))
    }

    /// A global supplied by the environment rather than source declarations.
    pub fn is_implicit(&self, name: &str) -> bool {
        matches!(self.implicit_setting(name), Some("readonly" | "writable"))
    }

    pub fn origin(&self, name: &str) -> Option<&'static str> {
        if self.declared.contains_key(name) { Some("comment") }
        else if self.configured.contains_key(name) { Some("configuration") }
        else if self.commonjs && COMMONJS.iter().any(|(global, _)| *global == name) { Some("commonjs") }
        else if self.is_builtin(name) { Some("builtin") }
        else { None }
    }

    /// Whether a `/* global */` comment names it (rather than the
    /// built-ins).
    pub fn in_comment(&self, name: &str) -> bool {
        self.declared.contains_key(name)
    }
}

/// Where `name` stands as a whole item in a directive's text (between
/// spaces, commas and a `:value`).
fn name_offset(body: &str, name: &str) -> Option<usize> {
    let sep = |c: Option<char>| c.is_none_or(|c| c.is_whitespace() || c == ',');
    body.match_indices(name).map(|(i, _)| i).find(|&i| {
        sep(body[..i].chars().next_back()) && {
            let after = body[i + name.len()..].chars().next();
            sep(after) || after == Some(':')
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_comments_declare_and_switch_off() {
        let src = "/* global a, b: writable, c:true */\n/*globals Promise:off d*/ x";
        let comment = |from: usize| {
            let start = src[from..].find("/*").unwrap() + from;
            Comment { start: start as u32, end: (src[start..].find("*/").unwrap() + start + 2) as u32, block: true }
        };
        let first = comment(0);
        let comments = vec![first, comment(first.end as usize)];
        let g = Globals::new(src, &comments, &Environment::default(), "script");
        assert_eq!(g.setting("a"), Some("readonly"));
        assert_eq!(g.setting("b"), Some("writable"));
        assert_eq!(g.setting("c"), Some("writable"));
        assert_eq!(g.setting("d"), Some("readonly"));
        assert_eq!(g.setting("Promise"), Some("off"));
        assert_eq!(g.setting("NaN"), Some("readonly"));
        assert_eq!(g.setting("window"), None);
    }
}

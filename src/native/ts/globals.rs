//! Global variables as eslint configures them: the ECMAScript built-ins
//! (eslint's `conf/globals.js`, latest edition, all read-only) and the
//! file's `/* global name, other: writable, Promise: off */` comments.

use std::collections::HashMap;

use super::comments::Comment;

/// eslint's ES2026 globals.
const BUILTINS: &[&str] = &[
    "AggregateError", "Array", "ArrayBuffer", "AsyncDisposableStack", "Atomics", "BigInt", "BigInt64Array", "BigUint64Array",
    "Boolean", "constructor", "DataView", "Date", "decodeURI", "decodeURIComponent", "DisposableStack", "encodeURI",
    "encodeURIComponent", "Error", "escape", "eval", "EvalError", "FinalizationRegistry", "Float16Array", "Float32Array",
    "Float64Array", "Function", "globalThis", "hasOwnProperty", "Infinity", "Int16Array", "Int32Array", "Int8Array", "Intl",
    "isFinite", "isNaN", "isPrototypeOf", "Iterator", "JSON", "Map", "Math", "NaN", "Number", "Object", "parseFloat", "parseInt",
    "Promise", "propertyIsEnumerable", "Proxy", "RangeError", "ReferenceError", "Reflect", "RegExp", "Set", "SharedArrayBuffer",
    "String", "SuppressedError", "Symbol", "SyntaxError", "Temporal", "toLocaleString", "toString", "TypeError", "Uint16Array",
    "Uint32Array", "Uint8Array", "Uint8ClampedArray", "undefined", "unescape", "URIError", "valueOf", "WeakMap", "WeakRef",
    "WeakSet",
];

/// One of the ECMAScript built-in globals.
pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

/// How a global is configured: `readonly`, `writable` or `off`.
pub struct Globals {
    /// From `/* global */` comments (the last setting of a name wins).
    declared: HashMap<String, &'static str>,
}

impl Globals {
    pub fn new(src: &str, comments: &[Comment]) -> Self {
        let mut declared = HashMap::new();
        for c in comments.iter().filter(|c| c.block) {
            let body = c.body(src).trim_start();
            let Some(rest) = body.strip_prefix("globals").or_else(|| body.strip_prefix("global")) else { continue };
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
                }
            }
        }
        Globals { declared }
    }

    /// The setting of global `name`, or `None` when nothing declares it.
    pub fn setting(&self, name: &str) -> Option<&'static str> {
        if let Some(&s) = self.declared.get(name) {
            return Some(s);
        }
        BUILTINS.contains(&name).then_some("readonly")
    }

    /// Whether a `/* global */` comment names it (rather than the
    /// built-ins).
    pub fn in_comment(&self, name: &str) -> bool {
        self.declared.contains_key(name)
    }
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
        let g = Globals::new(src, &comments);
        assert_eq!(g.setting("a"), Some("readonly"));
        assert_eq!(g.setting("b"), Some("writable"));
        assert_eq!(g.setting("c"), Some("writable"));
        assert_eq!(g.setting("d"), Some("readonly"));
        assert_eq!(g.setting("Promise"), Some("off"));
        assert_eq!(g.setting("NaN"), Some("readonly"));
        assert_eq!(g.setting("window"), None);
    }
}

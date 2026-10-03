//! Explicit strict-mode directive policies.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use crate::expr::Value;

fn directives(file: &NativeFile, node: u32) -> Vec<u32> {
    let s = &file.syntax;
    let body = if s.is(node, K::Program) { Some(node) } else { s.get(node, F::Body).filter(|&n| s.is(n, K::Block)) };
    body.into_iter().flat_map(|n| s.all(n, F::Body))
        .take_while(|&n| s.is(n, K::ExprStmt) && s.get(n, F::Expr)
            .is_some_and(|e| s.is(s.unparen(e), K::String) && file.static_string(e).as_deref() == Some("use strict")))
        .collect()
}

pub fn policy(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let own = directives(file, node);
    let program = s.is(node, K::Program);
    let simple = s.all(node, F::Param).all(|p| s.get(p, F::Name).is_some_and(|n| s.is(n, K::Ident)) && s.get(p, F::Init).is_none());
    let mut parent_strict = false;
    let mut parent_function = false;
    let mut in_class = false;
    let mut child = node;
    while let Some(parent) = s.parent(child) {
        if matches!(s.kind(parent), Some(K::Function | K::Method | K::Arrow)) {
            parent_function = true;
            parent_strict |= !directives(file, parent).is_empty();
        }
        if s.is(parent, K::Class) && s.field(child) == Some(F::Member) { in_class = true }
        child = parent;
    }
    let mut policies = Vec::new();
    for mode in ["global", "function", "never", "module"] {
        let mut reports = Vec::new();
        let mut add = |at: u32, message: String| reports.push(Value::Record(vec![
            ("offset".into(), Value::Num(f64::from(s.tree.node(at).start))),
            ("message".into(), Value::from(message)),
        ]));
        if program && mode == "global" && own.is_empty() {
            if let Some(first) = s.all(node, F::Body).next() {
                add(first, "Use the global form of 'use strict'.".into());
            }
        }
        if !program && mode == "function" && own.is_empty() && !parent_function && !in_class {
            add(node, if simple { "Use the function form of 'use strict'.".into() }
                else { format!("Wrap {} in a function with 'use strict' directive.", file.function_name_with_kind(node).unwrap_or_else(|| "function".into())) });
        }
        for (index, &directive) in own.iter().enumerate() {
            let message = if program {
                match mode {
                    "global" if index == 0 => continue,
                    "global" => "Multiple 'use strict' directives.",
                    "function" => "Use the function form of 'use strict'.",
                    "never" => "Strict mode is not permitted.",
                    _ => "'use strict' is unnecessary inside of modules.",
                }
            } else if !simple {
                if index == 0 { "'use strict' directive inside a function with non-simple parameter list throws a syntax error since ES2016." }
                else { "Multiple 'use strict' directives." }
            } else if mode == "function" {
                if index > 0 { "Multiple 'use strict' directives." }
                else if parent_strict { "Unnecessary 'use strict' directive." }
                else if in_class { "'use strict' is unnecessary inside of classes." }
                else { continue }
            } else {
                match mode {
                    "global" => "Use the global form of 'use strict'.",
                    "never" => "Strict mode is not permitted.",
                    _ => "'use strict' is unnecessary inside of modules.",
                }
            };
            add(directive, message.into());
        }
        policies.push((mode.into(), Value::List(reports)));
    }
    Value::Record(policies)
}

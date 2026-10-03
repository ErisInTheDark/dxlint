//! Script-global declaration metadata; global policy lives in scoping.zlint.

use std::collections::HashSet;

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

struct ExportedNames(HashSet<String>);

pub(super) fn redeclarations(file: &NativeFile, _: u32) -> Value<'static> {
    let s = &file.syntax;
    let ts = matches!(file.path.extension().and_then(|e| e.to_str()), Some("ts" | "tsx" | "mts" | "cts"));
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for comment in file.comments().iter().filter(|c| c.block) {
        let globals = crate::native::ts::globals::Globals::new(s.src, std::slice::from_ref(comment));
        for (name, offset) in globals.mentions {
            let builtin = crate::native::ts::globals::is_builtin(&name)
                && matches!(file.globals().setting(&name), Some("readonly" | "writable"));
            let syntax = file.source_type() == "script" && file.bindings().decl_scope.iter().any(|(&id, &scope)| scope == 0 && s.text(id) == name);
            let duplicate = !seen.insert(name.clone());
            out.push(Value::Record(vec![
                ("name".into(), Value::from(name)),
                ("offset".into(), Value::from(offset as usize)),
                ("builtin".into(), Value::Bool(builtin)),
                ("syntax".into(), Value::Bool(syntax && !ts)),
                ("duplicate".into(), Value::Bool(duplicate)),
            ]));
        }
    }
    Value::List(out)
}

pub(super) fn declarations(file: &NativeFile, node: u32) -> Value<'static> {
    if file.source_type() != "script" {
        return Value::Null;
    }
    let s = &file.syntax;
    let b = file.bindings();
    let exported = file.cached(|f| ExportedNames(crate::native::ts::unused::exported_names(f)));
    let owners: Vec<u32> = if s.is(node, K::Var) { s.all(node, F::Decl).collect() } else { vec![node] };
    let mut entries = Vec::new();
    for owner in owners {
        let Some(name) = s.get(owner, F::Name) else { continue };
        let span = s.tree.node(name);
        let mut declarations: Vec<u32> = b.decl_scope.iter().filter_map(|(&id, &scope)| {
            let n = s.tree.node(id);
            (scope == 0 && span.start <= n.start && n.end <= span.end).then_some(id)
        }).collect();
        declarations.sort_by_key(|&id| s.tree.node(id).start);
        for id in declarations {
            let kind = super::bindings::declaration_kind(s, id);
            if !matches!(kind, "var" | "let" | "const" | "function" | "class") || exported.0.contains(s.text(id)) {
                continue;
            }
            let setting = file.globals().setting(s.text(id)).filter(|&v| v != "off").unwrap_or("new");
            let label = match kind {
                "function" | "class" => kind.to_string(),
                kind => format!("'{kind}'"),
            };
            entries.push(Value::Record(vec![
                ("kind".into(), Value::from(kind)),
                ("label".into(), Value::from(label)),
                ("setting".into(), Value::from(setting)),
                ("offset".into(), Value::from(s.tree.node(owner).start as usize)),
            ]));
        }
    }
    Value::List(entries)
}

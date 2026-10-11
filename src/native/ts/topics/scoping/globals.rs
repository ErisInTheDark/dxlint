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
        let globals = crate::native::ts::globals::Globals::new(s.src, std::slice::from_ref(comment), &file.environment, file.source_type());
        for (name, offset) in globals.mentions {
            let builtin = file.globals().is_implicit(&name)
                && matches!(file.globals().setting(&name), Some("readonly" | "writable"));
            let syntax = file.source_type() == "script" && file.bindings().decl_scope.iter().any(|(&id, &scope)| scope == 0 && crate::native::ts::statics::identifier_name(s.text(id)) == name);
            let duplicate = !seen.insert(name.clone());
            out.push(Value::record(vec![
                ("name".into(), Value::from(name)),
                ("offset".into(), Value::from(offset as usize)),
                ("builtin".into(), Value::Bool(builtin)),
                ("syntax".into(), Value::Bool(syntax && !ts)),
                ("duplicate".into(), Value::Bool(duplicate)),
            ]));
        }
    }
    Value::list(out)
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
            let name = crate::native::ts::statics::identifier_name(s.text(id));
            entries.push(Value::record(vec![
                ("kind".into(), Value::from(kind)),
                ("exported".into(), Value::Bool(exported.0.contains(&name))),
                ("setting".into(), file.globals().setting(&name).map_or(Value::Null, Value::from)),
                ("name".into(), Value::from(name)),
                ("offset".into(), Value::from(s.tree.node(owner).start as usize)),
            ]));
        }
    }
    Value::list(entries)
}

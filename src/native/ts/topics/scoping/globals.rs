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
            let syntax = file.source_type() == "script" && file.bindings().decl_scope.iter().any(|(&id, &scope)| scope == 0 && s.text(id) == name);
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
            let name = s.text(id);
            entries.push(Value::record(vec![
                ("kind".into(), Value::from(kind)),
                ("name".into(), Value::from(name.to_string())),
                ("exported".into(), Value::Bool(exported.0.contains(name))),
                ("setting".into(), file.globals().setting(name).map_or(Value::Null, Value::from)),
                ("offset".into(), Value::from(s.tree.node(owner).start as usize)),
            ]));
        }
    }
    Value::list(entries)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn declaration_metadata_keeps_bindings_exempted_by_standard_rules() {
        let linter = native_linter(r#"
language .js { set sourceType "script" }
in ts_variable_declaration rule probe {
  each d in scoping_global_declarations
  when (d.exported && d.name == "a") || d.kind == "using"
  report at d.offset
  warn "binding"
}
rule no-implicit-globals { set lexicalBindings true }
"#);
        let source = "/* exported a */ const [a, b] = []; using resource = open();";
        let reports = linter.lint(Path::new("/root/a.js"), source);
        assert_eq!(reports.iter().filter(|r| r.rule == "probe").count(), 2, "{reports:#?}");
        assert_eq!(reports.iter().filter(|r| r.rule == "no-implicit-globals").count(), 1, "{reports:#?}");
        assert_eq!(reports.len(), 3, "{reports:#?}");
    }

    #[test]
    fn declaration_metadata_preserves_missing_and_disabled_global_settings() {
        let linter = native_linter(r#"
language .js { set sourceType "script" }
in ts_variable_declaration rule probe {
  each d in scoping_global_declarations
  when (d.name == "absent" && d.setting == null) || (d.name == "disabled" && d.setting == "off")
  report at d.offset
  warn "binding"
}
"#);
        let reports = linter.lint(Path::new("/root/a.js"), "/* global disabled: off */ var absent, disabled;");
        assert_eq!(reports.len(), 2, "{reports:#?}");
        assert!(reports.iter().all(|r| r.rule == "probe"), "{reports:#?}");
    }

    #[test]
    fn global_rule_owns_binding_exemptions_without_changing_declarator_sites() {
        let path = Path::new("/root/a.js");
        for lexical in [false, true] {
            let linter = native_linter(&format!(r#"
language .js {{ set sourceType "script" }}
rule no-implicit-globals {{ set lexicalBindings {lexical} }}
"#));
            let source = "var [a, b] = []; function f() {} let l; const c = 1; class C {}";
            let reports = linter.lint(path, source);
            assert_eq!(reports.len(), if lexical { 5 } else { 2 }, "{reports:#?}");
            assert_eq!(reports[0].start, source.find("[a").unwrap());
            assert!(reports.iter().all(|r| r.rule == "no-implicit-globals"), "{reports:#?}");
            let reports = linter.lint(path, "/* global writable: true, readonly: false, disabled: off */ var writable, readonly, disabled;");
            assert_eq!(reports.len(), 2, "{reports:#?}");
        }
        for options in ["set sourceType \"module\"", "set sourceType \"script\"\nset globalReturn true"] {
            let linter = native_linter(&format!("language .js {{ {options} }}\nrule no-implicit-globals {{ set lexicalBindings true }}"));
            assert!(linter.lint(path, "var a; function f() {} let l; const c = 1; class C {}").is_empty());
        }
    }
}

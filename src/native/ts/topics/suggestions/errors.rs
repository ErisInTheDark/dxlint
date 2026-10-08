//! Thrown constructions and their enclosing catch bindings.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics;
use crate::expr::Value;

fn node_data(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let bindings = file.bindings();
    let binding = bindings.refs.get(&node).copied()
        .or_else(|| bindings.decl_scope.contains_key(&node).then_some(node));
    Value::record(vec![
        ("kind".into(), s.kind(node).map_or(Value::Null, |kind| Value::from(kind.name()))),
        ("name".into(), if s.is(node, K::Ident) { Value::from(s.text(node).to_owned()) } else { Value::Null }),
        ("binding".into(), binding.map_or(Value::Null, |id| Value::from(id as usize))),
        ("offset".into(), Value::from(s.tree.node(node).start as usize)),
    ])
}

pub fn catch_throw(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(call) = s.get(node, F::Arg).map(|n| s.unparen(n)).filter(|&n| matches!(s.kind(n), Some(K::Call | K::New))) else { return Value::Null };
    let Some(callee) = s.get(call, F::Callee).map(|n| s.unparen(n)) else { return Value::Null };
    let target = if s.is(callee, K::Member) { s.get(callee, F::Property).unwrap_or(callee) } else { callee };
    let global = s.is(callee, K::Ident) && file.resolved(callee) == Some("global") && file.global_setting(callee) != Some("off");
    let mut child = node;
    let catch = loop {
        let Some(parent) = s.parent(child) else { return Value::Null };
        if matches!(s.kind(parent), Some(K::Function | K::Method | K::Arrow | K::StaticBlock)) { return Value::Null }
        if s.is(parent, K::Try) && s.field(child) == Some(F::Handler) { break parent }
        child = parent;
    };
    let parameter = s.get(catch, F::Param);
    let offset = s.tree.node(node).start;
    let arguments = s.all(call, F::Arg).map(|arg| {
        let arg = s.unparen(arg);
        let properties = if s.is(arg, K::Object) { s.all(arg, F::Property).map(|property| {
            let value = s.get(property, F::Value).map(|n| s.unparen(n)).unwrap_or(property);
            Value::record(vec![
                ("kind".into(), s.kind(property).map_or(Value::Null, |kind| Value::from(kind.name()))),
                ("name".into(), statics::property_name(s, property).map_or(Value::Null, Value::from)),
                ("value".into(), node_data(file, value)),
                ("offset".into(), Value::from(s.tree.node(property).start as usize)),
            ])
        }).collect() } else { Vec::new() };
        Value::record(vec![
            ("kind".into(), s.kind(arg).map_or(Value::Null, |kind| Value::from(kind.name()))),
            ("properties".into(), Value::list(properties)),
        ])
    }).collect();
    Value::record(vec![
        ("name".into(), if s.is(target, K::Ident) { Value::from(s.text(target).to_owned()) } else { Value::Null }),
        ("global".into(), Value::Bool(global)),
        ("parameter".into(), parameter.map_or(Value::Null, |n| node_data(file, n))),
        ("catch_offset".into(), Value::Num(f64::from(s.get(catch, F::Kind).map_or(offset, |n| s.tree.node(n).start)))),
        ("throw_offset".into(), Value::Num(f64::from(offset))),
        ("arguments".into(), Value::list(arguments)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn throw_metadata_exposes_arbitrary_object_properties_and_bound_values() {
        let linter = native_linter(r#"
in ts_throw rule probe {
  let caught catch_throw
  each property in (last(caught.arguments).properties.nonempty ?? [])
  when property.name == "details" && get(property.value, "binding") == { value caught; get "parameter"; get "binding" }
  report at property.value.offset
  warn "binding"
}
"#);
        for source in [
            "try {} catch (error) { throw new CustomError('text', {details: error, cause: error}); }",
            "try {} catch (error) { throw new handlers[kind]('text', {details: error}); }",
        ] {
            let reports = linter.lint(Path::new("/root/a.mjs"), source);
            assert_eq!(reports.len(), 1, "{source}\n{reports:#?}");
            assert!(reports.iter().all(|r| r.rule == "probe"), "{reports:#?}");
            assert_eq!(reports[0].start, source.find("details: error").unwrap() + "details: ".len());
        }
    }

    #[test]
    fn throw_metadata_distinguishes_same_named_bindings_and_keeps_spreads() {
        let linter = native_linter(r#"
in ts_throw rule probe {
  let caught catch_throw
  each property in (last(caught.arguments).properties.nonempty ?? [])
  when property.kind == "ts_spread" || property.name == "cause" && property.value.name == get(caught, "parameter").name && get(property.value, "binding") != { value caught; get "parameter"; get "binding" }
  report at property.offset
  warn "binding"
}
"#);
        let source = "try {} catch (error) { { let error; throw new Error('text', {cause: error, ...options}); } }";
        let reports = linter.lint(Path::new("/root/a.mjs"), source);
        assert_eq!(reports.len(), 2, "{reports:#?}");
        assert!(reports.iter().all(|r| r.rule == "probe"), "{reports:#?}");
    }

    #[test]
    fn cause_decisions_distinguish_binding_identity_and_uncertainty() {
        for (source, decision) in [
            ("try {} catch (error) { throw new Error('text', {}); }", "missing"),
            ("try {} catch (error) { throw new Error('text', {cause: other}); }", "incorrect"),
            ("try {} catch (error) { { let error; throw new Error('text', {cause: error}); } }", "shadowed"),
            ("try {} catch (error) { throw new Error('text', {cause: error}); }", "preserved"),
            ("try {} catch (error) { throw new Error('text', options); }", "unknown"),
            ("try {} catch (error) { throw new Error('text', {cause: other, ...options}); }", "unknown"),
        ] {
            let linter = native_linter(&format!(r#"
in ts_throw rule probe {{
  let caught catch_throw
  when {{
    value parameter: get(caught, "parameter")
    value caught.arguments
    slice 1
    cause: first
    value caught.arguments
    leading: slice 0 1
    value caught_throw_cause_kind(parameter, cause, leading)
  }} == "{decision}"
  warn "decision"
}}
"#));
            let reports = linter.lint(Path::new("/root/a.mjs"), source);
            assert_eq!(reports.len(), 1, "{decision}\n{source}\n{reports:#?}");
            assert!(reports.iter().all(|r| r.rule == "probe"), "{reports:#?}");
        }
    }

    #[test]
    fn caught_error_policy_preserves_reporting_and_selected_sites() {
        for (source, options, site) in [
            ("try {} catch (error) { throw new Error('text'); }", "", "throw"),
            ("try {} catch (error) { throw new Error('text', {cause: other}); }", "", "other"),
            ("try {} catch (error) { { let error; throw new Error('text', {cause: error}); } }", "", "throw"),
            ("try {} catch ({message}) { throw new Error('text'); }", "", "catch"),
            ("try {} catch { throw new Error('text'); }", "set requireCatchParameter true", "throw"),
            ("try {} catch (error) { throw new AggregateError([], 'text', {}); }", "", "throw"),
            ("try {} catch (error) { throw new CustomError('text', {cause: other}); }",
                "set errorClassNames [{name: 'CustomError', argumentPosition: 0}]", "other"),
        ] {
            let linter = native_linter(&format!("rule preserve-caught-error {{ {options} }}"));
            let reports = linter.lint(Path::new("/root/a.mjs"), source);
            assert_eq!(reports.len(), 1, "{source}\n{reports:#?}");
            assert_eq!(reports[0].rule, "preserve-caught-error");
            assert_eq!(reports[0].start, source.find(site).unwrap(), "{source}");
        }
    }

    #[test]
    fn caught_error_policy_keeps_uncertainty_and_nearest_scope_exemptions() {
        let linter = native_linter("rule preserve-caught-error");
        for source in [
            "try {} catch (error) { throw new Error('text', {cause: other, cause: error}); }",
            "try {} catch (error) { throw new Error(...args, {cause: other}); }",
            "try {} catch (error) { throw new Error('text', {cause: other, ...options}); }",
            "try {} catch (error) { throw new Error('text', options); }",
            "try {} catch { throw new Error('text'); }",
            "try {} catch (error) { (() => { throw new Error('text'); })(); }",
            "const Error = CustomError; try {} catch (error) { throw new Error('text'); }",
        ] {
            let reports = linter.lint(Path::new("/root/a.mjs"), source);
            assert!(reports.is_empty(), "{source}\n{reports:#?}");
        }
        let linter = native_linter("rule preserve-caught-error { set errorClassNames [{name: 'CustomError', argumentPosition: 0}] }");
        assert!(linter.lint(Path::new("/root/a.mjs"),
            "try {} catch (error) { throw new CustomError(...args, {cause: other}); }").is_empty());
    }
}

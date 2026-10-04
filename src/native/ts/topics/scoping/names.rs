//! Naming roles: original imported/destructured spelling versus local names.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn function_context(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut child = node;
    while let Some(parent) = s.parent(child).filter(|&n| s.is(n, K::Paren)) { child = parent; }
    let Some(parent) = s.parent(child) else { return Value::Null };
    let declaration = matches!(s.kind(parent), Some(K::Program | K::Block | K::Export | K::If | K::Labeled | K::Case | K::While | K::For));
    let default_export = s.is(parent, K::Export) && s.word(parent, F::Kind) == Some("default");
    let inferred = match s.kind(parent) {
        Some(K::Declarator | K::Param) => s.get(parent, F::Name).is_some_and(|n| s.is(n, K::Ident)),
        Some(K::Prop | K::ClassProp) => true,
        Some(K::Assign | K::AssignPattern) => s.get(parent, F::Left)
            .or_else(|| s.parent(parent).filter(|&p| matches!(s.kind(p), Some(K::Prop | K::PatternProp))).and_then(|p| s.get(p, F::Key)))
            .is_some_and(|n| s.is(n, K::Ident)),
        Some(K::PatternProp) => s.get(parent, F::Key).is_some_and(|n| s.is(n, K::Ident)),
        _ => false,
    };
    let name = s.get(node, F::Name);
    let recursive = name.and_then(|n| file.bindings().decls.get(&n))
        .is_some_and(|b| b.uses > 0 || b.self_uses > 0 || b.writes > 0);
    Value::Record(vec![
        ("eligible".into(), Value::Bool(!declaration || default_export)),
        ("expression".into(), Value::Bool(!declaration)),
        ("named".into(), Value::Bool(name.is_some())),
        ("recursive".into(), Value::Bool(recursive)),
        ("inferred".into(), Value::Bool(inferred)),
        ("generator".into(), Value::Bool(s.has_word(node, F::Modifier, "*"))),
    ])
}

pub(super) fn function_match(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(function_name) = s.get(node, F::Name) else { return Value::Null };
    let mut child = node;
    while let Some(parent) = s.parent(child).filter(|&n| s.is(n, K::Paren)) { child = parent; }
    let Some(parent) = s.parent(child) else { return Value::Null };
    let (target, property) = match s.kind(parent) {
        Some(K::Declarator) => (s.get(parent, F::Name).filter(|&n| s.is(n, K::Ident)).map(|n| s.text(n).to_string()), false),
        Some(K::Assign) => {
            let target = s.get(parent, F::Left).map(|n| s.unparen(n));
            match target {
                Some(n) if s.is(n, K::Ident) => (Some(s.text(n).to_string()), false),
                Some(n) if matches!(s.kind(n), Some(K::Member | K::Index)) => {
                    if s.is(n, K::Index) && s.get(n, F::Index).is_none_or(|n| !s.is(s.unparen(n), K::String)) { return Value::Null; }
                    (crate::native::ts::statics::property_name(s, n), true)
                }
                _ => return Value::Null,
            }
        }
        Some(K::Prop | K::ClassProp) => {
            let Some(key) = s.get(parent, F::Key) else { return Value::Null };
            let key = if s.is(key, K::Computed) { s.get(key, F::Expr).unwrap_or(key) } else { key };
            if !matches!(s.kind(key), Some(K::Ident | K::String)) { return Value::Null; }
            (crate::native::ts::statics::property_name(s, parent), true)
        }
        _ => return Value::Null,
    };
    let Some(target) = target else { return Value::Null };
    let commonjs = s.get(parent, F::Left).is_some_and(|left| {
        crate::native::ts::statics::property_name(s, left).as_deref() == Some("exports")
            && s.get(left, F::Object).is_some_and(|object| s.text(s.unparen(object)) == "module")
    });
    let mut descriptor = None;
    let mut descriptor_call = false;
    if target == "value" && s.is(parent, K::Prop) {
        if let Some(object) = s.parent(parent).filter(|&n| s.is(n, K::Object)) {
            if let Some(call) = s.parent(object).filter(|&n| s.is(n, K::Call)) {
                if descriptor_method(s, call, "defineProperty") {
                    descriptor_call = true;
                    descriptor = s.all(call, F::Arg).nth(1).filter(|&n| s.is(s.unparen(n), K::String))
                        .and_then(|n| crate::native::ts::statics::literal_text(s, n));
                }
            } else if let Some(holder) = s.parent(object).filter(|&n| s.is(n, K::Prop)) {
                if let Some(call) = s.parent(holder).and_then(|n| s.parent(n)).filter(|&n| s.is(n, K::Call)) {
                    if descriptor_method(s, call, "defineProperties") || descriptor_method(s, call, "create") {
                        descriptor_call = true;
                        descriptor = s.get(holder, F::Key).filter(|&n| s.is(n, K::Ident)).map(|n| s.text(n).to_string());
                    }
                }
            }
        }
    }
    Value::Record(vec![
        ("target".into(), Value::from(target)),
        ("name".into(), Value::from(s.text(function_name).to_string())),
        ("property".into(), Value::Bool(property)),
        ("commonjs".into(), Value::Bool(commonjs)),
        ("descriptor".into(), descriptor.map(Value::from).unwrap_or(Value::Null)),
        ("descriptor_call".into(), Value::Bool(descriptor_call)),
        ("offset".into(), Value::from(s.tree.node(parent).start as usize)),
    ])
}

fn descriptor_method(s: &crate::native::ts::syntax::Syntax, call: u32, method: &str) -> bool {
    s.get(call, F::Callee).is_some_and(|callee| {
        let callee = s.unparen(callee);
        crate::native::ts::statics::property_name(s, callee).as_deref() == Some(method)
            && s.get(callee, F::Object).is_some_and(|object| {
                let name = s.text(s.unparen(object));
                name == "Object" || name == "Reflect" && method == "defineProperty"
            })
    })
}

pub(super) fn camelcase(file: &NativeFile, node: u32) -> Value<'static> {
    if file.import_options_key(node).is_some() { return Value::Null; }
    let s = &file.syntax;
    let name = s.text(node).strip_prefix('#').unwrap_or(s.text(node));
    let body = name.trim_matches('_');
    if !body.contains('_') || body.to_uppercase() == body {
        return Value::Null;
    }
    let b = file.bindings();
    let mut child = node;
    let mut parent = s.parent(child);
    while let Some(paren) = parent.filter(|&n| s.is(n, K::Paren)) {
        child = paren;
        parent = s.parent(child);
    }
    let property = parent.is_some_and(|p| {
        matches!(s.kind(p), Some(K::Prop | K::Method | K::ClassProp)) && s.field(node) == Some(F::Key)
            && !s.parent(p).is_some_and(|object| s.is(object, K::Object) && b.write_sites.contains_key(&object))
            || s.is(p, K::Member) && s.is(node, K::Ident) && s.field(node) == Some(F::Property)
                && b.write_sites.get(&p).is_some_and(|&site| s.is(site, K::Assign))
    });
    let declaration = b.decls.get(&node);
    let reference = b.refs.get(&node).and_then(|n| b.decls.get(n));
    let binding = declaration.or(reference);
    let kind = binding.map(|b| b.kind).unwrap_or("");
    let imported = kind == "import";
    let globals = b.unresolved.contains(&node);
    let configured_global = globals && matches!(file.globals().setting(name), Some("readonly" | "writable"));
    let label = parent.is_some_and(|p| matches!(s.kind(p), Some(K::Labeled | K::Break | K::Continue)) && s.field(node) == Some(F::Label));
    let exported = parent.is_some_and(|p| s.is(p, K::ExportSpec) && s.field(node) == Some(F::Exported)
        || s.is(p, K::Export) && s.field(node) == Some(F::Namespace));
    let value_name = matches!(kind, "var" | "let" | "const" | "using" | "parameter" | "catch" | "function" | "function_name" | "class" | "class_name" | "import");
    let skip_reference = declaration.is_none() && parent.is_some_and(|p| {
        matches!(s.kind(p), Some(K::Call | K::New))
            || s.is(p, K::AssignPattern) && s.field(child) == Some(F::Right)
            || s.is(p, K::Param) && s.field(child) == Some(F::Init)
    });
    let mut value = node;
    if let Some(p) = s.parent(value).filter(|&p| s.is(p, K::AssignPattern)) {
        value = p;
    }
    let original = s.parent(value).is_some_and(|p| {
        match s.kind(p) {
            Some(K::Prop | K::PatternProp) => s.get(p, F::Key).is_some_and(|k| s.is(k, K::Ident) && s.text(k) == name),
            Some(K::ImportSpec) => s.get(p, F::Imported).is_some_and(|n| s.text(n).trim_matches(['\'', '"']) == name),
            _ => false,
        }
    });
    if !(property || label || exported) && (skip_reference || !(value_name || globals)) {
        return Value::Null;
    }
    Value::Record(vec![
        ("name".into(), Value::from(name.to_string())),
        ("property".into(), Value::Bool(property)),
        ("private".into(), Value::Bool(s.is(node, K::PrivateName))),
        ("import".into(), Value::Bool(imported && declaration.is_some())),
        ("original".into(), Value::Bool(original)),
        ("global".into(), Value::Bool(configured_global)),
        ("binding".into(), Value::Bool(value_name || globals)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn assignment_patterns_check_values_not_property_keys() {
        let path = Path::new("/root/a.mjs");
        let l = native_linter("rule camelcase { set ignoreGlobals true }");
        assert!(l.lint(path, "/* globals a_global */ ({ snake_key: a_global } = input);").is_empty());
        assert_eq!(l.lint(path, "({ snake_key: unknown_name } = input);").len(), 1);
        let l = native_linter("rule camelcase { set ignoreDestructuring true }");
        assert!(l.lint(path, "var { [{category_id} = query]: categoryId } = query;").is_empty());
        assert_eq!(l.lint(path, "const object = { snake_key: 1 };").len(), 1);
    }

    #[test]
    fn shorthand_defaults_infer_function_names_without_blessing_other_defaults() {
        let l = native_linter("rule func-names { set mode \"as-needed\" }");
        let path = Path::new("/root/a.mjs");
        assert!(l.lint(path, "({ value = function() {} } = input);").is_empty());
        assert_eq!(l.lint(path, "consume(function() {});").len(), 1);
    }

    #[test]
    fn import_option_keys_are_metadata_but_values_and_computed_keys_are_code() {
        let l = native_linter("rule camelcase");
        let path = Path::new("/root/a.mjs");
        assert!(l.lint(path, "import('data', { outer_key: { inner_key: 'json' } });").is_empty());
        assert_eq!(l.lint(path, "import('data', { outer_key: { [computed_key]: 'json' } });").len(), 1);
        assert_eq!(l.lint(path, "import('data', { outer_key: { inner_key: local_value } });").len(), 1);
    }
}

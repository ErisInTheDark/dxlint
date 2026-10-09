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
    Value::record(vec![
        ("eligible".into(), Value::Bool(!declaration || default_export)),
        ("expression".into(), Value::Bool(!declaration)),
        ("named".into(), Value::Bool(name.is_some())),
        ("recursive".into(), Value::Bool(recursive)),
        ("inferred".into(), Value::Bool(inferred)),
        ("generator".into(), Value::Bool(s.has_word(node, F::Modifier, "*"))),
    ])
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
    Value::record(vec![
        ("name".into(), Value::from(name.to_string())),
        ("property".into(), Value::Bool(property)),
        ("private".into(), Value::Bool(s.is(node, K::PrivateName))),
        ("import".into(), Value::Bool(imported && declaration.is_some())),
        ("original".into(), Value::Bool(original)),
        ("global".into(), Value::Bool(configured_global)),
        ("binding".into(), Value::Bool(value_name || globals)),
    ])
}

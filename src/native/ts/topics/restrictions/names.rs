//! Contexts where identifier, property and export names belong to the author.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics::property_name;
use crate::expr::Value;

pub fn name(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(parent) = s.parent(node) else { return Value::Null };
    let kind = s.kind(parent);
    let field = s.field(node);
    let key = field == Some(F::Key);
    let private = s.is(node, K::PrivateName);
    let name = s.text(node).trim_start_matches('#');
    let external = matches!(file.global_setting(node), Some("readonly" | "writable"));
    let attributes = import_attribute_key(file, node);
    let pattern_property = kind == Some(K::PatternProp) || kind == Some(K::Prop) && s.parent(parent).is_some_and(|n| file.write_site(n).is_some());
    let same_pattern_name = s.get(parent, F::Value).map(|n| s.get(n, F::Left).unwrap_or(n)).is_some_and(|n| s.word(parent, F::Key) == Some(s.text(n)));
    let assignment_member = kind == Some(K::Member) && field == Some(F::Property) && file.write_site(parent).is_some()
        && s.parent(parent).is_none_or(|p| !(s.is(p, K::Update) || s.is(p, K::Unary) && s.word(p, F::Op) == Some("delete")));
    let pattern_alias = pattern_property && key && s.get(parent, F::Value).is_some_and(|n| {
        !s.is(n, K::AssignPattern) || s.get(n, F::Left).is_some()
    });
    let renamed_import = kind == Some(K::ImportSpec) && field == Some(F::Imported) && s.get(parent, F::Local).is_some()
        || kind == Some(K::ExportSpec) && field == Some(F::Local) && s.get(parent, F::Exported).is_some() && s.parent(parent).is_some_and(|p| s.get(p, F::From).is_some());
    let deny = !external && !attributes && !renamed_import && !pattern_alias && !matches!(kind, Some(K::Call | K::New | K::MetaProperty))
        && !(kind == Some(K::Member) && field == Some(F::Property) && !assignment_member);
    let length_property = key && (if pattern_property { !pattern_alias || same_pattern_name } else { kind == Some(K::Prop) }) || assignment_member;
    let length = match kind {
        Some(K::Declarator | K::Param) => field == Some(F::Name),
        Some(K::AssignPattern) => field == Some(F::Left),
        Some(K::PatternProp | K::Prop) if pattern_property => if key { !pattern_alias || same_pattern_name } else { !same_pattern_name },
        Some(K::Prop) => key,
        Some(K::ImportSpec) => field == Some(F::Local) && s.get(parent, F::Imported).is_some_and(|n| file.static_name(n).unwrap_or_else(|| s.text(n).to_string()) != name),
        Some(K::Import) => matches!(field, Some(F::DefaultImport | F::Namespace)),
        Some(K::Function | K::Arrow | K::Method | K::ClassProp | K::Rest | K::ArrayPattern) => true,
        Some(K::Class) => s.parent(parent).is_some_and(|p| matches!(s.kind(p), Some(K::Program | K::Block | K::Export))),
        Some(K::Try) => field == Some(F::Param),
        Some(K::Member) => assignment_member,
        _ => false,
    };
    let mut checks = Vec::new();
    for properties in [false, true] {
        for class_fields in [false, true] {
            for only_declarations in [false, true] {
                for ignore_destructuring in [false, true] {
                    let check = if external || attributes {
                        false
                    } else if private {
                        kind != Some(K::ClassProp) || class_fields
                    } else {
                        match kind {
                            Some(K::Member) => properties && (field == Some(F::Object) || assignment_member),
                            Some(K::PatternProp | K::Prop) if pattern_property => !pattern_alias && !(ignore_destructuring && (key || same_pattern_name)),
                            Some(K::Prop) if key => properties,
                            Some(K::AssignPattern) => field == Some(F::Left) && properties && !ignore_destructuring,
                            Some(K::ImportSpec) => field == Some(F::Local) || field == Some(F::Imported) && s.get(parent, F::Local).is_none(),
                            Some(K::Import) => matches!(field, Some(F::DefaultImport | F::Namespace)),
                            Some(K::ClassProp) => class_fields,
                            Some(K::Call | K::New | K::MetaProperty) => false,
                            _ => !only_declarations || matches!(kind, Some(K::Function | K::Declarator)),
                        }
                    };
                    if check {
                        checks.push(Value::record(vec![
                            ("properties".into(), Value::Bool(properties)),
                            ("classFields".into(), Value::Bool(class_fields)),
                            ("onlyDeclarations".into(), Value::Bool(only_declarations)),
                            ("ignoreDestructuring".into(), Value::Bool(ignore_destructuring)),
                        ]));
                    }
                }
            }
        }
    }
    Value::record(vec![
        ("name".into(), Value::from(name.to_string())),
        ("deny".into(), Value::Bool(deny)),
        ("length".into(), Value::Bool(length && !attributes)),
        ("length_property".into(), Value::Bool(length_property)),
        ("checks".into(), Value::list(checks)),
    ])
}

/// Import attribute keys: the options' own, and those under `with`/`assert`.
fn import_attribute_key(file: &NativeFile, node: u32) -> bool {
    match file.import_options_key(node) {
        Some(None) => true,
        Some(Some(outer)) => outer == "with" || outer == "assert",
        None => false,
    }
}

pub fn properties(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let pattern = s.is(node, K::ObjectPattern) || s.is(node, K::Object);
    let object = if pattern {
        s.parent(node).and_then(|p| match s.kind(p) {
            Some(K::Declarator | K::Param) => s.get(p, F::Init),
            Some(K::Assign | K::AssignPattern) => s.get(p, F::Right),
            _ => None,
        })
    } else {
        s.get(node, F::Object)
    };
    let object = object.filter(|&n| s.is(s.unparen(n), K::Ident)).map(|n| s.text(s.unparen(n)).to_string());
    let candidates = if pattern { s.all(node, F::Property).collect() } else { vec![node] };
    Value::list(candidates.into_iter().filter_map(|n| {
        let name = property_name(s, n)?;
        Some(Value::record(vec![
            ("object".into(), object.clone().map_or(Value::Null, Value::from)),
            ("property".into(), Value::from(name)),
            ("offset".into(), Value::from(s.tree.node(node).start as usize)),
        ]))
    }).collect())
}

pub fn exports(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut names = Vec::new();
    if s.word(node, F::Kind) == Some("default") {
        names.push(("default".to_string(), node, "direct"));
    } else if let Some(namespace) = s.get(node, F::Namespace) {
        names.push((file.static_name(namespace).unwrap_or_else(|| s.text(namespace).to_string()), namespace, "namespaceFrom"));
    } else if let Some(decl) = s.get(node, F::Decl) {
        let mut stack = vec![decl];
        while let Some(n) = stack.pop() {
            if file.declaration_kinds(n).is_some() && s.is(n, K::Ident) {
                names.push((s.text(n).to_string(), n, "named"));
            } else {
                stack.extend(s.kids(n).filter(|(f, _)| !matches!(f, Some(F::Init | F::Body | F::Param))).map(|(_, n)| n));
            }
        }
        names.sort_by_key(|(_, n, _)| s.tree.node(*n).start);
    } else {
        for spec in s.all(node, F::Specifier) {
            if let Some(exported) = s.get(spec, F::Exported).or_else(|| s.get(spec, F::Local)) {
                let name = file.static_name(exported).unwrap_or_else(|| s.text(exported).to_string());
                let local = s.get(spec, F::Local).map(|n| file.static_name(n).unwrap_or_else(|| s.text(n).to_string()));
                let kind = if s.get(node, F::From).is_none() { "named" } else if local.as_deref() == Some("default") { "defaultFrom" } else { "namedFrom" };
                names.push((name, exported, kind));
            }
        }
    }
    Value::list(names.into_iter().map(|(name, n, kind)| Value::record(vec![
        ("name".into(), Value::from(name)), ("offset".into(), Value::from(s.tree.node(n).start as usize)), ("kind".into(), Value::from(kind)),
    ])).collect())
}

pub fn global_member(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(property) = property_name(s, node) else { return Value::Null };
    let mut at = s.get(node, F::Object).map(|n| s.unparen(n));
    let mut intermediates = Vec::new();
    while let Some(n) = at {
        if s.is(n, K::Ident) {
            if file.resolved(n) != Some("global") || !matches!(file.global_setting(n), Some("readonly" | "writable")) {
                return Value::Null;
            }
            let object = s.text(n);
            if intermediates.iter().any(|name| name != object) { return Value::Null; }
            let offset = s.get(node, F::Property).or_else(|| s.get(node, F::Index)).map_or(s.tree.node(node).start, |n| s.tree.node(n).start);
            return Value::record(vec![("object".into(), Value::from(object.to_string())), ("property".into(), Value::from(property)), ("offset".into(), Value::from(offset as usize))]);
        }
        if matches!(s.kind(n), Some(K::Member | K::Index)) {
            let Some(name) = property_name(s, n) else { break };
            intermediates.push(name);
            at = s.get(n, F::Object).map(|n| s.unparen(n));
        } else {
            break;
        }
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn restrictions_follow_destructuring_not_key_spelling() {
        let l = native_linter("rule no-restricted-properties { set options [{object: 'source', property: 'bad'}] }");
        assert_eq!(l.lint(Path::new("/root/file.ts"), "function f({bad: alias} = source) {}").len(), 1);
        assert_eq!(l.lint(Path::new("/root/file.ts"), "({bad: target.field} = source);").len(), 1);
        assert!(l.lint(Path::new("/root/file.ts"), "({bad: target.field} = other);").is_empty());
    }

    #[test]
    fn import_attributes_do_not_own_identifier_names() {
        let l = native_linter("rule id-denylist { set options ['with', 'type', 'value'] }");
        assert!(l.lint(Path::new("/root/file.ts"), "import('x', {with: {type: 'json'}})").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "import('x', {with: {[type]: value}})").len(), 2);
    }
}

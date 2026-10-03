//! Class and member declaration shapes used by style rules.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Lexer;
use crate::native::ts::syntax::Syntax;
use crate::native::ts::statics::property_name;

pub(super) fn member_name(s: &Syntax, node: u32) -> (String, &'static str) {
    s.get(node, F::Key).or_else(|| s.get(node, F::Name)).map(|n| {
        let key = s.get(n, F::Expr).unwrap_or(n);
        let name = property_name(s, node).unwrap_or_else(|| s.text(key).to_string());
        if matches!(s.kind(key), Some(K::String | K::Number | K::BigInt | K::Bool | K::Null))
            && !name.chars().enumerate().all(|(i, c)| c == '_' || c == '$' || c.is_alphabetic() || i > 0 && c.is_numeric())
        {
            (format!("\"{name}\""), "quoted")
        } else {
            let kind = match s.kind(key) {
                Some(K::PrivateName) => "private",
                Some(K::Ident | K::String | K::Number | K::BigInt | K::Bool | K::Null) => "normal",
                _ => "expression",
            };
            (name, kind)
        }
    }).unwrap_or_default()
}

pub(super) fn member_style(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let key = s.get(node, F::Key).or_else(|| s.get(node, F::Name));
    let (name, _) = member_name(s, node);
    let head = member_head(s, node);
    let public = s.all(node, F::Modifier).find(|&n| s.text(n) == "public").map(|n| s.tree.node(n).start).unwrap_or(head);
    Value::Record(vec![
        ("name".into(), Value::from(name)),
        ("head".into(), Value::from(head as usize)),
        ("public".into(), Value::from(public as usize)),
        ("key".into(), Value::from(key.map(|n| s.tree.node(s.get(n, F::Expr).unwrap_or(n)).start).unwrap_or(head) as usize)),
    ])
}

pub(super) fn member_head(s: &Syntax, node: u32) -> u32 {
    let start = s.all(node, F::Decorator).last().map_or(s.tree.node(node).start, |n| s.tree.node(n).end);
    start + Lexer::new(&s.src[start as usize..]).next_token().start
}

pub(super) fn extraneous_class(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let members: Vec<_> = s.all(node, F::Member).collect();
    if members.is_empty() { return Value::from("empty"); }
    let mut only_constructor = true;
    let mut only_static = true;
    for member in members {
        if s.is(member, K::Method) && s.word(member, F::Key) == Some("constructor") {
            if s.all(member, F::Param).any(|p| ["public", "protected", "private", "readonly"].iter().any(|m| s.has_word(p, F::Modifier, m))) {
                only_constructor = false;
                only_static = false;
            }
        } else {
            only_constructor = false;
            if s.is(member, K::IndexSig) || s.has_word(member, F::Modifier, "abstract")
                || matches!(s.kind(member), Some(K::ClassProp | K::Method)) && !s.has_word(member, F::Modifier, "static")
            {
                only_static = false;
            }
        }
    }
    if only_constructor { Value::from("constructor") } else if only_static { Value::from("static") } else { Value::Null }
}

fn supported_literal(s: &Syntax, node: u32) -> bool {
    let node = s.unparen(node);
    match s.kind(node) {
        Some(K::Number | K::BigInt | K::String | K::Regex | K::Null | K::Bool) => true,
        Some(K::Template) => s.all(node, F::Expr).next().is_none(),
        Some(K::TaggedTemplate) => s.get(node, F::Quasi).is_some_and(|n| supported_literal(s, n)),
        _ => false,
    }
}

pub(super) fn static_member_name(file: &NativeFile, node: u32) -> Option<String> {
    let s = &file.syntax;
    property_name(s, node).or_else(|| {
        let key = s.get(node, F::Key)?;
        file.static_string(s.get(key, F::Expr)?)
    })
}

pub(super) fn literal_member(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(class) = s.parent(node) else { return Value::Bool(false) };
    let name = static_member_name(file, node);
    if s.is(node, K::Method) {
        if s.word(node, F::Kind) != Some("get") || s.has_word(node, F::Modifier, "override") { return Value::Bool(false); }
        let literal = s.get(node, F::Body).and_then(|n| s.all(n, F::Body).next()).is_some_and(|n| {
            s.is(n, K::Return) && s.get(n, F::Arg).is_some_and(|n| supported_literal(s, n))
        });
        let setter = name.is_some() && s.all(class, F::Member).any(|n| s.is(n, K::Method) && s.word(n, F::Kind) == Some("set") && static_member_name(file, n) == name);
        Value::Bool(literal && !setter)
    } else {
        let readonly = s.has_word(node, F::Modifier, "readonly") && !s.has_word(node, F::Modifier, "declare") && !s.has_word(node, F::Modifier, "override");
        let assigned = name.is_some() && file.bindings().write_sites.keys().any(|&n| {
            if !matches!(s.kind(n), Some(K::Member | K::Index)) || !s.get(n, F::Object).is_some_and(|n| s.is(n, K::This)) || property_name(s, n) != name { return false; }
            let mut at = s.parent(n);
            while let Some(p) = at {
                if matches!(s.kind(p), Some(K::Function | K::Arrow | K::Method)) {
                    return s.is(p, K::Method) && s.word(p, F::Key) == Some("constructor") && s.parent(p) == Some(class);
                }
                at = s.parent(p);
            }
            false
        });
        Value::Bool(readonly && !assigned && s.get(node, F::Init).is_some_and(|n| supported_literal(s, n)))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn accessibility_includes_constructor_heads() {
        let linter = native_linter("rule explicit-member-accessibility:method");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "class A { constructor() {} }").len(), 1);
    }

    #[test]
    fn literal_getters_use_native_expression_shapes() {
        let linter = native_linter("rule class-literal-property-style");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "class A { get x() { return tag`literal`; } }").len(), 1);
        assert!(linter.lint(Path::new("/root/file.ts"), "class A { get x() { return `interpolated ${value}`; } }").is_empty());
    }

    #[test]
    fn constructor_does_not_hide_static_only_classes() {
        let linter = native_linter("rule no-extraneous-class");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "class A { constructor() {} static value = 1; }").len(), 1);
        assert!(linter.lint(Path::new("/root/file.ts"), "class A { constructor(public value: number) {} }").is_empty());
    }
}

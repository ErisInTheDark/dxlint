//! Numeric literal contexts shared by core and TypeScript magic-number rules.

use super::super::super::facts::{Facts, NativeFile};
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics::number_string;
use crate::expr::Value;

pub fn context(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut full = node;
    while let Some(parent) = s.parent(full).filter(|&n| s.is(n, K::Paren)) { full = parent; }
    let unary = s.parent(full).filter(|&n| s.is(n, K::Unary) && matches!(s.word(n, F::Op), Some("-" | "+")));
    if let Some(n) = unary { full = n; }
    let signed_type = s.parent(node).filter(|&n| s.is(n, K::LiteralType) && s.text(n).starts_with('-'));
    let sign = unary.and_then(|n| s.word(n, F::Op)).unwrap_or(if signed_type.is_some() { "-" } else { "" });
    let raw = format!("{sign}{}", s.text(node));
    let bigint = s.text(node).ends_with('n');
    let mut value = if bigint { super::super::super::statics::number(s.text(node).trim_end_matches('n')).unwrap_or(f64::NAN) }
        else { file.static_number(full).unwrap_or(f64::NAN) };
    if signed_type.is_some() || bigint && sign == "-" { value = -value; }
    let mut parent_node = full;
    while let Some(parent) = s.parent(parent_node).filter(|&n| s.is(n, K::Paren)) { parent_node = parent; }
    let parent = s.parent(parent_node);
    let kind = parent.and_then(|n| s.kind(n));
    let field = s.field(parent_node);
    let default_value = kind == Some(K::AssignPattern) && field == Some(F::Right) || kind == Some(K::Param) && field == Some(F::Init)
        || kind == Some(K::Assign) && field == Some(F::Right) && parent.and_then(|n| s.parent(n)).is_some_and(|n| s.is(n, K::Array) && file.write_site(n).is_some());
    let class_value = kind == Some(K::ClassProp) && field == Some(F::Init);
    let readonly = class_value && parent.is_some_and(|n| s.has_word(n, F::Modifier, "readonly"));
    let declaration = kind == Some(K::Declarator);
    let constant = parent.and_then(|n| s.parent(n)).is_some_and(|n| s.word(n, F::Kind) == Some("const"));
    let radix = parent.is_some_and(|n| {
        if !s.is(n, K::Call) || s.all(n, F::Arg).nth(1) != Some(parent_node) { return false; }
        let Some(callee) = s.get(n, F::Callee).map(|n| s.unparen(n)) else { return false };
        s.is(callee, K::Ident) && s.text(callee) == "parseInt" || matches!(s.kind(callee), Some(K::Member | K::Index))
            && s.word(callee, F::Object) == Some("Number") && super::super::super::statics::property_name(s, callee).as_deref() == Some("parseInt")
    });
    let literal_type = kind == Some(K::LiteralType);
    let mut ancestor = parent;
    while ancestor.and_then(|n| s.parent(n)).is_some_and(|n| matches!(s.kind(n), Some(K::Union | K::Intersection | K::ParenType))) {
        ancestor = ancestor.and_then(|n| s.parent(n));
    }
    let type_alias = literal_type && ancestor.and_then(|n| s.parent(n)).is_some_and(|n| s.is(n, K::TypeAlias));
    let type_index = ancestor.and_then(|n| s.parent(n)).is_some_and(|n| s.is(n, K::IndexedAccess));
    let object = matches!(kind, Some(K::Object | K::Prop | K::Method))
        || kind == Some(K::Assign) && parent.is_some_and(|n| s.get(n, F::Left).is_none_or(|n| !s.is(s.unparen(n), K::Ident)));
    Value::Record(vec![
        ("raw".into(), Value::from(raw)),
        ("value".into(), if bigint { Value::from(format!("{}n", number_string(value))) } else { Value::Num(value) }),
        ("offset".into(), Value::from(s.tree.node(signed_type.unwrap_or(full)).start as usize)),
        ("ts_raw".into(), Value::from(if sign == "+" { s.text(node).to_string() } else { format!("{sign}{}", s.text(node)) })),
        ("ts_offset".into(), Value::from(s.tree.node(if sign == "+" { node } else { signed_type.unwrap_or(full) }).start as usize)),
        ("default".into(), Value::Bool(default_value)),
        ("class_field".into(), Value::Bool(class_value)),
        ("readonly".into(), Value::Bool(readonly)),
        ("enum".into(), Value::Bool(kind == Some(K::EnumMember))),
        ("literal_type".into(), Value::Bool(type_alias)),
        ("type_index".into(), Value::Bool(type_index)),
        ("declaration".into(), Value::Bool(declaration)),
        ("constant".into(), Value::Bool(constant)),
        ("object".into(), Value::Bool(object)),
        ("radix".into(), Value::Bool(radix)),
        ("jsx".into(), Value::Bool(kind == Some(K::JsxExpression))),
        ("array_index".into(), Value::Bool(kind == Some(K::Index) && field == Some(F::Index) && value.fract() == 0.0 && (0.0..4294967295.0).contains(&value))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn signed_types_and_parenthesised_type_indices_keep_their_context() {
        let l = native_linter("rule no-magic-numbers {\nset ignore [-2]\nset ignoreTypeIndexes true\n}");
        assert!(l.lint(Path::new("/root/file.ts"), "type A = -2; type B = A[((1 & -3) | 4)];").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "type A = 2;").len(), 1);
    }
}

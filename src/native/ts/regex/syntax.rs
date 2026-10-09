//! Parsed regex structure as a node property.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use super::parse::{Ast, Kind};

impl NativeFile<'_> {
    pub(crate) fn regex_syntax(&self, node: u32) -> Value<'static> {
        let Some(site) = self.regex_site(node) else { return Value::Null };
        let Some(flags) = site.flags else { return Value::Null };
        if super::flags_error(&flags, "").is_some() { return Value::Null }
        let Ok(ast) = super::parse::parse(&site.pattern, flags.contains('u'), flags.contains('v')) else { return Value::Null };
        let mut children = vec![Vec::new(); ast.nodes.len()];
        for (index, node) in ast.nodes.iter().enumerate() {
            if let Some(parent) = node.parent { children[parent].push(index) }
        }
        Value::List(ast.nodes.iter().enumerate().map(|(index, _)| syntax_node(&ast, &children, index)).collect())
    }
}

fn syntax_node(ast: &Ast, children: &[Vec<usize>], index: usize) -> Value<'static> {
    let node = &ast.nodes[index];
    let kind = match &node.kind {
        Kind::Pattern => "pattern",
        Kind::Alternative => "alternative",
        Kind::Group => "group",
        Kind::CapturingGroup(_) => "capturing_group",
        Kind::Lookaround { .. } => "lookaround",
        Kind::Quantifier => "quantifier",
        Kind::Character(_) => "character",
        Kind::CharacterSet => "character_set",
        Kind::Class { .. } => "class",
        Kind::ClassRange => "class_range",
        Kind::StringDisjunction => "string_disjunction",
        Kind::Backreference(_) => "backreference",
        Kind::Assertion => "assertion",
    };
    let (character, value) = if let Kind::Character(value) = node.kind {
        (char::from_u32(value).map_or(Value::Null, |character| Value::from(character.to_string())), Value::from(value as usize))
    } else { (Value::Null, Value::Null) };
    Value::record(vec![
        ("id".into(), Value::from(index)),
        ("parent".into(), node.parent.map_or(Value::Null, Value::from)),
        ("kind".into(), Value::from(kind)),
        ("raw".into(), Value::from(ast.raw(index))),
        ("character".into(), character),
        ("value".into(), value),
        ("children".into(), Value::List(children[index].iter().copied().map(Value::from).collect())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    #[test]
    fn syntax_keeps_escaped_characters_distinct_from_assertions_and_alternatives() {
        with_native_file(r#"const value = /*@*//^a\$|b$/;"#, |file, nodes| {
            let records = file.regex_syntax(nodes[0]).items().to_vec();
            let alternatives: Vec<_> = records.iter().filter(|node| node.key("kind").text() == "alternative").collect();
            assert_eq!(alternatives.len(), 2);
            let first: Vec<_> = records.iter().filter(|node| node.key("parent") == alternatives[0].key("id")).collect();
            let second: Vec<_> = records.iter().filter(|node| node.key("parent") == alternatives[1].key("id")).collect();
            assert_eq!(first[0].key("kind").text(), "assertion");
            assert_eq!(first[2].key("kind").text(), "character");
            assert_eq!(first[2].key("character").text(), "$");
            assert_eq!(second[1].key("kind").text(), "assertion");
            for alternative in alternatives {
                let children: Vec<_> = records.iter().filter(|node| node.key("parent") == alternative.key("id")).map(|node| node.key("id")).collect();
                assert_eq!(alternative.key("children").items(), children);
            }
        });
    }

    #[test]
    fn constructor_patterns_use_static_values_and_reject_invalid_or_unknown_sites() {
        with_native_file(
            r#"const source = '^a'; const value = /*@*/new RegExp(source, 'u');"#,
            |file, nodes| {
                let records = file.regex_syntax(nodes[0]).items().to_vec();
                assert_eq!(records.iter().filter(|node| node.key("kind").text() == "alternative").count(), 1);
                let characters: Vec<_> = records.iter().filter(|node| node.key("kind").text() == "character").map(|node| node.key("character")).collect();
                assert_eq!(characters, [Value::from("a")]);
            },
        );
        for expression in ["new RegExp('[', 'u')", "new RegExp('^a', 'uv')", "new RegExp('^a', 'ii')", "new RegExp('^a', flags)", "pattern", "new LocalRegExp('^a')"] {
            with_native_file(&format!("const value = /*@*/{expression};"), |file, nodes| {
                assert!(matches!(file.regex_syntax(nodes[0]), Value::Null), "{expression}");
            });
        }
    }
}

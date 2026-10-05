//! Language-neutral selectors over a front end's logical syntax view.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

mod parse;
mod r#match;
mod regex;

pub enum Attribute<'a> {
    Missing,
    Null,
    Bool(bool),
    Number(f64),
    BigInt(Cow<'a, str>),
    String(Cow<'a, str>),
    Object(Cow<'a, str>),
}

pub trait Tree {
    fn kind(&self, node: u32) -> &str;
    fn parent(&self, node: u32) -> Option<u32>;
    fn children(&self, node: u32) -> &[u32];
    fn field(&self, node: u32) -> &str;
    /// Position and length of the containing array, including null slots.
    fn array_position(&self, node: u32) -> Option<(usize, usize)>;
    fn attribute(&self, node: u32, path: &[String]) -> Attribute<'_>;
}

pub struct Selector {
    expr: Expr,
    types: Option<Vec<String>>,
}

pub struct Span {
    pub node: u32,
    pub kind: &'static str,
    pub start: u32,
    pub end: u32,
}

enum Expr {
    Any(Vec<Expr>),
    All(Vec<Expr>),
    Not(Box<Expr>),
    Has(Box<Expr>),
    Type(String),
    Field(Vec<String>),
    Attribute(Vec<String>, Option<(Operator, Operand)>),
    Class(String),
    Nth(usize, bool),
    Join(Box<Expr>, Relation, Box<Expr>),
    Root,
}

#[derive(Clone, Copy)]
enum Relation { Descendant, Child, Sibling, Adjacent }

#[derive(Clone, Copy)]
enum Operator { Equal, Unequal, Less, LessEqual, Greater, GreaterEqual }

enum Operand {
    Literal(String, bool),
    Type(String),
    Regex(regex::Regex),
}

pub fn compile(source: &str) -> Result<Arc<Selector>, String> {
    static CACHE: OnceLock<Mutex<crate::fx_hash::FxHashMap<String, Arc<Selector>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(selector) = cache.lock().map_err(|_| "selector compiler cache poisoned")?.get(source) {
        return Ok(selector.clone());
    }
    let expr = parse::parse(source)?;
    let selector = Arc::new(Selector { types: expr.types(), expr });
    let mut cache = cache.lock().map_err(|_| "selector compiler cache poisoned")?;
    // Selectors are user input; bound the process-wide immutable compiler cache.
    if cache.len() >= 256 { cache.clear(); }
    cache.insert(source.to_owned(), selector.clone());
    Ok(selector)
}

impl Selector {
    pub fn matches(&self, tree: &impl Tree, node: u32) -> Result<bool, String> {
        if self.types.as_ref().is_some_and(|types| !types.iter().any(|t| t.eq_ignore_ascii_case(tree.kind(node)))) {
            return Ok(false);
        }
        self.expr.matches(tree, node, None)
    }
}

impl Expr {
    fn types(&self) -> Option<Vec<String>> {
        match self {
            Self::Type(t) => Some(vec![t.clone()]),
            Self::All(parts) => parts.iter().find_map(Self::types),
            Self::Any(parts) => parts.iter().map(Self::types).collect::<Option<Vec<_>>>().map(|parts| parts.into_iter().flatten().collect()),
            Self::Join(_, _, right) => right.types(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture;

    impl Tree for Fixture {
        fn kind(&self, node: u32) -> &str {
            ["Program", "CallExpression", "Identifier", "Identifier", "Literal"][node as usize]
        }
        fn parent(&self, node: u32) -> Option<u32> {
            match node { 0 => None, 1 => Some(0), _ => Some(1) }
        }
        fn children(&self, node: u32) -> &[u32] {
            match node { 0 => &[1], 1 => &[2, 3, 4], _ => &[] }
        }
        fn field(&self, node: u32) -> &str {
            match node { 1 => "body", 2 => "callee", 3 | 4 => "arguments", _ => "" }
        }
        fn array_position(&self, node: u32) -> Option<(usize, usize)> {
            match node { 1 => Some((0, 1)), 3 => Some((0, 3)), 4 => Some((2, 3)), _ => None }
        }
        fn attribute(&self, node: u32, path: &[String]) -> Attribute<'_> {
            match (node, path.join(".").as_str()) {
                (2, "name") => Attribute::String("call".into()),
                (3, "name") => Attribute::String("arg".into()),
                (4, "value") => Attribute::Number(2.0),
                (_, "optional") => Attribute::Bool(false),
                _ => Attribute::Missing,
            }
        }
    }

    fn selected(source: &str) -> Vec<u32> {
        let selector = compile(source).unwrap();
        (0..5).filter(|&node| selector.matches(&Fixture, node).unwrap()).collect()
    }

    #[test]
    fn relationships_use_fields_and_array_slots() {
        assert_eq!(selected("CallExpression > Identifier.callee"), vec![2]);
        assert_eq!(selected("Program Identifier"), vec![2, 3]);
        assert!(selected("Identifier + Literal").is_empty());
        assert_eq!(selected("Identifier ~ Literal"), vec![4]);
        assert_eq!(selected("Literal:nth-child(3)"), vec![4]);
        assert_eq!(selected("Identifier:first-child"), vec![3]);
        assert_eq!(selected("Literal:last-child"), vec![4]);
    }

    #[test]
    fn predicates_and_relative_has_compose() {
        assert_eq!(selected("CallExpression:has(> Identifier[name=arg])"), vec![1]);
        assert_eq!(selected(":matches(Identifier, Literal):not([name=call])"), vec![3, 4]);
        assert_eq!(selected("[value>=2]"), vec![4]);
        assert_eq!(selected("[value=02]"), vec![4]);
        assert!(selected("[name>2]").is_empty());
        assert_eq!(selected("[optional=false]"), vec![0, 1, 2, 3, 4]);
        assert_eq!(selected("Identifier[name=/^a.g$/]"), vec![3]);
        assert_eq!(selected(":expression"), vec![1, 2, 3, 4]);
    }

    #[test]
    fn malformed_and_unsupported_patterns_fail_explicitly() {
        for source in ["Identifier[", ":unknown", "[name=/(?=a)/]", "[name=/(a)\\1/]", "[value=+2]", "[name=foo.bar]"] {
            assert!(compile(source).is_err(), "{source}");
        }
    }
}

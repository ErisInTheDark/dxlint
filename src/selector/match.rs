use super::{Attribute, Expr, Operand, Operator, Relation, Tree};

impl Expr {
    pub(super) fn matches(&self, tree: &impl Tree, node: u32, root: Option<u32>) -> Result<bool, String> {
        let parent = || if root == Some(node) { None } else { tree.parent(node) };
        Ok(match self {
            Self::Any(parts) => {
                for part in parts { if part.matches(tree, node, root)? { return Ok(true); } }
                false
            }
            Self::All(parts) => {
                for part in parts { if !part.matches(tree, node, root)? { return Ok(false); } }
                true
            }
            Self::Not(expr) => !expr.matches(tree, node, root)?,
            Self::Has(expr) => {
                let mut pending = vec![node];
                while let Some(candidate) = pending.pop() {
                    if expr.matches(tree, candidate, Some(node))? { return Ok(true); }
                    pending.extend(tree.children(candidate));
                }
                false
            }
            Self::Type(kind) => tree.kind(node).eq_ignore_ascii_case(kind),
            Self::Root => parent().is_none(),
            Self::Field(path) => {
                let mut current = node;
                for (i, field) in path.iter().rev().enumerate() {
                    if tree.field(current) != field { return Ok(false); }
                    if i + 1 < path.len() {
                        let Some(p) = tree.parent(current).filter(|_| root != Some(current)) else { return Ok(false) };
                        current = p;
                    }
                }
                parent().is_some()
            }
            Self::Attribute(path, comparison) => {
                let attr = tree.attribute(node, path);
                match comparison {
                    None => !matches!(attr, Attribute::Missing | Attribute::Null),
                    Some((op, operand)) => {
                        let equal = match operand {
                            Operand::Literal(value, _) => attr.string() == *value,
                            Operand::Type(value) => attr.type_name() == value,
                            Operand::Regex(regex) => {
                                match (&attr, op) {
                                    (Attribute::String(value), _) => regex.matches(value)?,
                                    (_, Operator::Unequal) => regex.matches(&attr.string())?,
                                    _ => false,
                                }
                            }
                        };
                        match op {
                            Operator::Equal => equal,
                            Operator::Unequal => !equal,
                            _ => {
                                let Operand::Literal(value, numeric) = operand else { return Ok(false) };
                                let ordering = attr.compare(value, *numeric);
                                match op {
                                    Operator::Less => ordering.is_some_and(|o| o.is_lt()),
                                    Operator::LessEqual => ordering.is_some_and(|o| o.is_le()),
                                    Operator::Greater => ordering.is_some_and(|o| o.is_gt()),
                                    Operator::GreaterEqual => ordering.is_some_and(|o| o.is_ge()),
                                    _ => unreachable!(),
                                }
                            }
                        }
                    }
                }
            }
            Self::Class(class) => {
                let kind = tree.kind(node);
                let expression = || kind.ends_with("Expression") || kind.ends_with("Literal") || kind == "MetaProperty"
                    || kind == "Identifier" && parent().is_none_or(|p| tree.kind(p) != "MetaProperty");
                match class.as_str() {
                    "statement" => kind.ends_with("Statement") || kind.ends_with("Declaration"),
                    "declaration" => kind.ends_with("Declaration"),
                    "expression" => expression(),
                    "pattern" => kind.ends_with("Pattern") || expression(),
                    "function" => matches!(kind, "FunctionDeclaration" | "FunctionExpression" | "ArrowFunctionExpression"),
                    _ => unreachable!(),
                }
            }
            Self::Nth(n, last) => parent().is_some() && tree.array_position(node).is_some_and(|(index, len)| *n > 0 && if *last { len - index == *n } else { index + 1 == *n }),
            Self::Join(left, relation, right) => {
                if !right.matches(tree, node, root)? { return Ok(false); }
                match relation {
                    Relation::Child => match parent() { Some(p) => left.matches(tree, p, root)?, None => false },
                    Relation::Descendant => {
                        let mut p = parent();
                        while let Some(ancestor) = p {
                            if left.matches(tree, ancestor, root)? { return Ok(true); }
                            p = if root == Some(ancestor) { None } else { tree.parent(ancestor) };
                        }
                        false
                    }
                    Relation::Sibling | Relation::Adjacent => {
                        let (Some(p), Some((index, _))) = (parent(), tree.array_position(node)) else { return Ok(false) };
                        for &sibling in tree.children(p) {
                            if tree.field(sibling) != tree.field(node) { continue; }
                            let Some((other, _)) = tree.array_position(sibling) else { continue };
                            if other < index && (matches!(relation, Relation::Sibling) || other + 1 == index) && left.matches(tree, sibling, root)? { return Ok(true); }
                        }
                        false
                    }
                }
            }
        })
    }
}

impl Attribute<'_> {
    fn string(&self) -> String {
        match self {
            Self::Missing => "undefined".into(), Self::Null => "null".into(),
            Self::Bool(value) => value.to_string(), Self::Number(value) => crate::expr::value::js_number_text(*value),
            Self::String(value) | Self::BigInt(value) | Self::Object(value) => value.to_string(),
        }
    }
    fn type_name(&self) -> &str {
        match self { Self::Missing => "undefined", Self::Null | Self::Object(_) => "object", Self::Bool(_) => "boolean", Self::Number(_) => "number", Self::BigInt(_) => "bigint", Self::String(_) => "string" }
    }
    fn compare(&self, other: &str, numeric: bool) -> Option<std::cmp::Ordering> {
        use crate::expr::{functions, Value};
        let value = match self {
            Self::Missing => return None, Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(*value), Self::Number(value) => Value::Num(*value),
            Self::String(value) | Self::Object(value) => Value::from(value.to_string()),
            Self::BigInt(value) => Value::Record(std::sync::Arc::from([
                ("type".into(), Value::from("bigint")),
                ("value".into(), Value::from(value.to_string())),
            ])),
        };
        let other = if numeric { Value::Num(other.parse().ok()?) } else { Value::from(other.to_owned()) };
        let Value::Num(ordering) = (functions::lookup("js_compare").expect("registered JavaScript comparison").call)(&[value, other]).ok()? else { return None };
        ordering.partial_cmp(&0.0)
    }
}

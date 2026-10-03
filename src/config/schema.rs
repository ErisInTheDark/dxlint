//! Option schemas: what values a rule option takes, declared with the
//! option (`option max int(0..) = 2`) or as a named, shared `schema` item.
//! Checking a value fills in record defaults, so rules can read every
//! declared key.

use std::collections::HashMap;
use std::fmt;

use super::ast::Name;
use crate::expr::Value;

#[derive(Clone, Debug)]
pub enum Schema {
    Any,
    Str,
    Num,
    Int,
    Bool,
    Null,
    /// A string that compiles as a regex.
    Regex,
    /// Exactly this value.
    Lit(Value<'static>),
    /// A number (or a string's or list's length) within bounds.
    Range(Box<Schema>, Option<f64>, Option<f64>),
    List(Box<Schema>),
    Record(Vec<SchemaField>, Option<Box<Schema>>),
    Union(Vec<Schema>),
    /// A named `schema`.
    Named(Name),
}

#[derive(Clone, Debug)]
pub struct SchemaField {
    pub name: String,
    pub schema: Schema,
    pub default: FieldDefault,
    pub doc: Option<String>,
}

#[derive(Clone, Debug)]
pub enum FieldDefault {
    /// `key T`: must be given.
    Required,
    /// `key? T`: null when absent.
    Null,
    /// `key T = value`
    Value(Value<'static>),
}

/// `schema name T "doc"`
#[derive(Clone, Debug)]
pub struct SchemaDef {
    pub name: Name,
    pub schema: Schema,
    pub doc: Option<String>,
    pub library: bool,
}

/// Named schemas by name.
pub type Schemas = HashMap<String, SchemaDef>;

/// Why a value does not fit: the path to the misfit, what was expected
/// there, and the doc of the closest documented spot.
#[derive(Debug, PartialEq)]
pub struct Misfit {
    pub path: String,
    pub problem: String,
    pub doc: Option<String>,
}

impl fmt::Display for Misfit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.problem)?;
        } else {
            write!(f, "`{}` {}", self.path, self.problem)?;
        }
        if let Some(doc) = &self.doc {
            write!(f, " ({doc})")?;
        }
        Ok(())
    }
}

impl Schema {
    /// `value` checked against the schema, with record defaults filled in.
    pub fn check(&self, value: &Value<'static>, named: &Schemas) -> Result<Value<'static>, Misfit> {
        self.fit(value, named, &mut String::new(), None, 0)
    }

    fn fit(&self, value: &Value<'static>, named: &Schemas, path: &mut String, doc: Option<&str>, depth: usize) -> Result<Value<'static>, Misfit> {
        let misfit = |path: &str, problem: String| Misfit { path: path.trim_start_matches('.').to_string(), problem, doc: doc.map(str::to_string) };
        let expected = |path: &str| misfit(path, format!("must be {self}, got {}", shown(value)));
        match (self, value) {
            (Schema::Any, v) | (Schema::Str, v @ Value::Str(_)) | (Schema::Num, v @ Value::Num(_)) | (Schema::Bool, v @ Value::Bool(_)) => Ok(v.clone()),
            (Schema::Null, Value::Null) => Ok(Value::Null),
            (Schema::Int, Value::Num(n)) if n.fract() == 0.0 => Ok(value.clone()),
            (Schema::Regex, Value::Str(s)) => match crate::expr::re::compile(s) {
                Ok(_) => Ok(value.clone()),
                Err(e) => Err(misfit(path, format!("is not a valid regex: {e}"))),
            },
            (Schema::Lit(lit), v) if lit == v => Ok(v.clone()),
            (Schema::Range(inner, min, max), v) => {
                let v = inner.fit(v, named, path, doc, depth)?;
                let size = match &v {
                    Value::Num(n) => *n,
                    Value::Str(s) => s.chars().count() as f64,
                    Value::List(items) => items.len() as f64,
                    _ => return Ok(v),
                };
                if min.is_some_and(|m| size < m) || max.is_some_and(|m| size > m) {
                    return Err(expected(path));
                }
                Ok(v)
            }
            (Schema::List(item), Value::List(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for (i, v) in items.iter().enumerate() {
                    let len = path.len();
                    path.push_str(&format!("[{i}]"));
                    out.push(item.fit(v, named, path, doc, depth)?);
                    path.truncate(len);
                }
                Ok(Value::List(out))
            }
            (Schema::Record(fields, rest), Value::Record(entries)) => {
                for (k, _) in entries {
                    if rest.is_none() && !fields.iter().any(|f| f.name == **k) {
                        let known: Vec<&str> = fields.iter().map(|f| &*f.name).collect();
                        return Err(misfit(path, format!("has no key `{k}` (keys: {})", known.join(", "))));
                    }
                }
                let mut out = Vec::new();
                for f in fields {
                    let len = path.len();
                    path.push('.');
                    path.push_str(&f.name);
                    let doc = f.doc.as_deref().or(doc);
                    match (entries.iter().find(|(k, _)| **k == *f.name), &f.default) {
                        (Some((_, v)), _) => out.push((f.name.clone().into(), f.schema.fit(v, named, path, doc, depth)?)),
                        (None, FieldDefault::Required) => {
                            return Err(Misfit { path: path.trim_start_matches('.').to_string(), problem: "is required".into(), doc: doc.map(str::to_string) })
                        }
                        (None, FieldDefault::Null) => out.push((f.name.clone().into(), Value::Null)),
                        (None, FieldDefault::Value(d)) => out.push((f.name.clone().into(), d.clone())),
                    }
                    path.truncate(len);
                }
                if let Some(rest) = rest {
                    for (k, v) in entries.iter().filter(|(k, _)| !fields.iter().any(|f| f.name == **k)) {
                        let len = path.len();
                        path.push('.');
                        path.push_str(k);
                        out.push((k.clone(), rest.fit(v, named, path, doc, depth)?));
                        path.truncate(len);
                    }
                }
                Ok(Value::Record(out))
            }
            (Schema::Union(options), v) => {
                // The first option that takes it; otherwise the misfit of the
                // option of the value's own kind, which is the most telling.
                let mut closest = None;
                for o in options {
                    match o.fit(v, named, path, doc, depth) {
                        Ok(v) => return Ok(v),
                        Err(m) if o.kind_matches(v, named) && closest.is_none() => closest = Some(m),
                        Err(_) => {}
                    }
                }
                Err(closest.filter(|m: &Misfit| m.path.len() > path.trim_start_matches('.').len()).unwrap_or_else(|| expected(path)))
            }
            (Schema::Named(name), v) => match named.get(&name.text) {
                // Checked when the schema is defined.
                None => Ok(v.clone()),
                Some(_) if depth > 64 => Err(misfit(path, format!("schema `{}` refers to itself without end", name.text))),
                Some(def) => def.schema.fit(v, named, path, def.doc.as_deref().or(doc), depth + 1),
            },
            _ => Err(expected(path)),
        }
    }

    /// Whether the value is of the kind this schema takes (a record for a
    /// record schema), so its misfit is worth reporting from a union.
    fn kind_matches(&self, v: &Value, named: &Schemas) -> bool {
        match self {
            Schema::List(_) => matches!(v, Value::List(_)),
            Schema::Record(..) => matches!(v, Value::Record(_)),
            Schema::Range(inner, ..) => inner.kind_matches(v, named),
            Schema::Named(n) => named.get(&n.text).is_some_and(|d| d.schema.kind_matches(v, named)),
            _ => false,
        }
    }

    /// Named schemas this one refers to, with where.
    pub fn names(&self, out: &mut Vec<Name>) {
        match self {
            Schema::Named(n) => out.push(n.clone()),
            Schema::Range(inner, ..) | Schema::List(inner) => inner.names(out),
            Schema::Record(fields, rest) => {
                for f in fields {
                    f.schema.names(out);
                }
                if let Some(r) = rest {
                    r.names(out);
                }
            }
            Schema::Union(options) => options.iter().for_each(|o| o.names(out)),
            _ => {}
        }
    }

    /// The literal values it takes, for completion (`"always" | "never"`).
    pub fn choices(&self, named: &Schemas) -> Vec<Value<'static>> {
        match self {
            Schema::Lit(v) => vec![v.clone()],
            Schema::Bool => vec![Value::Bool(true), Value::Bool(false)],
            Schema::Union(options) => options.iter().flat_map(|o| o.choices(named)).collect(),
            Schema::Named(n) => named.get(&n.text).map(|d| d.schema.choices(named)).unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}

fn shown(v: &Value) -> String {
    match v {
        Value::List(_) => "a list".into(),
        Value::Record(_) => "a record".into(),
        other => other.literal(),
    }
}

fn bound(n: f64) -> String {
    Value::Num(n).to_string()
}

/// A list's element type, parenthesised when it is a union: `("a" | "b")[]`.
struct Element<'a>(&'a Schema);

impl fmt::Display for Element<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Schema::Union(_) => write!(f, "({})", self.0),
            other => write!(f, "{other}"),
        }
    }
}

impl fmt::Display for Schema {
    /// As written, with union options separated by `|` (records shortened).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Schema::Any => f.write_str("any"),
            Schema::Str => f.write_str("string"),
            Schema::Num => f.write_str("number"),
            Schema::Int => f.write_str("int"),
            Schema::Bool => f.write_str("boolean"),
            Schema::Null => f.write_str("null"),
            Schema::Regex => f.write_str("regex"),
            Schema::Lit(v) => f.write_str(&v.literal()),
            Schema::Range(inner, min, max) => {
                let range = format!("{}..{}", min.map(bound).unwrap_or_default(), max.map(bound).unwrap_or_default());
                match &**inner {
                    Schema::List(item) => write!(f, "{}[{range}]", Element(item)),
                    other => write!(f, "{other}({range})"),
                }
            }
            Schema::List(item) => write!(f, "{}[]", Element(item)),
            Schema::Record(fields, rest) => {
                let mut parts: Vec<String> = fields
                    .iter()
                    .map(|x| {
                        let mark = if matches!(x.default, FieldDefault::Null) { "?" } else { "" };
                        let default = match &x.default {
                            FieldDefault::Value(v) => format!(" = {}", v.literal()),
                            _ => String::new(),
                        };
                        format!("{}{mark}: {}{default}", x.name, x.schema)
                    })
                    .collect();
                if let Some(r) = rest {
                    parts.push(format!("[key: string]: {r}"));
                }
                write!(f, "{{ {} }}", parts.join("; "))
            }
            Schema::Union(options) => {
                let parts: Vec<String> = options.iter().map(ToString::to_string).collect();
                f.write_str(&parts.join(" | "))
            }
            Schema::Named(n) => f.write_str(&n.text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::source::{FileId, Span};

    fn rec(entries: &[(&str, Value<'static>)]) -> Value<'static> {
        Value::Record(entries.iter().map(|(k, v)| ((*k).into(), v.clone())).collect())
    }

    fn lit(s: &str) -> Schema {
        Schema::Lit(Value::from(s.to_string()))
    }

    fn field(name: &str, schema: Schema, default: FieldDefault, doc: Option<&str>) -> SchemaField {
        SchemaField { name: name.into(), schema, default, doc: doc.map(str::to_string) }
    }

    #[test]
    fn values_are_checked_and_record_defaults_filled() {
        let kind = Schema::Union(vec![lit("*"), lit("return")]);
        let mut named = Schemas::new();
        let n = Name { text: "kind".into(), span: Span::new(FileId(0), 0, 0) };
        named.insert("kind".into(), SchemaDef { name: n.clone(), schema: kind, doc: None, library: false });
        let pair = Schema::Record(
            vec![
                field("blankLine", Schema::Union(vec![lit("any"), lit("always")]), FieldDefault::Required, Some("what goes between")),
                field("prev", Schema::Named(n.clone()), FieldDefault::Value(Value::from("*")), None),
                field("note", Schema::Str, FieldDefault::Null, None),
            ],
            None,
        );
        let list = Schema::List(Box::new(pair));
        let good = Value::List(vec![rec(&[("blankLine", Value::from("always"))])]);
        assert_eq!(
            list.check(&good, &named),
            Ok(Value::List(vec![rec(&[("blankLine", Value::from("always")), ("prev", Value::from("*")), ("note", Value::Null)])]))
        );
        let bad = Value::List(vec![rec(&[("blankLine", Value::from("always"))]), rec(&[("blankLine", Value::from("sometimes"))])]);
        assert_eq!(list.check(&bad, &named).unwrap_err().to_string(), "`[1].blankLine` must be \"any\" | \"always\", got \"sometimes\" (what goes between)");
        let missing = Value::List(vec![rec(&[("prev", Value::from("return"))])]);
        assert_eq!(list.check(&missing, &named).unwrap_err().to_string(), "`[0].blankLine` is required (what goes between)");
        let unknown = Value::List(vec![rec(&[("blankLine", Value::from("any")), ("x", Value::Null)])]);
        assert!(list.check(&unknown, &named).unwrap_err().problem.contains("no key `x`"));
        let wrong_named = Value::List(vec![rec(&[("blankLine", Value::from("any")), ("prev", Value::from("if"))])]);
        assert_eq!(list.check(&wrong_named, &named).unwrap_err().path, "[0].prev");
    }

    #[test]
    fn ranges_regexes_maps_and_unions() {
        let named = Schemas::new();
        let count = Schema::Range(Box::new(Schema::Int), Some(0.0), None);
        assert!(count.check(&Value::Num(2.0), &named).is_ok());
        assert_eq!(count.check(&Value::Num(-1.0), &named).unwrap_err().problem, "must be int(0..), got -1");
        let names = Schema::Range(Box::new(Schema::List(Box::new(Schema::Union(vec![lit("a"), lit("b")])))), Some(1.0), None);
        assert_eq!(names.to_string(), "(\"a\" | \"b\")[1..]");
        assert!(names.check(&Value::List(Vec::new()), &named).is_err());
        assert!(count.check(&Value::Num(1.5), &named).is_err());
        assert!(Schema::Regex.check(&Value::from("^_"), &named).is_ok());
        assert!(Schema::Regex.check(&Value::from("("), &named).unwrap_err().problem.starts_with("is not a valid regex"));
        let map = Schema::Record(Vec::new(), Some(Box::new(Schema::Bool)));
        assert!(map.check(&rec(&[("a", Value::Bool(true))]), &named).is_ok());
        assert_eq!(map.check(&rec(&[("a", Value::Num(1.0))]), &named).unwrap_err().path, "a");
        // A union reports the misfit inside the option of the value's kind.
        let either = Schema::Union(vec![lit("never"), Schema::Record(vec![field("max", Schema::Int, FieldDefault::Value(Value::Num(1.0)), None)], None)]);
        assert_eq!(either.check(&rec(&[("max", Value::from("x"))]), &named).unwrap_err().path, "max");
        assert_eq!(either.check(&Value::from("x"), &named).unwrap_err().problem, "must be \"never\" | { max: int = 1 }, got \"x\"");
    }
}

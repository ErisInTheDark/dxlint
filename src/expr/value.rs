use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;

/// A runtime value in formatter expressions. Text borrows from the linted
/// file where it can (`'s`), so reading `text` or a field never allocates.
#[derive(Clone, Debug, PartialEq)]
pub enum Value<'s> {
    Null,
    Bool(bool),
    Num(f64),
    Str(Cow<'s, str>),
    List(Vec<Value<'s>>),
}

impl<'s> Value<'s> {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Num(_) => "num",
            Value::Str(_) => "str",
            Value::List(_) => "list",
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
        }
    }

    /// Text form used in templates and string functions; borrows when it
    /// already is text.
    pub fn text(&self) -> Cow<'_, str> {
        match self {
            Value::Str(s) => Cow::Borrowed(s),
            other => Cow::Owned(other.to_string()),
        }
    }

    pub fn into_owned(self) -> Value<'static> {
        match self {
            Value::Null => Value::Null,
            Value::Bool(b) => Value::Bool(b),
            Value::Num(n) => Value::Num(n),
            Value::Str(s) => Value::Str(Cow::Owned(s.into_owned())),
            Value::List(items) => Value::List(items.into_iter().map(Value::into_owned).collect()),
        }
    }

    pub fn num(&self) -> Result<f64, String> {
        match self {
            Value::Num(n) => Ok(*n),
            Value::Bool(b) => Ok(f64::from(u8::from(*b))),
            Value::Str(s) => s.trim().parse().map_err(|_| format!("expected a number, got \"{s}\"")),
            Value::Null => Ok(0.0),
            Value::List(_) => Err("expected a number, got a list".into()),
        }
    }

    /// Lists stay lists; anything else becomes a one-item list (null: empty).
    pub fn items(&self) -> Vec<Value<'s>> {
        match self {
            Value::List(l) => l.clone(),
            Value::Null => Vec::new(),
            other => vec![other.clone()],
        }
    }

    /// Loose equality: numbers compare numerically even against numeric text.
    pub fn loose_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Num(a), Value::Str(s)) | (Value::Str(s), Value::Num(a)) => s.trim().parse::<f64>().is_ok_and(|b| b == *a),
            (Value::Null, Value::Str(s)) | (Value::Str(s), Value::Null) => s.is_empty(),
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::List(a), Value::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.loose_eq(y)),
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            _ => false,
        }
    }

    pub fn compare(&self, other: &Value) -> Ordering {
        match (self.num(), other.num()) {
            (Ok(a), Ok(b)) if !matches!((self, other), (Value::Str(_), Value::Str(_))) => {
                a.partial_cmp(&b).unwrap_or(Ordering::Equal)
            }
            _ => self.text().cmp(&other.text()),
        }
    }
}

impl fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => Ok(()),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => write!(f, "{}", *n as i64),
            Value::Num(n) => write!(f, "{n}"),
            Value::Str(s) => f.write_str(s),
            Value::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                Ok(())
            }
        }
    }
}

impl<'s> From<&'s str> for Value<'s> {
    fn from(s: &'s str) -> Self {
        Value::Str(Cow::Borrowed(s))
    }
}

impl From<String> for Value<'_> {
    fn from(s: String) -> Self {
        Value::Str(Cow::Owned(s))
    }
}

impl From<f64> for Value<'_> {
    fn from(n: f64) -> Self {
        Value::Num(n)
    }
}

impl From<usize> for Value<'_> {
    fn from(n: usize) -> Self {
        Value::Num(n as f64)
    }
}

impl From<bool> for Value<'_> {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

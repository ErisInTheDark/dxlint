use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

/// A runtime value in formatter expressions. Text borrows from the linted
/// file where it can (`'s`), so reading `text` or a field never allocates.
/// Lists and records are shared: copying one (reading a `let`, a parameter,
/// a record's key) never copies its items.
#[derive(Clone, Debug, PartialEq)]
pub enum Value<'s> {
    Null,
    Bool(bool),
    Num(f64),
    Str(Text<'s>),
    List(Arc<[Value<'s>]>),
    /// `{ key: value }`, keys in written order.
    Record(Arc<[(Key, Value<'s>)]>),
    /// A node of the file's tree (from a link such as `declaration`): reads
    /// as its text, and `.x` reads `x` on it.
    Node(u32, Text<'s>),
}

/// Text of a value: borrowed from the linted file, or shared, so copying a
/// value (a memoised fact's `space_before.text`) never copies the text.
#[derive(Clone)]
pub enum Text<'s> {
    Borrowed(&'s str),
    Shared(Arc<str>),
}

impl std::ops::Deref for Text<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            Text::Borrowed(s) => s,
            Text::Shared(s) => s,
        }
    }
}

impl Text<'_> {
    fn into_static(self) -> Text<'static> {
        match self {
            Text::Borrowed(s) => Text::Shared(s.into()),
            Text::Shared(s) => Text::Shared(s),
        }
    }
}

impl PartialEq for Text<'_> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl fmt::Display for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self)
    }
}

impl fmt::Debug for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

/// A record key: usually a literal name, which costs no allocation.
pub type Key = Cow<'static, str>;

impl<'s> Value<'s> {
    pub fn list(items: Vec<Value<'s>>) -> Self {
        Value::List(items.into())
    }

    /// From a `Vec` or, without an extra copy, an array.
    pub fn record(entries: impl Into<Arc<[(Key, Value<'s>)]>>) -> Self {
        Value::Record(entries.into())
    }

    /// Text that outlives the file it came from, in one allocation; the
    /// most common gaps (`""`, `" "`) in none.
    pub fn text_copy(text: &str) -> Value<'static> {
        Value::Str(match text {
            "" => Text::Borrowed(""),
            " " => Text::Borrowed(" "),
            _ => Text::Shared(text.into()),
        })
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Num(_) => "num",
            Value::Str(_) => "str",
            Value::List(_) => "list",
            Value::Record(_) => "record",
            Value::Node(..) => "node",
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Record(r) => !r.is_empty(),
            Value::Node(..) => true,
        }
    }

    /// A node as the text it stands for (outside `.x` reads and `is`).
    fn as_text(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Node(_, s) => Some(s),
            _ => None,
        }
    }

    /// The value owning all its text.
    pub fn into_static(self) -> Value<'static> {
        match self {
            Value::Null => Value::Null,
            Value::Bool(b) => Value::Bool(b),
            Value::Num(n) => Value::Num(n),
            Value::Str(s) => Value::Str(s.into_static()),
            Value::List(items) => Value::List(items.iter().cloned().map(Value::into_static).collect()),
            Value::Record(entries) => Value::Record(entries.iter().map(|(k, v)| (k.clone(), v.clone().into_static())).collect()),
            Value::Node(id, s) => Value::Node(id, s.into_static()),
        }
    }

    /// A record's value for `key` (null when missing, or not a record).
    pub fn key(&self, key: &str) -> Value<'s> {
        match self {
            Value::Record(entries) => entries.iter().find(|(k, _)| &**k == key).map_or(Value::Null, |(_, v)| v.clone()),
            _ => Value::Null,
        }
    }

    /// The value as zlint source: `"a"`, `[1, { k: true }]`.
    pub fn literal(&self) -> String {
        match self {
            Value::Null => "null".into(),
            Value::Str(s) | Value::Node(_, s) => format!("{:?}", &**s),
            Value::List(items) => format!("[{}]", items.iter().map(Value::literal).collect::<Vec<_>>().join(", ")),
            Value::Record(entries) => {
                if entries.is_empty() {
                    return "{}".into();
                }
                let body: Vec<String> = entries.iter().map(|(k, v)| format!("{k}: {}", v.literal())).collect();
                format!("{{ {} }}", body.join(", "))
            }
            other => other.to_string(),
        }
    }

    /// Text form used in templates and string functions; borrows when it
    /// already is text.
    pub fn text(&self) -> Cow<'_, str> {
        match self.as_text() {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(self.to_string()),
        }
    }

    pub fn into_owned(self) -> Value<'static> {
        self.into_static()
    }

    pub fn num(&self) -> Result<f64, String> {
        match self {
            Value::Num(n) => Ok(*n),
            Value::Bool(b) => Ok(f64::from(u8::from(*b))),
            Value::Str(s) | Value::Node(_, s) => s.trim().parse().map_err(|_| format!("expected a number, got \"{s}\"")),
            Value::Null => Ok(0.0),
            Value::List(_) => Err("expected a number, got a list".into()),
            Value::Record(_) => Err("expected a number, got a record".into()),
        }
    }

    /// Like [`Value::items`], borrowing a list's items.
    pub fn item_slice(&self) -> Cow<'_, [Value<'s>]> {
        match self {
            Value::List(l) => Cow::Borrowed(l),
            Value::Null => Cow::Borrowed(&[]),
            other => Cow::Owned(vec![other.clone()]),
        }
    }

    /// Lists stay lists; anything else becomes a one-item list (null: empty).
    pub fn items(&self) -> Vec<Value<'s>> {
        match self {
            Value::List(l) => l.to_vec(),
            Value::Null => Vec::new(),
            other => vec![other.clone()],
        }
    }

    /// Loose equality: numbers compare numerically even against numeric text.
    pub fn loose_eq(&self, other: &Value) -> bool {
        match (self, other) {
            // The same node; otherwise nodes compare as their text.
            (Value::Node(a, _), Value::Node(b, _)) => a == b,
            (Value::Node(_, s), v) | (v, Value::Node(_, s)) => Value::Str(Text::Borrowed(s)).loose_eq(v),
            (Value::Num(a), Value::Str(s)) | (Value::Str(s), Value::Num(a)) => s.trim().parse::<f64>().is_ok_and(|b| b == *a),
            (Value::Null, Value::Str(s)) | (Value::Str(s), Value::Null) => s.is_empty(),
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::List(a), Value::List(b)) => a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.loose_eq(y)),
            (Value::Record(a), Value::Record(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v.loose_eq(v2)))
            }
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
            Value::Str(s) | Value::Node(_, s) => f.write_str(s),
            Value::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                Ok(())
            }
            Value::Record(_) => f.write_str(&self.literal()),
        }
    }
}

impl<'s> From<&'s str> for Value<'s> {
    fn from(s: &'s str) -> Self {
        Value::Str(Text::Borrowed(s))
    }
}

impl From<String> for Value<'_> {
    fn from(s: String) -> Self {
        Value::Str(Text::Shared(s.into()))
    }
}

impl<'s> From<Cow<'s, str>> for Value<'s> {
    fn from(s: Cow<'s, str>) -> Self {
        match s {
            Cow::Borrowed(s) => Value::from(s),
            Cow::Owned(s) => Value::from(s),
        }
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

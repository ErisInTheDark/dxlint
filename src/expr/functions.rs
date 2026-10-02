//! Built-in functions available in formatter expressions. Adding one is a
//! single table entry; `zlint functions` lists them from here.

use super::case;
use super::value::Value;

pub struct Function {
    pub name: &'static str,
    pub signature: &'static str,
    pub doc: &'static str,
    pub min: usize,
    pub max: usize,
    pub call: for<'s> fn(&[Value<'s>]) -> Result<Value<'s>, String>,
}

impl std::fmt::Debug for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.signature)
    }
}

pub fn lookup(name: &str) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.name == name)
}

pub fn names() -> impl Iterator<Item = &'static str> {
    FUNCTIONS.iter().map(|f| f.name)
}

fn text(v: &Value) -> String {
    v.text().into_owned()
}

fn count(v: &Value) -> Result<usize, String> {
    let n = v.num()?;
    if n < 0.0 || n.fract() != 0.0 {
        return Err(format!("expected a whole non-negative number, got {n}"));
    }
    Ok(n as usize)
}

macro_rules! text_fn {
    ($name:literal, $doc:literal, $f:expr) => {
        Function { name: $name, signature: concat!($name, "(text)"), doc: $doc, min: 1, max: 1, call: |a| {
                let f: fn(&str) -> String = $f;
                Ok(Value::from(f(&a[0].text())))
            },
        }
    };
}

fn trim_with(s: &str, chars: Option<&Value>, start: bool, end: bool) -> String {
    let set: Option<Vec<char>> = chars.map(|c| text(c).chars().collect());
    let pred = |c: char| match &set {
        Some(set) => set.contains(&c),
        None => c.is_whitespace(),
    };
    let s = if start { s.trim_start_matches(pred) } else { s };
    let s = if end { s.trim_end_matches(pred) } else { s };
    s.to_string()
}

fn pad<'s>(a: &[Value<'s>], at_start: bool) -> Result<Value<'s>, String> {
    let s = text(&a[0]);
    let width = count(&a[1])?;
    let fill = a.get(2).map(text).unwrap_or_else(|| " ".into());
    let fill = fill.chars().next().ok_or("pad fill cannot be empty")?;
    let missing = width.saturating_sub(s.chars().count());
    let padding: String = std::iter::repeat_n(fill, missing).collect();
    Ok(Value::from(if at_start { padding + &s } else { s + &padding }))
}

fn slice<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let resolve = |i: f64, len: usize| -> usize {
        if i < 0.0 {
            len.saturating_sub((-i) as usize)
        } else {
            (i as usize).min(len)
        }
    };
    match &a[0] {
        Value::List(items) => {
            let len = items.len();
            let from = resolve(a[1].num()?, len);
            let to = a.get(2).map(|v| v.num()).transpose()?.map_or(len, |t| resolve(t, len));
            Ok(Value::List(items.get(from..to.max(from)).unwrap_or_default().to_vec()))
        }
        other => {
            let chars: Vec<char> = text(other).chars().collect();
            let len = chars.len();
            let from = resolve(a[1].num()?, len);
            let to = a.get(2).map(|v| v.num()).transpose()?.map_or(len, |t| resolve(t, len));
            Ok(Value::from(chars[from..to.max(from)].iter().collect::<String>()))
        }
    }
}

fn fold_num<'s>(a: &[Value<'s>], pick: fn(f64, f64) -> f64) -> Result<Value<'s>, String> {
    let items: Vec<Value<'s>> = if a.len() == 1 { a[0].items() } else { a.to_vec() };
    let mut it = items.iter();
    let first = it.next().ok_or("needs at least one value")?.num()?;
    it.try_fold(first, |acc, v| Ok(pick(acc, v.num()?))).map(Value::Num)
}

pub static FUNCTIONS: &[Function] = &[
    Function {
        name: "len",
        signature: "len(value)",
        doc: "characters in a text, or items in a list",
        min: 1,
        max: 1,
        call: |a| {
            Ok(Value::from(match &a[0] {
                Value::List(l) => l.len(),
                v => v.text().chars().count(),
            }))
        },
    },
    text_fn!("lower", "lowercase text", |s| s.to_lowercase()),
    text_fn!("upper", "uppercase text", |s| s.to_uppercase()),
    text_fn!("snake_case", "`fooBar` -> `foo_bar`", case::snake),
    text_fn!("camel_case", "`foo_bar` -> `fooBar`", case::camel),
    text_fn!("pascal_case", "`foo_bar` -> `FooBar`", case::pascal),
    text_fn!("kebab_case", "`fooBar` -> `foo-bar`", case::kebab),
    text_fn!("screaming_case", "`fooBar` -> `FOO_BAR`", case::screaming),
    text_fn!("title_case", "`foo_bar` -> `Foo Bar`", case::title),
    Function {
        name: "trim",
        signature: "trim(text, chars?)",
        doc: "remove whitespace (or the given chars) from both ends",
        min: 1,
        max: 2,
        call: |a| Ok(Value::from(trim_with(&text(&a[0]), a.get(1), true, true))),
    },
    Function {
        name: "trim_start",
        signature: "trim_start(text, chars?)",
        doc: "remove whitespace (or the given chars) from the start",
        min: 1,
        max: 2,
        call: |a| Ok(Value::from(trim_with(&text(&a[0]), a.get(1), true, false))),
    },
    Function {
        name: "trim_end",
        signature: "trim_end(text, chars?)",
        doc: "remove whitespace (or the given chars) from the end",
        min: 1,
        max: 2,
        call: |a| Ok(Value::from(trim_with(&text(&a[0]), a.get(1), false, true))),
    },
    Function {
        name: "replace",
        signature: "replace(text, from, to)",
        doc: "replace every occurrence of `from` with `to`",
        min: 3,
        max: 3,
        call: |a| {
            let from = text(&a[1]);
            if from.is_empty() {
                return Err("replace: `from` cannot be empty".into());
            }
            Ok(Value::from(text(&a[0]).replace(&from, &text(&a[2]))))
        },
    },
    Function {
        name: "repeat",
        signature: "repeat(text, n)",
        doc: "the text repeated n times",
        min: 2,
        max: 2,
        call: |a| {
            let n = count(&a[1])?;
            if n > 1_000_000 {
                return Err("repeat: count too large".into());
            }
            Ok(Value::from(text(&a[0]).repeat(n)))
        },
    },
    Function {
        name: "pad_start",
        signature: "pad_start(text, width, fill?)",
        doc: "pad on the left to `width` characters",
        min: 2,
        max: 3,
        call: |a| pad(a, true),
    },
    Function {
        name: "pad_end",
        signature: "pad_end(text, width, fill?)",
        doc: "pad on the right to `width` characters",
        min: 2,
        max: 3,
        call: |a| pad(a, false),
    },
    Function {
        name: "starts_with",
        signature: "starts_with(text, prefix)",
        doc: "does the text start with prefix?",
        min: 2,
        max: 2,
        call: |a| Ok(Value::Bool(a[0].text().starts_with(&*a[1].text()))),
    },
    Function {
        name: "ends_with",
        signature: "ends_with(text, suffix)",
        doc: "does the text end with suffix?",
        min: 2,
        max: 2,
        call: |a| Ok(Value::Bool(a[0].text().ends_with(&*a[1].text()))),
    },
    Function {
        name: "contains",
        signature: "contains(text_or_list, needle)",
        doc: "substring test for text, membership test for lists",
        min: 2,
        max: 2,
        call: |a| {
            Ok(Value::Bool(match &a[0] {
                Value::List(items) => items.iter().any(|i| i.loose_eq(&a[1])),
                v => v.text().contains(&*a[1].text()),
            }))
        },
    },
    Function {
        name: "split",
        signature: "split(text, sep)",
        doc: "split text into a list",
        min: 2,
        max: 2,
        call: |a| {
            let sep = text(&a[1]);
            if sep.is_empty() {
                return Ok(Value::List(text(&a[0]).chars().map(|c| Value::from(c.to_string())).collect()));
            }
            Ok(Value::List(text(&a[0]).split(&sep).map(|p| Value::from(p.to_string())).collect()))
        },
    },
    Function {
        name: "lines",
        signature: "lines(text)",
        doc: "split text into lines",
        min: 1,
        max: 1,
        call: |a| Ok(Value::List(text(&a[0]).lines().map(|p| Value::from(p.to_string())).collect())),
    },
    Function {
        name: "join",
        signature: "join(list, sep?)",
        doc: "join list items with a separator (default none)",
        min: 1,
        max: 2,
        call: |a| {
            let sep = a.get(1).map(text).unwrap_or_default();
            Ok(Value::from(a[0].items().iter().map(text).collect::<Vec<_>>().join(&sep)))
        },
    },
    Function {
        name: "sort",
        signature: "sort(list)",
        doc: "sorted copy of a list",
        min: 1,
        max: 1,
        call: |a| {
            let mut items = a[0].items();
            items.sort_by(|x, y| x.compare(y));
            Ok(Value::List(items))
        },
    },
    Function {
        name: "unique",
        signature: "unique(list)",
        doc: "list without repeated items (first one wins)",
        min: 1,
        max: 1,
        call: |a| {
            let mut out: Vec<Value> = Vec::new();
            for item in a[0].items() {
                if !out.iter().any(|o| o.loose_eq(&item)) {
                    out.push(item);
                }
            }
            Ok(Value::List(out))
        },
    },
    Function {
        name: "reverse",
        signature: "reverse(text_or_list)",
        doc: "reversed text or list",
        min: 1,
        max: 1,
        call: |a| {
            Ok(match &a[0] {
                Value::List(items) => Value::List(items.iter().rev().cloned().collect()),
                v => Value::from(text(v).chars().rev().collect::<String>()),
            })
        },
    },
    Function {
        name: "first",
        signature: "first(list)",
        doc: "first item (null when empty)",
        min: 1,
        max: 1,
        call: |a| Ok(a[0].items().into_iter().next().unwrap_or(Value::Null)),
    },
    Function {
        name: "last",
        signature: "last(list)",
        doc: "last item (null when empty)",
        min: 1,
        max: 1,
        call: |a| Ok(a[0].items().into_iter().last().unwrap_or(Value::Null)),
    },
    Function {
        name: "count",
        signature: "count(value)",
        doc: "how many items a field matched: 0 for null, 1 for a single value",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(a[0].items().len())),
    },
    Function {
        name: "slice",
        signature: "slice(text_or_list, from, to?)",
        doc: "part of a text or list; negative indexes count from the end",
        min: 2,
        max: 3,
        call: slice,
    },
    Function {
        name: "str",
        signature: "str(value)",
        doc: "value as text",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(text(&a[0]))),
    },
    Function {
        name: "num",
        signature: "num(value)",
        doc: "value as a number",
        min: 1,
        max: 1,
        call: |a| a[0].num().map(Value::Num),
    },
    Function {
        name: "default",
        signature: "default(value, fallback)",
        doc: "fallback when value is null or empty",
        min: 2,
        max: 2,
        call: |a| Ok(if a[0].truthy() || matches!(a[0], Value::Num(_) | Value::Bool(_)) { a[0].clone() } else { a[1].clone() }),
    },
    Function {
        name: "min",
        signature: "min(a, b, ...) or min(list)",
        doc: "smallest number",
        min: 1,
        max: 64,
        call: |a| fold_num(a, f64::min),
    },
    Function {
        name: "max",
        signature: "max(a, b, ...) or max(list)",
        doc: "largest number",
        min: 1,
        max: 64,
        call: |a| fold_num(a, f64::max),
    },
    Function {
        name: "abs",
        signature: "abs(n)",
        doc: "absolute value",
        min: 1,
        max: 1,
        call: |a| Ok(Value::Num(a[0].num()?.abs())),
    },
    Function {
        name: "round",
        signature: "round(n)",
        doc: "nearest whole number",
        min: 1,
        max: 1,
        call: |a| Ok(Value::Num(a[0].num()?.round())),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value<'static>]) -> Value<'static> {
        (lookup(name).unwrap().call)(args).unwrap()
    }

    #[test]
    fn text_helpers() {
        assert_eq!(call("trim_end", &["ab\r\n\n".into(), "\r\n".into()]), Value::from("ab"));
        assert_eq!(call("pad_start", &["7".into(), 3.0.into(), "0".into()]), Value::from("007"));
        assert_eq!(call("slice", &["hello".into(), (-3.0).into()]), Value::from("llo"));
    }

    #[test]
    fn list_helpers_accept_single_values() {
        assert_eq!(call("join", &["a".into(), ", ".into()]), Value::from("a"));
        let sorted = call("sort", &[Value::List(vec!["b".into(), "a".into()])]);
        assert_eq!(call("join", &[sorted, ",".into()]), Value::from("a,b"));
        assert_eq!(call("count", &[Value::Null]), Value::from(0usize));
        assert_eq!(call("max", &[Value::List(vec![1.0.into(), 5.0.into()])]), Value::from(5.0));
    }
}

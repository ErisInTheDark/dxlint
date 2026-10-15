//! Built-in functions available in formatter expressions. Adding one is a
//! single table entry; `zlint functions` lists them from here.

use super::case;
use super::value::Value;
use std::borrow::Cow;

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

fn text<'a>(v: &'a Value<'_>) -> Cow<'a, str> {
    v.text()
}

fn count(v: &Value) -> Result<usize, String> {
    let n = v.num()?;
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 {
        return Err(format!("expected a whole non-negative number, got {n}"));
    }
    Ok(n as usize)
}

fn javascript_number_text(text: &str) -> f64 {
    // JavaScript StringNumericLiteral whitespace differs from Rust's trim.
    let text = text.trim_matches(|c| matches!(c,
        '\t' | '\u{000b}' | '\u{000c}' | ' ' | '\u{00a0}' | '\u{feff}' | '\n' | '\r'
        | '\u{2028}' | '\u{2029}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'));
    match text {
        "" => return 0.0,
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let radix = match text.get(..2) {
        Some("0x" | "0X") => Some(16),
        Some("0o" | "0O") => Some(8),
        Some("0b" | "0B") => Some(2),
        _ => None,
    };
    if let Some(radix) = radix {
        let digits = &text[2..];
        if digits.is_empty() || !digits.bytes().all(|digit| char::from(digit).is_digit(radix)) {
            return f64::NAN;
        }
        return BigIntDecimal::parse(text, false).map_or(f64::NAN, |value| value.to_number());
    }
    if !text.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.' | b'e' | b'E')) {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

fn javascript_literal_number(kind: &str, text: &str) -> Option<f64> {
    match kind {
        "string" => Some(javascript_number_text(text)),
        "boolean" => match text { "true" => Some(1.0), "false" => Some(0.0), _ => None },
        "null" => Some(0.0),
        "undefined" | "regex" => Some(f64::NAN),
        "number" => super::js::number(text),
        "bigint" => BigIntDecimal::parse(text, true).map(|value| value.to_number()),
        _ => None,
    }
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
    Ok(Value::from(if at_start { padding + &s } else { s.into_owned() + &padding }))
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
            Ok(Value::List(items.get(from..to.max(from)).unwrap_or_default().into()))
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

/// `byte_slice(text, from, to?)`: by byte offsets (what `offset` and native
/// spans count), not characters; negative counts from the end.
fn byte_slice<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let s = a[0].text();
    let len = s.len();
    let resolve = |i: f64| if i < 0.0 { len.saturating_sub((-i) as usize) } else { (i as usize).min(len) };
    let from = resolve(a[1].num()?);
    let to = a.get(2).map(|v| v.num()).transpose()?.map_or(len, resolve).max(from);
    match s.get(from..to) {
        Some(part) => Ok(a[0].text_part(part)),
        None => Err(format!("byte_slice: {from}..{to} is not on character boundaries")),
    }
}

fn byte_char<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let text = a[0].text();
    let offset = a[1].num()?;
    if !offset.is_finite() || offset < 0.0 || offset.fract() != 0.0 || offset > text.len() as f64 {
        return Err("byte_char: offset must be an integer within text".into());
    }
    let at = offset as usize;
    if !text.is_char_boundary(at) {
        return Err("byte_char: offset is not on a character boundary".into());
    }
    let part = if a.get(2).is_some_and(Value::truthy) {
        let width = text[..at].chars().next_back().map_or(0, char::len_utf8);
        &text[at - width..at]
    } else {
        let width = text[at..].chars().next().map_or(0, char::len_utf8);
        &text[at..at + width]
    };
    Ok(a[0].text_part(part))
}

/// Extended grapheme clusters, approximately: what joins onto the character
/// before it (joiners and what they join, marks, variation selectors,
/// emoji modifiers and tags, the second half of a flag) starts none.
fn grapheme_count(text: &str) -> usize {
    let mut count = 0;
    let mut joined = false;
    let mut flag_half = false;
    for c in text.chars() {
        let v = c as u32;
        let regional = (0x1F1E6..=0x1F1FF).contains(&v);
        let extends = c == '\u{200D}'
            || joined
            || super::js::is_mark(c)
            || (0xFE00..=0xFE0F).contains(&v)
            || (0x1F3FB..=0x1F3FF).contains(&v)
            || (0xE0020..=0xE007F).contains(&v)
            || regional && flag_half;
        joined = c == '\u{200D}';
        flag_half = regional && !flag_half;
        if !extends {
            count += 1;
        }
    }
    count
}

fn fold_num<'s>(a: &[Value<'s>], pick: fn(f64, f64) -> f64) -> Result<Value<'s>, String> {
    let items: Vec<Value<'s>> = if a.len() == 1 { a[0].items() } else { a.to_vec() };
    let mut it = items.iter();
    let first = it.next().ok_or("needs at least one value")?.num()?;
    it.try_fold(first, |acc, v| Ok(pick(acc, v.num()?))).map(Value::Num)
}

pub static FUNCTIONS: &[Function] = &[
    Function {
        name: "js_re_test",
        signature: "js_re_test(text, pattern, flags?)",
        doc: "stateless JavaScript regex match; unsupported features and work exhaustion fail the rule",
        min: 2,
        max: 3,
        call: |a| crate::js_regex::test(&text(&a[0]), &text(&a[1]), &a.get(2).map(text).unwrap_or_default()).map(Value::Bool),
    },
    Function {
        name: "re_valid",
        signature: "re_valid(pattern)",
        doc: "can the engine compile this regex pattern?",
        min: 1,
        max: 1,
        call: |a| Ok(Value::Bool(super::re::compile(&text(&a[0])).is_ok())),
    },
    Function {
        name: "re_test",
        signature: "re_test(text, pattern)",
        doc: "does the regex match anywhere in the text?",
        min: 2,
        max: 2,
        call: super::re::test,
    },
    Function {
        name: "re_find",
        signature: "re_find(text, pattern)",
        doc: "the first regex match (null when none)",
        min: 2,
        max: 2,
        call: super::re::find,
    },
    Function {
        name: "re_count",
        signature: "re_count(text, pattern)",
        doc: "how many times the regex matches",
        min: 2,
        max: 2,
        call: super::re::count,
    },
    Function {
        name: "re_escape",
        signature: "re_escape(text)",
        doc: "text with regex metacharacters escaped, for building patterns",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(regex_syntax::escape(&a[0].text()))),
    },
    Function {
        name: "re_replace",
        signature: "re_replace(text, pattern, replacement)",
        doc: "replace every regex match; `$1` / `${name}` insert groups",
        min: 3,
        max: 3,
        call: super::re::replace,
    },
    Function {
        name: "len",
        signature: "len(value)",
        doc: "characters in a text, items in a list, or keys in a record",
        min: 1,
        max: 1,
        call: |a| {
            Ok(Value::from(match &a[0] {
                Value::List(l) => l.len(),
                Value::Record(r) => r.len(),
                v => v.text().chars().count(),
            }))
        },
    },
    Function {
        name: "compare_text",
        signature: "compare_text(a, b, natural?, insensitive?)",
        doc: "-1, 0 or 1: text order by UTF-16 unit, as sorting rules compare names (`natural`: `a2` before `a10`; `insensitive`: ignoring case)",
        min: 2,
        max: 4,
        call: |a| {
            let flag = |i: usize| a.get(i).is_some_and(Value::truthy);
            let order = super::js::compare_names(&a[0].text(), &a[1].text(), flag(3), flag(2));
            Ok(Value::Num(order as i8 as f64))
        },
    },
    Function {
        name: "js_string",
        signature: "js_string(text)",
        doc: "Decode a quoted JavaScript string literal, including escapes and line continuations; null for invalid syntax. Lone surrogates become U+FFFD",
        min: 1,
        max: 1,
        call: |a| Ok(super::js::strings::decode(&a[0].text()).map_or(Value::Null, Value::from)),
    },
    Function {
        name: "json_string",
        signature: "json_string(text)",
        doc: "Text as a JSON string literal, quotes included (JavaScript `JSON.stringify` of a string)",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(serde_json::to_string(&*a[0].text()).unwrap_or_default())),
    },
    Function {
        name: "js_string_parts",
        signature: "js_string_parts(text)",
        doc: "Decode a quoted string or substitution-free template to text, UTF-16 units and Unicode-scalar spans. Offsets are bytes within the literal; null for invalid syntax",
        min: 1,
        max: 1,
        call: |a| {
            let Some(decoded) = super::js::strings::decode_mapped(&a[0].text()) else { return Ok(Value::Null) };
            let span = |source: std::ops::Range<usize>| Value::record(vec![
                ("offset".into(), Value::Num(source.start as f64)),
                ("end_offset".into(), Value::Num(source.end as f64)),
            ]);
            Ok(Value::record(vec![
                ("text".into(), Value::from(decoded.text)),
                ("spans".into(), Value::list(decoded.spans.into_iter().map(span).collect())),
                ("units".into(), Value::list(decoded.units.into_iter().map(|unit| Value::record(vec![
                    ("value".into(), Value::Num(f64::from(unit.value))),
                    ("offset".into(), Value::Num(unit.source.start as f64)),
                    ("end_offset".into(), Value::Num(unit.source.end as f64)),
                ])).collect())),
            ]))
        },
    },
    Function {
        name: "js_bigint",
        signature: "js_bigint(text)",
        doc: "A JavaScript bigint literal as exact decimal text, including radix prefixes and separators; null for invalid syntax",
        min: 1,
        max: 1,
        call: |a| Ok(super::js::bigint_literal(&a[0].text()).map_or(Value::Null, Value::from)),
    },
    Function {
        name: "js_number",
        signature: "js_number(value)",
        doc: "JavaScript Number conversion of primitive values or literal_value records; NaN for non-numeric strings, null for unsupported values (including symbols)",
        min: 1,
        max: 1,
        call: |a| {
            let number = match &a[0] {
                Value::Num(n) => Some(*n),
                Value::Bool(b) => Some(f64::from(u8::from(*b))),
                Value::Null => Some(0.0),
                Value::Str(s) | Value::Node(_, s) => Some(javascript_number_text(s)),
                Value::Record(_) => javascript_literal_number(&a[0].key("type").text(), &a[0].key("value").text()),
                _ => None,
            };
            Ok(number.map_or(Value::Null, Value::Num))
        },
    },
    Function {
        name: "js_to_string",
        signature: "js_to_string(value)",
        doc: "JavaScript `String(value)` of a primitive or `literal_value` record (numbers as JavaScript prints them, bigints without `n`); null for other values",
        min: 1,
        max: 1,
        call: |a| Ok(match &a[0] {
            Value::Num(n) => Value::from(super::js::number_to_string(*n)),
            Value::Bool(b) => Value::from(b.to_string()),
            Value::Null => Value::from("null"),
            Value::Str(_) | Value::Node(..) => a[0].clone(),
            Value::Record(_) => match &*a[0].key("type").text() {
                "number" => javascript_literal_number("number", &a[0].key("value").text()).map_or(Value::Null, |n| Value::from(super::js::number_to_string(n))),
                "string" | "bigint" | "boolean" => Value::from(a[0].key("value").text().into_owned()),
                "null" => Value::from("null"),
                _ => Value::Null,
            },
            _ => Value::Null,
        }),
    },
    Function {
        name: "js_number_to_precision",
        signature: "js_number_to_precision(value, digits)",
        doc: "JavaScript `value.toPrecision(digits)` for a number and whole `digits` 1..100; null otherwise",
        min: 2,
        max: 2,
        call: |a| Ok(match (&a[0], &a[1]) {
            (Value::Num(value), Value::Num(digits)) if digits.fract() == 0.0 && *digits >= 1.0 =>
                super::js::number_format::to_precision(*value, *digits as usize).map_or(Value::Null, Value::from),
            _ => Value::Null,
        }),
    },
    Function {
        name: "integer_radix",
        signature: "integer_radix(value, radix)",
        doc: "A whole finite number written in `radix` 2..36 (lowercase digits, exact for every double); null for fractions, infinities and other radixes",
        min: 2,
        max: 2,
        call: |a| Ok(match (&a[0], &a[1]) {
            (Value::Num(value), Value::Num(radix)) if radix.fract() == 0.0 && *radix >= 0.0 && *radix <= 36.0 =>
                super::js::number_format::integer_radix(*value, *radix as u32).map_or(Value::Null, Value::from),
            _ => Value::Null,
        }),
    },
    Function {
        name: "js_number_binary",
        signature: "js_number_binary(operator, left, right)",
        doc: "A JavaScript numeric binary operator (`+ - * / % ** & | ^ << >> >>>`) on two numbers, with JavaScript's results (NaN, Infinity, ToInt32 for bitwise); null for other operators or non-number operands",
        min: 3,
        max: 3,
        call: |a| Ok(match (&a[1], &a[2]) {
            (Value::Num(x), Value::Num(y)) => super::js::number_binary(&a[0].text(), *x, *y).map_or(Value::Null, Value::Num),
            _ => Value::Null,
        }),
    },
    Function {
        name: "js_number_unary",
        signature: "js_number_unary(operator, value)",
        doc: "A JavaScript numeric unary operator (`- + ~`) on a number; null for other operators or a non-number operand",
        min: 2,
        max: 2,
        call: |a| Ok(match &a[1] {
            Value::Num(x) => super::js::number_unary(&a[0].text(), *x).map_or(Value::Null, Value::Num),
            _ => Value::Null,
        }),
    },
    Function {
        name: "js_char_at",
        signature: "js_char_at(text, index)",
        doc: "JavaScript `text[index]`: the UTF-16 unit at `index` as text (a lone surrogate becomes U+FFFD, as js_string decodes them); null when out of range or `index` is not a whole number",
        min: 2,
        max: 2,
        call: |a| {
            let Value::Num(index) = a[1] else { return Ok(Value::Null) };
            if index < 0.0 || index.fract() != 0.0 { return Ok(Value::Null) }
            Ok(a[0].text().encode_utf16().nth(index as usize).map_or(Value::Null, |unit| Value::from(String::from_utf16_lossy(&[unit]))))
        },
    },
    Function {
        name: "js_compare",
        signature: "js_compare(a, b)",
        doc: "-1, 0 or 1 as JavaScript's `<` orders two literal values (`literal_value` records or plain values): strings by UTF-16 unit, bigints exactly, otherwise as numbers; null when unordered",
        min: 2,
        max: 2,
        call: |a| {
            let typed = |v: &Value| -> (String, String) {
                match v {
                    Value::Record(_) => (v.key("type").text().into_owned(), v.key("value").text().into_owned()),
                    Value::Num(_) => ("number".into(), v.text().into_owned()),
                    Value::Bool(_) => ("boolean".into(), v.text().into_owned()),
                    Value::Null => ("null".into(), "null".into()),
                    _ => ("string".into(), v.text().into_owned()),
                }
            };
            let ((ta, va), (tb, vb)) = (typed(&a[0]), typed(&a[1]));
            if ta == "string" && tb == "string" {
                return Ok(Value::Num(va.encode_utf16().cmp(vb.encode_utf16()) as i8 as f64));
            }
            let number = |t: &str, v: &str| javascript_literal_number(t, v).unwrap_or(f64::NAN);
            if ta == "bigint" || tb == "bigint" {
                let order = if ta == "bigint" {
                    BigIntDecimal::parse(&va, true).and_then(|x| match tb.as_str() {
                        "bigint" | "string" => BigIntDecimal::parse(&vb, tb == "bigint").map(|y| x.compare(&y)),
                        _ => x.compare_number(number(&tb, &vb)),
                    })
                } else {
                    BigIntDecimal::parse(&vb, true).and_then(|y| match ta.as_str() {
                        "string" => BigIntDecimal::parse(&va, false).map(|x| x.compare(&y)),
                        _ => y.compare_number(number(&ta, &va)).map(std::cmp::Ordering::reverse),
                    })
                };
                return Ok(order.map_or(Value::Null, |o| Value::Num(o as i8 as f64)));
            }
            let (x, y) = (number(&ta, &va), number(&tb, &vb));
            Ok(x.partial_cmp(&y).map_or(Value::Null, |o| Value::Num(o as i8 as f64)))
        },
    },
    Function {
        name: "byte_len",
        signature: "byte_len(text)",
        doc: "bytes in a text's UTF-8: how far it moves an `offset`",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(a[0].text().len())),
    },
    Function {
        name: "utf16_len",
        signature: "utf16_len(text)",
        doc: "UTF-16 units in a text: its length as JavaScript counts it (`😀` is 2)",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(a[0].text().encode_utf16().count())),
    },
    Function {
        name: "keys",
        signature: "keys(record)",
        doc: "a record's keys, in order",
        min: 1,
        max: 1,
        call: |a| match &a[0] {
            Value::Record(r) => Ok(Value::List(r.iter().map(|(k, _)| Value::from(k.to_string())).collect())),
            Value::Null => Ok(Value::list(Vec::new())),
            v => Err(format!("expected a record, got a {}", v.type_name())),
        },
    },
    Function {
        name: "get",
        signature: "get(record_or_list, key)",
        doc: "a record's value for a computed key, or a list's item at a 0-based index (null when missing)",
        min: 2,
        max: 2,
        call: |a| Ok(match (&a[0], &a[1]) {
            (Value::List(items), Value::Num(at)) if *at >= 0.0 && at.fract() == 0.0 => items.get(*at as usize).cloned().unwrap_or(Value::Null),
            (record, key) => record.key(&key.text()),
        }),
    },
    Function {
        name: "values",
        signature: "values(record)",
        doc: "a record's values, in key order",
        min: 1,
        max: 1,
        call: |a| match &a[0] {
            Value::Record(r) => Ok(Value::List(r.iter().map(|(_, v)| v.clone()).collect())),
            Value::Null => Ok(Value::list(Vec::new())),
            v => Err(format!("expected a record, got a {}", v.type_name())),
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
            Ok(Value::from(text(&a[0]).replace(from.as_ref(), &text(&a[2]))))
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
        name: "index_of",
        signature: "index_of(list, value)",
        doc: "position of the first item equal to value, from 0 (-1 when absent)",
        min: 2,
        max: 2,
        call: |a| Ok(Value::Num(a[0].item_slice().iter().position(|i| i.loose_eq(&a[1])).map_or(-1.0, |i| i as f64))),
    },
    Function {
        name: "split",
        signature: "split(text, sep)",
        doc: "split text into a list",
        min: 2,
        max: 2,
        call: |a| {
            let sep = text(&a[1]);
            let s = a[0].text();
            if sep.is_empty() {
                return Ok(Value::List(s.char_indices().map(|(at, c)| a[0].text_part(&s[at..at + c.len_utf8()])).collect()));
            }
            Ok(Value::List(s.split(sep.as_ref()).map(|part| a[0].text_part(part)).collect()))
        },
    },
    Function {
        name: "lines",
        signature: "lines(text)",
        doc: "split text into lines",
        min: 1,
        max: 1,
        call: |a| {
            let s = a[0].text();
            Ok(Value::List(s.lines().map(|part| a[0].text_part(part)).collect()))
        },
    },
    Function {
        name: "join",
        signature: "join(list, sep?)",
        doc: "join list items with a separator (default none)",
        min: 1,
        max: 2,
        call: |a| {
            let sep = a.get(1).map(text).unwrap_or_default();
            Ok(Value::from(a[0].item_slice().iter().map(text).collect::<Vec<_>>().join(&sep)))
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
            Ok(Value::list(items))
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
            Ok(Value::list(out))
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
        call: |a| Ok(a[0].item_slice().first().cloned().unwrap_or(Value::Null)),
    },
    Function {
        name: "last",
        signature: "last(list)",
        doc: "last item (null when empty)",
        min: 1,
        max: 1,
        call: |a| Ok(a[0].item_slice().last().cloned().unwrap_or(Value::Null)),
    },
    Function {
        name: "count",
        signature: "count(value)",
        doc: "how many items a field matched: 0 for null, 1 for a single value",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(a[0].item_slice().len())),
    },
    Function {
        name: "zip",
        signature: "zip(a, b)",
        doc: "pairs `[a_i, b_i]` of two lists, as long as the shorter: `zip(xs, slice(xs, 1))` is each item with the next",
        min: 2,
        max: 2,
        call: |a| Ok(Value::list(a[0].item_slice().iter().zip(a[1].item_slice().iter()).map(|(x, y)| Value::list(vec![x.clone(), y.clone()])).collect())),
    },
    Function {
        name: "slice",
        signature: "slice(text_or_list, from, to?)",
        doc: "part of a text (by characters) or list; negative indexes count from the end",
        min: 2,
        max: 3,
        call: slice,
    },
    Function {
        name: "byte_slice",
        signature: "byte_slice(text, from, to?)",
        doc: "part of a text by byte offsets (what `offset`, `end_offset` and native spans count); negative counts from the end; an error off a character boundary",
        min: 2,
        max: 3,
        call: byte_slice,
    },
    Function {
        name: "byte_char",
        signature: "byte_char(text, offset, before?)",
        doc: "character at a byte boundary, or preceding it when before is true; empty at file edges; errors for invalid offsets",
        min: 2,
        max: 3,
        call: byte_char,
    },
    Function {
        name: "str",
        signature: "str(value)",
        doc: "value as text",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(text(&a[0]).into_owned())),
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
        name: "nonempty",
        signature: "nonempty(value)",
        doc: "null for empty text, a list or a record; otherwise the value unchanged",
        min: 1,
        max: 1,
        call: |a| Ok(a[0].clone().nonempty()),
    },
    Function {
        name: "take",
        signature: "take(list, n)",
        doc: "the first n items",
        min: 2,
        max: 2,
        call: |a| Ok(Value::list(a[0].item_slice().iter().take(count(&a[1])?).cloned().collect())),
    },
    Function {
        name: "drop",
        signature: "drop(list, n)",
        doc: "items after the first n",
        min: 2,
        max: 2,
        call: |a| Ok(Value::list(a[0].item_slice().iter().skip(count(&a[1])?).cloned().collect())),
    },
    Function {
        name: "flat",
        signature: "flat(list)",
        doc: "flatten one level of lists",
        min: 1,
        max: 1,
        call: |a| Ok(Value::list(a[0].item_slice().iter().flat_map(|item| item.item_slice().iter().cloned()).collect())),
    },
    Function {
        name: "concat",
        signature: "concat(list, other)",
        doc: "append the items of other",
        min: 2,
        max: 2,
        call: |a| Ok(Value::list(a[0].item_slice().iter().chain(a[1].item_slice()).cloned().collect())),
    },
    Function {
        name: "chunk",
        signature: "chunk(list, n)",
        doc: "consecutive groups of n items; includes a shorter final group",
        min: 2,
        max: 2,
        call: |a| {
            let n = count(&a[1])?;
            if n == 0 { return Err("chunk size must be positive".into()); }
            Ok(Value::list(a[0].item_slice().chunks(n).map(|chunk| Value::list(chunk.to_vec())).collect()))
        },
    },
    Function {
        name: "windows",
        signature: "windows(list, n)",
        doc: "overlapping groups of exactly n items",
        min: 2,
        max: 2,
        call: |a| {
            let n = count(&a[1])?;
            if n == 0 { return Err("window size must be positive".into()); }
            Ok(Value::list(a[0].item_slice().windows(n).map(|window| Value::list(window.to_vec())).collect()))
        },
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
        name: "sum",
        signature: "sum(a, b, ...) or sum(list)",
        doc: "total of numbers (0 when there are none)",
        min: 1,
        max: 64,
        call: |a| {
            let items: Vec<Value> = if a.len() == 1 { a[0].items() } else { a.to_vec() };
            items.iter().try_fold(0.0, |acc, v| Ok(acc + v.num()?)).map(Value::Num)
        },
    },
    Function {
        name: "compact",
        signature: "compact(list)",
        doc: "the list without null and empty items (always a list)",
        min: 1,
        max: 1,
        call: |a| Ok(Value::List(a[0].items().into_iter().filter(|v| !matches!(v, Value::Null) && v.text() != "").collect())),
    },
    Function {
        name: "any",
        signature: "any(list)",
        doc: "is any item truthy?",
        min: 1,
        max: 1,
        call: |a| Ok(Value::Bool(a[0].item_slice().iter().any(Value::truthy))),
    },
    Function {
        name: "regex_flags_error",
        signature: "regex_flags_error(flags, allowed)",
        doc: "what `RegExp`'s constructor says about `flags` (`u` with `v`, duplicates, unknown ones; extra `allowed` flags pass), as eslint's no-invalid-regexp words it; null when fine",
        min: 1,
        max: 2,
        call: |a| {
            let allowed: String = a.get(1).map(|v| v.items().iter().map(|x| x.text().into_owned()).collect()).unwrap_or_default();
            Ok(match super::js::regex_flags_error(&a[0].text(), &allowed) {
                Some(m) => Value::from(m),
                None => Value::Null,
            })
        },
    },
    Function {
        name: "graphemes",
        signature: "graphemes(text)",
        doc: "how many characters a reader sees in `text`: joined emoji (`👨‍👩‍👧‍👦`), flags, modifiers and combining marks count once",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(grapheme_count(&a[0].text()))),
    },
    Function {
        name: "type_of",
        signature: "type_of(x)",
        doc: "what kind of value `x` is: `null`, `bool`, `num`, `str`, `list` or `record` (`==` treats `\"\"` and `null` alike; this does not)",
        min: 1,
        max: 1,
        call: |a| Ok(Value::from(a[0].type_name())),
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
        name: "divide",
        signature: "divide(a, b)",
        doc: "a divided by b (`/` starts a regex); null when b is 0",
        min: 2,
        max: 2,
        call: |a| {
            let b = a[1].num()?;
            Ok(if b == 0.0 { Value::Null } else { Value::Num(a[0].num()? / b) })
        },
    },
    Function {
        name: "ceil",
        signature: "ceil(n)",
        doc: "smallest whole number not below n",
        min: 1,
        max: 1,
        call: |a| Ok(Value::Num(a[0].num()?.ceil())),
    },
    Function {
        name: "floor",
        signature: "floor(n)",
        doc: "largest whole number not above n",
        min: 1,
        max: 1,
        call: |a| Ok(Value::Num(a[0].num()?.floor())),
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

struct BigIntDecimal {
    negative: bool,
    digits: String,
}

impl BigIntDecimal {
    fn to_number(&self) -> f64 {
        let number = self.digits.parse().unwrap_or(f64::INFINITY);
        if self.negative { -number } else { number }
    }

    fn parse(text: &str, literal: bool) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return (!literal).then(|| Self { negative: false, digits: "0".into() });
        }
        let body = text.strip_prefix(['-', '+']).unwrap_or(text);
        if !literal && (body.contains(['_', 'n']) || (body != text && !body.bytes().all(|b| b.is_ascii_digit()))) {
            return None;
        }
        if body.trim_end_matches('n').is_empty() {
            return None;
        }
        if matches!(body.to_ascii_lowercase().as_str(), "0x" | "0o" | "0b") {
            return None;
        }
        let digits = super::js::bigint_decimal(body)?;
        Some(Self { negative: text.starts_with('-') && digits != "0", digits })
    }

    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        let magnitude = self.digits.len().cmp(&other.digits.len()).then_with(|| self.digits.cmp(&other.digits));
        match (self.negative, other.negative) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (true, true) => magnitude.reverse(),
            (false, false) => magnitude,
        }
    }

    fn compare_number(&self, number: f64) -> Option<std::cmp::Ordering> {
        if number.is_nan() {
            return None;
        }
        if number.is_infinite() {
            return Some(if number.is_sign_positive() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater });
        }
        // Fixed decimal formatting of an integral f64 retains its exact integer,
        // unlike conversion of an arbitrary bigint into a floating-point value.
        let integer = Self::parse(&format!("{:.0}", number.trunc()), false)?;
        let order = self.compare(&integer);
        Some(if order.is_eq() { 0.0_f64.partial_cmp(&number.fract())? } else { order })
    }
}

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
    fn byte_char_reads_both_sides_of_utf8_boundaries() {
        for input in [Value::from("a\u{1f600}b"), Value::from("a\u{1f600}b".to_owned()), Value::Node(7, "a\u{1f600}b".into())] {
            for (at, before, expected) in [(0, false, "a"), (1, false, "\u{1f600}"), (5, false, "b"),
                (6, false, ""), (0, true, ""), (1, true, "a"), (5, true, "\u{1f600}"), (6, true, "b")] {
                assert_eq!(call("byte_char", &[input.clone(), (at as f64).into(), before.into()]), Value::from(expected));
            }
            assert_eq!(call("byte_char", &[input.clone(), 1.0.into()]), Value::from("\u{1f600}"));
            for invalid in [-1.0, 2.0, 7.0, 1.5, f64::NAN, f64::INFINITY] {
                assert!((lookup("byte_char").unwrap().call)(&[input.clone(), invalid.into()]).is_err());
            }
        }
    }

    #[test]
    fn text_parts_preserve_boundaries_coercion_and_output_lifetimes() {
        for input in [Value::from("  a\u{1f600}b  "), Value::from("  a\u{1f600}b  ".to_owned()), Value::Node(7, "  a\u{1f600}b  ".into())] {
            assert_eq!(call("byte_slice", &[input.clone(), 3.0.into(), 7.0.into()]), Value::from("\u{1f600}"));
            assert_eq!(call("byte_slice", &[input.clone(), (-7.0).into(), (-3.0).into()]), Value::from("\u{1f600}"));
            assert_eq!(call("byte_slice", &[input.clone(), 20.0.into(), (-20.0).into()]), Value::from(""));
            assert!((lookup("byte_slice").unwrap().call)(&[input, 4.0.into()]).is_err());
        }
        assert_eq!(call("byte_slice", &[42.0.into(), 0.0.into(), 1.0.into()]), Value::from("4"));
        for owned in [false, true] {
            let text = |s: &'static str| if owned { Value::from(s.to_owned()) } else { Value::from(s) };
            assert_eq!(call("split", &[text("a::b::"), "::".into()]), Value::list(vec!["a".into(), "b".into(), "".into()]));
            assert_eq!(call("split", &[text("a\u{1f600}"), "".into()]), Value::list(vec!["a".into(), "\u{1f600}".into()]));
            assert_eq!(call("split", &[text(""), "".into()]), Value::list(vec![]));
            assert_eq!(call("split", &[text(""), ",".into()]), Value::list(vec!["".into()]));
            assert_eq!(call("lines", &[text("a\r\n\nb\r")]), Value::list(vec!["a".into(), "".into(), "b\r".into()]));
            assert_eq!(call("lines", &[text("a\n")]), Value::list(vec!["a".into()]));
            assert_eq!(call("lines", &[text("")]), Value::list(vec![]));
        }
    }

    #[test]
    fn js_numeric_strings_follow_runtime_conversion_not_literal_syntax() {
        for (input, expected) in [
            ("", 0.0), (" \t\n", 0.0), ("010", 10.0), ("0x10", 16.0),
            ("0o10", 8.0), ("0b10", 2.0), ("-.25", -0.25), ("5e-1", 0.5),
            ("\u{feff}2\u{feff}", 2.0), ("Infinity", f64::INFINITY),
            ("+Infinity", f64::INFINITY), ("-Infinity", f64::NEG_INFINITY),
        ] {
            let actual = call("js_number", &[Value::from(input)]).num().unwrap();
            assert_eq!(actual, expected, "{input:?}");
        }
        for input in ["1_0", "1n", "inf", "infinity", "+0x0", "-0x0", "\u{0085}2", "0x2\u{0085}", "not a number"] {
            assert!(call("js_number", &[Value::from(input)]).num().unwrap().is_nan(), "{input:?}");
        }
        for (kind, value, expected) in [("boolean", "false", 0.0), ("null", "null", 0.0), ("bigint", "0n", 0.0), ("string", "0x10", 16.0)] {
            let literal = Value::record(vec![("type".into(), Value::from(kind)), ("value".into(), Value::from(value))]);
            assert_eq!(call("js_number", &[literal]).num().unwrap(), expected);
        }
        let undefined = Value::record(vec![("type".into(), Value::from("undefined")), ("value".into(), Value::from("undefined"))]);
        assert!(call("js_number", &[undefined]).num().unwrap().is_nan());
        let symbol = Value::record(vec![("type".into(), Value::from("symbol")), ("value".into(), Value::from("key"))]);
        assert_eq!(call("js_number", &[symbol]), Value::Null);
    }

    #[test]
    fn js_numeric_comparison_shares_string_coercion_and_preserves_text_order() {
        for input in ["1_0", "1n", "inf", "\u{0085}2"] {
            assert_eq!(call("js_compare", &[Value::from(input), Value::Num(0.0)]), Value::Null, "{input:?}");
            let literal = Value::record(vec![("type".into(), Value::from("string")), ("value".into(), Value::from(input))]);
            assert_eq!(call("js_compare", &[literal, Value::Num(0.0)]), Value::Null, "{input:?}");
        }
        assert_eq!(call("js_compare", &[Value::from("010"), Value::Num(9.0)]), Value::Num(1.0));
        assert_eq!(call("js_compare", &[Value::from("\u{feff}2"), Value::Num(1.0)]), Value::Num(1.0));
        assert_eq!(call("js_compare", &[Value::from("10"), Value::from("2")]), Value::Num(-1.0));
    }

    #[test]
    fn js_compare_keeps_bigint_order_exact_across_numeric_domains() {
        let literal = |kind: &str, value: &str| Value::record(vec![
            ("type".into(), Value::from(kind.to_owned())),
            ("value".into(), Value::from(value.to_owned())),
        ]);
        for (left, right, expected) in [
            ("9007199254740992n", "9007199254740993n", -1.0),
            ("-9007199254740993n", "-9007199254740992n", -1.0),
            ("0x20000000000001n", "9007199254740993n", 0.0),
            ("-0n", "0n", 0.0),
        ] {
            let a = literal("bigint", left);
            let b = literal("bigint", right);
            assert_eq!(call("js_compare", &[a.clone(), b.clone()]), Value::Num(expected));
            assert_eq!(call("js_compare", &[b, a]), Value::Num(-expected));
        }
        for (kind, value, expected) in [
            ("number", "9007199254740992", 1.0),
            ("string", "9007199254740993", 0.0),
            ("number", "Infinity", -1.0),
        ] {
            assert_eq!(call("js_compare", &[literal("bigint", "9007199254740993n"), literal(kind, value)]), Value::Num(expected));
        }
        assert_eq!(call("js_compare", &[literal("bigint", "1n"), literal("number", "1.5")]), Value::Num(-1.0));
        assert_eq!(call("js_compare", &[literal("bigint", "-1n"), literal("number", "-1.5")]), Value::Num(1.0));
        assert_eq!(call("js_compare", &[literal("bigint", "1n"), literal("string", "1.5")]), Value::Null);
    }

    #[test]
    fn list_helpers_accept_single_values() {
        assert_eq!(call("join", &["a".into(), ", ".into()]), Value::from("a"));
        let sorted = call("sort", &[Value::list(vec!["b".into(), "a".into()])]);
        assert_eq!(call("join", &[sorted, ",".into()]), Value::from("a,b"));
        assert_eq!(call("count", &[Value::Null]), Value::from(0usize));
        assert_eq!(call("max", &[Value::list(vec![1.0.into(), 5.0.into()])]), Value::from(5.0));
    }

    #[test]
    fn zip_pairs_up_to_the_shorter_list() {
        let xs = Value::list(vec!["a".into(), "b".into(), "c".into()]);
        let next = call("slice", &[xs.clone(), 1.0.into()]);
        let pairs = |v: Value<'static>| v.item_slice().iter().map(|p| p.item_slice().iter().map(text).collect::<Vec<_>>().join("")).collect::<Vec<_>>();
        assert_eq!(pairs(call("zip", &[xs, next])), vec!["ab", "bc"]);
        assert_eq!(pairs(call("zip", &[Value::Null, "x".into()])), Vec::<String>::new());
    }
}

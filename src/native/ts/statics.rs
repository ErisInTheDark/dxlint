//! Values the source states outright: literals as JavaScript reads them
//! (`0x10` is 16, `'\x41'` is "A") and property names that are known
//! without running anything (eslint's `getStaticPropertyName`).

use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;
use crate::expr::value::js_number_text;

/// A numeric literal's value: decimal, `0x`/`0o`/`0b`, legacy octal and
/// `_` separators.
pub fn number(text: &str) -> Option<f64> {
    let t: String = text.chars().filter(|&c| c != '_').collect();
    let lower = t.to_ascii_lowercase();
    let radix = match lower.get(..2) {
        Some("0x") => Some((16, &t[2..])),
        Some("0o") => Some((8, &t[2..])),
        Some("0b") => Some((2, &t[2..])),
        _ if t.len() > 1 && t.starts_with('0') && t.chars().all(|c| c.is_ascii_digit()) && !t.contains(['8', '9']) => Some((8, &t[1..])),
        _ => None,
    };
    match radix {
        Some((r, digits)) => Some(digits.chars().try_fold(0f64, |acc, c| Some(acc * f64::from(r) + f64::from(c.to_digit(r)?)))?),
        None => t.parse().ok(),
    }
}

/// The value of a literal expression as text (`String(value)`): strings,
/// templates without substitutions, numbers, bigints, booleans, null and
/// regexes.
pub fn literal_text(s: &Syntax, node: u32) -> Option<String> {
    let text = s.text(node);
    match s.kind(node)? {
        K::String => super::regex::cook(text),
        K::Template if s.all(node, F::Expr).next().is_none() => super::regex::cook(text),
        K::Number => Some(js_number_text(number(text)?)),
        K::BigInt => bigint_decimal(text),
        K::Bool | K::Null | K::Regex => Some(text.to_string()),
        K::Paren => literal_text(s, s.get(node, F::Expr)?),
        _ => None,
    }
}

/// A bigint literal's value in decimal (`0x10n` is `16`), exactly.
pub(crate) fn bigint_decimal(text: &str) -> Option<String> {
    let digits = text.trim_end_matches('n').replace('_', "");
    let (radix, body) = match digits.get(..2).map(str::to_ascii_lowercase).as_deref() {
        Some("0x") => (16, &digits[2..]),
        Some("0o") => (8, &digits[2..]),
        Some("0b") => (2, &digits[2..]),
        _ => (10, digits.as_str()),
    };
    // Little-endian base 10^9 limbs.
    let mut limbs: Vec<u64> = vec![0];
    for c in body.chars() {
        let mut carry = u64::from(c.to_digit(radix)?);
        for limb in &mut limbs {
            let v = *limb * u64::from(radix) + carry;
            *limb = v % 1_000_000_000;
            carry = v / 1_000_000_000;
        }
        if carry > 0 {
            limbs.push(carry);
        }
    }
    let mut out = limbs.last().copied().unwrap_or(0).to_string();
    for limb in limbs.iter().rev().skip(1) {
        out.push_str(&format!("{limb:09}"));
    }
    Some(out)
}

/// A literal-like expression's JavaScript type and value as text: strings,
/// static templates, numbers (also `-1`, `+1`), bigints, booleans and
/// `null`. (`undefined` is a name that can be shadowed: rules decide.)
pub fn literal_value(s: &Syntax, node: u32) -> Option<(&'static str, String)> {
    let text = s.text(node);
    let Some(kind) = s.kind(node) else {
        if !text.starts_with(['\'', '"']) {
            return None;
        }
        let mut lexer = super::lexer::Lexer::new(text);
        let token = lexer.next_token();
        return if token.tok == super::lexer::Tok::Str && token.end as usize == text.len() && lexer.broken.is_none() {
            Some(("string", super::regex::cook(text)?))
        } else {
            None
        };
    };
    Some(match kind {
        K::String => ("string", super::regex::cook(text)?),
        K::Template if s.all(node, F::Expr).next().is_none() => ("string", super::regex::cook(text)?),
        K::Number => ("number", js_number_text(number(text)?)),
        K::BigInt => ("bigint", bigint_decimal(text)?),
        K::Bool => ("boolean", text.to_string()),
        K::Null => ("null", "null".into()),
        K::Regex => ("regex", text.to_string()),
        K::Paren => return literal_value(s, s.get(node, F::Expr)?),
        K::Unary => {
            let op = s.word(node, F::Op)?;
            let (kind, value) = literal_value(s, s.get(node, F::Arg)?)?;
            if kind == "bigint" && op == "-" {
                return Some(("bigint", if value == "0" { value } else { format!("-{value}") }));
            }
            let n = match kind {
                "number" => value.parse::<f64>().ok()?,
                "string" if op == "+" || op == "-" => value.trim().parse::<f64>().unwrap_or(if value.trim().is_empty() { 0.0 } else { f64::NAN }),
                _ => return None,
            };
            match op {
                "-" => ("number", if n == 0.0 { "-0".into() } else { js_number_text(-n) }),
                "+" => ("number", js_number_text(n)),
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// How eslint's sorting rules order names: by UTF-16 unit, optionally
/// ignoring case, and optionally natural (`a2` before `a10`).
pub fn compare_names(a: &str, b: &str, insensitive: bool, natural: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let a = if insensitive { a.to_lowercase() } else { a.to_string() };
    let b = if insensitive { b.to_lowercase() } else { b.to_string() };
    let a: Vec<_> = a.encode_utf16().collect();
    let b: Vec<_> = b.encode_utf16().collect();
    if !natural {
        return a.cmp(&b);
    }
    // natural-compare 1.4 weights punctuation before letters; a leading zero
    // stays lexical, while nonzero digit runs use JavaScript Number precision.
    let code = |text: &[u16], pos: usize| match text.get(pos).copied().unwrap_or(0) {
        45 => 65,
        n @ 46..=47 => n - 1,
        n @ 48..=57 => n + 18,
        n @ 58..=64 => n - 11,
        n @ 65..=90 => n + 11,
        n @ 91..=96 => n - 37,
        n @ 97..=122 => n + 5,
        n @ 123..=127 => n - 63,
        n => n,
    };
    let digits = |text: &[u16], pos: &mut usize| {
        let start = *pos;
        while text.get(*pos).is_some_and(|n| (48..=57).contains(n)) {
            *pos += 1;
        }
        String::from_utf16_lossy(&text[start..*pos]).parse::<f64>().expect("decimal digit run")
    };
    let (mut i, mut j) = (0, 0);
    loop {
        let (left, right) = (code(&a, i), code(&b, j));
        let order = if (67..76).contains(&left) && (67..76).contains(&right) {
            digits(&a, &mut i).partial_cmp(&digits(&b, &mut j)).expect("non-NaN digit values")
        } else {
            i += 1;
            j += 1;
            left.cmp(&right)
        };
        if order != Ordering::Equal {
            return order;
        }
        if right == 0 {
            return Ordering::Equal;
        }
    }
}

/// An identifier as JavaScript reads it: `foo` is `foo`.
pub fn identifier_name(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    super::regex::cook(&format!("\"{text}\"")).unwrap_or_else(|| text.to_string())
}

/// A member's or property's name when it is known statically: `a`, `"a"`,
/// `1`, `[`a`]`; `None` for private names (`#a`) and other computed keys.
pub fn property_name(s: &Syntax, node: u32) -> Option<String> {
    let key = match s.kind(node)? {
        K::Paren => return property_name(s, s.get(node, F::Expr)?),
        K::Prop | K::Method | K::ClassProp | K::PatternProp | K::PropSig | K::MethodSig => s.get(node, F::Key)?,
        K::EnumMember => s.get(node, F::Name)?,
        K::Member => s.get(node, F::Property)?,
        K::Index => return literal_text(s, s.get(node, F::Index)?),
        _ => return None,
    };
    match s.kind(key)? {
        K::Ident => Some(identifier_name(s.text(key))),
        K::PrivateName => None,
        K::Computed => literal_text(s, s.get(key, F::Expr)?),
        _ => literal_text(s, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn pattern_fields_read_complete_quoted_literals_not_expressions() {
        use crate::grammar::{NativeFront, Node, Tree, TEXT};
        let front = NativeFront { name: "ts", base: 0, labels: Vec::new() };
        for (text, expected) in [
            (r#""t\u0079pe""#, Some("type")),
            (r"'a\nb'", Some("a\nb")),
            (r#""""#, Some("")),
            (r#""a" + "b""#, None),
            (r#""unterminated"#, None),
            (r#"'mismatched""#, None),
            ("identifier", None),
            ("`template`", None),
        ] {
            let tree = Tree::from_native(
                vec![Node { matcher: TEXT, start: 0, end: text.len() as u32, children: Vec::new() }],
                0,
            );
            let syntax = Syntax::new(&tree, text, &front);
            assert_eq!(literal_value(&syntax, 0), expected.map(|s| ("string", s.to_string())), "{text:?}");
        }
    }

    #[test]
    fn natural_order_preserves_punctuation_and_numeric_semantics() {
        for (a, b, expected) in [
            ("_", "A", Ordering::Less),
            ("a2", "a10", Ordering::Less),
            ("a02", "a2", Ordering::Less),
            ("a002", "a02", Ordering::Less),
            ("9007199254740992", "9007199254740993", Ordering::Equal),
            ("a2b", "a2c", Ordering::Less),
            ("a\0b", "a\0c", Ordering::Equal),
        ] {
            assert_eq!(compare_names(a, b, false, true), expected, "{a:?} vs {b:?}");
            assert_eq!(compare_names(b, a, false, true), expected.reverse(), "{b:?} vs {a:?}");
        }
        assert_eq!(compare_names("_", "A", false, false), Ordering::Greater);
        assert_eq!(compare_names("A2", "a2", true, true), Ordering::Equal);
        assert_eq!(compare_names(&"9".repeat(400), &"8".repeat(400), false, true), Ordering::Equal);
    }

    #[test]
    fn identifier_escapes_decode() {
        assert_eq!(identifier_name("\\u0066oo"), "foo");
        assert_eq!(identifier_name("b\\u{61}r"), "bar");
        assert_eq!(identifier_name("plain"), "plain");
    }

    #[test]
    fn numbers_read_like_javascript() {
        assert_eq!(number("0x10"), Some(16.0));
        assert_eq!(number("0o17"), Some(15.0));
        assert_eq!(number("0b101"), Some(5.0));
        assert_eq!(number("017"), Some(15.0));
        assert_eq!(number("1_000"), Some(1000.0));
        assert_eq!(number(".5"), Some(0.5));
    }
}

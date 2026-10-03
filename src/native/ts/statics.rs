//! Values the source states outright: literals as JavaScript reads them
//! (`0x10` is 16, `'\x41'` is "A") and property names that are known
//! without running anything (eslint's `getStaticPropertyName`).

use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;

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

/// `String(n)` in JavaScript.
pub fn number_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if n == 0.0 {
        return "0".into();
    }
    let abs = n.abs();
    if (1e-6..1e21).contains(&abs) {
        return format!("{n}");
    }
    // Exponent form: shortest digits, `e+21` / `e-7`.
    let s = format!("{n:e}");
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    format!("{mantissa}e{}{}", if exp < 0 { "-" } else { "+" }, exp.abs())
}

/// The value of a literal expression as text (`String(value)`): strings,
/// templates without substitutions, numbers, bigints, booleans, null and
/// regexes.
pub fn literal_text(s: &Syntax, node: u32) -> Option<String> {
    let text = s.text(node);
    match s.kind(node)? {
        K::String => super::regex::cook(text),
        K::Template if s.all(node, F::Expr).next().is_none() => super::regex::cook(text),
        K::Number => Some(number_string(number(text)?)),
        K::BigInt => Some(text.trim_end_matches('n').replace('_', "")),
        K::Bool | K::Null | K::Regex => Some(text.to_string()),
        K::Paren => literal_text(s, s.get(node, F::Expr)?),
        _ => None,
    }
}

/// A literal-like expression's JavaScript type and value as text: strings,
/// static templates, numbers (also `-1`, `+1`), bigints, booleans and
/// `null`. (`undefined` is a name that can be shadowed: rules decide.)
pub fn literal_value(s: &Syntax, node: u32) -> Option<(&'static str, String)> {
    let text = s.text(node);
    Some(match s.kind(node)? {
        K::String => ("string", super::regex::cook(text)?),
        K::Template if s.all(node, F::Expr).next().is_none() => ("string", super::regex::cook(text)?),
        K::Number => ("number", number_string(number(text)?)),
        K::BigInt => ("bigint", text.trim_end_matches('n').replace('_', "")),
        K::Bool => ("boolean", text.to_string()),
        K::Null => ("null", "null".into()),
        K::Regex => ("regex", text.to_string()),
        K::Paren => return literal_value(s, s.get(node, F::Expr)?),
        K::Unary => {
            let op = s.word(node, F::Op)?;
            let (kind, value) = literal_value(s, s.get(node, F::Arg)?)?;
            let n = match kind {
                "number" => value.parse::<f64>().ok()?,
                "string" if op == "+" || op == "-" => value.trim().parse::<f64>().unwrap_or(if value.trim().is_empty() { 0.0 } else { f64::NAN }),
                _ => return None,
            };
            match op {
                "-" => ("number", if n == 0.0 { "-0".into() } else { number_string(-n) }),
                "+" => ("number", number_string(n)),
                _ => return None,
            }
        }
        _ => return None,
    })
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
        K::Ident => Some(s.text(key).to_string()),
        K::PrivateName => None,
        K::Computed => literal_text(s, s.get(key, F::Expr)?),
        _ => literal_text(s, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_and_print_like_javascript() {
        assert_eq!(number("0x10"), Some(16.0));
        assert_eq!(number("0o17"), Some(15.0));
        assert_eq!(number("0b101"), Some(5.0));
        assert_eq!(number("017"), Some(15.0));
        assert_eq!(number("1_000"), Some(1000.0));
        assert_eq!(number(".5"), Some(0.5));
        assert_eq!(number_string(1.0), "1");
        assert_eq!(number_string(0.1), "0.1");
        assert_eq!(number_string(1e21), "1e+21");
        assert_eq!(number_string(1e-7), "1e-7");
    }
}

//! Values the source states outright: literals as JavaScript reads them
//! (`0x10` is 16, `'\x41'` is "A") and property names that are known
//! without running anything (eslint's `getStaticPropertyName`).

use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;
use crate::expr::value::js_number_text;

pub use crate::expr::js::{bigint_decimal, number};

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

    #[test]
    fn pattern_fields_read_complete_quoted_literals_not_expressions() {
        use crate::grammar::{NativeFront, Node, Tree, TEXT};
        let front = NativeFront { name: "ts", frontend: &crate::native::ts::frontend::TS, base: 0, labels: Vec::new() };
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
    fn identifier_escapes_decode() {
        assert_eq!(identifier_name("\\u0066oo"), "foo");
        assert_eq!(identifier_name("b\\u{61}r"), "bar");
        assert_eq!(identifier_name("plain"), "plain");
    }
}

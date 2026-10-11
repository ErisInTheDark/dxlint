//! JavaScript value semantics the expression functions use, and the native
//! front end shares: numeric and bigint literals, eslint's name order,
//! `RegExp` flags and Unicode marks.

use std::cmp::Ordering;
use std::sync::OnceLock;

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

/// A bigint literal's value in decimal (`0x10n` is `16`), exactly.
pub fn bigint_decimal(text: &str) -> Option<String> {
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

/// How eslint's sorting rules order names: by UTF-16 unit, optionally
/// ignoring case, and optionally natural (`a2` before `a10`).
pub fn compare_names(a: &str, b: &str, insensitive: bool, natural: bool) -> Ordering {
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

/// no-invalid-regexp's flags check: `u` with `v`, duplicates, then
/// unknown flags (`allowed` extra ones pass).
pub fn regex_flags_error(flags: &str, allowed: &str) -> Option<String> {
    // Standard flags are not "allowed" twice.
    let mut extra: Vec<char> = allowed.chars().filter(|c| !"dgimsuvy".contains(*c)).collect();
    extra.dedup();
    let all: Vec<char> = "dgimsuvy".chars().chain(extra).collect();
    let mut rest = flags.to_string();
    for f in &all {
        if let Some(i) = rest.find(*f) {
            rest.remove(i);
        }
    }
    if flags.contains('u') && flags.contains('v') {
        return Some("Regex 'u' and 'v' flags cannot be used together".into());
    }
    let dupes: String = rest.chars().filter(|c| all.contains(c)).collect();
    if !dupes.is_empty() {
        return Some(format!("Duplicate flags ('{dupes}') supplied to RegExp constructor"));
    }
    (!rest.is_empty()).then(|| format!("Invalid flags supplied to RegExp constructor '{rest}'"))
}

/// Unicode general category M (marks).
pub fn is_mark(c: char) -> bool {
    static RE: OnceLock<regex_automata::meta::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex_automata::meta::Regex::new(r"^[\p{Mn}\p{Mc}\p{Me}]$").expect("valid"));
    let mut buf = [0u8; 4];
    re.is_match(c.encode_utf8(&mut buf).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn numbers_read_like_javascript() {
        assert_eq!(number("0x10"), Some(16.0));
        assert_eq!(number("0o17"), Some(15.0));
        assert_eq!(number("0b101"), Some(5.0));
        assert_eq!(number("017"), Some(15.0));
        assert_eq!(number("1_000"), Some(1000.0));
        assert_eq!(number(".5"), Some(0.5));
    }

    #[test]
    fn flags_are_checked_like_the_constructor() {
        assert_eq!(regex_flags_error("gg", "").as_deref(), Some("Duplicate flags ('g') supplied to RegExp constructor"));
        assert_eq!(regex_flags_error("z", "").as_deref(), Some("Invalid flags supplied to RegExp constructor 'z'"));
        assert_eq!(regex_flags_error("uv", "").as_deref(), Some("Regex 'u' and 'v' flags cannot be used together"));
        assert!(regex_flags_error("gz", "z").is_none());
    }
}

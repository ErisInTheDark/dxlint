//! Language-neutral text primitives: UTF-16 order and assembly, natural
//! order, Unicode marks.

use std::cmp::Ordering;
use std::ops::Range;
use std::sync::OnceLock;

/// Text by UTF-16 unit, optionally ignoring case, and optionally natural
/// (`a2` before `a10`, natural-compare's punctuation weights).
pub fn compare_names(a: &str, b: &str, insensitive: bool, natural: bool) -> Ordering {
    let a = if insensitive { a.to_lowercase() } else { a.to_string() };
    let b = if insensitive { b.to_lowercase() } else { b.to_string() };
    let a: Vec<_> = a.encode_utf16().collect();
    let b: Vec<_> = b.encode_utf16().collect();
    if !natural {
        return a.cmp(&b);
    }
    // Punctuation weighs before letters; a leading zero stays lexical, while
    // nonzero digit runs compare as double-precision numbers.
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

/// Unicode general category M (marks).
pub fn is_mark(c: char) -> bool {
    static RE: OnceLock<regex_automata::meta::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex_automata::meta::Regex::new(r"^[\p{Mn}\p{Mc}\p{Me}]$").expect("valid"));
    let mut buf = [0u8; 4];
    re.is_match(c.encode_utf8(&mut buf).as_bytes())
}

/// One UTF-16 unit and where it was written.
pub struct Unit {
    pub value: u16,
    pub source: Range<usize>,
}

/// Text assembled from code points (each written at a source span): its
/// UTF-16 units, and the text with each scalar's span (a surrogate pair's
/// spans joined; a lone surrogate becomes U+FFFD). `None` past U+10FFFF.
pub fn from_code_points(points: impl IntoIterator<Item = (u32, Range<usize>)>) -> Option<(Vec<Unit>, String, Vec<Range<usize>>)> {
    let mut units = Vec::new();
    for (point, source) in points {
        if point <= u32::from(u16::MAX) {
            units.push(Unit { value: point as u16, source });
        } else {
            for &value in char::from_u32(point)?.encode_utf16(&mut [0; 2]).iter() {
                units.push(Unit { value, source: source.clone() });
            }
        }
    }
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut index = 0;
    while let Some(unit) = units.get(index) {
        let mut source = unit.source.clone();
        let point = if (0xD800..=0xDBFF).contains(&unit.value) {
            match units.get(index + 1).filter(|next| (0xDC00..=0xDFFF).contains(&next.value)) {
                Some(next) => {
                    source.end = source.end.max(next.source.end);
                    index += 1;
                    0x10000 + ((u32::from(unit.value) - 0xD800) << 10) + (u32::from(next.value) - 0xDC00)
                }
                None => u32::from(unit.value),
            }
        } else {
            u32::from(unit.value)
        };
        text.push(char::from_u32(point).unwrap_or(char::REPLACEMENT_CHARACTER));
        spans.push(source);
        index += 1;
    }
    Some((units, text, spans))
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
    fn code_points_pair_surrogates_and_keep_written_spans() {
        let (units, text, spans) = from_code_points([(0xD83D, 0..6), (0xDC3E, 6..12), (0x61, 12..13), (0x1F43E, 13..23), (0xDC3E, 23..29)]).unwrap();
        assert_eq!(text, "\u{1F43E}a\u{1F43E}\u{FFFD}");
        assert_eq!(spans, [0..12, 12..13, 13..23, 23..29]);
        assert_eq!(units.len(), 6);
        assert!(from_code_points([(0x110000, 0..1)]).is_none());
    }
}

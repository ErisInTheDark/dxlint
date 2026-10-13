//! JavaScript string and plain template literals, shared by value functions and native reads.

pub struct Unit {
    pub value: u16,
    pub source: std::ops::Range<usize>,
}

pub struct Decoded {
    pub units: Vec<Unit>,
    pub text: String,
    pub spans: Vec<std::ops::Range<usize>>,
}

pub fn decode(text: &str) -> Option<String> {
    text.chars().next().filter(|quote| matches!(quote, '\'' | '"'))?;
    decode_literal(text)
}

pub fn decode_literal(text: &str) -> Option<String> {
    decode_mapped(text).map(|decoded| decoded.text)
}

pub fn decode_mapped(text: &str) -> Option<Decoded> {
    let quote = text.chars().next().filter(|quote| matches!(quote, '\'' | '"' | '`'))?;
    let body = text.strip_prefix(quote)?.strip_suffix(quote)?;
    let mut chars = body.char_indices().peekable();
    let mut units = Vec::with_capacity(body.len());
    while let Some((at, ch)) = chars.next() {
        if ch == quote || quote != '`' && matches!(ch, '\n' | '\r') {
            return None;
        }
        if quote == '`' && ch == '$' && chars.peek().is_some_and(|(_, next)| *next == '{') {
            return None;
        }
        let point = if ch != '\\' {
            ch as u32
        } else {
            let (_, escaped) = chars.next()?;
            match escaped {
                '\n' | '\u{2028}' | '\u{2029}' => continue,
                '\r' => {
                    if chars.peek().is_some_and(|(_, next)| *next == '\n') { chars.next(); }
                    continue;
                }
                'b' => 8,
                'f' => 12,
                'n' => 10,
                'r' => 13,
                't' => 9,
                'v' => 11,
                'x' => u32::from(fixed_hex(&mut chars, 2)?),
                'u' if chars.peek().is_some_and(|(_, next)| *next == '{') => {
                    chars.next();
                    code_point(&mut chars)?
                }
                'u' => u32::from(fixed_hex(&mut chars, 4)?),
                '0'..='7' => {
                    let mut value = escaped.to_digit(8)?;
                    let remaining = if value <= 3 { 2 } else { 1 };
                    for _ in 0..remaining {
                        let Some(digit) = chars.peek().and_then(|(_, ch)| ch.to_digit(8)) else { break };
                        chars.next();
                        value = value * 8 + digit;
                    }
                    value
                }
                _ => escaped as u32,
            }
        };
        let source = at + 1..chars.peek().map_or(text.len() - 1, |(at, _)| at + 1);
        if point <= u32::from(u16::MAX) {
            units.push(Unit { value: point as u16, source });
        } else {
            for &value in char::from_u32(point)?.encode_utf16(&mut [0; 2]).iter() {
                units.push(Unit { value, source: source.clone() });
            }
        }
    }
    // Rust strings cannot contain lone UTF-16 surrogates. Keep the agreed
    // replacement policy at this text-decoding edge.
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
    Some(Decoded { units, text, spans })
}

fn fixed_hex(chars: &mut impl Iterator<Item = (usize, char)>, count: usize) -> Option<u16> {
    (0..count).try_fold(0u16, |value, _| Some(value * 16 + chars.next()?.1.to_digit(16)? as u16))
}

fn code_point(chars: &mut impl Iterator<Item = (usize, char)>) -> Option<u32> {
    let mut value = 0u32;
    let mut found = false;
    for (_, ch) in chars {
        if ch == '}' { return found.then_some(value) }
        value = value.checked_mul(16)?.checked_add(ch.to_digit(16)?)?;
        if value > 0x10FFFF { return None }
        found = true;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{decode, decode_mapped};

    #[test]
    fn decoded_units_and_scalars_keep_their_written_spans() {
        let source = "'a\\uD83D\\uDC3E\\\r\n\\x62🐾'";
        let value = decode_mapped(source).expect("valid mapped literal");
        assert_eq!(value.text, decode(source).unwrap());
        assert_eq!(value.units.iter().map(|unit| unit.value).collect::<Vec<_>>(), value.text.encode_utf16().collect::<Vec<_>>());
        let written = value.spans.iter().map(|span| &source[span.clone()]).collect::<Vec<_>>();
        assert_eq!(written, ["a", "\\uD83D\\uDC3E", "\\x62", "🐾"]);
        let units = value.units.iter().map(|unit| &source[unit.source.clone()]).collect::<Vec<_>>();
        assert_eq!(units, ["a", "\\uD83D", "\\uDC3E", "\\x62", "🐾", "🐾"]);
    }

    #[test]
    fn unmappable_surrogates_preserve_units_without_losing_source_positions() {
        let source = "'\\uD83Da\\uDC3E'";
        let value = decode_mapped(source).expect("valid surrogate units");
        assert_eq!(value.text, decode(source).unwrap());
        assert_eq!(value.units.iter().map(|unit| unit.value).collect::<Vec<_>>(), [0xD83D, u16::from(b'a'), 0xDC3E]);
        assert_eq!(value.spans.iter().map(|span| &source[span.clone()]).collect::<Vec<_>>(), ["\\uD83D", "a", "\\uDC3E"]);
    }

    #[test]
    fn template_values_share_escape_mapping_but_do_not_evaluate_substitutions() {
        let source = "`a\\${b}\\\r\n\\x63`";
        let value = decode_mapped(source).expect("escaped template substitution");
        assert_eq!(value.text, "a${b}c");
        assert_eq!(value.spans.iter().map(|span| &source[span.clone()]).collect::<Vec<_>>(), ["a", "\\$", "{", "b", "}", "\\x63"]);
        assert!(decode_mapped("`a${b}`").is_none());
        assert!(decode_mapped("`unterminated").is_none());
    }

    #[test]
    fn equivalent_literal_spellings_have_the_same_value() {
        let forms = ["\"a\\u{1F43E}b\"", "'a\\uD83D\\uDC3Eb'", "\"a🐾b\"", "\"\\x61🐾\\u0062\""];
        let value = decode(forms[0]).expect("valid literal");
        for form in forms {
            assert_eq!(decode(form).as_deref(), Some(value.as_str()), "{form}");
        }
        assert_ne!(decode("'a🐾c'"), Some(value));
    }

    #[test]
    fn escapes_and_line_continuations_preserve_quoted_content() {
        assert_eq!(decode("'a\\\r\nb\\\nc\\\rd'"), decode("\"abcd\""));
        assert_eq!(decode("'\\'\\\\\\\"'"), Some("'\\\"".to_owned()));
        assert_eq!(decode("\"\\n\\r\\t\\b\\f\\v\\0\""), Some("\n\r\t\u{8}\u{c}\u{b}\0".to_owned()));
        assert_eq!(decode("'\\141\\x62\\u0063'"), decode("'abc'"));
    }

    #[test]
    fn malformed_literals_do_not_produce_partial_values() {
        for text in ["a", "'a\"", "'a\nb'", "\"\\xq0\"", "\"\\u000g\"", "\"\\u{110000}\"", "\"\\u{}\"", "'\\'"] {
            assert_eq!(decode(text), None, "{text}");
        }
    }
}

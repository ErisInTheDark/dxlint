//! Quoted JavaScript string literals, shared by value-only DSL functions.

pub fn decode(text: &str) -> Option<String> {
    let quote = text.chars().next().filter(|quote| matches!(quote, '\'' | '"'))?;
    let body = text.strip_prefix(quote)?.strip_suffix(quote)?;
    let mut chars = body.chars().peekable();
    let mut units = Vec::with_capacity(body.len());
    while let Some(ch) = chars.next() {
        if ch == quote || matches!(ch, '\n' | '\r') {
            return None;
        }
        if ch != '\\' {
            units.extend_from_slice(ch.encode_utf16(&mut [0; 2]));
            continue;
        }
        let escaped = chars.next()?;
        let unit = match escaped {
            '\n' | '\u{2028}' | '\u{2029}' => continue,
            '\r' => {
                if chars.peek() == Some(&'\n') { chars.next(); }
                continue;
            }
            'b' => 8,
            'f' => 12,
            'n' => 10,
            'r' => 13,
            't' => 9,
            'v' => 11,
            'x' => fixed_hex(&mut chars, 2)?,
            'u' if chars.peek() == Some(&'{') => {
                chars.next();
                let point = code_point(&mut chars)?;
                if point > 0xFFFF {
                    units.extend_from_slice(char::from_u32(point)?.encode_utf16(&mut [0; 2]));
                    continue;
                }
                point as u16
            }
            'u' => fixed_hex(&mut chars, 4)?,
            '0'..='7' => {
                let mut value = escaped.to_digit(8)? as u16;
                let remaining = if value <= 3 { 2 } else { 1 };
                for _ in 0..remaining {
                    let Some(digit) = chars.peek().and_then(|ch| ch.to_digit(8)) else { break };
                    chars.next();
                    value = value * 8 + digit as u16;
                }
                value
            }
            _ => {
                units.extend_from_slice(escaped.encode_utf16(&mut [0; 2]));
                continue;
            }
        };
        units.push(unit);
    }
    // Rust strings cannot contain lone UTF-16 surrogates. Keep the agreed
    // replacement policy at this text-decoding edge.
    Some(String::from_utf16_lossy(&units))
}

fn fixed_hex(chars: &mut impl Iterator<Item = char>, count: usize) -> Option<u16> {
    (0..count).try_fold(0u16, |value, _| Some(value * 16 + chars.next()?.to_digit(16)? as u16))
}

fn code_point(chars: &mut impl Iterator<Item = char>) -> Option<u32> {
    let mut value = 0u32;
    let mut found = false;
    for ch in chars {
        if ch == '}' { return found.then_some(value) }
        value = value.checked_mul(16)?.checked_add(ch.to_digit(16)?)?;
        if value > 0x10FFFF { return None }
        found = true;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::decode;

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

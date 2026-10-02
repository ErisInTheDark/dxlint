//! Character classes: `[a-z_]`, `[^"\\]`, `\w`, plus the unicode-aware
//! builtin classes (`alpha`, `alnum`, ...). ASCII membership is a bitmap.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    Alpha,
    Alnum,
    Word,
    Space,
    Upper,
    Lower,
}

impl Named {
    pub fn contains(self, c: char) -> bool {
        match self {
            Named::Alpha => c.is_alphabetic(),
            Named::Alnum => c.is_alphanumeric(),
            Named::Word => c.is_alphanumeric() || c == '_',
            Named::Space => c.is_whitespace(),
            Named::Upper => c.is_uppercase(),
            Named::Lower => c.is_lowercase(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharClass {
    ranges: Vec<(char, char)>,
    named: Vec<Named>,
    negated: bool,
    ascii: [u64; 2],
}

impl CharClass {
    pub fn new(ranges: Vec<(char, char)>, named: Vec<Named>, negated: bool) -> Self {
        let mut class = CharClass { ranges, named, negated, ascii: [0; 2] };
        for b in 0u8..128 {
            if class.slow_contains(b as char) {
                class.ascii[(b >> 6) as usize] |= 1 << (b & 63);
            }
        }
        class
    }

    pub fn named(n: Named) -> Self {
        CharClass::new(Vec::new(), vec![n], false)
    }

    pub fn ranges(ranges: &[(char, char)]) -> Self {
        CharClass::new(ranges.to_vec(), Vec::new(), false)
    }

    fn slow_contains(&self, c: char) -> bool {
        let hit = self.ranges.iter().any(|&(a, b)| a <= c && c <= b) || self.named.iter().any(|n| n.contains(c));
        hit != self.negated
    }

    #[inline]
    pub fn contains(&self, c: char) -> bool {
        let v = c as u32;
        if v < 128 {
            self.ascii[(v >> 6) as usize] & (1 << (v & 63)) != 0
        } else {
            self.slow_contains(c)
        }
    }

    #[inline]
    pub fn contains_ascii(&self, b: u8) -> bool {
        b < 128 && self.ascii[(b >> 6) as usize] & (1 << (b & 63)) != 0
    }

    /// Explicit ranges, named classes, and whether the class is negated.
    pub fn parts(&self) -> (&[(char, char)], &[Named], bool) {
        (&self.ranges, &self.named, self.negated)
    }

    /// Could any non-ASCII char match? Used by prefilters.
    pub fn may_match_non_ascii(&self) -> bool {
        self.negated || !self.named.is_empty() || self.ranges.iter().any(|&(_, b)| b as u32 >= 128)
    }

    /// Adds the other ASCII case of every ASCII letter in the class.
    pub fn fold_ascii(self) -> Self {
        if self.negated {
            // [^a] under (?i) must also exclude `A`: fold the excluded set.
            let mut excluded = self.clone();
            excluded.negated = false;
            let folded = excluded.fold_ascii();
            return CharClass::new(folded.ranges, folded.named, true);
        }
        let mut ranges = self.ranges.clone();
        for b in b'A'..=b'z' {
            let c = b as char;
            if c.is_ascii_alphabetic() && self.contains(c) {
                let other = if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() };
                ranges.push((other, other));
            }
        }
        CharClass::new(ranges, self.named, false)
    }

    /// Parses the raw text between `[` and `]`.
    pub fn parse(raw: &str) -> Result<CharClass, String> {
        let (negated, body) = match raw.strip_prefix('^') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let mut ranges = Vec::new();
        let mut named = Vec::new();
        let mut chars = body.chars().peekable();
        let mut items: Vec<Option<char>> = Vec::new(); // None = named class placeholder

        while let Some(c) = chars.next() {
            let item = if c == '\\' {
                match chars.next() {
                    Some('n') => Some('\n'),
                    Some('t') => Some('\t'),
                    Some('r') => Some('\r'),
                    Some('d') => {
                        ranges.push(('0', '9'));
                        None
                    }
                    Some('w') => {
                        named.push(Named::Word);
                        None
                    }
                    Some('s') => {
                        named.push(Named::Space);
                        None
                    }
                    Some('u') => {
                        let rest: String = chars.clone().collect();
                        let (hex, _) = rest
                            .strip_prefix('{')
                            .and_then(|r| r.split_once('}'))
                            .ok_or("expected `\\u{HEX}` in class")?;
                        let v = u32::from_str_radix(hex, 16).map_err(|_| "invalid hex in `\\u{...}`")?;
                        for _ in 0..hex.len() + 2 {
                            chars.next();
                        }
                        Some(char::from_u32(v).ok_or("invalid unicode scalar")?)
                    }
                    Some(c @ ('\\' | ']' | '[' | '-' | '^' | '"' | '\'' | '`' | '.' | '/' | '{' | '}' | '(' | ')' | '|' | '*' | '+' | '?')) => Some(c),
                    Some(other) => return Err(format!("unknown class escape `\\{other}`")),
                    None => return Err("class ends with a lone backslash".into()),
                }
            } else {
                Some(c)
            };

            let Some(lo) = item else {
                items.push(None);
                continue;
            };
            // Range `a-z`, but a trailing `-` is literal.
            if chars.peek() == Some(&'-') {
                let mut look = chars.clone();
                look.next();
                if let Some(&hi) = look.peek() {
                    if hi != '\\' {
                        chars.next();
                        chars.next();
                        if lo > hi {
                            return Err(format!("invalid range `{lo}-{hi}`"));
                        }
                        ranges.push((lo, hi));
                        continue;
                    }
                }
            }
            items.push(item);
        }
        for c in items.into_iter().flatten() {
            ranges.push((c, c));
        }
        if ranges.is_empty() && named.is_empty() {
            return Err("empty character class".into());
        }
        Ok(CharClass::new(ranges, named, negated))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_escapes_and_negation() {
        let c = CharClass::parse(r"a-c_\d").unwrap();
        assert!(c.contains('b') && c.contains('_') && c.contains('7') && !c.contains('d'));
        let n = CharClass::parse(r#"^"\\"#).unwrap();
        assert!(!n.contains('"') && !n.contains('\\') && n.contains('x') && n.contains('é'));
        let dash = CharClass::parse("a-").unwrap();
        assert!(dash.contains('-') && dash.contains('a'));
        assert!(CharClass::parse("z-a").is_err());
    }

    #[test]
    fn named_word_is_unicode_aware() {
        let c = CharClass::parse(r"\w").unwrap();
        assert!(c.contains('ü') && c.contains('_') && !c.contains('-'));
    }
}

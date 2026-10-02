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
    Digit,
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
            Named::Digit => c.is_ascii_digit(),
        }
    }
}

/// A class's pieces: explicit ranges, named classes, complemented named
/// classes (`\S` inside `[...]`), and whether the whole class is negated.
pub struct ClassParts<'a> {
    pub ranges: &'a [(char, char)],
    pub named: &'a [Named],
    pub not_named: &'a [Named],
    pub negated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharClass {
    ranges: Vec<(char, char)>,
    named: Vec<Named>,
    /// `\S`, `\W`, `\D` inside `[...]`: everything outside these classes.
    not_named: Vec<Named>,
    negated: bool,
    ascii: [u64; 2],
}

impl CharClass {
    pub fn new(ranges: Vec<(char, char)>, named: Vec<Named>, negated: bool) -> Self {
        CharClass::with_not_named(ranges, named, Vec::new(), negated)
    }

    pub fn with_not_named(ranges: Vec<(char, char)>, named: Vec<Named>, not_named: Vec<Named>, negated: bool) -> Self {
        let mut class = CharClass { ranges, named, not_named, negated, ascii: [0; 2] };
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
        let hit = self.ranges.iter().any(|&(a, b)| a <= c && c <= b)
            || self.named.iter().any(|n| n.contains(c))
            || self.not_named.iter().any(|n| !n.contains(c));
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

    /// What the class is made of, for translating it elsewhere.
    pub fn parts(&self) -> ClassParts<'_> {
        ClassParts { ranges: &self.ranges, named: &self.named, not_named: &self.not_named, negated: self.negated }
    }

    /// Could any non-ASCII char match? Used by prefilters.
    pub fn may_match_non_ascii(&self) -> bool {
        self.negated || !self.named.is_empty() || !self.not_named.is_empty() || self.ranges.iter().any(|&(_, b)| b as u32 >= 128)
    }

    /// Adds the other ASCII case of every ASCII letter in the class.
    pub fn fold_ascii(self) -> Self {
        if self.negated {
            // [^a] under (?i) must also exclude `A`: fold the excluded set.
            let mut excluded = self.clone();
            excluded.negated = false;
            let folded = excluded.fold_ascii();
            return CharClass::with_not_named(folded.ranges, folded.named, folded.not_named, true);
        }
        let mut ranges = self.ranges.clone();
        for b in b'A'..=b'z' {
            let c = b as char;
            if c.is_ascii_alphabetic() && self.contains(c) {
                let other = if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() };
                ranges.push((other, other));
            }
        }
        CharClass::with_not_named(ranges, self.named, self.not_named, false)
    }

    /// Parses the raw text between `[` and `]`.
    pub fn parse(raw: &str) -> Result<CharClass, String> {
        let (negated, body) = match raw.strip_prefix('^') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let mut ranges = Vec::new();
        let mut named = Vec::new();
        let mut not_named = Vec::new();
        let mut chars = body.chars().peekable();

        let mut add = |item: ClassItem, ranges: &mut Vec<(char, char)>| match item {
            ClassItem::Char(c) => ranges.push((c, c)),
            ClassItem::Digits => ranges.push(('0', '9')),
            ClassItem::Named(n) => named.push(n),
            ClassItem::NotNamed(n) => not_named.push(n),
        };
        while chars.peek().is_some() {
            let item = class_item(&mut chars)?;
            // Range `a-z` (either end may be escaped), but a trailing `-`,
            // or one before a class escape (`a-\d`), is literal.
            let mut look = chars.clone();
            if let (ClassItem::Char(lo), Some('-')) = (item, look.next()) {
                if look.peek().is_some() {
                    chars.next();
                    match class_item(&mut chars)? {
                        ClassItem::Char(hi) if lo > hi => return Err(format!("invalid range `{lo}-{hi}`")),
                        ClassItem::Char(hi) => ranges.push((lo, hi)),
                        other => {
                            ranges.push((lo, lo));
                            ranges.push(('-', '-'));
                            add(other, &mut ranges);
                        }
                    }
                    continue;
                }
            }
            add(item, &mut ranges);
        }
        if ranges.is_empty() && named.is_empty() && not_named.is_empty() {
            return Err("empty character class".into());
        }
        Ok(CharClass::with_not_named(ranges, named, not_named, negated))
    }
}

#[derive(Clone, Copy)]
enum ClassItem {
    Char(char),
    Digits,
    Named(Named),
    NotNamed(Named),
}

/// One item of a class body: a character (maybe escaped) or a class escape.
fn class_item(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<ClassItem, String> {
    let c = chars.next().ok_or("empty class item")?;
    if c != '\\' {
        return Ok(ClassItem::Char(c));
    }
    Ok(match chars.next() {
        Some('n') => ClassItem::Char('\n'),
        Some('t') => ClassItem::Char('\t'),
        Some('r') => ClassItem::Char('\r'),
        Some('d') => ClassItem::Digits,
        Some('w') => ClassItem::Named(Named::Word),
        Some('s') => ClassItem::Named(Named::Space),
        Some('D') => ClassItem::NotNamed(Named::Digit),
        Some('W') => ClassItem::NotNamed(Named::Word),
        Some('S') => ClassItem::NotNamed(Named::Space),
        Some('u') => {
            let rest: String = chars.clone().collect();
            let (hex, _) = rest.strip_prefix('{').and_then(|r| r.split_once('}')).ok_or("expected `\\u{HEX}` in class")?;
            let v = u32::from_str_radix(hex, 16).map_err(|_| "invalid hex in `\\u{...}`")?;
            for _ in 0..hex.len() + 2 {
                chars.next();
            }
            ClassItem::Char(char::from_u32(v).ok_or("invalid unicode scalar")?)
        }
        Some('x') => {
            let hex: String = chars.by_ref().take(2).collect();
            let v = u32::from_str_radix(&hex, 16).ok().filter(|_| hex.len() == 2).ok_or("expected `\\xHH` in class")?;
            ClassItem::Char(char::from_u32(v).ok_or("invalid `\\x` escape")?)
        }
        Some(c @ ('\\' | ']' | '[' | '-' | '^' | '"' | '\'' | '`' | '.' | '/' | '{' | '}' | '(' | ')' | '|' | '*' | '+' | '?')) => ClassItem::Char(c),
        Some(other) => return Err(format!("unknown class escape `\\{other}`")),
        None => return Err("class ends with a lone backslash".into()),
    })
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
        let ctl = CharClass::parse(r"\x00-\x08\x1f").unwrap();
        assert!(ctl.contains('\0') && ctl.contains('\x05') && ctl.contains('\x1f') && !ctl.contains('\t') && !ctl.contains('-'));
        let escaped_ends = CharClass::parse(r"\--\/").unwrap();
        assert!(escaped_ends.contains('.') && !escaped_ends.contains('0'));
        let dash_before_escape = CharClass::parse(r"a-\d").unwrap();
        assert!(dash_before_escape.contains('-') && dash_before_escape.contains('5') && !dash_before_escape.contains('b'));
        assert!(CharClass::parse(r"\x1").is_err());
    }

    #[test]
    fn negated_escapes_inside_classes() {
        let any = CharClass::parse(r"\s\S").unwrap();
        assert!(any.contains('\n') && any.contains('x') && any.contains('é'));
        let c = CharClass::parse(r"a\D").unwrap();
        assert!(c.contains('a') && c.contains('-') && !c.contains('5'));
    }

    #[test]
    fn named_word_is_unicode_aware() {
        let c = CharClass::parse(r"\w").unwrap();
        assert!(c.contains('ü') && c.contains('_') && !c.contains('-'));
    }
}

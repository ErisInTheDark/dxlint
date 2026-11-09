//! Parser for `/.../` regex literals. Only the linear-time subset exists:
//! literals, classes, groups, alternation, greedy/lazy quantifiers, anchors
//! and word boundaries. No backreferences, no lookaround.

use super::class::{CharClass, Named};
use super::{Look, Pat};

const MAX_COUNT: u32 = 1000;

pub fn parse(raw: &str) -> Result<Pat, String> {
    let mut p = Parser { chars: raw.chars().collect(), i: 0, fold: false };
    if p.eat_str("(?i)") {
        p.fold = true;
    }
    let pat = p.alt()?;
    if p.i < p.chars.len() {
        return Err(format!("unexpected `{}`", p.chars[p.i]));
    }
    Ok(pat)
}

struct Parser {
    chars: Vec<char>,
    i: usize,
    fold: bool,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn eat_str(&mut self, s: &str) -> bool {
        let n = s.chars().count();
        if self.chars.len() >= self.i + n && self.chars[self.i..self.i + n].iter().copied().eq(s.chars()) {
            self.i += n;
            true
        } else {
            false
        }
    }

    fn alt(&mut self) -> Result<Pat, String> {
        let mut branches = vec![self.seq()?];
        while self.eat('|') {
            branches.push(self.seq()?);
        }
        Ok(if branches.len() == 1 { branches.pop().expect("one") } else { Pat::Alt(branches) })
    }

    fn seq(&mut self) -> Result<Pat, String> {
        let mut items: Vec<Pat> = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            let pat = self.quantified(atom)?;
            // Merge literal runs so prefilters see whole words.
            if let (Some(Pat::Lit { bytes: prev, fold: pf }), Pat::Lit { bytes, fold }) = (items.last_mut(), &pat) {
                if *pf == *fold {
                    prev.extend_from_slice(bytes);
                    continue;
                }
            }
            items.push(pat);
        }
        Ok(match items.len() {
            0 => Pat::Empty,
            1 => items.pop().expect("one"),
            _ => Pat::Seq(items),
        })
    }

    fn quantified(&mut self, atom: Pat) -> Result<Pat, String> {
        let (min, max) = match self.peek() {
            Some('*') => (0, None),
            Some('+') => (1, None),
            Some('?') => (0, Some(1)),
            Some('{') => match self.counted()? {
                Some(range) => range,
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };
        // Consume the quantifier char (or the closing `}` of `{n,m}`).
        self.i += 1;
        if matches!(atom, Pat::Look(_) | Pat::Empty | Pat::Around { .. }) {
            return Err("nothing to repeat".into());
        }
        let greedy = !self.eat('?');
        if matches!(self.peek(), Some('*' | '+' | '?')) {
            return Err("stacked quantifiers are not supported; wrap the inner part in `(?:...)`".into());
        }
        Ok(Pat::Repeat { inner: Box::new(atom), min, max, greedy })
    }

    /// Parses `{n}`, `{n,}`, `{n,m}`, leaving the cursor on the closing `}`.
    /// Returns `None` (and consumes nothing) when `{` is a plain literal.
    fn counted(&mut self) -> Result<Option<(u32, Option<u32>)>, String> {
        let start = self.i;
        self.i += 1;
        let num = |p: &mut Parser| {
            let s = p.i;
            while p.peek().is_some_and(|c| c.is_ascii_digit()) {
                p.i += 1;
            }
            p.chars[s..p.i].iter().collect::<String>().parse::<u32>().ok()
        };
        let Some(min) = num(self) else {
            self.i = start;
            return Ok(None);
        };
        let max = if self.eat(',') { num(self) } else { Some(min) };
        if self.peek() != Some('}') {
            self.i = start;
            return Ok(None);
        }
        if min > MAX_COUNT || max.is_some_and(|m| m > MAX_COUNT) {
            return Err(format!("counted repetition above {MAX_COUNT}"));
        }
        if max.is_some_and(|m| m < min) {
            return Err(format!("invalid repetition {{{min},{}}}", max.unwrap_or(0)));
        }
        Ok(Some((min, max)))
    }

    fn atom(&mut self) -> Result<Pat, String> {
        let c = self.peek().ok_or("unexpected end of regex")?;
        self.i += 1;
        Ok(match c {
            '(' => {
                let around = if self.eat_str("?=") {
                    Some((false, false))
                } else if self.eat_str("?!") {
                    Some((false, true))
                } else if self.eat_str("?<=") {
                    Some((true, false))
                } else if self.eat_str("?<!") {
                    Some((true, true))
                } else {
                    None
                };
                if let Some((behind, negate)) = around {
                    let inner = self.alt()?;
                    if !self.eat(')') {
                        return Err("unclosed lookaround `(?...`".into());
                    }
                    if !inner.labels().is_empty() {
                        return Err("lookarounds cannot contain labels".into());
                    }
                    let mut nested = false;
                    inner.walk(&mut |p| nested |= matches!(p, Pat::Around { .. }));
                    if nested {
                        return Err("lookarounds cannot be nested".into());
                    }
                    return Ok(Pat::Around { behind, negate, inner: Box::new(inner) });
                }
                let name = if self.eat_str("?:") {
                    None
                } else if self.eat_str("?<") {
                    let s = self.i;
                    while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        self.i += 1;
                    }
                    let name: String = self.chars[s..self.i].iter().collect();
                    if name.is_empty() || !self.eat('>') {
                        return Err("expected `(?<name>...)`".into());
                    }
                    Some(name)
                } else if self.peek() == Some('?') {
                    return Err("inline flags are not supported (only a leading `(?i)`)".into());
                } else {
                    None
                };
                let inner = self.alt()?;
                if !self.eat(')') {
                    return Err("unclosed group `(`".into());
                }
                match name {
                    Some(name) => Pat::Capture { name, inner: Box::new(inner) },
                    None => inner,
                }
            }
            ')' => return Err("unmatched `)`".into()),
            '[' => {
                let s = self.i;
                if self.peek() == Some('^') {
                    self.i += 1;
                }
                loop {
                    match self.peek() {
                        None => return Err("unclosed character class `[`".into()),
                        Some('\\') => self.i += 2,
                        Some(']') => break,
                        Some(_) => self.i += 1,
                    }
                }
                let raw: String = self.chars[s..self.i].iter().collect();
                self.i += 1;
                let class = CharClass::parse(&raw)?;
                Pat::Class(if self.fold { class.fold_ascii() } else { class })
            }
            '.' => Pat::Any { newline: false },
            '^' => Pat::Look(Look::Bol),
            '$' => Pat::Look(Look::Eol),
            '*' | '+' | '?' => return Err(format!("nothing to repeat before `{c}`")),
            '\\' => self.escape()?,
            c => self.literal(c),
        })
    }

    fn literal(&self, c: char) -> Pat {
        let mut buf = [0u8; 4];
        let fold = self.fold && c.is_ascii_alphabetic();
        Pat::Lit { bytes: c.encode_utf8(&mut buf).as_bytes().to_vec(), fold }
    }

    fn escape(&mut self) -> Result<Pat, String> {
        let c = self.peek().ok_or("regex ends with a lone backslash")?;
        self.i += 1;
        let digits = || CharClass::ranges(&[('0', '9')]);
        Ok(match c {
            'd' => Pat::Class(digits()),
            'D' => Pat::Class(CharClass::new(vec![('0', '9')], vec![], true)),
            'w' => Pat::Class(CharClass::named(Named::Word)),
            'W' => Pat::Class(CharClass::new(vec![], vec![Named::Word], true)),
            's' => Pat::Class(CharClass::named(Named::Space)),
            'S' => Pat::Class(CharClass::new(vec![], vec![Named::Space], true)),
            'b' => Pat::Look(Look::WordBoundary),
            'B' => Pat::Look(Look::NotWordBoundary),
            'A' => Pat::Look(Look::Bof),
            'z' => Pat::Look(Look::Eof),
            'n' => self.literal('\n'),
            't' => self.literal('\t'),
            'r' => self.literal('\r'),
            'x' => {
                let hex: String = self.chars.get(self.i..self.i + 2).ok_or("expected `\\xHH`")?.iter().collect();
                self.i += 2;
                let v = u32::from_str_radix(&hex, 16).map_err(|_| "invalid `\\xHH` escape")?;
                self.literal(char::from_u32(v).ok_or("invalid `\\x` escape")?)
            }
            'u' => {
                if !self.eat('{') {
                    return Err("expected `\\u{HEX}`".into());
                }
                let s = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.i += 1;
                }
                let hex: String = self.chars[s..self.i].iter().collect();
                if !self.eat('}') {
                    return Err("expected `}` after `\\u{HEX`".into());
                }
                let v = u32::from_str_radix(&hex, 16).map_err(|_| "invalid `\\u{...}` escape")?;
                self.literal(char::from_u32(v).ok_or("invalid unicode scalar")?)
            }
            c if c.is_ascii_punctuation() || c == ' ' => self.literal(c),
            c => return Err(format!("unknown escape `\\{c}`")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_runs_merge() {
        match parse(r"fn\(").unwrap() {
            Pat::Lit { bytes, .. } => assert_eq!(bytes, b"fn("),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn quantifiers_and_groups() {
        assert!(matches!(parse("a+?").unwrap(), Pat::Repeat { min: 1, max: None, greedy: false, .. }));
        assert!(matches!(parse("(?<n>x){2,3}").unwrap(), Pat::Repeat { min: 2, max: Some(3), .. }));
        assert!(matches!(parse("a{x").unwrap(), Pat::Lit { .. }));
    }

    #[test]
    fn lookarounds_parse() {
        assert!(matches!(parse("(?<=[=(])").unwrap(), Pat::Around { behind: true, negate: false, .. }));
        assert!(matches!(parse("(?!x)").unwrap(), Pat::Around { behind: false, negate: true, .. }));
        assert!(parse("(?=(?<n>x))").is_err());
        assert!(parse("(?=x)+").is_err());
    }

    #[test]
    fn rejects_unsupported_features() {
        assert!(parse("(?s)x").is_err());
        assert!(parse("a**").is_err());
        assert!(parse("(a").is_err());
        assert!(parse(r"\1").is_err());
    }
}

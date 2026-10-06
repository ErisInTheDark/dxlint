//! An on-demand TypeScript scanner. The parser decides what `/` and `}` mean
//! (division or regex, block end or template continuation) and asks for a
//! rescan; `>` is always one token, so `a >> b` and `A<B<C>>` both work and
//! the expression parser glues `>` runs back together.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Ident,
    PrivateName,
    Num,
    BigInt,
    Str,
    /// `` `...` `` without substitutions.
    Template,
    /// `` `...${ ``
    TemplateHead,
    /// `}...${`
    TemplateMiddle,
    /// `` }...` ``
    TemplateTail,
    Regex,
    P(&'static str),
    Eof,
}

#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub tok: Tok,
    pub start: u32,
    pub end: u32,
    /// A line break sits between the previous token and this one.
    pub nl: bool,
}

#[derive(Clone)]
pub struct Lexer<'s> {
    pub src: &'s str,
    pub pos: usize,
    /// Set when a token ran past the end of the text (unterminated).
    pub broken: Option<u32>,
}

// Longest first within each starting byte.
const PUNCT: &[&str] = &[
    "...", "?.", "??=", "??", "?", "===", "==", "=>", "=", "!==", "!=", "!", "**=", "**", "*=", "*", "&&=", "&&", "&=", "&",
    "||=", "||", "|=", "|", "<<=", "<<", "<=", "<", ">", "++", "+=", "+", "--", "-=", "-", "%=", "%", "^=", "^", "/=", "/",
    "{", "}", "(", ")", "[", "]", ";", ",", ":", "~", "@", ".", "#",
];

/// The longest punctuator starting `rest`. Checks the first byte before
/// comparing, and compares inline: a `starts_with` per candidate called
/// `memcmp` for every token.
fn punctuator(rest: &[u8]) -> Option<&'static str> {
    let first = *rest.first()?;
    PUNCT.iter().copied().find(|p| {
        let p = p.as_bytes();
        p[0] == first && rest.len() >= p.len() && p.iter().zip(rest).all(|(a, b)| a == b)
    })
}

fn ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$' || b >= 0x80 || b == b'\\'
}

fn ident_part(b: u8) -> bool {
    ident_start(b) || b.is_ascii_digit()
}

/// Non-ASCII characters that are whitespace or line breaks to JavaScript,
/// never part of a name (`f ()` is `f`, a line break, `()`).
fn js_space(c: char) -> bool {
    matches!(c, '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

impl<'s> Lexer<'s> {
    pub fn new(src: &'s str) -> Self {
        let mut lx = Lexer { src, pos: 0, broken: None };
        if src.starts_with("#!") {
            lx.pos = src.find('\n').unwrap_or(src.len());
        }
        lx
    }

    fn bytes(&self) -> &'s [u8] {
        self.src.as_bytes()
    }

    /// Skips whitespace and comments; true when a line break was crossed.
    fn trivia(&mut self) -> bool {
        let b = self.bytes();
        let mut nl = false;
        while self.pos < b.len() {
            match b[self.pos] {
                b'\n' | b'\r' => {
                    nl = true;
                    self.pos += 1;
                }
                b' ' | b'\t' | 0x0b | 0x0c => self.pos += 1,
                b'/' if b.get(self.pos + 1) == Some(&b'/') => {
                    self.pos += super::comments::line_comment_end(&self.src[self.pos..]);
                }
                b'/' if b.get(self.pos + 1) == Some(&b'*') => {
                    let body = &b[self.pos + 2..];
                    match memchr::memmem::find(body, b"*/") {
                        Some(n) => {
                            nl |= memchr::memchr(b'\n', &body[..n]).is_some();
                            self.pos += n + 4;
                        }
                        None => {
                            self.broken.get_or_insert(self.pos as u32);
                            self.pos = b.len();
                        }
                    }
                }
                // Unicode spaces and line separators (NBSP, BOM, U+2028...).
                0x80.. => match self.src[self.pos..].chars().next() {
                    Some(c) if js_space(c) => {
                        nl |= matches!(c, '\u{2028}' | '\u{2029}');
                        self.pos += c.len_utf8();
                    }
                    _ => break,
                },
                _ => break,
            }
        }
        nl
    }

    pub fn next_token(&mut self) -> Token {
        let nl = self.trivia();
        let b = self.bytes();
        let start = self.pos;
        let tok = if start >= b.len() {
            Tok::Eof
        } else {
            let c = b[start];
            if ident_start(c) {
                while self.pos < b.len() && ident_part(b[self.pos]) {
                    let ch = if b[self.pos] >= 0x80 { self.src[self.pos..].chars().next() } else { None };
                    if ch.is_some_and(js_space) {
                        break;
                    }
                    // `A` and `\u{1F600}` escapes inside identifiers.
                    self.pos += if let Some(ch) = ch {
                        ch.len_utf8()
                    } else if b[self.pos] == b'\\' && b.get(self.pos + 2) == Some(&b'{') {
                        memchr::memchr(b'}', &b[self.pos..]).map_or(2, |n| n + 1)
                    } else if b[self.pos] == b'\\' {
                        2
                    } else {
                        1
                    };
                }
                self.pos = self.pos.min(b.len());
                Tok::Ident
            } else if c.is_ascii_digit() || (c == b'.' && b.get(start + 1).is_some_and(u8::is_ascii_digit)) {
                self.number()
            } else if c == b'"' || c == b'\'' {
                self.string(c)
            } else if c == b'`' {
                self.pos += 1;
                self.template_rest(Tok::Template, Tok::TemplateHead)
            } else if c == b'#' && b.get(start + 1).is_some_and(|&n| ident_start(n)) {
                self.pos += 1;
                while self.pos < b.len() && ident_part(b[self.pos]) {
                    self.pos += 1;
                }
                Tok::PrivateName
            } else {
                let rest = &self.src[start..];
                match punctuator(rest.as_bytes()) {
                    Some(p) => {
                        // `?.5` is `?` then `.5`.
                        if p == "?." && b.get(start + 2).is_some_and(u8::is_ascii_digit) {
                            self.pos += 1;
                            Tok::P("?")
                        } else {
                            self.pos += p.len();
                            Tok::P(p)
                        }
                    }
                    None => {
                        // Unknown character: skip it as a one-char punctuator.
                        let len = rest.chars().next().map_or(1, char::len_utf8);
                        self.broken.get_or_insert(start as u32);
                        self.pos += len;
                        Tok::P("?unknown")
                    }
                }
            }
        };
        Token { tok, start: start as u32, end: self.pos as u32, nl }
    }

    fn number(&mut self) -> Tok {
        let b = self.bytes();
        let start = self.pos;
        if b[start] == b'0' && matches!(b.get(start + 1), Some(b'x' | b'X' | b'o' | b'O' | b'b' | b'B')) {
            self.pos += 2;
            while self.pos < b.len() && (b[self.pos].is_ascii_hexdigit() || b[self.pos] == b'_') {
                self.pos += 1;
            }
        } else {
            let digits = |lx: &mut Self| {
                while lx.pos < b.len() && (b[lx.pos].is_ascii_digit() || b[lx.pos] == b'_') {
                    lx.pos += 1;
                }
            };
            digits(self);
            if self.pos < b.len() && b[self.pos] == b'.' {
                self.pos += 1;
                digits(self);
            }
            if self.pos < b.len() && matches!(b[self.pos], b'e' | b'E') {
                let save = self.pos;
                self.pos += 1;
                if self.pos < b.len() && matches!(b[self.pos], b'+' | b'-') {
                    self.pos += 1;
                }
                if self.pos < b.len() && b[self.pos].is_ascii_digit() {
                    digits(self);
                } else {
                    self.pos = save;
                }
            }
        }
        if self.pos < b.len() && b[self.pos] == b'n' {
            self.pos += 1;
            return Tok::BigInt;
        }
        Tok::Num
    }

    fn string(&mut self, quote: u8) -> Tok {
        let b = self.bytes();
        self.pos += 1;
        while self.pos < b.len() {
            match b[self.pos] {
                // A line continuation may end in `\r\n`.
                b'\\' => self.pos += if b[self.pos + 1..].starts_with(b"\r\n") { 3 } else { 2 },
                c if c == quote => {
                    self.pos += 1;
                    return Tok::Str;
                }
                b'\n' => break,
                _ => self.pos += 1,
            }
        }
        self.broken.get_or_insert(self.pos.min(b.len()) as u32);
        self.pos = self.pos.min(b.len());
        Tok::Str
    }

    /// After `` ` `` or `}`: up to the closing `` ` `` or the next `${`.
    fn template_rest(&mut self, closed: Tok, opened: Tok) -> Tok {
        let b = self.bytes();
        while self.pos < b.len() {
            match b[self.pos] {
                b'\\' => self.pos += 2,
                b'`' => {
                    self.pos += 1;
                    return closed;
                }
                b'$' if b.get(self.pos + 1) == Some(&b'{') => {
                    self.pos += 2;
                    return opened;
                }
                _ => self.pos += 1,
            }
        }
        self.broken.get_or_insert(b.len() as u32);
        self.pos = b.len();
        closed
    }

    /// Where JSX text starting at `from` ends: the next `{` or `<`, if any.
    pub fn jsx_text_end(&self, from: usize) -> Option<usize> {
        memchr::memchr2(b'{', b'<', &self.bytes()[from..]).map(|n| from + n)
    }

    /// The end of a JSX name part starting at `from`: identifier
    /// characters and `-`.
    pub fn jsx_name_end(&self, from: usize) -> usize {
        let b = self.bytes();
        let mut end = from;
        while end < b.len() && (ident_part(b[end]) || b[end] == b'-') {
            end += 1;
        }
        end
    }

    /// Re-reads a `}` token as the continuation of a template.
    pub fn rescan_template(&mut self, close_brace: Token) -> Token {
        self.pos = close_brace.start as usize + 1;
        let tok = self.template_rest(Tok::TemplateTail, Tok::TemplateMiddle);
        Token { tok, start: close_brace.start, end: self.pos as u32, nl: close_brace.nl }
    }

    /// Re-reads a `/` or `/=` token as a regex literal.
    pub fn rescan_regex(&mut self, slash: Token) -> Token {
        let b = self.bytes();
        self.pos = slash.start as usize + 1;
        let mut class = false;
        while self.pos < b.len() {
            match b[self.pos] {
                b'\\' => self.pos += 2,
                b'[' => {
                    class = true;
                    self.pos += 1;
                }
                b']' => {
                    class = false;
                    self.pos += 1;
                }
                b'/' if !class => {
                    self.pos += 1;
                    while self.pos < b.len() && ident_part(b[self.pos]) {
                        self.pos += 1;
                    }
                    return Token { tok: Tok::Regex, start: slash.start, end: self.pos as u32, nl: slash.nl };
                }
                b'\n' => break,
                _ => self.pos += 1,
            }
        }
        self.broken.get_or_insert(slash.start);
        self.pos = self.pos.min(b.len());
        Token { tok: Tok::Regex, start: slash.start, end: self.pos as u32, nl: slash.nl }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_comments_leave_following_tokens_on_a_new_line() {
        for terminator in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            let src = format!("return // note{terminator}value");
            let mut lexer = Lexer::new(&src);
            assert_eq!(lexer.next_token().tok, Tok::Ident);
            let value = lexer.next_token();
            assert_eq!(&src[value.start as usize..value.end as usize], "value", "{terminator:?}");
            assert!(value.nl, "{terminator:?}");
            assert_eq!(lexer.next_token().tok, Tok::Eof);
        }
    }

    #[test]
    fn unicode_spaces_end_names_and_line_separators_break_lines() {
        let src = "f\u{2028}()\u{202f}ünï\u{1680}x";
        let mut lx = Lexer::new(src);
        let mut toks = Vec::new();
        loop {
            let t = lx.next_token();
            if t.tok == Tok::Eof {
                break;
            }
            toks.push((&src[t.start as usize..t.end as usize], t.nl));
        }
        assert_eq!(toks, vec![("f", false), ("(", true), (")", false), ("ünï", false), ("x", false)]);
    }

    #[test]
    fn strings_continue_over_crlf() {
        let src = "'a\\\r\nb' x";
        let mut lx = Lexer::new(src);
        let t = lx.next_token();
        assert_eq!((t.tok, &src[t.start as usize..t.end as usize]), (Tok::Str, "'a\\\r\nb'"));
        assert!(lx.broken.is_none());
    }
}

//! Tokens of the `.zlint` language. The lexer is context free: patterns,
//! expressions and items all share one token vocabulary, which is what lets
//! macros splice token trees into any position.

use std::rc::Rc;

use super::error::ConfigError;
use super::source::{FileId, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    /// `"..."`; text is the raw, undecoded content between the quotes.
    Str,
    /// `/.../`; text is the raw regex between the slashes.
    Regex,
    Num,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    Comma,
    Colon,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Pipe,
    Question,
    Star,
    Plus,
    Minus,
    Percent,
    Dot,
    Tilde,
    /// Line break or `;`. Ends `match` and `macro x = ...` lines.
    Newline,
    /// Invisible delimiters wrapped around macro expansions and arguments.
    /// Patterns and expressions treat them as parentheses; everything else
    /// skips them (the same trick as rustc's none-delimited groups).
    InvOpen,
    InvClose,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: Tk,
    pub text: Rc<str>,
    pub span: Span,
}

impl Token {
    pub fn is(&self, kind: Tk) -> bool {
        self.kind == kind
    }

    pub fn is_ident(&self, name: &str) -> bool {
        self.kind == Tk::Ident && &*self.text == name
    }

    /// True when `next` starts exactly where `self` ends (no whitespace).
    pub fn touches(&self, next: &Token) -> bool {
        self.span.file == next.span.file && self.span.end == next.span.start
    }

    pub fn describe(&self) -> String {
        match self.kind {
            Tk::Ident => format!("`{}`", self.text),
            Tk::Str => "a string".into(),
            Tk::Regex => "a regex".into(),
            Tk::Num => format!("number `{}`", self.text),
            Tk::Newline => "end of line".into(),
            Tk::InvOpen | Tk::InvClose => "macro boundary".into(),
            _ => format!("`{}`", self.text),
        }
    }
}

/// Lexes `text`, whose first byte sits at `base` inside file `file`.
pub fn lex(file: FileId, text: &str, base: usize, errors: &mut Vec<ConfigError>) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let span = |a: usize, b: usize| Span::new(file, base + a, base + b);
    let push = |out: &mut Vec<Token>, kind: Tk, a: usize, b: usize, t: &str| {
        out.push(Token { kind, text: Rc::from(t), span: Span::new(file, base + a, base + b) });
    };

    while i < bytes.len() {
        let b = bytes[i];
        let start = i;
        match b {
            b'\n' | b';' => {
                push(&mut out, Tk::Newline, i, i + 1, if b == b'\n' { "\n" } else { ";" });
                i += 1;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            b'#' => {
                i = memchr::memchr(b'\n', &bytes[i..]).map_or(bytes.len(), |n| i + n);
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                push(&mut out, Tk::Ident, start, i, &text[start..i]);
            }
            b'0'..=b'9' => {
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                push(&mut out, Tk::Num, start, i, &text[start..i]);
            }
            // Single quotes exist mostly so templates can nest strings:
            // "{join(args, ', ')}".
            b'"' | b'\'' => {
                let quote = b;
                i += 1;
                let mut closed = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        c if c == quote => {
                            closed = true;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                let inner_end = i.min(bytes.len());
                if closed {
                    i += 1;
                } else {
                    errors.push(ConfigError::new(span(start, inner_end), "unterminated string"));
                }
                push(&mut out, Tk::Str, start, i.min(bytes.len()), &text[start + 1..inner_end]);
            }
            b'/' => {
                // Regex literal: ends at an unescaped `/` outside a class.
                i += 1;
                let mut in_class = false;
                let mut closed = false;
                while i < bytes.len() && bytes[i] != b'\n' {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'[' => {
                            in_class = true;
                            i += 1;
                        }
                        b']' => {
                            in_class = false;
                            i += 1;
                        }
                        b'/' if !in_class => {
                            closed = true;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                let inner_end = i.min(bytes.len());
                if closed {
                    i += 1;
                } else {
                    errors.push(
                        ConfigError::new(span(start, inner_end), "unterminated regex")
                            .note("regexes are written `/.../` on one line; escape a literal slash as `\\/`"),
                    );
                }
                push(&mut out, Tk::Regex, start, i.min(bytes.len()), &text[start + 1..inner_end]);
            }
            _ => {
                let two = bytes.get(i + 1).copied();
                let (kind, len) = match (b, two) {
                    (b'=', Some(b'=')) => (Tk::EqEq, 2),
                    (b'!', Some(b'=')) => (Tk::Ne, 2),
                    (b'<', Some(b'=')) => (Tk::Le, 2),
                    (b'>', Some(b'=')) => (Tk::Ge, 2),
                    (b'&', Some(b'&')) => (Tk::AndAnd, 2),
                    (b'|', Some(b'|')) => (Tk::OrOr, 2),
                    (b'(', _) => (Tk::LParen, 1),
                    (b')', _) => (Tk::RParen, 1),
                    (b'{', _) => (Tk::LBrace, 1),
                    (b'}', _) => (Tk::RBrace, 1),
                    (b'[', _) => (Tk::LBrack, 1),
                    (b']', _) => (Tk::RBrack, 1),
                    (b',', _) => (Tk::Comma, 1),
                    (b':', _) => (Tk::Colon, 1),
                    (b'=', _) => (Tk::Eq, 1),
                    (b'<', _) => (Tk::Lt, 1),
                    (b'>', _) => (Tk::Gt, 1),
                    (b'!', _) => (Tk::Bang, 1),
                    (b'|', _) => (Tk::Pipe, 1),
                    (b'?', _) => (Tk::Question, 1),
                    (b'*', _) => (Tk::Star, 1),
                    (b'+', _) => (Tk::Plus, 1),
                    (b'-', _) => (Tk::Minus, 1),
                    (b'%', _) => (Tk::Percent, 1),
                    (b'.', _) => (Tk::Dot, 1),
                    (b'~', _) => (Tk::Tilde, 1),
                    _ => {
                        let ch = text[i..].chars().next().unwrap_or('?');
                        errors.push(ConfigError::new(span(i, i + ch.len_utf8()), format!("unexpected character `{ch}`")));
                        i += ch.len_utf8();
                        continue;
                    }
                };
                push(&mut out, kind, i, i + len, &text[i..i + len]);
                i += len;
            }
        }
    }
    out
}

/// Decodes `\n`, `\t`, `\u{...}` and friends inside a string literal.
pub fn decode_escapes(raw: &str, span: Span) -> Result<String, ConfigError> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some(c @ ('\\' | '"' | '\'' | '{' | '}')) => out.push(c),
            Some('u') => {
                let rest = chars.as_str();
                let parsed = rest
                    .strip_prefix('{')
                    .and_then(|r| r.split_once('}'))
                    .and_then(|(hex, _)| Some((char::from_u32(u32::from_str_radix(hex, 16).ok()?)?, hex.len())));
                match parsed {
                    Some((c, n)) => {
                        out.push(c);
                        chars = rest[n + 2..].chars();
                    }
                    None => return Err(ConfigError::new(span, "invalid unicode escape, expected `\\u{HEX}`")),
                }
            }
            Some(other) => {
                return Err(ConfigError::new(span, format!("unknown escape `\\{other}`"))
                    .note("supported escapes: \\n \\t \\r \\0 \\\\ \\\" \\' \\{ \\} \\u{HEX}"))
            }
            None => return Err(ConfigError::new(span, "string ends with a lone backslash")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tk> {
        let mut errors = Vec::new();
        let toks = lex(FileId(0), src, 0, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        toks.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn regexes_keep_slashes_inside_classes_and_escapes() {
        let mut errors = Vec::new();
        let toks = lex(FileId(0), r#"/a[/]\/b/ "x\"y""#, 0, &mut errors);
        assert!(errors.is_empty());
        assert_eq!(toks.iter().map(|t| t.kind).collect::<Vec<_>>(), vec![Tk::Regex, Tk::Str]);
        assert_eq!(&*toks[0].text, r"a[/]\/b");
    }

    #[test]
    fn operators_prefer_longest_form() {
        assert_eq!(kinds("a == c != d || e"), vec![Tk::Ident, Tk::EqEq, Tk::Ident, Tk::Ne, Tk::Ident, Tk::OrOr, Tk::Ident]);
    }

    #[test]
    fn decode_handles_unicode_escape() {
        let s = Span::new(FileId(0), 0, 0);
        assert_eq!(decode_escapes(r"a\u{1F600}\n", s).unwrap(), "a😀\n");
        assert!(decode_escapes(r"\q", s).is_err());
    }
}

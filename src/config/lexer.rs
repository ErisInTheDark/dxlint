//! Tokens of the `.dx` language. The lexer is context free: patterns,
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
    /// What an unquoted `import` names: `.ts`, `rules:recommended`,
    /// `../shared/rules`. Text is the raw word.
    Path,
    /// `=>`, between a lambda's parameter and its body.
    Arrow,
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
    /// `|>`: the next step of an inline pipeline.
    PipeGt,
    /// `&`: an intersection in `unwraps a & b`.
    Amp,
    /// `\`: a difference in `unwraps a \ nullish`.
    Backslash,
    Question,
    Nullish,
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

thread_local! {
    static LEXING: std::cell::Cell<std::time::Duration> = const { std::cell::Cell::new(std::time::Duration::ZERO) };
}

/// Time this thread spent lexing since the last call (for `--stats`).
pub fn take_lexing_time() -> std::time::Duration {
    LEXING.with(|time| time.replace(std::time::Duration::ZERO))
}

/// Lexes `text`, whose first byte sits at `base` inside file `file`.
pub fn lex(file: FileId, text: &str, base: usize, errors: &mut Vec<ConfigError>) -> Vec<Token> {
    let started = std::time::Instant::now();
    let tokens = lex_text(file, text, base, errors);
    LEXING.with(|time| time.set(time.get() + started.elapsed()));
    tokens
}

fn lex_text(file: FileId, text: &str, base: usize, errors: &mut Vec<ConfigError>) -> Vec<Token> {
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
                // A `-` between name characters belongs to the name
                // (`no-dbg`), so subtraction is written `a - b`.
                let name_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
                while i < bytes.len() && (name_char(bytes[i]) || bytes[i] == b'-' && bytes.get(i + 1).is_some_and(|&b| name_char(b))) {
                    i += 1;
                }
                // `import x` starting an item: x is a path, whatever
                // characters it uses (`no-unassigned-import` is a name).
                let item_start = (start == 0 || matches!(bytes[start - 1], b' ' | b'\t' | b'\n' | b'\r' | b'{' | b';'))
                    && out.last().is_none_or(|t| matches!(t.kind, Tk::Newline | Tk::LBrace | Tk::RBrace));
                push(&mut out, Tk::Ident, start, i, &text[start..i]);
                if item_start && &text[start..i] == "import" {
                    let mut at = i;
                    while at < bytes.len() && matches!(bytes[at], b' ' | b'\t') {
                        at += 1;
                    }
                    if at > i && at < bytes.len() && !matches!(bytes[at], b'"' | b'\'' | b'\n' | b'\r' | b';' | b'#') {
                        let path_start = at;
                        while at < bytes.len() && !matches!(bytes[at], b' ' | b'\t' | b'\n' | b'\r' | b';' | b'#') {
                            at += 1;
                        }
                        push(&mut out, Tk::Path, path_start, at, &text[path_start..at]);
                        i = at;
                    }
                }
            }
            // `@macro`, `@name(...)`, `@each`: what happens when the config
            // loads. One identifier token, `@` included.
            b'@' if bytes.get(i + 1).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_') => {
                i += 1;
                let name_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
                while i < bytes.len() && (name_char(bytes[i]) || bytes[i] == b'-' && bytes.get(i + 1).is_some_and(|&b| name_char(b))) {
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
                // `7e-8`: an exponent only when digits follow it.
                if matches!(bytes.get(i), Some(b'e' | b'E')) {
                    let digits = i + 1 + usize::from(matches!(bytes.get(i + 1), Some(b'+' | b'-')));
                    if bytes.get(digits).is_some_and(u8::is_ascii_digit) {
                        i = digits;
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                push(&mut out, Tk::Num, start, i, &text[start..i]);
            }
            b'"' if text[i..].starts_with("\"\"\"") => {
                let (end, raw) = block_string(text, i, file, base, errors);
                push(&mut out, Tk::Str, start, end, &raw);
                i = end;
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
                    (b'=', Some(b'>')) => (Tk::Arrow, 2),
                    (b'!', Some(b'=')) => (Tk::Ne, 2),
                    (b'<', Some(b'=')) => (Tk::Le, 2),
                    (b'>', Some(b'=')) => (Tk::Ge, 2),
                    (b'&', Some(b'&')) => (Tk::AndAnd, 2),
                    (b'|', Some(b'|')) => (Tk::OrOr, 2),
                    (b'|', Some(b'>')) => (Tk::PipeGt, 2),
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
                    (b'&', _) => (Tk::Amp, 1),
                    (b'\\', _) => (Tk::Backslash, 1),
                    (b'?', Some(b'?')) => (Tk::Nullish, 2),
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

/// A `"""` string starting at `start`: its end and its raw (still escaped)
/// text. The text starts on the line after the opening `"""`; the closing
/// `"""` sits on its own line, and its indentation is removed from every
/// line. A line ending in `\` continues onto the next without a line break.
fn block_string(text: &str, start: usize, file: FileId, base: usize, errors: &mut Vec<ConfigError>) -> (usize, String) {
    let span = |a: usize, b: usize| Span::new(file, base + a, base + b);
    let bytes = text.as_bytes();
    let open_end = start + 3;
    let line_end = memchr::memchr(b'\n', &bytes[open_end..]).map_or(bytes.len(), |n| open_end + n);
    if !text[open_end..line_end].trim().is_empty() {
        errors.push(ConfigError::new(span(start, line_end), "a `\"\"\"` string starts on the line after its `\"\"\"`"));
    }
    let content_start = (line_end + 1).min(bytes.len());
    let mut close = None;
    let mut i = content_start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' if text[i..].starts_with("\"\"\"") => {
                close = Some(i);
                break;
            }
            _ => i += 1,
        }
    }
    let Some(close) = close else {
        errors.push(ConfigError::new(span(start, open_end), "unterminated `\"\"\"` string"));
        return (bytes.len(), String::new());
    };
    let end = close + 3;
    let close_line = text[..close].rfind('\n').map_or(0, |n| n + 1);
    let strip = &text[close_line..close];
    if close_line < content_start || !strip.bytes().all(|b| b == b' ' || b == b'\t') {
        errors.push(ConfigError::new(span(close, end), "the closing `\"\"\"` sits on a line of its own"));
        return (end, String::new());
    }

    let mut out = String::new();
    let mut at = content_start;
    let mut first = true;
    let mut joined = false;
    while at < close_line {
        let next = memchr::memchr(b'\n', &bytes[at..close_line]).map_or(close_line, |n| at + n + 1);
        let line = text[at..next].trim_end_matches('\n').trim_end_matches('\r');
        if !first && !joined {
            out.push('\n');
        }
        first = false;
        let body = if line.trim().is_empty() {
            ""
        } else if let Some(rest) = line.strip_prefix(strip) {
            rest
        } else {
            errors.push(ConfigError::new(span(at, at + line.len()), "line is indented less than the closing `\"\"\"`"));
            line.trim_start()
        };
        let slashes = body.len() - body.trim_end_matches('\\').len();
        joined = slashes % 2 == 1;
        out.push_str(if joined { &body[..body.len() - 1] } else { body });
        at = next;
    }
    (end, out)
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
        assert_eq!(kinds("a == c != d || e => f"), vec![Tk::Ident, Tk::EqEq, Tk::Ident, Tk::Ne, Tk::Ident, Tk::OrOr, Tk::Ident, Tk::Arrow, Tk::Ident]);
    }

    #[test]
    fn import_takes_a_bare_path() {
        let mut errors = Vec::new();
        let toks = lex(FileId(0), "import ../a/b-c.dx # x\nimport rules:recommended\nimport \"q\"\nrule no-unassigned-import {}", 0, &mut errors);
        let paths: Vec<&str> = toks.iter().filter(|t| t.kind == Tk::Path).map(|t| &*t.text).collect();
        assert_eq!(paths, vec!["../a/b-c.dx", "rules:recommended"]);
        assert!(toks.iter().any(|t| t.kind == Tk::Str), "quoted paths stay strings");
    }

    #[test]
    fn names_hold_inner_dashes_and_numbers_take_exponents() {
        let mut errors = Vec::new();
        let toks = lex(FileId(0), "no-dbg x-1 a- b -c a--b 7e-8 2.5E+3 1em", 0, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        let pieces: Vec<(Tk, &str)> = toks.iter().map(|t| (t.kind, &*t.text)).collect();
        use Tk::{Ident as I, Minus as M, Num as N};
        assert_eq!(pieces, vec![
            (I, "no-dbg"), (I, "x-1"), (I, "a"), (M, "-"), (I, "b"), (M, "-"), (I, "c"),
            (I, "a"), (M, "-"), (M, "-"), (I, "b"),
            (N, "7e-8"), (N, "2.5E+3"), (N, "1"), (I, "em"),
        ]);
    }

    fn block(src: &str) -> (String, Vec<ConfigError>) {
        let mut errors = Vec::new();
        let toks = lex(FileId(0), src, 0, &mut errors);
        (toks.iter().find(|t| t.kind == Tk::Str).map_or(String::new(), |t| t.text.to_string()), errors)
    }

    #[test]
    fn block_strings_lose_the_closing_indentation_and_join_at_backslashes() {
        let (text, errors) = block("x \"\"\"\r\n    /*!\r\n     * a \"quote\" \\\r\n    b\\\\\r\n\r\n    */\r\n    \"\"\" y");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(text, "/*!\n * a \"quote\" b\\\\\n\n*/");
        assert_eq!(block("\"\"\"\n\"\"\"").0, "");
    }

    #[test]
    fn block_strings_need_their_own_lines() {
        assert_eq!(block("\"\"\"a\n\"\"\"").1.len(), 1);
        assert_eq!(block("\"\"\"\n  a\n b\n  \"\"\"").1.len(), 1);
        assert_eq!(block("\"\"\"\n  a \"\"\"").1.len(), 1);
        assert_eq!(block("\"\"\"\n  a").1.len(), 1);
    }

    #[test]
    fn decode_handles_unicode_escape() {
        let s = Span::new(FileId(0), 0, 0);
        assert_eq!(decode_escapes(r"a\u{1F600}\n", s).unwrap(), "a😀\n");
        assert!(decode_escapes(r"\q", s).is_err());
    }
}

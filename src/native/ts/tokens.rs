//! The file as tokens, where rules care about tokens rather than nodes:
//! the tokens of a node's text, and the punctuation between nodes that has
//! no node of its own (the `(` of a call).

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::lexer::{Lexer, Tok};

/// JavaScript line terminators.
fn breaks_line(text: &str) -> bool {
    text.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

impl NativeFile<'_> {
    /// The tokens of a node's text (comments and spacing dropped).
    pub fn tokens(&self, node: u32) -> Vec<&str> {
        let text = self.syntax.text(node);
        let mut lx = Lexer::new(text);
        let mut out = Vec::new();
        loop {
            let t = lx.next_token();
            if t.tok == Tok::Eof {
                break;
            }
            out.push(&text[t.start as usize..t.end as usize]);
        }
        out
    }

    /// After byte `from`: the first `open` character outside comments, past
    /// closing parentheses; and the end of the last token before it.
    fn punctuation_after(&self, from: usize, open: char) -> Option<(usize, usize)> {
        let src = self.syntax.src;
        let b = src.as_bytes();
        let mut i = from;
        let mut last = from;
        while i < b.len() {
            match b[i] {
                b')' => {
                    i += 1;
                    last = i;
                }
                b'/' if b.get(i + 1) == Some(&b'/') => i = src[i..].find('\n').map_or(b.len(), |n| i + n),
                b'/' if b.get(i + 1) == Some(&b'*') => i = src[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 2),
                c if c.is_ascii_whitespace() => i += 1,
                _ => {
                    let ch = src[i..].chars().next()?;
                    if ch == open {
                        return Some((i, last));
                    }
                    if ch.is_whitespace() {
                        i += ch.len_utf8();
                        continue;
                    }
                    return None;
                }
            }
        }
        None
    }

    /// eslint's no-unexpected-multiline: where a line break makes the next
    /// line continue this expression: the offset of the `(`, `[`, template
    /// or `/` that does, with what it is (`function`, `property`,
    /// `taggedTemplate`, `division`).
    pub fn unexpected_multiline(&self, node: u32) -> Option<(usize, &'static str)> {
        let s = &self.syntax;
        let end = |n: u32| s.tree.node(n).end as usize;
        match s.kind(node)? {
            K::Call if s.get(node, F::Optional).is_none() && s.all(node, F::Arg).next().is_some() => {
                let before = s.all(node, F::TypeArg).last().or_else(|| s.get(node, F::Callee))?;
                let (open, last) = self.punctuation_after(end(before), '(')?;
                breaks_line(&s.src[last..open]).then_some((open, "function"))
            }
            K::Index if s.get(node, F::Optional).is_none() => {
                let (open, last) = self.punctuation_after(end(s.get(node, F::Object)?), '[')?;
                breaks_line(&s.src[last..open]).then_some((open, "property"))
            }
            K::TaggedTemplate => {
                let quasi = s.get(node, F::Quasi)?;
                let before = s.all(node, F::TypeArg).last().or_else(|| s.get(node, F::Tag))?;
                let at = s.tree.node(quasi).start as usize;
                let (_, last) = self.punctuation_after(end(before), '`').unwrap_or((at, end(before)));
                breaks_line(&s.src[last..at]).then_some((at, "taggedTemplate"))
            }
            // `a \n /foo/g` read as `a / foo / g`.
            K::Binary if s.word(node, F::Op) == Some("/") => {
                let outer = s.parent(node).filter(|&p| s.is(p, K::Binary) && s.word(p, F::Op) == Some("/") && s.field(node) == Some(F::Left))?;
                // The token right after the second `/`, touching it, is a
                // word of regex flags (`/bar/g.test(x)`).
                let right = s.get(outer, F::Right)?;
                let at = s.tree.node(right).start as usize;
                let word: String = s.src[at..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
                if word.is_empty() || !word.chars().all(|c| "dgimsuvy".contains(c)) || !s.src[..at].ends_with('/') {
                    return None;
                }
                let left = s.get(node, F::Left)?;
                let (open, last) = self.punctuation_after(end(left), '/')?;
                breaks_line(&s.src[last..open]).then_some((open, "division"))
            }
            _ => None,
        }
    }
}

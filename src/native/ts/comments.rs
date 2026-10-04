//! The comments of a file. The parser skips them as trivia, so they are
//! found by scanning the text between string, template and regex literals
//! (the only places a `/*` or `//` is not a comment).

use super::kinds::Kind;
use super::syntax::Syntax;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comment {
    pub start: u32,
    pub end: u32,
    /// `/* ... */` rather than `// ...`.
    pub block: bool,
}

impl Comment {
    /// The text between the delimiters.
    pub fn body<'s>(&self, src: &'s str) -> &'s str {
        let text = &src[self.start as usize..self.end as usize];
        if self.block {
            text.get(2..text.len().saturating_sub(2)).unwrap_or("")
        } else {
            &text[2..]
        }
    }
}

/// Kinds whose text holds no comments: literals and JSX text.
pub fn is_literal(kind: Kind) -> bool {
    matches!(kind, Kind::String | Kind::Template | Kind::Regex | Kind::JsxText)
}

/// Every comment, in order.
pub fn comments(s: &Syntax) -> Vec<Comment> {
    let literals = (0..s.tree.nodes.len() as u32).filter(|&id| s.kind(id).is_some_and(is_literal)).map(|id| {
        let n = s.tree.node(id);
        (n.start as usize, n.end as usize)
    });
    scan(s.src, literals.collect())
}

/// The comments of `src` outside the `literals` spans.
pub fn scan(src: &str, mut literals: Vec<(usize, usize)>) -> Vec<Comment> {
    literals.sort_unstable();
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut lit = literals.into_iter().peekable();
    // A `#!` line is not code.
    let mut i = if src.starts_with("#!") { src.find('\n').unwrap_or(b.len()) } else { 0 };
    while i + 1 < b.len() {
        while lit.peek().is_some_and(|&(_, end)| end <= i) {
            lit.next();
        }
        if let Some(&(start, end)) = lit.peek() {
            if start <= i {
                i = end;
                continue;
            }
        }
        match (b[i], b[i + 1]) {
            (b'/', b'/') => {
                let end = memchr::memchr(b'\n', &b[i..]).map_or(b.len(), |n| i + n);
                let end = if end > i && b[end - 1] == b'\r' { end - 1 } else { end };
                out.push(Comment { start: i as u32, end: end as u32, block: false });
                i = end;
            }
            (b'/', b'*') => {
                let end = memchr::memmem::find(&b[i + 2..], b"*/").map_or(b.len(), |n| i + 2 + n + 2);
                out.push(Comment { start: i as u32, end: end as u32, block: true });
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

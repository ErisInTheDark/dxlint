//! The file as tokens, where rules care about tokens rather than nodes:
//! the tokens of a node's text, and the punctuation between nodes that has
//! no node of its own (the `(` of a call).

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K, ALL_KINDS};
use super::lexer::{Lexer, Tok, Token};
use super::Parsed;
use crate::expr::Value;
use crate::grammar::{Edge, Node, TEXT};
use std::collections::HashMap;

pub(crate) struct Lexed(pub(crate) Vec<Token>);
pub(crate) struct LexedComments(pub(crate) Vec<Token>);

pub(crate) fn lex_comments(file: &NativeFile<'_>) -> LexedComments {
    let lexed = file.cached(lex);
    let mut tokens = lexed.0.clone();
    tokens.extend(file.comments().iter().filter(|comment| {
        let at = lexed.0.partition_point(|token| token.end <= comment.start);
        lexed.0.get(at).is_none_or(|token| token.start >= comment.end)
    }).map(|comment| Token {
        tok: Tok::P(if comment.block { "/*" } else { "//" }),
        start: comment.start,
        end: comment.end,
        nl: false,
    }));
    tokens.sort_unstable_by_key(|token| token.start);
    LexedComments(tokens)
}

pub(crate) fn lex(file: &NativeFile<'_>) -> Lexed {
    let syntax = &file.syntax;
    let mut opaque = HashMap::new();
    for id in 0..syntax.tree.nodes.len() as u32 {
        if matches!(syntax.kind(id), Some(K::Regex | K::JsxText)) {
            let node = syntax.tree.node(id);
            opaque.insert(node.start, (node.end, Tok::Regex));
        } else if matches!(syntax.kind(id), Some(K::Template | K::TemplateType)) {
            for quasi in syntax.all(id, F::Quasi) {
                let node = syntax.tree.node(quasi);
                opaque.insert(node.start, (node.end, Tok::Template));
            }
        }
    }
    Lexed(lex_around(syntax.src, &opaque))
}

/// The parser's nodes as written: kind and field numbers of this front end.
fn raw_kind(node: &Node) -> Option<K> {
    (node.matcher != TEXT).then(|| ALL_KINDS.get(node.matcher as usize).copied()).flatten()
}

/// Gives each punctuation and keyword token that no child node covers a
/// token node (`ts_comma`, `ts_keyword`, ...) under the innermost node
/// holding it, in field `token`, so rules can target tokens as layers.
pub fn attach(src: &str, parsed: &mut Parsed) {
    let nodes = &parsed.nodes;
    let mut opaque = HashMap::new();
    for node in nodes {
        match raw_kind(node) {
            Some(K::Regex | K::JsxText) => {
                opaque.insert(node.start, (node.end, Tok::Regex));
            }
            Some(K::Template | K::TemplateType) => {
                for e in node.children.iter().filter(|e| e.label == F::Quasi as u16) {
                    let quasi = &nodes[e.node as usize];
                    opaque.insert(quasi.start, (quasi.end, Tok::Template));
                }
            }
            _ => {}
        }
    }
    let tokens = lex_around(src, &opaque);
    let mut owned: Vec<(u32, Token)> = Vec::new();
    let mut stack = vec![parsed.root];
    while let Some(id) = stack.pop() {
        let node = &nodes[id as usize];
        let mut children: Vec<&Node> = node.children.iter().map(|e| &nodes[e.node as usize]).collect();
        children.sort_by_key(|c| c.start);
        let mut at = tokens.partition_point(|t| t.start < node.start);
        let mut next_child = children.iter().peekable();
        while let Some(token) = tokens.get(at).filter(|t| t.end <= node.end) {
            while next_child.next_if(|c| c.end <= token.start).is_some() {}
            match next_child.peek() {
                // Inside a child: skip past it.
                Some(c) if c.start <= token.start && token.end <= c.end => {
                    at = tokens.partition_point(|t| t.start < c.end);
                    next_child.next();
                }
                _ => {
                    // A node written as one token (a name, a literal) is that token.
                    if (token.start, token.end) != (node.start, node.end) {
                        owned.push((id, *token));
                    }
                    at += 1;
                }
            }
        }
        stack.extend(node.children.iter().filter(|e| nodes[e.node as usize].matcher != TEXT).map(|e| e.node));
    }
    for (owner, token) in owned {
        let Some(kind) = token_kind(src, &token) else { continue };
        let id = parsed.nodes.len() as u32;
        parsed.nodes.push(Node { matcher: kind as u32, start: token.start, end: token.end, children: Vec::new() });
        parsed.nodes[owner as usize].children.push(Edge { label: F::Token as u16, node: id });
    }
}

/// The layer a token reads as from outside: the node written exactly as
/// it (`ts_identifier`, `ts_number`...), else its token layer.
pub(crate) fn token_layer(file: &NativeFile<'_>, token: &Token) -> Option<K> {
    file.cached(exact_nodes).0.get(&(token.start, token.end)).copied().or_else(|| token_kind(file.syntax.src, token))
}

/// Syntax nodes by their exact span; the innermost wins.
struct ExactNodes(HashMap<(u32, u32), K>);

fn exact_nodes(file: &NativeFile<'_>) -> ExactNodes {
    let s = &file.syntax;
    let mut map = HashMap::new();
    for &id in &s.tree.order {
        if let Some(kind) = s.kind(id).filter(|k| !k.is_token()) {
            let n = s.tree.node(id);
            map.insert((n.start, n.end), kind);
        }
    }
    ExactNodes(map)
}

/// A gap between tokens: its text, whether whitespace (outside comments)
/// separates them, line breaks, blank lines and comments.
pub(crate) fn gap_value(src: &str, from: usize, to: usize) -> Value<'static> {
    let text = &src[from..to.max(from)];
    let unified = text.replace("\r\n", "\n");
    let lines: Vec<&str> = unified.split(['\n', '\r', '\u{2028}', '\u{2029}']).collect();
    let blank_lines = lines.iter().skip(1).take(lines.len().saturating_sub(2)).filter(|l| l.trim().is_empty()).count();
    Value::record(vec![
        ("text".into(), Value::from(text.to_owned())),
        ("start".into(), Value::from(from)),
        ("end".into(), Value::from(to.max(from))),
        ("spaced".into(), Value::Bool(gap_has_space(text))),
        ("line_break".into(), Value::Bool(breaks_line(text))),
        ("blank_lines".into(), Value::from(blank_lines)),
        ("comments".into(), Value::Bool(!text.trim().is_empty())),
    ])
}

/// The token layer a punctuator or keyword gets (none for literals).
fn token_kind(src: &str, token: &Token) -> Option<K> {
    Some(match token.tok {
        Tok::Ident if src[token.start as usize..].starts_with(|c: char| c.is_ascii_alphabetic()) => K::Keyword,
        Tok::P(p) => match p {
            "," => K::Comma,
            ";" => K::Semicolon,
            ":" => K::Colon,
            "." => K::Dot,
            "?" => K::Question,
            "..." => K::Ellipsis,
            "=" => K::Equals,
            "(" => K::OpenParen,
            ")" => K::CloseParen,
            "[" => K::OpenBracket,
            "]" => K::CloseBracket,
            "{" => K::OpenBrace,
            "}" => K::CloseBrace,
            "<" => K::OpenAngle,
            ">" => K::CloseAngle,
            _ => K::Punctuator,
        },
        _ => return None,
    })
}

/// Every code token of `src`, `opaque` spans (start -> end, kind) taken whole.
fn lex_around(src: &str, opaque: &HashMap<u32, (u32, Tok)>) -> Vec<Token> {
    let mut lexer = Lexer::new(src);
    let mut tokens = Vec::new();
    loop {
        // JSX trivia belongs to its parser span, not the ordinary lexer.
        if let Some(&(end, kind)) = opaque.get(&(lexer.pos as u32)) {
            if end as usize > lexer.pos {
                tokens.push(Token { tok: kind, start: lexer.pos as u32, end, nl: false });
                lexer.pos = end as usize;
                continue;
            }
        }
        let mut token = lexer.next_token();
        if token.tok == Tok::Eof {
            break;
        }
        if let Some(&(end, kind)) = opaque.get(&token.start) {
            token.end = end;
            token.tok = kind;
            lexer.pos = end as usize;
        }
        tokens.push(token);
    }
    tokens
}

pub(crate) fn gap_has_space(mut gap: &str) -> bool {
    while !gap.is_empty() {
        if let Some(comment) = gap.strip_prefix("/*") {
            gap = comment.find("*/").map_or("", |end| &comment[end + 2..]);
        } else if gap.starts_with("//") {
            gap = gap.find(['\n', '\r', '\u{2028}', '\u{2029}']).map_or("", |end| &gap[end..]);
        } else {
            let ch = gap.chars().next().expect("nonempty gap");
            if ch.is_whitespace() {
                return true;
            }
            gap = &gap[ch.len_utf8()..];
        }
    }
    false
}

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

#[cfg(test)]
mod lexical_tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    #[test]
    fn opaque_spans_do_not_leak_punctuation_or_comments() {
        with_native_file("const value = `a , /* not a comment */ ${other} z`; /[,;]/.test(value);", |file, _| {
            let code = file.cached(lex);
            let items = file.cached(lex_comments);
            assert_eq!(code.0.len(), items.0.len());
            assert!(!code.0.iter().any(|token| matches!(token.tok, Tok::P(","))));
            assert!(code.0.windows(2).all(|pair| pair[0].end <= pair[1].start));
            for node in 0..file.syntax.tree.nodes.len() as u32 {
                if file.syntax.is(node, K::Regex) {
                    let span = file.syntax.tree.node(node);
                    assert!(code.0.iter().any(|token| token.start == span.start && token.end == span.end));
                }
            }
        });
    }

    #[test]
    fn comment_gaps_and_all_line_terminators_retain_offsets() {
        with_native_file("a/* \n */+b;\r\nc;\rd;\u{2028}e;\u{2029}f;", |file, _| {
            let code = file.cached(lex);
            let plus = code.0.iter().position(|token| token.tok == Tok::P("+")).expect("operator");
            let gap = &file.syntax.src[code.0[plus - 1].end as usize..code.0[plus].start as usize];
            assert!(breaks_line(gap));
            assert!(!gap_has_space(gap));
            assert!(!gap_has_space("// text"));
            assert!(gap_has_space("/* text */ "));
            assert!(gap_has_space("// text\n"));
            assert!(gap_has_space("\u{00a0}/* text */"));
            for terminator in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
                let text = terminator.repeat(2);
                let Value::Record(gap) = gap_value(&text, 0, text.len()) else { unreachable!() };
                assert!(gap.iter().any(|(key, value)| key == "line_break" && *value == Value::Bool(true)));
                assert!(gap.iter().any(|(key, value)| key == "blank_lines" && *value == Value::from(1usize)));
            }
            let items = file.cached(lex_comments);
            assert!(items.0.iter().any(|token| token.tok == Tok::P("/*")));
            assert!(items.0.windows(2).all(|pair| pair[0].end <= pair[1].start));
        });
    }
}

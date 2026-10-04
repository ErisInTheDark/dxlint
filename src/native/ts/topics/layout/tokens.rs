//! Cached parser-aware tokens. Opaque regex, template and JSX spans stay intact.
//! Use `file.cached(lex).0` for code, `file.cached(lex_comments).0` for comments too.

use std::collections::HashMap;

use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::{Lexer, Tok, Token};

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
    let mut lexer = Lexer::new(file.syntax.src);
    let mut tokens = Vec::new();
    loop {
        // JSX text may begin with whitespace or comment-looking text.
        // Keep trivia scanning inside the parser's lexical boundaries.
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
    Lexed(tokens)
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

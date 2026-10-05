//! Parser-aware tokens, gaps and nodes for uncovered punctuation.
//! Native literal spans retain their syntax's boundaries: JSX strings,
//! for example, treat backslashes as text rather than escapes.

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K, ALL_KINDS};
use super::lexer::{Lexer, Tok, Token};
use super::Parsed;
use crate::expr::Value;
use crate::grammar::{Edge, Node, TEXT};
use std::collections::HashMap;

/// Shared, so threads linting one file in rule groups scan it once.
#[derive(Clone)]
pub(crate) struct Lexed(pub(crate) std::sync::Arc<[Token]>);
pub(crate) struct LexedComments(pub(crate) Vec<Token>);

/// Requested token kinds, indexed by the native kind registry.
pub type TokenMask = [bool; ALL_KINDS.len()];
#[cfg(test)]
pub const ALL_TOKENS: TokenMask = [true; ALL_KINDS.len()];

/// Adds every token kind.
pub fn add_all_tokens(mask: &mut TokenMask) {
    for k in ALL_KINDS.iter().filter(|k| k.is_token()) {
        mask[*k as usize] = true;
    }
}

pub(crate) fn lex_comments(file: &NativeFile<'_>) -> LexedComments {
    let lexed = file.cached(lex);
    let mut tokens = lexed.0.to_vec();
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
        if syntax.is(id, K::String) {
            let node = syntax.tree.node(id);
            opaque.insert(node.start, (node.end, Tok::Str));
        } else if matches!(syntax.kind(id), Some(K::Regex | K::JsxText)) {
            let node = syntax.tree.node(id);
            opaque.insert(node.start, (node.end, Tok::Regex));
        } else if matches!(syntax.kind(id), Some(K::Template | K::TemplateType)) {
            for quasi in syntax.all(id, F::Quasi) {
                let node = syntax.tree.node(quasi);
                opaque.insert(node.start, (node.end, Tok::Template));
            }
        }
    }
    Lexed(lex_around(syntax.src, &opaque).into())
}

/// The parser's nodes as written: kind and field numbers of this front end.
fn raw_kind(node: &Node) -> Option<K> {
    (node.matcher != TEXT).then(|| ALL_KINDS.get(node.matcher as usize).copied()).flatten()
}

/// Gives each punctuation and keyword token that no child node covers a
/// token node (`ts_comma`, `ts_keyword`, ...) under the innermost node
/// holding it, in field `token`, so rules can target tokens as layers.
/// Only the kinds in `wanted`: which syntax node owns each token does not
/// depend on the selection.
pub fn attach_selected(src: &str, parsed: &mut Parsed, wanted: &TokenMask) {
    if !wanted.iter().any(|&wanted| wanted) {
        return;
    }
    let nodes = &parsed.nodes;
    let mut opaque = HashMap::new();
    for node in nodes {
        match raw_kind(node) {
            Some(K::String) => {
                opaque.insert(node.start, (node.end, Tok::Str));
            }
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
    let mut owned = Vec::new();
    let mut stack = vec![parsed.root];
    let mut children = Vec::new();
    while let Some(id) = stack.pop() {
        let node = &nodes[id as usize];
        stack.extend(node.children.iter().filter(|e| nodes[e.node as usize].matcher != TEXT).map(|e| e.node));
        let mut at = tokens.partition_point(|t| t.start < node.start);
        // A node written as one token already owns it, even when another
        // syntax node shares its span. Its children still need visiting.
        if tokens.get(at).is_some_and(|token| (token.start, token.end) == (node.start, node.end)) {
            continue;
        }
        children.clear();
        children.extend(node.children.iter().map(|e| &nodes[e.node as usize]));
        children.sort_by_key(|c| c.start);
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
                    if let Some(kind) = token_kind(src, token).filter(|&kind| wanted[kind as usize]) {
                        owned.push((id, kind, token.start, token.end));
                    }
                    at += 1;
                }
            }
        }
    }
    for (owner, kind, start, end) in owned {
        let id = parsed.nodes.len() as u32;
        parsed.nodes.push(Node { matcher: kind as u32, start, end, children: Vec::new() });
        parsed.nodes[owner as usize].children.push(Edge { label: F::Token as u16, node: id });
    }
    parsed.tokens = Some(Lexed(tokens.into()));
}

/// The layer a token reads as from outside: the node written exactly as
/// it (`ts_identifier`, `ts_number`...), else its token layer.
pub(crate) fn token_layer(file: &NativeFile<'_>, token: &Token) -> Option<K> {
    exact_node(file, token.start, token.end).or_else(|| token_kind(file.syntax.src, token))
}

/// The innermost syntax node spanning exactly `start..end`. Descends by
/// binary search (children are sorted by start, and syntax children are
/// disjoint), so a query costs the depth, not a whole-file index.
fn exact_node(file: &NativeFile<'_>, start: u32, end: u32) -> Option<K> {
    let s = &file.syntax;
    let (mut id, mut found) = (s.root(), None);
    loop {
        let n = s.tree.node(id);
        if (n.start, n.end) == (start, end) {
            found = s.kind(id).filter(|k| !k.is_token()).or(found);
        }
        let upto = n.children.partition_point(|e| s.tree.node(e.node).start <= start);
        let mut inside = None;
        for e in n.children[..upto].iter().rev() {
            let c = s.tree.node(e.node);
            // Pattern layers and labelled text hold no syntax; empty nodes
            // hold nothing.
            if s.kind(e.node).is_none() || c.start == c.end {
                continue;
            }
            inside = (end <= c.end).then_some(e.node);
            break;
        }
        match inside {
            Some(child) => id = child,
            None => return found,
        }
    }
}

/// A code token as a value: `{text, kind, start, end}` (`is` reads its kind).
fn token_record(file: &NativeFile<'_>, t: &Token) -> Value<'static> {
    token_record_kind(file, t, token_layer(file, t))
}

fn token_record_kind(file: &NativeFile<'_>, t: &Token, kind: Option<K>) -> Value<'static> {
    Value::record([
        ("text".into(), Value::text_copy(&file.syntax.src[t.start as usize..t.end as usize])),
        ("kind".into(), kind.map_or(Value::Null, |k| Value::from(k.name()))),
        ("start".into(), Value::from(t.start as usize)),
        ("end".into(), Value::from(t.end as usize)),
    ])
}

fn prefix_child(s: &super::syntax::Syntax<'_>, node: u32, at: u32) -> Option<u32> {
    s.kids(node).filter_map(|(_, child)| {
        let span = s.tree.node(child);
        (span.start <= at && at < span.end && s.kind(child).is_some_and(|kind| !kind.is_token())).then_some(child)
    }).last()
}

fn opaque_prefix(file: &NativeFile<'_>, mut node: u32, at: u32) -> Option<Token> {
    let s = &file.syntax;
    loop {
        let span = s.tree.node(node);
        let opaque = match s.kind(node) {
            Some(K::String) => Some((span.start, span.end, Tok::Str)),
            Some(K::Regex | K::JsxText) => Some((span.start, span.end, Tok::Regex)),
            Some(K::Template | K::TemplateType) => s.all(node, F::Quasi).find_map(|quasi| {
                let span = s.tree.node(quasi);
                (span.start == at && span.end > at).then_some((span.start, span.end, Tok::Template))
            }),
            _ => None,
        };
        if let Some((start, end, tok)) = opaque.filter(|&(start, _, _)| start == at) {
            return Some(Token { tok, start, end, nl: false });
        }
        node = prefix_child(s, node, at)?;
    }
}

/// Native boundaries already identify the first token's lexical context.
/// Walk only its boundary branch instead of indexing the entire file.
fn first_native_token(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let span = s.tree.node(node);
    let token = opaque_prefix(file, node, span.start).unwrap_or_else(|| {
        let mut lexer = Lexer::new(s.src);
        lexer.pos = span.start as usize;
        let token = lexer.next_token();
        opaque_prefix(file, node, token.start).unwrap_or(token)
    });
    if token.tok == Tok::Eof || token.end > span.end {
        return Value::Null;
    }
    let mut current = node;
    let mut kind = None;
    loop {
        let span = s.tree.node(current);
        if span.start == token.start && span.end == token.end {
            kind = s.kind(current);
        }
        let Some(child) = prefix_child(s, current, token.start) else { break };
        current = child;
    }
    token_record_kind(file, &token, kind.or_else(|| token_kind(s.src, &token)))
}

/// The first (`last`: last) code token inside a node.
pub(crate) fn edge(file: &NativeFile<'_>, node: u32, last: bool) -> Value<'static> {
    if !last && file.syntax.kind(node).is_some_and(|kind| !kind.is_token()) {
        return first_native_token(file, node);
    }
    let n = file.syntax.tree.node(node);
    let lexed = file.cached(lex);
    let from = lexed.0.partition_point(|t| t.start < n.start);
    let to = lexed.0.partition_point(|t| t.end <= n.end);
    let inside = lexed.0.get(from..to.max(from)).unwrap_or_default();
    let Some(t) = (if last { inside.last() } else { inside.first() }) else { return Value::Null };
    token_record(file, t)
}

/// Matching brackets of a file: each bracket's start -> its partner's index.
struct BracketPairs(HashMap<u32, usize>);

fn bracket_pairs(file: &NativeFile<'_>) -> BracketPairs {
    let lexed = file.cached(lex);
    let mut pairs = HashMap::new();
    let mut open: Vec<usize> = Vec::new();
    for (i, t) in lexed.0.iter().enumerate() {
        let want = match t.tok {
            Tok::P("(" | "[" | "{") => {
                open.push(i);
                continue;
            }
            Tok::P(")") => "(",
            Tok::P("]") => "[",
            Tok::P("}") => "{",
            _ => continue,
        };
        // An unmatched closer (recovered syntax) pairs with nothing.
        if let Some(at) = open.iter().rposition(|&o| lexed.0[o].tok == Tok::P(want)) {
            let o = open[at];
            open.truncate(at);
            pairs.insert(lexed.0[o].start, i);
            pairs.insert(t.start, o);
        }
    }
    BracketPairs(pairs)
}

/// The bracket matching the one a node starts with (a bracket token node,
/// `(a)`), or else the one it ends with.
pub(crate) fn paired(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let n = file.syntax.tree.node(node);
    let lexed = file.cached(lex);
    let pairs = file.cached(bracket_pairs);
    let at = |offset: u32| pairs.0.get(&offset).copied();
    let partner = at(n.start).or_else(|| {
        let last = lexed.0.partition_point(|t| t.end <= n.end).checked_sub(1)?;
        (lexed.0[last].end == n.end).then(|| at(lexed.0[last].start)).flatten()
    });
    partner.map_or(Value::Null, |i| token_record(file, &lexed.0[i]))
}

/// A gap between tokens: its text, whether whitespace (outside comments)
/// separates them, line breaks, blank lines and comments.
pub(crate) fn gap_value(src: &str, from: usize, to: usize) -> Value<'static> {
    let text = &src[from..to.max(from)];
    // Most gaps are "" or a few spaces: no lines, no comments to walk.
    if !text.bytes().any(|b| matches!(b, b'\n' | b'\r' | b'/') || b >= 0x80) {
        return Value::record([
            ("text".into(), Value::text_copy(text)),
            ("start".into(), Value::from(from)),
            ("end".into(), Value::from(to.max(from))),
            ("spaced".into(), Value::Bool(text.bytes().any(|b| b.is_ascii_whitespace() || b == 0x0b))),
            ("line_break".into(), Value::Bool(false)),
            ("blank_lines".into(), Value::from(0usize)),
            ("blank_gaps".into(), Value::from(0usize)),
            ("comments".into(), Value::Bool(!text.trim().is_empty())),
        ]);
    }
    let unified = text.replace("\r\n", "\n");
    let lines: Vec<&str> = unified.split(['\n', '\r', '\u{2028}', '\u{2029}']).collect();
    let blank_lines = lines.iter().skip(1).take(lines.len().saturating_sub(2)).filter(|l| l.trim().is_empty()).count();
    let (mut spaced, mut blank_gaps) = (false, 0usize);
    for gap in uncommented_gaps(text) {
        let (mut breaks, mut previous) = (0usize, '\0');
        for ch in gap.chars() {
            spaced |= ch.is_whitespace();
            if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') && !(ch == '\n' && previous == '\r') {
                breaks += 1;
            }
            previous = ch;
        }
        blank_gaps += breaks.saturating_sub(1);
    }
    Value::record([
        ("text".into(), Value::text_copy(text)),
        ("start".into(), Value::from(from)),
        ("end".into(), Value::from(to.max(from))),
        ("spaced".into(), Value::Bool(spaced)),
        ("line_break".into(), Value::Bool(breaks_line(text))),
        ("blank_lines".into(), Value::from(blank_lines)),
        ("blank_gaps".into(), Value::from(blank_gaps)),
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

pub(crate) fn gap_has_space(gap: &str) -> bool {
    uncommented_gaps(gap).any(|part| part.chars().any(char::is_whitespace))
}

/// Whitespace intervals between tokens/comments; comment boundaries must not join.
fn uncommented_gaps(mut gap: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if gap.is_empty() {
            return None;
        }
        let start = gap
            .match_indices('/')
            .find_map(|(pos, _)| (gap[pos..].starts_with("/*") || gap[pos..].starts_with("//")).then_some(pos))
            .unwrap_or(gap.len());
        let part = &gap[..start];
        let comment = &gap[start..];
        gap = if let Some(block) = comment.strip_prefix("/*") {
            block.find("*/").map_or("", |end| &block[end + 2..])
        } else {
            comment.find(['\n', '\r', '\u{2028}', '\u{2029}']).map_or("", |end| &comment[end..])
        };
        Some(part)
    })
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

    fn assert_first_tokens(file: &NativeFile<'_>) {
        let code = file.cached(lex);
        for node in 0..file.syntax.tree.nodes.len() as u32 {
            let span = file.syntax.tree.node(node);
            let expected = code.0.iter().find(|token| span.start <= token.start && token.end <= span.end)
                .map_or(Value::Null, |token| token_record(file, token));
            assert_eq!(edge(file, node, false), expected, "{:?}: {:?}", file.syntax.kind(node), file.syntax.text(node));
        }
    }

    #[test]
    fn token_layers_find_the_innermost_node_written_exactly_as_the_token() {
        use crate::engine::tests::native_linter;
        use crate::grammar::Tree;
        use crate::native::ts::{modules::Project, syntax::Syntax};
        use std::path::Path;

        let linter = native_linter("");
        let front = linter.grammar.natives.first().expect("native ts");
        let lang = linter.grammar.lang_for(Path::new("a.tsx")).expect("ts language");
        for (src, jsx) in [
            ("const a = [x, (y), f(z), -1, `t ${u} v`, /r/g, this, null]; // c\n", false),
            ("class C<T> extends B { #p = 1; m(a?: T): void { super.m(a!); } }\n/* x */ type U = T[keyof T];", false),
            ("const view = <Box a=\"s\" b={ value }>text {kid}</Box>;", true),
        ] {
            let mut parsed = super::super::parse(src, jsx).expect("valid syntax");
            attach_selected(src, &mut parsed, &ALL_TOKENS);
            let nodes = front.remap(parsed.nodes);
            let tree = Tree::graft(&linter.grammar, lang, src, nodes, parsed.root, None, None);
            let project = Project::default();
            let file = NativeFile::new(Syntax::new(&tree, src, front), Path::new("/root/a.tsx"), &project);
            for token in file.cached(lex).0.iter() {
                // By definition: the last syntax node in document order.
                let expected = tree.order.iter().rev().copied()
                    .filter(|&id| (tree.node(id).start, tree.node(id).end) == (token.start, token.end))
                    .find_map(|id| file.syntax.kind(id).filter(|k| !k.is_token()));
                assert_eq!(exact_node(&file, token.start, token.end), expected, "{:?}", &src[token.start as usize..token.end as usize]);
            }
        }
    }

    #[test]
    fn first_tokens_agree_with_parser_aware_token_boundaries() {
        for source in [
            "",
            "/* before */ /[/*,;]/u.test(value); value / other; debugger\ntrue; false; null;",
            "const value = `a ${other} z`; type Name = `a${string}z`; const empty = ``;",
            "type Count = 1; function f<T>(x: T): T { return x; } const v = 'a\\\\b';",
        ] {
            with_native_file(source, |file, _| assert_first_tokens(file));
        }
    }

    #[test]
    fn selected_tokens_preserve_their_spans_and_owners() {
        let src = "function run<T>(left, right) { const values = [left, ...right]; if (left) return values; }";
        let parsed = super::super::parse(src, false).expect("valid syntax");
        let original_len = parsed.nodes.len();
        let projection = |parsed: &Parsed| {
            let mut tokens = Vec::new();
            for (owner, node) in parsed.nodes[..original_len].iter().enumerate() {
                for edge in node.children.iter().filter(|edge| edge.label == F::Token as u16) {
                    let token = &parsed.nodes[edge.node as usize];
                    tokens.push((owner, token.start, token.end, raw_kind(token).expect("token kind")));
                }
            }
            tokens.sort_by_key(|token| (token.0, token.1));
            tokens
        };
        let mut all = Parsed { nodes: parsed.nodes.clone(), root: parsed.root, tokens: None };
        attach_selected(src, &mut all, &ALL_TOKENS);
        let all_tokens = projection(&all);
        let mut wanted = [false; ALL_KINDS.len()];
        for kind in [K::Comma, K::OpenBrace, K::Ellipsis] {
            wanted[kind as usize] = true;
        }
        let expected: Vec<_> = all_tokens.into_iter().filter(|token| wanted[token.3 as usize]).collect();
        assert!(!expected.is_empty());
        let mut selected = Parsed { nodes: parsed.nodes.clone(), root: parsed.root, tokens: None };
        attach_selected(src, &mut selected, &wanted);
        assert_eq!(projection(&selected), expected);
        let mut none = parsed;
        attach_selected(src, &mut none, &[false; ALL_KINDS.len()]);
        assert_eq!(none.nodes.len(), original_len);
        assert!(projection(&none).is_empty());
    }

    #[test]
    fn attached_stream_is_reused_without_changing_indexed_queries() {
        use crate::engine::tests::native_linter;
        use crate::grammar::Tree;
        use crate::native::ts::{modules::Project, syntax::Syntax};
        use std::path::Path;

        let linter = native_linter("");
        let project = Project::default();
        let front = linter.grammar.natives.first().expect("native ts");
        let mut selected = [false; ALL_KINDS.len()];
        selected[K::Comma as usize] = true;
        for (src, jsx) in [
            ("", false),
            ("/* before */ /[/*,;]/u.test(value); value / other;\nconst a = [1, 2];", false),
            ("const v = `a ${`b ${other} c`} z`; type Name = `a${string}z`; const empty = ``;", false),
            ("function f<T>(x: T): T { return x; } const n = value >> 2; // after\n", false),
            ("const view = <Box a=\"\\\" b={ value }> \n/* raw, text */ \\ </Box>;", true),
        ] {
            for (wanted, graft) in [(ALL_TOKENS, false), (selected, true), ([false; ALL_KINDS.len()], true)] {
                let mut parsed = super::super::parse(src, jsx).expect("valid syntax");
                attach_selected(src, &mut parsed, &wanted);
                let attached = wanted.iter().any(|&wanted| wanted);
                assert_eq!(parsed.tokens.is_some(), attached);
                let nodes = front.remap(parsed.nodes);
                let tree = if graft {
                    let lang = linter.grammar.lang_for(Path::new("a.tsx")).expect("ts language");
                    Tree::graft(&linter.grammar, lang, src, nodes, parsed.root, None, None)
                } else {
                    Tree::from_native(nodes, parsed.root)
                };
                let path = Path::new("/root/a.tsx");
                let lazy = NativeFile::new(Syntax::new(&tree, src, front), path, &project);
                let seeded = NativeFile::new(Syntax::new(&tree, src, front), path, &project)
                    .with_tokens(parsed.tokens);
                let code = if attached {
                    seeded.cached::<Lexed>(|_| panic!("attached stream must not be lexed again"))
                } else {
                    seeded.cached(lex)
                };
                let projection = |tokens: &[Token]| tokens.iter()
                    .map(|t| (t.tok, t.start, t.end, t.nl)).collect::<Vec<_>>();
                assert_eq!(projection(&code.0), projection(&lazy.cached(lex).0));
                assert_eq!(projection(&seeded.cached(lex_comments).0), projection(&lazy.cached(lex_comments).0));
                for node in 0..tree.nodes.len() as u32 {
                    assert_eq!(edge(&seeded, node, true), edge(&lazy, node, true));
                    assert_eq!(paired(&seeded, node), paired(&lazy, node));
                    for fact in super::super::topics::syntax::FACTS {
                        assert_eq!((fact.get)(&seeded, node), (fact.get)(&lazy, node));
                    }
                }
            }
        }
    }

    #[test]
    fn raw_jsx_strings_preserve_following_delimiters() {
        use crate::engine::tests::native_linter;
        use crate::grammar::Tree;
        use crate::native::ts::{modules::Project, syntax::Syntax};
        use std::path::Path;

        let src = "const view = <Box a=\"\\\" b={ value }> \n/* raw, text */ \\ </Box>; function after() { return 1; }";
        let linter = native_linter("");
        let front = linter.grammar.natives.first().expect("native ts");
        let mut parsed = super::super::parse(src, true).expect("valid jsx");
        let boundaries: Vec<_> = parsed.nodes.iter()
            .filter(|node| matches!(raw_kind(node), Some(K::JsxExpression | K::Block)))
            .flat_map(|node| [(node.start, K::OpenBrace, Tok::P("{")), (node.end - 1, K::CloseBrace, Tok::P("}"))])
            .collect();
        assert!(!boundaries.is_empty());
        attach_selected(src, &mut parsed, &ALL_TOKENS);
        let attached: Vec<_> = boundaries.iter().map(|&(at, kind, _)| {
            parsed.nodes.iter().any(|node| node.start == at && raw_kind(node) == Some(kind))
        }).collect();
        let tree = Tree::from_native(front.remap(parsed.nodes), parsed.root);
        let project = Project::default();
        let file = NativeFile::new(Syntax::new(&tree, src, front), Path::new("/root/a.tsx"), &project);
        assert_first_tokens(&file);
        let code = file.cached(lex);
        for &(at, _, tok) in &boundaries {
            assert!(code.0.iter().any(|token| token.start == at && token.tok == tok), "missing code token at {at}");
        }
        assert!(attached.into_iter().all(|present| present), "missing attached delimiter");
    }

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
    fn blank_gaps_exclude_comment_interiors_without_joining_intervals() {
        for (text, count) in [
            ("/*\n\n\n*/", 0usize),
            ("\n/* note */\n", 0),
            ("\n\n/*\n\n*/\n\n", 2),
            ("// note\n\n", 1),
            ("/* unterminated\n\n", 0),
            ("// unterminated", 0),
        ] {
            let Value::Record(gap) = gap_value(text, 0, text.len()) else { unreachable!() };
            assert_eq!(
                gap.iter().find(|(key, _)| key == "blank_gaps").map(|(_, value)| value),
                Some(&Value::from(count)),
                "{text:?}"
            );
        }
        for terminator in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
            let text = format!("/*{terminator}{terminator}*/{terminator}{terminator}");
            let Value::Record(gap) = gap_value(&text, 0, text.len()) else { unreachable!() };
            assert_eq!(
                gap.iter().find(|(key, _)| key == "blank_gaps").map(|(_, value)| value),
                Some(&Value::from(1usize))
            );
            assert_eq!(
                gap.iter().find(|(key, _)| key == "blank_lines").map(|(_, value)| value),
                Some(&Value::from(2usize))
            );
        }
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

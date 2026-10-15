//! Parser-aware tokens and nodes for uncovered punctuation.
//! Native literal spans retain their syntax's boundaries: JSX strings,
//! for example, treat backslashes as text rather than escapes.

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K, ALL_KINDS};
use super::lexer::{Lexer, Tok, Token};
use super::Parsed;
use crate::grammar::{Edge, Node, TEXT};
// Keyed by byte offsets, looked up per token: the fast hasher.
use crate::fx_hash::FxHashMap as HashMap;

/// Shared, so threads linting one file in rule groups scan it once.
#[derive(Clone)]
pub(crate) struct Lexed(pub(crate) std::sync::Arc<[Token]>);
pub(crate) struct LexedComments(pub(crate) Vec<Token>);

/// Requested token kinds, indexed by the native kind registry.
pub type TokenMask = [bool; ALL_KINDS.len()];
#[cfg(test)]
pub const ALL_TOKENS: TokenMask = [true; ALL_KINDS.len()];

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
    let mut opaque = HashMap::default();
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

/// Gives each punctuation and keyword token that no syntax child covers a
/// token node (`ts_comma`, `ts_keyword`, ...) under the innermost node
/// holding it, in field `token`, so rules can target tokens as layers.
/// Only the kinds in `wanted`: which syntax node owns each token does not
/// depend on the selection.
pub fn attach_selected(src: &str, parsed: &mut Parsed, wanted: &TokenMask) {
    if !wanted.iter().any(|&wanted| wanted) {
        return;
    }
    let nodes = &parsed.nodes;
    let mut opaque = HashMap::default();
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
        // Labelled text projects syntax fields, but does not own code tokens.
        children.extend(node.children.iter().map(|e| &nodes[e.node as usize]).filter(|child| child.matcher != TEXT));
        if !children.windows(2).all(|pair| pair[0].start <= pair[1].start) {
            children.sort_by_key(|c| c.start);
        }
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

impl NativeFile<'_> {
    /// Parser-aware tokens inside a node; trivia outside literals is dropped.
    pub fn tokens(&self, node: u32) -> Vec<&str> {
        let span = self.syntax.tree.node(node);
        let code = self.cached(lex);
        let from = code.0.partition_point(|token| token.start < span.start);
        code.0[from..].iter().take_while(|token| token.end <= span.end)
            .map(|token| &self.syntax.src[token.start as usize..token.end as usize])
            .collect()
    }
}

#[cfg(test)]
mod lexical_tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    #[test]
    fn token_queries_include_code_projected_as_plain_field_text() {
        for (src, jsx) in [
            ("const value = receiver?.property; receiver?.[index]; receiver?.(argument);", false),
            ("const value = left + right; value += other; !value; value as unknown;", false),
            ("export default class C { public readonly value?: number; async method() {} }", false),
            ("type T = { readonly [K in keyof U]+?: U[K] };", false),
            ("const view = <Box enabled={left && right}>raw ?. + text</Box>;", true),
        ] {
            let mut parsed = super::super::parse(src, jsx).expect("valid syntax");
            let syntax_nodes = parsed.nodes.clone();
            attach_selected(src, &mut parsed, &ALL_TOKENS);
            let code = &parsed.tokens.as_ref().expect("attached stream").0;
            for token in code.iter() {
                let Some(kind) = token_kind(src, token) else { continue };
                if syntax_nodes.iter().any(|node| {
                    raw_kind(node).is_some() && (node.start, node.end) == (token.start, token.end)
                }) {
                    continue;
                }
                let attached = parsed.nodes[syntax_nodes.len()..].iter().filter(|node| {
                    (node.start, node.end) == (token.start, token.end) && raw_kind(node) == Some(kind)
                }).count();
                assert_eq!(attached, 1, "code token missing or duplicated: {:?}", &src[token.start as usize..token.end as usize]);
            }
        }
    }

    #[test]
    fn token_equality_preserves_literal_contents_but_ignores_code_trivia() {
        for (left, right, equal) in [
            ("`${0} `", "`${0}`", false),
            ("`${0} `", "`${0 /* comment */} `", true),
            ("`a ${`b ${x} `} z`", "`a ${`b ${x}`} z`", false),
            ("a + b", "a /* comment */ + b", true),
            ("/a b/", "/ab/", false),
            ("'a b'", "'ab'", false),
        ] {
            let source = format!("const left = {left}; const right = {right};");
            with_native_file(&source, |file, _| {
                let find = |text| (0..file.syntax.tree.nodes.len() as u32)
                    .find(|&node| file.syntax.kind(node).is_some() && file.syntax.text(node) == text)
                    .expect("expression");
                let (a, b) = (find(left), find(right));
                assert_eq!(file.equal_tokens(a, b), equal, "{left} vs {right}");
                assert_eq!(file.equal_tokens(b, a), equal, "{right} vs {left}");
            });
        }
    }

    #[test]
    fn jsx_tag_tokens_belong_to_disjoint_tag_owners() {
        use crate::native::ts::kinds::KINDS;

        let src = "const view = <><Box<Type<Nested>> a=\">\" b={left > right}>raw <Child /></Box></>;";
        let parsed = super::super::parse(src, true).expect("valid JSX");
        let mut wanted = [false; ALL_KINDS.len()];
        for kind in [K::OpenAngle, K::CloseAngle, K::Punctuator] {
            wanted[kind as usize] = true;
        }
        let project = |mask: &[bool; ALL_KINDS.len()]| {
            let mut copy = Parsed { nodes: parsed.nodes.clone(), root: parsed.root, tokens: None };
            attach_selected(src, &mut copy, mask);
            let mut tokens = Vec::new();
            for owner in &copy.nodes[..parsed.nodes.len()] {
                for edge in owner.children.iter().filter(|edge| edge.label == F::Token as u16) {
                    let token = &copy.nodes[edge.node as usize];
                    if wanted[raw_kind(token).unwrap() as usize] {
                        tokens.push((token.start, token.end, KINDS[owner.matcher as usize].0));
                    }
                }
            }
            tokens.sort_unstable();
            tokens
        };
        let all = project(&ALL_TOKENS);
        assert_eq!(all, project(&wanted));
        for (start, end, owner) in &all {
            assert!(!matches!(*owner, "ts_jsx_element" | "ts_jsx_fragment"), "tag token has no tag owner: {}", &src[*start as usize..*end as usize]);
        }
        for delimiter in ["<>", "<Box", "<Child", "</Box", "</>"] {
            let start = src.find(delimiter).unwrap() as u32;
            let owners: Vec<_> = all.iter().filter(|token| token.0 == start).map(|token| token.2).collect();
            assert_eq!(owners, [if delimiter.starts_with("</") { "ts_jsx_closing" } else { "ts_jsx_opening" }], "{delimiter}");
        }
        assert!(all.windows(2).all(|pair| pair[0].0 != pair[1].0));
    }

    #[test]
    fn selected_tokens_preserve_their_spans_and_owners() {
        for (src, jsx) in [
            ("const values = [call(left, right), ...other];", false),
            ("const value = receiver?.property + receiver?.[index]; receiver?.(argument);", false),
            ("function run<T>(left, right) { const values = [left, ...right]; if (left) return values; }", false),
            ("@dec export default class C<T> { method(a: T, b: T) { return /[,;]/.test(`${a},${b}`); } }", false),
            ("const view = <Box a=\"\\\" b={call(left, right)}>raw,; {child}</Box>;", true),
        ] {
            let parsed = super::super::parse(src, jsx).expect("valid syntax");
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
            if src.starts_with("const values") {
                let mut commas: Vec<_> = all_tokens.iter().filter(|token| token.3 == K::Comma).collect();
                commas.sort_by_key(|token| token.1);
                let owners: Vec<_> = commas.into_iter()
                    .map(|token| raw_kind(&parsed.nodes[token.0]).expect("syntax owner")).collect();
                assert_eq!(owners, [K::Call, K::Array]);
            }
            let mut wanted = [false; ALL_KINDS.len()];
            for kind in [K::Comma, K::OpenBrace, K::Ellipsis, K::Punctuator] {
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
                    Tree::graft(&linter.grammar, lang, src, nodes, parsed.root, None, false, None)
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
            }
        }
    }

    #[test]
    fn raw_jsx_strings_preserve_following_delimiters() {
        use crate::engine::tests::native_front;
        use crate::grammar::Tree;
        use crate::native::ts::{modules::Project, syntax::Syntax};
        use std::path::Path;

        let src = "const view = <Box a=\"\\\" b={ value }> \n/* raw, text */ \\ </Box>; function after() { return 1; }";
        let front = native_front();
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
        let tree = Tree::from_native(parsed.nodes, parsed.root);
        let project = Project::default();
        let file = NativeFile::new(Syntax::new(&tree, src, &front), Path::new("/root/a.tsx"), &project);
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
    fn line_comments_preserve_following_tokens_and_continuation_punctuation() {
        for terminator in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            let src = format!("callee // note{terminator}(argument);");
            with_native_file(&src, |file, _| {
                let tokens = file.tokens(file.syntax.root());
                assert_eq!(tokens, ["callee", "(", "argument", ")", ";"], "{terminator:?}");
                let comments = file.comments();
                assert_eq!(comments.len(), 1);
                assert_eq!(comments[0].body(&src), " note", "{terminator:?}");
            });
        }
    }

    #[test]
    fn comments_join_the_code_stream_in_order() {
        with_native_file("a/* \n */+b;\r\nc;\rd;\u{2028}e;\u{2029}f;", |file, _| {
            let items = file.cached(lex_comments);
            assert!(items.0.iter().any(|token| token.tok == Tok::P("/*")));
            assert!(items.0.windows(2).all(|pair| pair[0].end <= pair[1].start));
        });
    }
}

//! A recursive-descent TypeScript/JavaScript parser producing layer nodes.
//! It stops at the first syntax error (the file then gets no `ts_*` nodes
//! and one diagnostic) and speculates, with full backtracking, only where
//! TypeScript itself is ambiguous: arrow functions, generic calls and
//! function types.

mod expr;
mod stmt;
mod types;

use super::kinds::{Field, Kind};
use super::lexer::{Lexer, Tok, Token};
use crate::grammar::{Edge, Node, TEXT};

/// Parsing stopped; the reason is in [`Parser::error`].
pub(super) struct Stop;
pub(super) type PResult<T> = Result<T, Stop>;

/// Nesting deeper than this stops the parse (protects the stack).
const MAX_DEPTH: u32 = 1000;

/// Child edges of a node under construction.
#[derive(Default)]
pub(super) struct Kids(pub Vec<Edge>);

impl Kids {
    pub fn add(&mut self, field: Field, node: u32) {
        self.0.push(Edge { label: field as u16, node });
    }

    pub fn opt(&mut self, field: Field, node: Option<u32>) {
        if let Some(n) = node {
            self.add(field, n);
        }
    }
}

pub struct Parser<'s> {
    lx: Lexer<'s>,
    src: &'s str,
    tok: Token,
    prev_end: u32,
    pub nodes: Vec<Node>,
    /// `.tsx`/`.jsx`: `<` starts JSX, never a type assertion.
    jsx: bool,
    depth: u32,
    /// Inside `for (...;` heads, where `in` is not an operator.
    no_in: bool,
    /// Inside a generator, where `yield` is a keyword.
    in_gen: bool,
    /// Parsing a conditional type's `extends` clause.
    no_cond: bool,
    pub error: Option<(u32, String)>,
}

#[derive(Clone)]
struct Saved<'s> {
    lx: Lexer<'s>,
    tok: Token,
    prev_end: u32,
    nodes: usize,
    no_in: bool,
}

/// Words that never name a variable.
pub(super) const RESERVED: &[&str] = &[
    "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "enum", "export",
    "extends", "false", "finally", "for", "function", "if", "import", "in", "instanceof", "new", "null", "return", "super",
    "switch", "this", "throw", "true", "try", "typeof", "var", "void", "while", "with",
];

impl<'s> Parser<'s> {
    pub fn new(src: &'s str, jsx: bool) -> Self {
        let mut lx = Lexer::new(src);
        let tok = lx.next_token();
        Parser { lx, src, tok, prev_end: 0, nodes: Vec::new(), jsx, depth: 0, no_in: false, in_gen: false, no_cond: false, error: None }
    }

    /// Parses the whole file; the root is the last node.
    pub fn parse_program(&mut self) -> PResult<u32> {
        let mut kids = Kids::default();
        while self.tok.tok != Tok::Eof {
            let s = self.statement()?;
            kids.add(Field::Body, s);
        }
        if let Some(at) = self.lx.broken {
            return self.fail_at(at, "unterminated token");
        }
        let root = self.nodes.len() as u32;
        self.nodes.push(Node { matcher: Kind::Program as u32, start: 0, end: self.src.len() as u32, children: kids.0 });
        Ok(root)
    }

    // ---- tokens ------------------------------------------------------------

    pub(super) fn text(&self, t: Token) -> &'s str {
        &self.src[t.start as usize..t.end as usize]
    }

    pub(super) fn cur(&self) -> &'s str {
        self.text(self.tok)
    }

    pub(super) fn bump(&mut self) -> Token {
        let t = self.tok;
        self.prev_end = t.end;
        self.tok = self.lx.next_token();
        t
    }

    pub(super) fn is(&self, p: &str) -> bool {
        matches!(self.tok.tok, Tok::P(x) if x == p)
    }

    pub(super) fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn expect(&mut self, p: &str) -> PResult<Token> {
        if self.is(p) {
            Ok(self.bump())
        } else {
            self.fail(&format!("expected `{p}`"))
        }
    }

    pub(super) fn is_word(&self, w: &str) -> bool {
        self.tok.tok == Tok::Ident && self.cur() == w
    }

    pub(super) fn eat_word(&mut self, w: &str) -> bool {
        if self.is_word(w) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn expect_word(&mut self, w: &str) -> PResult<Token> {
        if self.is_word(w) {
            Ok(self.bump())
        } else {
            self.fail(&format!("expected `{w}`"))
        }
    }

    /// The token after the current one, without consuming anything.
    pub(super) fn peek(&self) -> Token {
        self.lx.clone().next_token()
    }

    pub(super) fn peek_text(&self) -> (Token, &'s str) {
        let t = self.peek();
        (t, self.text(t))
    }

    /// An identifier that can name a binding (not a reserved word).
    pub(super) fn is_binding_ident(&self) -> bool {
        self.tok.tok == Tok::Ident && !RESERVED.contains(&self.cur())
    }

    /// Statement end: `;`, or a line break, `}` or end of file before the
    /// next token (automatic semicolon insertion).
    pub(super) fn semi(&mut self) -> PResult<()> {
        if self.eat(";") || self.tok.nl || self.is("}") || self.tok.tok == Tok::Eof {
            Ok(())
        } else {
            self.fail("expected `;`")
        }
    }

    // ---- nodes -------------------------------------------------------------

    pub(super) fn finish(&mut self, kind: Kind, start: u32, kids: Kids) -> u32 {
        self.finish_at(kind, start, self.prev_end, kids)
    }

    pub(super) fn finish_at(&mut self, kind: Kind, start: u32, end: u32, kids: Kids) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(Node { matcher: kind as u32, start, end: end.max(start), children: kids.0 });
        id
    }

    /// A leaf of one token.
    pub(super) fn leaf(&mut self, kind: Kind) -> u32 {
        let t = self.bump();
        self.finish_at(kind, t.start, t.end, Kids::default())
    }

    /// Plain labelled text (an operator, a modifier keyword).
    pub(super) fn text_node(&mut self, start: u32, end: u32) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(Node { matcher: TEXT, start, end, children: Vec::new() });
        id
    }

    /// The current token as labelled text.
    pub(super) fn token_text(&mut self) -> u32 {
        let t = self.bump();
        self.text_node(t.start, t.end)
    }

    pub(super) fn start(&self) -> u32 {
        self.tok.start
    }

    // ---- errors and speculation ----------------------------------------

    pub(super) fn fail<T>(&mut self, msg: &str) -> PResult<T> {
        let at = self.tok.start;
        let found = match self.tok.tok {
            Tok::Eof => "end of file".to_string(),
            _ => format!("`{}`", self.cur().chars().take(20).collect::<String>()),
        };
        self.fail_at(at, &format!("{msg}, found {found}"))
    }

    pub(super) fn fail_at<T>(&mut self, at: u32, msg: &str) -> PResult<T> {
        if self.error.is_none() {
            self.error = Some((at, msg.to_string()));
        }
        Err(Stop)
    }

    fn save(&self) -> Saved<'s> {
        Saved { lx: self.lx.clone(), tok: self.tok, prev_end: self.prev_end, nodes: self.nodes.len(), no_in: self.no_in }
    }

    fn restore(&mut self, s: Saved<'s>) {
        self.lx = s.lx;
        self.tok = s.tok;
        self.prev_end = s.prev_end;
        self.nodes.truncate(s.nodes);
        self.no_in = s.no_in;
        self.error = None;
    }

    /// Runs `f`; on failure puts everything back and returns `None`.
    pub(super) fn try_parse<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> Option<T> {
        let saved = self.save();
        let had_error = self.error.clone();
        match f(self) {
            Ok(v) => Some(v),
            Err(Stop) => {
                self.restore(saved);
                self.error = had_error;
                None
            }
        }
    }

    pub(super) fn enter(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("nesting is too deep");
        }
        Ok(())
    }

    pub(super) fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Re-reads the current `/` or `/=` as a regex.
    pub(super) fn rescan_regex(&mut self) {
        self.tok = self.lx.rescan_regex(self.tok);
    }

    /// Re-reads the current `}` as a template continuation.
    pub(super) fn rescan_template(&mut self) {
        self.tok = self.lx.rescan_template(self.tok);
    }

    /// A `>` run touching the current `>`, as one operator (`>>=`, `>=`).
    pub(super) fn glued_gt(&self) -> &'static str {
        let rest = &self.src[self.tok.start as usize..];
        for op in [">>>=", ">>>", ">>=", ">>", ">="] {
            if rest.starts_with(op) {
                return op;
            }
        }
        ">"
    }

    /// Consumes a glued `>` operator of `len` bytes.
    pub(super) fn bump_glued(&mut self, len: usize) -> Token {
        let start = self.tok.start;
        let nl = self.tok.nl;
        self.lx.pos = start as usize + len;
        self.prev_end = start + len as u32;
        self.tok = self.lx.next_token();
        Token { tok: Tok::P(">"), start, end: start + len as u32, nl }
    }
}

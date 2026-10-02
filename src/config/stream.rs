//! The parser's token source: a stack of frames. Imports push a whole file's
//! tokens, macro invocations push their substituted bodies wrapped in
//! invisible delimiters. Both are spliced in place, so an import really is
//! "paste that file here" and a macro really is "paste this token tree here".

use std::collections::HashMap;
use std::rc::Rc;

use super::error::ConfigError;
use super::lexer::{Tk, Token};
use super::source::Span;

const MAX_MACRO_DEPTH: usize = 64;

pub struct MacroDef {
    pub name: Rc<str>,
    pub params: Vec<Rc<str>>,
    pub body: Rc<[Token]>,
}

struct Frame {
    tokens: Rc<[Token]>,
    pos: usize,
    is_macro: bool,
}

pub struct Stream {
    frames: Vec<Frame>,
    pub macros: HashMap<Rc<str>, Rc<MacroDef>>,
    /// Span of the last consumed token, for "expected X after here" errors.
    pub last_span: Option<Span>,
}

impl Stream {
    pub fn new() -> Self {
        Stream { frames: Vec::new(), macros: HashMap::new(), last_span: None }
    }

    pub fn push_tokens(&mut self, tokens: Vec<Token>) {
        self.frames.push(Frame { tokens: tokens.into(), pos: 0, is_macro: false });
    }

    fn settle(&mut self) {
        while let Some(f) = self.frames.last() {
            if f.pos < f.tokens.len() {
                break;
            }
            self.frames.pop();
        }
    }

    /// Looks `n` tokens ahead without consuming or expanding anything.
    pub fn peek_raw(&mut self, n: usize) -> Option<&Token> {
        self.settle();
        let mut remaining = n;
        for frame in self.frames.iter().rev() {
            let avail = frame.tokens.len() - frame.pos;
            if remaining < avail {
                return Some(&frame.tokens[frame.pos + remaining]);
            }
            remaining -= avail;
        }
        None
    }

    pub fn next_raw(&mut self) -> Option<Token> {
        self.settle();
        let frame = self.frames.last_mut()?;
        let tok = frame.tokens[frame.pos].clone();
        frame.pos += 1;
        self.last_span = Some(tok.span);
        Some(tok)
    }

    pub fn is_macro(&self, name: &str) -> bool {
        self.macros.contains_key(name)
    }

    fn macro_depth(&self) -> usize {
        self.frames.iter().filter(|f| f.is_macro).count()
    }

    /// If the next token invokes a macro, replaces it (and its arguments)
    /// with the expansion and returns true.
    pub fn expand(&mut self, errors: &mut Vec<ConfigError>) -> bool {
        let name = match self.peek_raw(0) {
            Some(tok) if tok.kind == Tk::Ident => tok.text.clone(),
            _ => return false,
        };
        let Some(def) = self.macros.get(&name).cloned() else { return false };
        let call = self.next_raw().expect("peeked");

        let mut args: Vec<Vec<Token>> = Vec::new();
        if !def.params.is_empty() {
            let open = self.peek_raw(0).cloned();
            match open {
                Some(open) if open.kind == Tk::LParen && call.touches(&open) => {
                    self.next_raw();
                    args = self.collect_args(&call, errors);
                }
                _ => {
                    errors.push(
                        ConfigError::new(call.span, format!("macro `{}` needs arguments", def.name))
                            .note(format!("call it like `{}({})`", def.name, join(&def.params))),
                    );
                    return true;
                }
            }
            if args.len() != def.params.len() {
                errors.push(ConfigError::new(
                    call.span,
                    format!("macro `{}` takes {} argument(s) but got {}", def.name, def.params.len(), args.len()),
                ));
                return true;
            }
        }

        if self.macro_depth() >= MAX_MACRO_DEPTH {
            errors.push(
                ConfigError::new(call.span, format!("macro `{}` expands too deeply", def.name))
                    .note("macros cannot recurse; use structural atoms like `paren(...)` for nesting"),
            );
            return true;
        }

        let inv = |kind: Tk| Token { kind, text: Rc::from(""), span: call.span };
        let mut out = Vec::with_capacity(def.body.len() + 2);
        out.push(inv(Tk::InvOpen));
        for t in def.body.iter() {
            match def.params.iter().position(|p| t.kind == Tk::Ident && *p == t.text) {
                Some(i) => {
                    out.push(inv(Tk::InvOpen));
                    out.extend(args[i].iter().cloned());
                    out.push(inv(Tk::InvClose));
                }
                None => out.push(t.clone()),
            }
        }
        out.push(inv(Tk::InvClose));
        self.frames.push(Frame { tokens: out.into(), pos: 0, is_macro: true });
        true
    }

    fn collect_args(&mut self, call: &Token, errors: &mut Vec<ConfigError>) -> Vec<Vec<Token>> {
        let mut args = vec![Vec::new()];
        let mut depth = 0usize;
        loop {
            let Some(t) = self.next_raw() else {
                errors.push(ConfigError::new(call.span, "unclosed macro arguments"));
                break;
            };
            match t.kind {
                Tk::LParen | Tk::LBrace | Tk::LBrack | Tk::InvOpen => depth += 1,
                Tk::RParen if depth == 0 => break,
                Tk::RParen | Tk::RBrace | Tk::RBrack | Tk::InvClose => depth = depth.saturating_sub(1),
                Tk::Comma if depth == 0 => {
                    args.push(Vec::new());
                    continue;
                }
                Tk::Newline => continue,
                _ => {}
            }
            args.last_mut().expect("non-empty").push(t);
        }
        if args.len() == 1 && args[0].is_empty() {
            args.clear();
        }
        args
    }
}

fn join(params: &[Rc<str>]) -> String {
    params.iter().map(|p| &**p).collect::<Vec<_>>().join(", ")
}

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
    /// The name where it is defined.
    pub span: Span,
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
    /// Every macro call: (name, call span), for tooling.
    pub calls: Vec<(Rc<str>, Span)>,
    /// Every token consumed, in order: the configuration as the parser saw
    /// it, imports and macros expanded (see [`Stream::fingerprint`]).
    seen: crate::fx_hash::FxHasher,
    /// The last token seen ended a line (or there was none): line ends in a
    /// row count once, and leading ones not at all.
    at_line_end: bool,
}

impl Stream {
    pub fn new() -> Self {
        Stream {
            frames: Vec::new(),
            macros: HashMap::new(),
            last_span: None,
            calls: Vec::new(),
            seen: Default::default(),
            at_line_end: true,
        }
    }

    /// A hash of every token consumed so far (kinds and texts, no
    /// positions): comments, spacing and blank lines never change it, and
    /// any change to what was parsed does.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::Hasher;
        self.seen.finish()
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
        use std::hash::{Hash, Hasher};
        self.settle();
        let frame = self.frames.last_mut()?;
        let tok = frame.tokens[frame.pos].clone();
        frame.pos += 1;
        self.last_span = Some(tok.span);
        // `;` and a line break mean the same.
        let line_end = tok.kind == Tk::Newline;
        if !(line_end && self.at_line_end) {
            std::mem::discriminant(&tok.kind).hash(&mut self.seen);
            if !line_end {
                // Fx pads its final word with zeros; the length distinguishes
                // token texts ending in NULs and keeps token boundaries intact.
                self.seen.write_usize(tok.text.len());
                self.seen.write(tok.text.as_bytes());
            }
            self.seen.write_u8(0xff);
        }
        self.at_line_end = line_end;
        Some(tok)
    }

    /// `@name` for a defined macro: bare names never expand.
    pub fn is_macro(&self, name: &str) -> bool {
        name.strip_prefix('@').is_some_and(|name| self.macros.contains_key(name))
    }

    fn macro_depth(&self) -> usize {
        self.frames.iter().filter(|f| f.is_macro).count()
    }

    /// If the next token invokes a macro, replaces it (and its arguments)
    /// with the expansion and returns true.
    pub fn expand(&mut self, errors: &mut Vec<ConfigError>) -> bool {
        // `@name` calls a macro; a bare `name` is always just a name.
        let name: Rc<str> = match self.peek_raw(0).and_then(|tok| (tok.kind == Tk::Ident).then(|| tok.text.strip_prefix('@')).flatten()) {
            Some(name) => Rc::from(name),
            None => return false,
        };
        let Some(def) = self.macros.get(&name).cloned() else { return false };
        let call = self.next_raw().expect("peeked");
        self.calls.push((def.name.clone(), call.span));

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
        for (k, t) in def.body.iter().enumerate() {
            // `x.name` reads a key: `name` is not the parameter there.
            let after_dot = k > 0 && def.body[k - 1].kind == Tk::Dot && def.body[k - 1].touches(t);
            match def.params.iter().position(|p| t.kind == Tk::Ident && *p == t.text && !after_dot) {
                Some(i) => {
                    out.push(inv(Tk::InvOpen));
                    out.extend(args[i].iter().cloned());
                    // Ends where the parameter did, so what touches it in
                    // the body still does: `param.key`.
                    out.push(Token { kind: Tk::InvClose, text: Rc::from(""), span: Span { start: t.span.end, ..t.span } });
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
                Tk::Newline if depth == 0 => continue,
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::source::FileId;

    fn fingerprint(tokens: &[(Tk, &str)]) -> u64 {
        let mut stream = Stream::new();
        stream.push_tokens(tokens.iter().enumerate().map(|(i, &(kind, text))| Token {
            kind, text: Rc::from(text), span: Span::new(FileId(0), i, i + 1),
        }).collect());
        while stream.next_raw().is_some() {}
        stream.fingerprint()
    }

    #[test]
    fn fingerprints_distinguish_token_kinds_boundaries_and_trailing_nuls() {
        let variants = [
            vec![(Tk::Ident, "x")],
            vec![(Tk::Ident, "x\0")],
            vec![(Tk::Ident, "x\0\0")],
            vec![(Tk::Str, "x")],
            vec![(Tk::Ident, "a"), (Tk::Ident, "bc")],
            vec![(Tk::Ident, "ab"), (Tk::Ident, "c")],
        ];
        let hashes: std::collections::HashSet<_> = variants.iter().map(|tokens| fingerprint(tokens)).collect();
        assert_eq!(hashes.len(), variants.len());
    }

    #[test]
    fn fingerprints_canonicalise_line_endings_without_losing_token_order() {
        let tokens = [(Tk::Ident, "a"), (Tk::Newline, "\n"), (Tk::Ident, "b")];
        let extra_lines = [
            (Tk::Newline, "\n"), (Tk::Ident, "a"), (Tk::Newline, ";"),
            (Tk::Newline, "\n"), (Tk::Newline, ";"), (Tk::Ident, "b"),
        ];
        assert_eq!(fingerprint(&tokens), fingerprint(&extra_lines));
        assert_ne!(fingerprint(&tokens), fingerprint(&[(Tk::Ident, "b"), (Tk::Newline, "\n"), (Tk::Ident, "a")]));
    }
}

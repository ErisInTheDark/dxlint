//! Matcher patterns: a sequence of items on one line (newlines are allowed
//! inside parentheses), ending at the line's end or at a `{` block. Items:
//! `/regex/`, `"literal"`, matcher names, `label:item`, `:name`, `until`,
//! `( a or b )`, and glued-on `? + *`.

use super::super::ast::{Name, Use};
use super::super::error::ConfigError;
use super::super::lexer::{decode_escapes, Tk, Token};
use super::super::source::Span;
use super::Parser;
use crate::pattern::{regex, Pat};

impl Parser<'_> {
    pub(super) fn field_name(&mut self, token: &Token) -> bool {
        if matches!(&*token.text, "nodes" | "nonempty") {
            self.errors.push(ConfigError::new(token.span, format!("field `{}` is a reserved expression suffix", token.text)));
            false
        } else {
            true
        }
    }

    /// Parses a pattern running to the end of the line.
    pub(super) fn pattern_line(&mut self) -> Option<(Pat, Span)> {
        let start = self.here()?;
        let before = self.errors.len();
        let pat = self.pat_alt(0);
        if self.errors.len() > before {
            return None;
        }
        let end = self.ts.last_span.unwrap_or(start);
        if matches!(pat, Pat::Empty) {
            self.errors.push(ConfigError::new(start, "expected a pattern"));
            return None;
        }
        Some((pat, start.to(end)))
    }

    fn pat_peek(&mut self, depth: usize) -> Option<Token> {
        self.skip(depth > 0, false);
        self.ts.peek_raw(0).cloned()
    }

    fn pat_alt(&mut self, depth: usize) -> Pat {
        let mut branches = vec![self.pat_seq(depth)];
        while self.pat_peek(depth).is_some_and(|t| is_or(&t)) {
            self.ts.next_raw();
            branches.push(self.pat_seq(depth));
        }
        if branches.len() == 1 {
            branches.pop().expect("one")
        } else {
            Pat::Alt(branches)
        }
    }

    fn pat_seq(&mut self, depth: usize) -> Pat {
        let mut items = Vec::new();
        while let Some(t) = self.pat_peek(depth) {
            if is_or(&t) || matches!(t.kind, Tk::Newline | Tk::RParen | Tk::InvClose | Tk::LBrace | Tk::RBrace) {
                break;
            }
            match self.pat_postfix(depth) {
                Some(p) => items.push(p),
                None => {
                    // Skip the rest of the pattern after an error.
                    while let Some(t) = self.pat_peek(depth) {
                        if matches!(t.kind, Tk::Newline | Tk::RParen | Tk::InvClose | Tk::LBrace | Tk::RBrace) {
                            break;
                        }
                        self.ts.next_raw();
                    }
                    break;
                }
            }
        }
        match items.len() {
            0 => Pat::Empty,
            1 => items.pop().expect("one"),
            _ => Pat::Seq(items),
        }
    }

    /// The next raw token, if it is glued to the previous one.
    fn glued(&mut self, kind: Tk) -> bool {
        let last = self.ts.last_span;
        match (last, self.ts.peek_raw(0)) {
            (Some(l), Some(t)) => t.kind == kind && l.file == t.span.file && l.end == t.span.start,
            _ => false,
        }
    }

    fn pat_postfix(&mut self, depth: usize) -> Option<Pat> {
        let t = self.pat_peek(depth)?;

        // `label:item`
        if t.kind == Tk::Ident {
            let label_follows = self.ts.peek_raw(1).is_some_and(|c| c.kind == Tk::Colon && t.touches(c));
            if label_follows {
                if !self.field_name(&t) { return None; }
                if self.ts.is_macro(&t.text) {
                    self.errors.push(ConfigError::new(t.span, format!("label `{}` clashes with a macro of the same name", t.text)));
                    return None;
                }
                self.ts.next_raw();
                self.ts.next_raw();
                let inner = self.pat_postfix(depth)?;
                return Some(label(inner, &t.text));
            }
        }

        let atom = self.pat_atom(depth)?;
        let (min, max) = if self.glued(Tk::Question) {
            (0, Some(1))
        } else if self.glued(Tk::Star) {
            (0, None)
        } else if self.glued(Tk::Plus) {
            (1, None)
        } else {
            return Some(atom);
        };
        let q = self.ts.next_raw().expect("peeked");
        if matches!(atom, Pat::Skip) {
            self.errors.push(ConfigError::new(q.span, "`until` cannot be repeated"));
            return None;
        }
        let greedy = if self.glued(Tk::Question) {
            self.ts.next_raw();
            false
        } else {
            true
        };
        Some(Pat::Repeat { inner: Box::new(atom), min, max, greedy })
    }

    fn pat_atom(&mut self, depth: usize) -> Option<Pat> {
        let t = self.pat_peek(depth)?;
        match t.kind {
            Tk::Regex => {
                self.ts.next_raw();
                match regex::parse(&t.text) {
                    Ok(Pat::Empty) => {
                        self.errors.push(ConfigError::new(t.span, "empty regex"));
                        None
                    }
                    Ok(p) => Some(p),
                    Err(e) => {
                        self.errors.push(ConfigError::new(t.span, format!("invalid regex: {e}")));
                        None
                    }
                }
            }
            Tk::Str => {
                self.ts.next_raw();
                match decode_escapes(&t.text, t.span) {
                    Ok(s) if s.is_empty() => {
                        self.errors.push(ConfigError::new(t.span, "empty literal"));
                        None
                    }
                    Ok(s) => Some(Pat::lit(&s)),
                    Err(e) => {
                        self.errors.push(e);
                        None
                    }
                }
            }
            Tk::Ident if &*t.text == "until" => {
                self.ts.next_raw();
                Some(Pat::Skip)
            }
            Tk::LParen | Tk::InvOpen => {
                self.ts.next_raw();
                let close = if t.kind == Tk::LParen { Tk::RParen } else { Tk::InvClose };
                let inner = self.pat_alt(depth + 1);
                match self.pat_peek(depth + 1) {
                    Some(c) if c.kind == close => {
                        self.ts.next_raw();
                        Some(inner)
                    }
                    other => {
                        let span = other.map(|o| o.span).unwrap_or(t.span);
                        self.errors.push(ConfigError::new(span, "expected `)` to close the group"));
                        None
                    }
                }
            }
            Tk::Colon => {
                self.ts.next_raw();
                match self.ts.peek_raw(0).cloned() {
                    Some(n) if n.kind == Tk::Ident && t.touches(&n) => {
                        if !self.field_name(&n) { return None; }
                        self.ts.next_raw();
                        self.config.uses.push(Use::Layer(Name { text: n.text.to_string(), span: n.span }));
                        Some(Pat::Ref { name: n.text.to_string(), label: Some(n.text.to_string()) })
                    }
                    _ => {
                        self.errors.push(ConfigError::new(t.span, "expected a matcher name right after `:`, like `:params`"));
                        None
                    }
                }
            }
            Tk::Ident => {
                if self.ts.expand(&mut self.errors) {
                    return self.pat_atom(depth);
                }
                self.ts.next_raw();
                self.config.uses.push(Use::Layer(Name { text: t.text.to_string(), span: t.span }));
                Some(Pat::Ref { name: t.text.to_string(), label: None })
            }
            _ => {
                self.errors.push(
                    ConfigError::new(t.span, format!("expected a pattern item, found {}", t.describe()))
                        .note("pattern items: /regex/, \"literal\", matcher names, label:item, :name, until, ( a or b )"),
                );
                None
            }
        }
    }

    /// One pattern item for the right-hand side of `~` in expressions.
    pub(super) fn pattern_item(&mut self) -> Option<Pat> {
        self.pat_postfix(1)
    }
}

/// `or` between pattern branches (not a label `or:`).
fn is_or(t: &Token) -> bool {
    t.is_ident("or")
}

fn label(inner: Pat, name: &str) -> Pat {
    match inner {
        Pat::Ref { name: target, label: None } => Pat::Ref { name: target, label: Some(name.to_string()) },
        other => Pat::Capture { name: name.to_string(), inner: Box::new(other) },
    }
}

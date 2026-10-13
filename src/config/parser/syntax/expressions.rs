//! Structured grammar expressions.

use super::super::Parser;
use crate::config::error::ConfigError;
use crate::config::lexer::{decode_escapes, Tk};
use crate::pattern::syntax::{ContextChange, Expr, TokenMatch};
use crate::pattern::regex;

impl Parser<'_> {
    pub(super) fn syntax_constructor_name(name: &str) -> bool {
        matches!(name, "emit" | "try" | "peek" | "not" | "discard" | "text" | "goal" | "set" |
            "clear" | "if" | "same_text" | "call" | "terminator" | "scope" | "reuse")
    }

    pub(super) fn syntax_expression(&mut self, depth: usize) -> Option<Expr> {
        if depth >= 400 {
            self.error_here("grammar expression nesting limit");
            return None;
        }
        let mut choices = vec![self.syntax_sequence(depth)?];
        while self.syntax_peek(depth).is_some_and(|token| token.kind == Tk::Pipe || token.is_ident("or")) {
            self.ts.next_raw();
            choices.push(self.syntax_sequence(depth)?);
        }
        Some(if choices.len() == 1 { choices.pop().expect("one choice") } else { Expr::Choice(choices) })
    }

    fn syntax_sequence(&mut self, depth: usize) -> Option<Expr> {
        let mut sequence = Vec::new();
        while let Some(token) = self.syntax_peek(depth) {
            if token.is_ident("or") || matches!(token.kind, Tk::Pipe | Tk::Newline | Tk::RParen | Tk::InvClose | Tk::Comma | Tk::LBrace | Tk::RBrace) {
                break;
            }
            sequence.push(self.syntax_postfix(depth, false)?);
        }
        Some(if sequence.len() == 1 { sequence.pop().expect("one item") } else { Expr::Seq(sequence) })
    }

    pub(super) fn syntax_postfix(&mut self, depth: usize, captured: bool) -> Option<Expr> {
        let token = self.syntax_peek(depth)?;
        if token.kind == Tk::Ident && self.ts.peek_raw(1).is_some_and(|next| next.kind == Tk::Colon) {
            if !self.field_name(&token) { return None; }
            self.ts.next_raw();
            self.ts.next_raw();
            let inner = self.syntax_postfix(depth, true)?;
            return Some(Expr::Bind { field: token.text.to_string(), inner: Box::new(inner) });
        }
        let mut expression = self.syntax_atom(depth, captured)?;
        if let Some(suffix) = self.syntax_peek(depth) {
            let repeat = match suffix.kind {
                Tk::Question => Some((0, Some(1))),
                Tk::Star => Some((0, None)),
                Tk::Plus => Some((1, None)),
                _ => None,
            };
            if let Some((min, max)) = repeat {
                self.ts.next_raw();
                expression = Expr::Repeat { inner: Box::new(expression), min, max };
            }
        }
        Some(expression)
    }

    fn syntax_atom(&mut self, depth: usize, captured: bool) -> Option<Expr> {
        let token = self.syntax_peek(depth)?;
        if token.kind == Tk::Ident && self.ts.expand(&mut self.errors) {
            return self.syntax_atom(depth, captured);
        }
        self.ts.next_raw();
        match token.kind {
            Tk::LParen | Tk::InvOpen => {
                let inner = self.syntax_expression(depth + 1)?;
                let closing = if token.kind == Tk::LParen { Tk::RParen } else { Tk::InvClose };
                if self.syntax_peek(depth + 1).is_none_or(|token| token.kind != closing) {
                    self.error_here("expected a closing grammar parenthesis");
                    return None;
                }
                self.ts.next_raw();
                Some(inner)
            }
            Tk::Str => {
                let text = match decode_escapes(&token.text, token.span) {
                    Ok(text) => text,
                    Err(error) => { self.errors.push(error); return None; }
                };
                let inner = Expr::Token(TokenMatch { token: String::new(), text: Some(text), emit: None });
                Some(if captured { inner } else { Expr::Discard(Box::new(inner)) })
            }
            Tk::Regex => match regex::parse(&token.text) {
                Ok(pattern) if !pattern.nullable() => Some(Expr::TokenPattern { pattern, emit: None }),
                Ok(_) => {
                    self.errors.push(ConfigError::new(token.span, "token predicates must consume text"));
                    None
                }
                Err(message) => { self.errors.push(ConfigError::new(token.span, message)); None }
            },
            Tk::Ident => {
                match &*token.text {
                    "end" => return Some(Expr::End),
                    "cut" => return Some(Expr::Cut),
                    "same_line" => return Some(Expr::SameLine),
                    "fail" => return Some(Expr::Choice(Vec::new())),
                    _ => {}
                }
                let constructor = Self::syntax_constructor_name(&token.text) &&
                    self.syntax_peek(depth).is_some_and(|next| next.kind == Tk::LParen &&
                        next.span.file == token.span.file && next.span.start == token.span.end);
                if !constructor {
                    return Some(Expr::Call { production: token.text.to_string(), min_binding_power: 0 });
                }
                self.syntax_constructor(&token.text, depth)
            }
            _ => {
                self.errors.push(ConfigError::new(token.span, "expected a grammar token, production or constructor"));
                None
            }
        }
    }

    fn syntax_constructor(&mut self, name: &str, depth: usize) -> Option<Expr> {
        self.expect(Tk::LParen, "`(` before grammar arguments")?;
        let depth = depth + 1;
        let expression = match name {
            "try" | "peek" | "not" | "discard" | "text" => {
                let inner = Box::new(self.syntax_expression(depth)?);
                match name {
                    "try" => Expr::Try(inner),
                    "peek" => Expr::Look { inner, negative: false },
                    "not" => Expr::Look { inner, negative: true },
                    "text" => Expr::Text(inner),
                    _ => Expr::Discard(inner),
                }
            }
            "emit" => {
                let kind = self.ident("a public node kind")?.text;
                self.expect(Tk::Comma, "`,` before the emitted body")?;
                Expr::Emit { kind, inner: Box::new(self.syntax_expression(depth)?) }
            }
            "goal" | "set" | "clear" => {
                let value = self.ident("a goal or flag name")?.text;
                self.expect(Tk::Comma, "`,` before the scoped body")?;
                let change = match name {
                    "goal" => ContextChange { goal: Some(value), ..ContextChange::default() },
                    "set" => ContextChange { set: vec![value], ..ContextChange::default() },
                    _ => ContextChange { clear: vec![value], ..ContextChange::default() },
                };
                Expr::Context { change, inner: Box::new(self.syntax_expression(depth)?) }
            }
            "if" => {
                let name = self.ident("a flag name")?.text;
                self.expect(Tk::Comma, "`,` before the enabled branch")?;
                let yes = Box::new(self.syntax_expression(depth)?);
                self.expect(Tk::Comma, "`,` before the disabled branch")?;
                let no = Box::new(self.syntax_expression(depth)?);
                Expr::Flag { name, yes, no }
            }
            "same_text" => {
                let left = self.syntax_field_path()?;
                self.expect(Tk::Comma, "`,` between field paths")?;
                let right = self.syntax_field_path()?;
                let ignore = if self.syntax_peek(depth).is_some_and(|token| token.kind == Tk::Comma) {
                    self.bump();
                    Some(self.syntax_pattern()?)
                } else { None };
                Expr::SameText { left, right, ignore }
            }
            "call" => {
                let production = self.ident("a production name")?.text;
                self.expect(Tk::Comma, "`,` before binding power")?;
                let min_binding_power = self.syntax_power()?;
                Expr::Call { production, min_binding_power }
            }
            "scope" => {
                let capture = self.ident("a private capture name")?.text;
                self.expect(Tk::Comma, "`,` before the captured header")?;
                let header = Box::new(self.syntax_expression(depth)?);
                self.expect(Tk::Comma, "`,` before the continuation")?;
                let body = Box::new(self.syntax_expression(depth)?);
                Expr::Scope { capture, header, body }
            }
            "reuse" => Expr::Reuse(self.ident("a private capture name")?.text),
            "terminator" => self.syntax_terminator(depth)?,
            _ => unreachable!(),
        };
        self.expect(Tk::RParen, "`)` after grammar arguments")?;
        Some(expression)
    }

    fn syntax_field_path(&mut self) -> Option<String> {
        let mut path = self.ident("a field path")?.text;
        while self.syntax_peek(1).is_some_and(|token| token.kind == Tk::Dot) {
            self.ts.next_raw();
            path.push('.');
            path.push_str(&self.ident("a field name")?.text);
        }
        Some(path)
    }

    pub(super) fn syntax_power(&mut self) -> Option<u16> {
        let token = self.expect(Tk::Num, "an integer binding power")?;
        match token.text.parse() {
            Ok(power) => Some(power),
            Err(_) => {
                self.errors.push(ConfigError::new(token.span, "binding power must be an integer from 0 to 65535"));
                None
            }
        }
    }

    fn syntax_terminal(&mut self, depth: usize) -> Option<TokenMatch> {
        match self.syntax_atom(depth, true)? {
            Expr::Token(token) => Some(token),
            Expr::Call { production, min_binding_power: 0 } => Some(TokenMatch { token: production, text: None, emit: None }),
            _ => { self.error_here("expected a named token or literal"); None }
        }
    }

    fn syntax_terminator(&mut self, depth: usize) -> Option<Expr> {
        let explicit = self.syntax_terminal(depth)?;
        let mut before = Vec::new();
        let mut newline = false;
        let mut eof = false;
        while self.syntax_peek(depth).is_some_and(|token| token.kind == Tk::Comma) {
            self.ts.next_raw();
            let option = self.ident("a terminator condition")?;
            match option.text.as_str() {
                "newline" => newline = true,
                "eof" => eof = true,
                "before" => {
                    self.expect(Tk::LParen, "`(` before boundary tokens")?;
                    while self.syntax_peek(depth + 1).is_some_and(|token| token.kind != Tk::RParen) {
                        before.push(self.syntax_terminal(depth + 1)?);
                        if self.syntax_peek(depth + 1).is_some_and(|token| token.kind == Tk::Comma) {
                            self.ts.next_raw();
                        } else { break; }
                    }
                    self.expect(Tk::RParen, "`)` after boundary tokens")?;
                }
                _ => {
                    self.errors.push(ConfigError::new(option.span, "terminator conditions are before(...), newline and eof"));
                    return None;
                }
            }
        }
        Some(Expr::Terminator { explicit, before, newline, eof })
    }
}

//! Declared precedence rows.

use super::super::Parser;
use crate::config::error::ConfigError;
use crate::config::lexer::Tk;
use crate::pattern::syntax::{Expr, Operator, Precedence, Prefix};

impl Parser<'_> {
    pub(super) fn syntax_precedence(&mut self) -> Option<Expr> {
        let atom = Box::new(self.syntax_postfix(0, false)?);
        self.expect(Tk::LBrace, "`{` before precedence rows")?;
        let mut prefix = Vec::new();
        let mut rows = Vec::new();
        while let Some(token) = self.peek() {
            if token.kind == Tk::RBrace { self.bump(); break; }
            let form = self.ident("prefix, left, right or suffix")?;
            let power = self.syntax_power()?;
            let operator = self.syntax_postfix(0, true)?;
            let (operator, capture) = match operator {
                Expr::Bind { field, inner } => (*inner, Some(field)),
                operator => (operator, None),
            };
            // `=>` already belongs to the shared lexer; no new punctuation.
            self.expect(Tk::Arrow, "`=>` before an operator node")?;
            let kind = self.ident("an operator node kind")?.text;
            self.expect(Tk::LParen, "`(` before operand fields")?;
            let mut fields = Vec::new();
            while self.peek().is_some_and(|token| token.kind != Tk::RParen) {
                fields.push(self.ident("an operand or operator field")?.text);
                if self.peek().is_some_and(|token| token.kind == Tk::Comma) { self.bump(); }
                else { break; }
            }
            self.expect(Tk::RParen, "`)` after operand fields")?;
            let flatten = self.syntax_peek(0).is_some_and(|token| token.is_ident("flatten"));
            if flatten { self.bump(); }
            match form.text.as_str() {
                "prefix" if !flatten && matches!(fields.len(), 1 | 2) => {
                    let operator_field = if fields.len() == 2 { Some(fields[0].clone()) } else { capture.clone() };
                    prefix.push(Prefix {
                        operator, binding_power: power, kind, operator_field,
                        argument: fields.last().expect("argument").clone(),
                    });
                }
                "left" | "right" if matches!(fields.len(), 2 | 3) => {
                    let operator_field = if fields.len() == 3 { Some(fields[1].clone()) } else { capture.clone() };
                    rows.push(Operator {
                        operator, binding_power: power, right_associative: form.text == "right",
                        operand: true, kind, left: fields[0].clone(), operator_field,
                        right: Some(fields.last().expect("right").clone()),
                        flatten,
                    });
                }
                "suffix" if matches!(fields.len(), 1 | 2) => {
                    let operator_field = if fields.len() == 2 { Some(fields[1].clone()) } else { capture.clone() };
                    rows.push(Operator {
                        operator, binding_power: power, right_associative: false,
                        operand: false, kind, left: fields[0].clone(), operator_field, right: None,
                        flatten,
                    });
                }
                _ => {
                    self.errors.push(ConfigError::new(form.span, "prefix/suffix rows need one or two fields; left/right rows need two or three"));
                    return None;
                }
            }
            self.end_of_item();
        }
        Some(Expr::Pratt(Precedence { atom, prefix, rows }))
    }
}

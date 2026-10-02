//! Expressions used by rules: messages, `when`, `fix`, options.
//! Strings are templates: `"{name}: {snake_case(type)}"`.

use std::rc::Rc;

use super::super::ast::{BinOp, Expr, ExprKind, Name, TemplatePart, UnOp};
use super::super::error::ConfigError;
use super::super::lexer::{decode_escapes, lex, Tk, Token};
use super::super::source::Span;
use super::Parser;

impl Parser<'_> {
    pub(super) fn expr(&mut self) -> Option<Expr> {
        let t = self.operand_peek()?;
        if t.is_ident("if") {
            self.ts.next_raw();
            let cond = self.expr()?;
            self.expect_kw("then")?;
            let then = self.expr()?;
            self.expect_kw("else")?;
            let other = self.expr()?;
            let span = t.span.to(other.span);
            return Some(Expr { kind: ExprKind::If(Box::new(cond), Box::new(then), Box::new(other)), span });
        }
        self.e_or()
    }

    fn expect_kw(&mut self, kw: &str) -> Option<()> {
        match self.operand_peek() {
            Some(t) if t.is_ident(kw) => {
                self.ts.next_raw();
                Some(())
            }
            other => {
                let span = other.map(|t| t.span).or(self.ts.last_span);
                self.errors.push(ConfigError::new(span, format!("expected `{kw}`")));
                None
            }
        }
    }

    /// Where an operand is expected, line breaks are insignificant.
    fn operand_peek(&mut self) -> Option<Token> {
        self.skip(true, false);
        self.ts.peek_raw(0).cloned()
    }

    /// Binary operators only continue across lines inside brackets.
    fn op_peek(&mut self) -> Option<Tk> {
        self.skip(self.expr_depth > 0, false);
        self.ts.peek_raw(0).map(|t| t.kind)
    }

    fn binary(&mut self, ops: &[(Tk, BinOp)], next: fn(&mut Self) -> Option<Expr>) -> Option<Expr> {
        let mut lhs = next(self)?;
        while let Some(op) = self.op_peek().and_then(|k| ops.iter().find(|(t, _)| *t == k).map(|(_, op)| *op)) {
            self.ts.next_raw();
            let rhs = next(self)?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span };
        }
        Some(lhs)
    }

    fn e_or(&mut self) -> Option<Expr> {
        self.binary(&[(Tk::OrOr, BinOp::Or)], Self::e_and)
    }

    fn e_and(&mut self) -> Option<Expr> {
        self.binary(&[(Tk::AndAnd, BinOp::And)], Self::e_cmp)
    }

    fn e_cmp(&mut self) -> Option<Expr> {
        let lhs = self.e_add()?;
        let op = match self.op_peek() {
            Some(Tk::EqEq) => BinOp::Eq,
            Some(Tk::Ne) => BinOp::Ne,
            Some(Tk::Lt) => BinOp::Lt,
            Some(Tk::Le) => BinOp::Le,
            Some(Tk::Gt) => BinOp::Gt,
            Some(Tk::Ge) => BinOp::Ge,
            Some(Tk::Tilde) => {
                let tilde = self.ts.next_raw().expect("peeked");
                self.expr_depth += 1;
                let pat = self.pattern_item();
                self.expr_depth -= 1;
                let pat = pat?;
                let span = lhs.span.to(self.ts.last_span.unwrap_or(tilde.span));
                return Some(Expr { kind: ExprKind::Test(Box::new(lhs), Box::new(pat)), span });
            }
            _ => return Some(lhs),
        };
        self.ts.next_raw();
        let rhs = self.e_add()?;
        let span = lhs.span.to(rhs.span);
        Some(Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span })
    }

    fn e_add(&mut self) -> Option<Expr> {
        self.binary(&[(Tk::Plus, BinOp::Add), (Tk::Minus, BinOp::Sub)], Self::e_mul)
    }

    fn e_mul(&mut self) -> Option<Expr> {
        self.binary(&[(Tk::Star, BinOp::Mul), (Tk::Percent, BinOp::Rem)], Self::e_unary)
    }

    fn e_unary(&mut self) -> Option<Expr> {
        let t = self.operand_peek()?;
        let op = match t.kind {
            Tk::Bang => UnOp::Not,
            Tk::Minus => UnOp::Neg,
            _ => return self.e_primary(),
        };
        self.ts.next_raw();
        let inner = self.e_unary()?;
        let span = t.span.to(inner.span);
        Some(Expr { kind: ExprKind::Unary(op, Box::new(inner)), span })
    }

    fn e_primary(&mut self) -> Option<Expr> {
        let Some(t) = self.operand_peek() else {
            self.error_here("expected an expression, found end of file");
            return None;
        };
        let simple = |kind| Some(Expr { kind, span: t.span });
        match t.kind {
            Tk::Num => {
                self.ts.next_raw();
                simple(ExprKind::Num(t.text.parse().unwrap_or(0.0)))
            }
            Tk::Str => {
                self.ts.next_raw();
                self.template(&t)
            }
            // `/.../` in an expression is the regex source as text, for the
            // `re_*` functions: no string escaping needed.
            Tk::Regex => {
                self.ts.next_raw();
                simple(ExprKind::Str(t.text.to_string()))
            }
            Tk::LBrack => {
                self.ts.next_raw();
                let items = self.comma_list(Tk::RBrack, "`]`")?;
                let span = t.span.to(self.ts.last_span.unwrap_or(t.span));
                Some(Expr { kind: ExprKind::List(items), span })
            }
            Tk::LParen | Tk::InvOpen => {
                self.ts.next_raw();
                let close = if t.kind == Tk::LParen { Tk::RParen } else { Tk::InvClose };
                self.expr_depth += 1;
                let inner = self.expr();
                self.skip(true, false);
                let closed = self.ts.peek_raw(0).is_some_and(|c| c.kind == close);
                self.expr_depth -= 1;
                let inner = inner?;
                if !closed {
                    self.error_here("expected `)`");
                    return None;
                }
                self.ts.next_raw();
                Some(inner)
            }
            Tk::Ident => match &*t.text {
                "true" | "false" => {
                    self.ts.next_raw();
                    simple(ExprKind::Bool(&*t.text == "true"))
                }
                "null" => {
                    self.ts.next_raw();
                    simple(ExprKind::Null)
                }
                _ if self.ts.is_macro(&t.text) => {
                    self.ts.expand(&mut self.errors);
                    self.e_primary()
                }
                _ => self.path_or_call(t),
            },
            _ => {
                self.errors.push(ConfigError::new(t.span, format!("expected an expression, found {}", t.describe())));
                None
            }
        }
    }

    fn comma_list(&mut self, close: Tk, what: &str) -> Option<Vec<Expr>> {
        self.expr_depth += 1;
        let mut items = Vec::new();
        let result = loop {
            self.skip(true, false);
            if self.ts.peek_raw(0).is_some_and(|c| c.kind == close) {
                self.ts.next_raw();
                break Some(items);
            }
            match self.expr() {
                Some(e) => items.push(e),
                None => break None,
            }
            self.skip(true, false);
            match self.ts.peek_raw(0).map(|c| c.kind) {
                Some(Tk::Comma) => {
                    self.ts.next_raw();
                }
                Some(k) if k == close => {}
                _ => {
                    self.error_here(format!("expected `,` or {what}"));
                    break None;
                }
            }
        };
        self.expr_depth -= 1;
        result
    }

    fn path_or_call(&mut self, first: Token) -> Option<Expr> {
        self.ts.next_raw();
        let mut path = vec![Name { text: first.text.to_string(), span: first.span }];
        let mut last = first.clone();
        loop {
            let dot = self.ts.peek_raw(0).cloned();
            let field = self.ts.peek_raw(1).cloned();
            match (dot, field) {
                (Some(d), Some(f)) if d.kind == Tk::Dot && f.kind == Tk::Ident && last.touches(&d) && d.touches(&f) => {
                    self.ts.next_raw();
                    self.ts.next_raw();
                    path.push(Name { text: f.text.to_string(), span: f.span });
                    last = f;
                }
                _ => break,
            }
        }
        let span = first.span.to(last.span);
        let paren = self.ts.peek_raw(0).cloned();
        if let Some(p) = paren.filter(|p| p.kind == Tk::LParen && last.touches(p)) {
            if path.len() > 1 {
                self.errors.push(ConfigError::new(span, "only plain function names can be called"));
                return None;
            }
            self.ts.next_raw();
            let args = self.comma_list(Tk::RParen, "`)`")?;
            let span = span.to(self.ts.last_span.unwrap_or(p.span));
            return Some(Expr { kind: ExprKind::Call(path.pop().expect("one"), args), span });
        }
        Some(Expr { kind: ExprKind::Var(path), span })
    }

    /// Splits a string literal into literal text and `{expr}` holes.
    fn template(&mut self, tok: &Token) -> Option<Expr> {
        let raw: &str = &tok.text;
        if !raw.contains('{') && !raw.contains('}') {
            return match decode_escapes(raw, tok.span) {
                Ok(s) => Some(Expr { kind: ExprKind::Str(s), span: tok.span }),
                Err(e) => {
                    self.errors.push(e);
                    None
                }
            };
        }
        let base = tok.span.start as usize + 1;
        let bytes = raw.as_bytes();
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut i = 0;
        let mut seg_start = 0;
        let flush = |lit: &mut String, parts: &mut Vec<TemplatePart>| {
            if !lit.is_empty() {
                parts.push(TemplatePart::Lit(std::mem::take(lit)));
            }
        };
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'{' if bytes.get(i + 1) == Some(&b'{') => {
                    lit.push_str(&self.decode_piece(&raw[seg_start..i], tok.span)?);
                    lit.push('{');
                    i += 2;
                    seg_start = i;
                }
                b'}' if bytes.get(i + 1) == Some(&b'}') => {
                    lit.push_str(&self.decode_piece(&raw[seg_start..i], tok.span)?);
                    lit.push('}');
                    i += 2;
                    seg_start = i;
                }
                b'{' => {
                    lit.push_str(&self.decode_piece(&raw[seg_start..i], tok.span)?);
                    let Some(close) = find_hole_end(bytes, i + 1) else {
                        self.errors.push(
                            ConfigError::new(tok.span, "unclosed `{` in string").note("write `{{` for a literal brace"),
                        );
                        return None;
                    };
                    flush(&mut lit, &mut parts);
                    let hole = &raw[i + 1..close];
                    let span = Span::new(tok.span.file, base + i, base + close + 1);
                    parts.push(TemplatePart::Expr(self.hole(hole, base + i + 1, span)?, hole.trim().to_string()));
                    i = close + 1;
                    seg_start = i;
                }
                b'}' => {
                    self.errors.push(ConfigError::new(tok.span, "unmatched `}` in string").note("write `}}` for a literal brace"));
                    return None;
                }
                _ => i += 1,
            }
        }
        lit.push_str(&self.decode_piece(&raw[seg_start..], tok.span)?);
        flush(&mut lit, &mut parts);
        Some(Expr { kind: ExprKind::Template(parts), span: tok.span })
    }

    fn decode_piece(&mut self, raw: &str, span: Span) -> Option<String> {
        match decode_escapes(raw, span) {
            Ok(s) => Some(s),
            Err(e) => {
                self.errors.push(e);
                None
            }
        }
    }

    /// Parses `{...}` content by splicing its tokens into the stream as an
    /// invisible group, so holes share macros with the rest of the file.
    fn hole(&mut self, text: &str, base: usize, span: Span) -> Option<Expr> {
        let mut tokens = lex(span.file, text, base, &mut self.errors);
        tokens.retain(|t| t.kind != Tk::Newline);
        if tokens.is_empty() {
            self.errors.push(ConfigError::new(span, "empty `{}` in string"));
            return None;
        }
        // The closing marker gets a unique zero-width span so we can tell
        // whether parsing consumed the whole hole.
        let close_span = Span { start: span.end, ..span };
        tokens.insert(0, Token { kind: Tk::InvOpen, text: Rc::from(""), span });
        tokens.push(Token { kind: Tk::InvClose, text: Rc::from(""), span: close_span });
        self.ts.push_tokens(tokens);
        let expr = self.e_primary();
        if expr.is_none() || self.ts.last_span != Some(close_span) {
            // Drain whatever is left of the hole so parsing can continue.
            while self.ts.last_span != Some(close_span) {
                if self.ts.next_raw().is_none() {
                    break;
                }
            }
            if expr.is_some() {
                self.errors.push(ConfigError::new(span, "unexpected tokens in `{...}`"));
            }
            return None;
        }
        expr
    }
}

/// Finds the `}` closing a template hole, skipping nested braces and strings.
fn find_hole_end(bytes: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' if depth == 0 => return Some(i),
            b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

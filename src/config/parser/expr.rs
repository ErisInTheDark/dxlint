//! Expressions used by rules: messages, `when`, `fix`, options.
//! Strings are templates: `"{name}: {snake_case(type)}"`.

use std::rc::Rc;

use super::super::ast::{BinOp, Expr, ExprKind, IsName, Name, PipelineStage, PipelineStep, TemplatePart, UnOp};
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
        let lhs = self.e_or()?;
        if self.op_peek() == Some(Tk::Nullish) {
            self.spaced_op();
            let rhs = self.expr()?;
            let span = lhs.span.to(rhs.span);
            return Some(Expr { kind: ExprKind::Nullish(Box::new(lhs), Box::new(rhs)), span });
        }
        Some(lhs)
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

    /// Binary operators continue across lines inside brackets, or when the
    /// next line starts with one (`&&`, `||`, a comparison): no item or
    /// clause can start that way.
    fn op_peek(&mut self) -> Option<Tk> {
        let continues = self.expr_depth > 0 || self.next_line_continues();
        self.skip(continues, false);
        self.ts.peek_raw(0).map(|t| t.kind)
    }

    fn next_line_continues(&mut self) -> bool {
        let mut i = 0;
        let mut crossed = false;
        while let Some(t) = self.ts.peek_raw(i) {
            match t.kind {
                Tk::Newline => crossed = true,
                Tk::InvOpen | Tk::InvClose => {}
                kind => {
                    return crossed
                        && (matches!(kind, Tk::AndAnd | Tk::OrOr | Tk::Nullish | Tk::EqEq | Tk::Ne | Tk::Lt | Tk::Le | Tk::Gt | Tk::Ge | Tk::Tilde)
                            || t.is_ident("is")
                            || t.is_ident("not"))
                }
            }
            i += 1;
        }
        false
    }

    /// Consumes a binary operator, which must stand apart from both
    /// operands: `a - 1`, not `a-1` or `a -1`.
    fn spaced_op(&mut self) -> Token {
        let before = self.ts.last_span;
        let op = self.ts.next_raw().expect("peeked");
        let touches_before = before.is_some_and(|b| b.file == op.span.file && b.end == op.span.start);
        let touches_after = self.ts.peek_raw(0).is_some_and(|next| op.touches(next));
        if touches_before || touches_after {
            self.errors.push(ConfigError::new(op.span, format!("put spaces around `{}`", op.text)));
        }
        op
    }

    fn binary(&mut self, ops: &[(Tk, BinOp)], next: fn(&mut Self) -> Option<Expr>) -> Option<Expr> {
        let mut lhs = next(self)?;
        while let Some(op) = self.op_peek().and_then(|k| ops.iter().find(|(t, _)| *t == k).map(|(_, op)| *op)) {
            self.spaced_op();
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
        // `is a`, `not a` alone: about the node itself.
        if let Some(t) = self.ts.peek_raw(0).filter(|t| t.is_ident("is") || t.is_ident("not")).cloned() {
            let lhs = Expr { kind: ExprKind::Var(vec![Name { text: "node".into(), span: t.span }]), span: t.span };
            return self.is_test(lhs);
        }
        let lhs = self.e_join()?;
        let next = self.op_peek();
        if next == Some(Tk::Ident) && self.ts.peek_raw(0).is_some_and(|t| t.is_ident("is") || t.is_ident("not")) {
            return self.is_test(lhs);
        }
        let op = match next {
            Some(Tk::EqEq) => BinOp::Eq,
            Some(Tk::Ne) => BinOp::Ne,
            Some(Tk::Lt) => BinOp::Lt,
            Some(Tk::Le) => BinOp::Le,
            Some(Tk::Gt) => BinOp::Gt,
            Some(Tk::Ge) => BinOp::Ge,
            Some(Tk::Tilde) => {
                let tilde = self.spaced_op();
                self.expr_depth += 1;
                let pat = self.pattern_item();
                self.expr_depth -= 1;
                let pat = pat?;
                let span = lhs.span.to(self.ts.last_span.unwrap_or(tilde.span));
                return Some(Expr { kind: ExprKind::Test(Box::new(lhs), Box::new(pat)), span });
            }
            _ => return Some(lhs),
        };
        self.spaced_op();
        let rhs = self.e_join()?;
        let span = lhs.span.to(rhs.span);
        Some(Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span })
    }

    /// One `is` alternative starting at token `t`: `name`, `name.field`,
    /// `layer name` or `.lang`.
    fn is_name(&mut self, t: Token) -> Option<IsName> {
        let glued = |p: &mut Self, prev: &Token| p.ts.peek_raw(0).is_some_and(|n| prev.touches(n));
        if t.kind == Tk::Dot {
            return match self.ts.next_raw() {
                Some(l) if l.kind == Tk::Ident && t.touches(&l) => Some(IsName::Lang(Name { text: l.text.to_string(), span: t.span.to(l.span) })),
                _ => {
                    self.errors.push(ConfigError::new(t.span, "expected a language right after `.`, like `.rust`"));
                    None
                }
            };
        }
        let name = Name { text: t.text.to_string(), span: t.span };
        if name.text == "layer" && self.ts.peek_raw(0).is_some_and(|n| n.kind == Tk::Ident) {
            let l = self.ts.next_raw().expect("peeked");
            return Some(IsName::Layer(Name { text: l.text.to_string(), span: l.span }));
        }
        if glued(self, &t) && self.ts.peek_raw(0).is_some_and(|d| d.kind == Tk::Dot) {
            let dot = self.ts.next_raw().expect("peeked");
            return match self.ts.next_raw() {
                Some(f) if f.kind == Tk::Ident && dot.touches(&f) => Some(IsName::Field(name, Name { text: f.text.to_string(), span: f.span })),
                _ => {
                    self.errors.push(ConfigError::new(dot.span, "expected a field name right after `.`, like `ts_if.test`"));
                    None
                }
            };
        }
        Some(IsName::Kind(name))
    }

    /// `value is name`, `value is (a, b.field, layer l, .lang)`, or the same
    /// with `not`, after `value`. Parentheses with commas make a group in
    /// place.
    fn is_test(&mut self, lhs: Expr) -> Option<Expr> {
        let kw = self.ts.next_raw().expect("peeked");
        self.skip(false, true);
        let mut names = Vec::new();
        match self.ts.peek_raw(0).map(|t| t.kind) {
            Some(Tk::LParen) => {
                self.ts.next_raw();
                loop {
                    self.skip(true, true);
                    match self.ts.next_raw() {
                        Some(t) if t.kind == Tk::RParen => break,
                        Some(t) if t.kind == Tk::Comma => {}
                        Some(t) if matches!(t.kind, Tk::Ident | Tk::Dot) => names.push(self.is_name(t)?),
                        other => {
                            let span = other.map(|t| t.span).or(self.ts.last_span);
                            self.errors.push(ConfigError::new(span, format!("expected layer or group names in `{} (a, b)`", kw.text)));
                            return None;
                        }
                    }
                }
            }
            Some(Tk::LBrack) => {
                let span = self.ts.peek_raw(0).map(|t| t.span);
                self.errors.push(ConfigError::new(span, format!("several layers go in parentheses: `{} (a, b)`", kw.text)));
                return None;
            }
            _ => match self.ts.next_raw() {
                Some(t) if matches!(t.kind, Tk::Ident | Tk::Dot) => names.push(self.is_name(t)?),
                other => {
                    let span = other.map(|t| t.span).or(Some(kw.span));
                    self.errors.push(
                        ConfigError::new(span, format!("expected a layer or group name after `{}`", kw.text))
                            .note(format!("several go in parentheses: `layer {} (a, b)`", kw.text)),
                    );
                    return None;
                }
            },
        }
        if names.is_empty() {
            self.errors.push(ConfigError::new(kw.span, format!("`{} ()` names no layers", kw.text)));
            return None;
        }
        let span = lhs.span.to(self.ts.last_span.unwrap_or(kw.span));
        let test = Expr { kind: ExprKind::Is(Box::new(lhs), names), span };
        Some(if kw.is_ident("not") { Expr { kind: ExprKind::Unary(UnOp::Not, Box::new(test)), span } } else { test })
    }

    /// Values side by side are joined: `text "="`, `[a] [b]`.
    fn e_join(&mut self) -> Option<Expr> {
        let mut lhs = self.e_add()?;
        while self.joins_next() {
            let rhs = self.e_add()?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr { kind: ExprKind::Binary(BinOp::Join, Box::new(lhs), Box::new(rhs)), span };
        }
        Some(lhs)
    }

    /// Does another joined value follow? The words that may follow an
    /// expression (`then`, `else`, `fix`), a rule-clause `{` and the end of the line
    /// (outside brackets) end it.
    fn joins_next(&mut self) -> bool {
        self.skip(self.expr_depth > 0, false);
        let Some(kind) = self.ts.peek_raw(0).map(|token| token.kind) else { return false };
        if kind == Tk::LBrace { return self.pipeline_marker_at(1); }
        self.ts.peek_raw(0).is_some_and(|t| match t.kind {
            Tk::Str | Tk::Num | Tk::Regex | Tk::LBrack | Tk::LParen | Tk::InvOpen => true,
            // `set` ends a rule line's message: `rule x error "..." set never`.
            Tk::Ident => !matches!(&*t.text, "then" | "else" | "fix" | "as" | "if" | "is" | "not" | "by" | "from" | "class" | "set"),
            _ => false,
        })
    }

    fn e_add(&mut self) -> Option<Expr> {
        let sum = self.binary(&[(Tk::Plus, BinOp::Add), (Tk::Minus, BinOp::Sub)], Self::e_mul)?;
        self.only_numbers_add(&sum);
        Some(sum)
    }

    fn only_numbers_add(&mut self, e: &Expr) {
        if let ExprKind::Binary(BinOp::Add, a, b) = &e.kind {
            for side in [a, b] {
                if matches!(side.kind, ExprKind::Str(_) | ExprKind::Template(_) | ExprKind::List(_)) {
                    self.errors.push(
                        ConfigError::new(side.span, "`+` adds numbers").note("put values side by side to join them: `text \"!\"`"),
                    );
                }
            }
            self.only_numbers_add(a);
        }
    }

    /// One value without operators or joins (`100`, `"x"`, `-1`, `(a b)`).
    pub(super) fn operand(&mut self) -> Option<Expr> {
        self.e_unary()
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

    /// A primary, then `.key` reads into what it gives (`find(xs, x => ...).name`).
    fn e_primary(&mut self) -> Option<Expr> {
        let mut e = self.e_atom()?;
        loop {
            let dot = self.ts.peek_raw(0).cloned();
            let key = self.ts.peek_raw(1).cloned();
            match (dot, key, self.ts.last_span) {
                (Some(d), Some(k), Some(last)) if d.kind == Tk::Dot && k.kind == Tk::Ident && last.end == d.span.start && d.touches(&k) => {
                    self.ts.next_raw();
                    self.ts.next_raw();
                    let span = e.span.to(k.span);
                    e = Expr { kind: ExprKind::Key(Box::new(e), Name { text: k.text.to_string(), span: k.span }), span };
                }
                _ => break,
            }
        }
        Some(e)
    }

    fn e_atom(&mut self) -> Option<Expr> {
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
            Tk::LBrace => {
                self.ts.next_raw();
                self.skip(true, false);
                if self.pipeline_marker_at(0) {
                    self.pipeline(t)
                } else {
                    self.record(t)
                }
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

    fn pipeline(&mut self, open: Token) -> Option<Expr> {
        // A pipeline has its own line boundaries even inside call brackets.
        let depth = std::mem::replace(&mut self.expr_depth, 0);
        let parsed = self.pipeline_steps(&open);
        self.expr_depth = depth;
        parsed.map(|steps| Expr {
            kind: ExprKind::Pipeline(steps),
            span: open.span.to(self.ts.last_span.unwrap_or(open.span)),
        })
    }

    fn pipeline_label(&mut self) -> Option<Name> {
        let name = self.ts.peek_raw(0)?.clone();
        if name.kind != Tk::Ident || !self.ts.peek_raw(1).is_some_and(|next| next.kind == Tk::Colon) {
            return None;
        }
        self.ts.next_raw();
        self.ts.next_raw();
        Some(Name { text: name.text.to_string(), span: name.span })
    }

    fn stage_params(&mut self) -> Option<Vec<Name>> {
        self.expect(Tk::LParen, "lambda parameters `(item)` or `(item, index)`")?;
        let mut params = Vec::new();
        loop {
            self.skip(false, false);
            match self.ts.next_raw() {
                Some(token) if token.kind == Tk::RParen => return Some(params),
                Some(token) if token.kind == Tk::Ident => {
                    params.push(Name { text: token.text.to_string(), span: token.span });
                    match self.ts.peek_raw(0).map(|token| token.kind) {
                        Some(Tk::Comma) => { self.ts.next_raw(); }
                        Some(Tk::RParen) => {}
                        _ => { self.error_here("expected `,` or `)` in stage parameters"); return None; }
                    }
                }
                other => {
                    self.errors.push(ConfigError::new(other.map(|token| token.span), "expected stage parameter names"));
                    return None;
                }
            }
        }
    }

    fn pipeline_steps(&mut self, open: &Token) -> Option<Vec<PipelineStep>> {
        let mut steps = Vec::new();
        let mut labels = std::collections::HashSet::new();
        loop {
            self.skip(true, false);
            let Some(token) = self.ts.peek_raw(0).cloned() else {
                self.errors.push(ConfigError::new(open.span, "unclosed pipeline"));
                return None;
            };
            if token.kind == Tk::RBrace {
                self.ts.next_raw();
                break;
            }
            let mut label = self.pipeline_label();
            let name = self.ts.next_raw()?;
            if name.kind != Tk::Ident {
                self.errors.push(ConfigError::new(name.span, "expected `value` or a stage name in a pipeline"));
                return None;
            }
            let stage = if name.is_ident("value") {
                if label.is_none() { label = self.pipeline_label(); }
                PipelineStage::Value(self.expr()?)
            } else if name.is_ident("then") {
                let params = self.stage_params()?;
                if params.len() != 1 {
                    self.errors.push(ConfigError::new(name.span, "`then` takes one parameter: `then (value) expression`"));
                    return None;
                }
                PipelineStage::Then(params.into_iter().next().expect("one"), self.expr()?)
            } else {
                let function = Name { text: name.text.to_string(), span: name.span };
                let mut args = Vec::new();
                if crate::expr::Each::named(&name.text).is_some() || name.is_ident("walk") {
                    if crate::expr::Each::named(&name.text).is_some_and(|(_, kind)| matches!(kind, crate::expr::Each::Fold | crate::expr::Each::Zip)) {
                        args.push(self.operand()?);
                    }
                    let paired_only = name.is_ident("zip")
                        && self.ts.peek_raw(0).is_none_or(|token| matches!(token.kind, Tk::Newline | Tk::RBrace));
                    if !paired_only {
                        let params = self.stage_params()?;
                        let body = self.expr()?;
                        let span = name.span.to(body.span);
                        args.push(Expr { kind: ExprKind::Lambda(params, Box::new(body)), span });
                    }
                } else {
                    while self.ts.peek_raw(0).is_some_and(|token| !matches!(token.kind, Tk::Newline | Tk::RBrace)) {
                        args.push(self.operand()?);
                    }
                }
                self.call_arguments(&args);
                PipelineStage::Call(function, args)
            };
            if steps.is_empty() && !matches!(stage, PipelineStage::Value(_)) {
                self.errors.push(ConfigError::new(open.span, "a pipeline starts with `value expression`"));
                return None;
            }
            if let Some(label) = &label {
                if !labels.insert(label.text.clone()) {
                    self.errors.push(ConfigError::new(label.span, format!("duplicate pipeline label `{}`", label.text)));
                    return None;
                }
            }
            let span = token.span.to(self.ts.last_span.unwrap_or(name.span));
            steps.push(PipelineStep { label, stage, span });
            match self.ts.peek_raw(0).map(|token| token.kind) {
                Some(Tk::Newline | Tk::RBrace) => {}
                None => { self.errors.push(ConfigError::new(open.span, "unclosed pipeline")); return None; }
                _ => { self.error_here("pipeline steps are separated by a newline or `;`"); return None; }
            }
        }
        if steps.is_empty() {
            self.errors.push(ConfigError::new(open.span, "a pipeline needs `value expression`"));
            return None;
        }
        Some(steps)
    }

    /// `{ key: value, ... }` after its `{`. Keys are names or strings.
    fn record(&mut self, open: Token) -> Option<Expr> {
        self.expr_depth += 1;
        let mut entries: Vec<(Name, Expr)> = Vec::new();
        let result = loop {
            self.skip(true, false);
            let key = match self.ts.next_raw() {
                Some(t) if t.kind == Tk::RBrace => break Some(()),
                Some(t) if t.kind == Tk::Ident => Name { text: t.text.to_string(), span: t.span },
                Some(t) if t.kind == Tk::Str => match decode_escapes(&t.text, t.span) {
                    Ok(text) => Name { text, span: t.span },
                    Err(e) => {
                        self.errors.push(e);
                        break None;
                    }
                },
                other => {
                    let span = other.map(|t| t.span).or(self.ts.last_span);
                    self.errors.push(ConfigError::new(span, "expected a key (`name:`) or `}` in a record"));
                    break None;
                }
            };
            if entries.iter().any(|(k, _)| k.text == key.text) {
                self.errors.push(ConfigError::new(key.span, format!("`{}` appears twice in this record", key.text)));
                break None;
            }
            self.skip(true, false);
            if !self.ts.peek_raw(0).is_some_and(|c| c.kind == Tk::Colon) {
                self.error_here(format!("expected `:` after `{}`", key.text));
                break None;
            }
            self.ts.next_raw();
            let Some(value) = self.expr() else { break None };
            entries.push((key, value));
            self.skip(true, false);
            match self.ts.peek_raw(0).map(|c| c.kind) {
                Some(Tk::Comma) => {
                    self.ts.next_raw();
                }
                Some(Tk::RBrace) => {}
                _ => {
                    self.error_here("expected `,` or `}`");
                    break None;
                }
            }
        };
        self.expr_depth -= 1;
        result?;
        let span = open.span.to(self.ts.last_span.unwrap_or(open.span));
        Some(Expr { kind: ExprKind::Record(entries), span })
    }

    /// The parameters of a lambda starting here, and how many tokens lead
    /// up to its body (through the `=>`).
    fn lambda_params(&mut self) -> Option<(Vec<Name>, usize)> {
        let name = |t: &Token| Name { text: t.text.to_string(), span: t.span };
        let first = self.ts.peek_raw(0)?.clone();
        if first.kind == Tk::Ident {
            return self.ts.peek_raw(1).is_some_and(|a| a.kind == Tk::Arrow).then(|| (vec![name(&first)], 2));
        }
        if first.kind != Tk::LParen {
            return None;
        }
        let mut params = Vec::new();
        let mut at = 1;
        loop {
            let t = self.ts.peek_raw(at)?.clone();
            if t.kind != Tk::Ident {
                return None;
            }
            params.push(name(&t));
            match self.ts.peek_raw(at + 1)?.kind {
                Tk::Comma => at += 2,
                Tk::RParen => break,
                _ => return None,
            }
        }
        self.ts.peek_raw(at + 2).is_some_and(|a| a.kind == Tk::Arrow).then_some((params, at + 3))
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
            // `x => body` or `(a, b) => body`: a lambda, for functions that
            // take one.
            if let Some((params, skip)) = self.lambda_params() {
                let start = self.ts.peek_raw(0).map(|t| t.span).expect("peeked");
                for _ in 0..skip {
                    self.ts.next_raw();
                }
                match self.expr() {
                    Some(body) => {
                        let span = start.to(body.span);
                        items.push(Expr { kind: ExprKind::Lambda(params, Box::new(body)), span });
                    }
                    None => break None,
                }
            } else {
                match self.expr() {
                    Some(e) => items.push(e),
                    None => break None,
                }
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
            self.ts.next_raw();
            let args = self.comma_list(Tk::RParen, "`)`")?;
            self.call_arguments(&args);
            let span = span.to(self.ts.last_span.unwrap_or(p.span));
            let name = path.pop().expect("one");
            if path.is_empty() {
                return Some(Expr { kind: ExprKind::Call(name, args), span });
            }
            let receiver = Expr { span: path[0].span.to(path[path.len() - 1].span), kind: ExprKind::Var(path) };
            return Some(Expr { kind: ExprKind::NodeCall(Box::new(receiver), name, args), span });
        }
        Some(Expr { kind: ExprKind::Var(path), span })
    }

    fn call_arguments(&mut self, args: &[Expr]) {
        fn simple(expr: &Expr) -> bool {
            match &expr.kind {
                ExprKind::Str(_) | ExprKind::Num(_) | ExprKind::Bool(_) | ExprKind::Null
                | ExprKind::Var(_) | ExprKind::Lambda(..) => true,
                ExprKind::Template(parts) => parts.iter().all(|part| matches!(part, TemplatePart::Lit(_))),
                ExprKind::List(items) => items.iter().all(simple),
                ExprKind::Record(entries) => entries.iter().all(|(_, value)| simple(value)),
                ExprKind::Key(value, _) | ExprKind::Unary(_, value)
                | ExprKind::Test(value, _) | ExprKind::Is(value, _) => simple(value),
                ExprKind::Binary(_, left, right) | ExprKind::Nullish(left, right) => simple(left) && simple(right),
                ExprKind::Call(..) | ExprKind::NodeCall(..) | ExprKind::If(..) | ExprKind::Pipeline(_) => false,
            }
        }
        for arg in args.iter().filter(|arg| !simple(arg)) {
            self.errors.push(ConfigError::new(arg.span, "call arguments must be simple values")
                .note("use a pipeline or an intermediate `let` for calls and other compound values"));
        }
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
        // `"""` strings lose indentation, so their holes have no exact
        // position; errors inside them point at the whole string.
        let exact = (tok.span.end - tok.span.start) as usize == raw.len() + 2;
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
                // `\u{...}` is an escape, not a hole.
                b'\\' if bytes.get(i + 1) == Some(&b'u') && bytes.get(i + 2) == Some(&b'{') => {
                    i = raw[i..].find('}').map_or(bytes.len(), |n| i + n + 1);
                }
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
                    let (span, hole_base) = if exact {
                        (Span::new(tok.span.file, base + i, base + close + 1), base + i + 1)
                    } else {
                        (tok.span, tok.span.start as usize)
                    };
                    parts.push(TemplatePart::Expr(self.hole(hole, hole_base, span)?, hole.trim().to_string()));
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

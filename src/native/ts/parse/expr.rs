//! Expressions, binding patterns, functions' parameters and bodies.

use super::super::kinds::{Field, Kind};
use super::super::lexer::Tok;
use super::{Kids, PResult, Parser, RESERVED};

/// Binary operators: (text, precedence, kind).
fn binary_op(op: &str) -> Option<(u8, Kind)> {
    Some(match op {
        "??" => (1, Kind::Logical),
        "||" => (2, Kind::Logical),
        "&&" => (3, Kind::Logical),
        "|" => (4, Kind::Binary),
        "^" => (5, Kind::Binary),
        "&" => (6, Kind::Binary),
        "==" | "!=" | "===" | "!==" => (7, Kind::Binary),
        "<" | ">" | "<=" | ">=" | "instanceof" | "in" => (8, Kind::Binary),
        "<<" | ">>" | ">>>" => (9, Kind::Binary),
        "+" | "-" => (10, Kind::Binary),
        "*" | "/" | "%" => (11, Kind::Binary),
        "**" => (12, Kind::Binary),
        _ => return None,
    })
}

const ASSIGN_OPS: &[&str] = &["=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", "&=", "|=", "^=", "&&=", "||=", "??="];

impl<'s> Parser<'s> {
    /// A full expression, commas included.
    pub(super) fn expression(&mut self) -> PResult<u32> {
        let start = self.start();
        let first = self.assign()?;
        if !self.is(",") {
            return Ok(first);
        }
        let mut kids = Kids::default();
        kids.add(Field::Expr, first);
        while self.eat(",") {
            let e = self.assign()?;
            kids.add(Field::Expr, e);
        }
        Ok(self.finish(Kind::Sequence, start, kids))
    }

    /// An expression with `in` allowed again (inside brackets).
    pub(super) fn expression_in(&mut self) -> PResult<u32> {
        let no_in = std::mem::replace(&mut self.no_in, false);
        let e = self.expression();
        self.no_in = no_in;
        e
    }

    pub(super) fn assign_in(&mut self) -> PResult<u32> {
        let no_in = std::mem::replace(&mut self.no_in, false);
        let e = self.assign();
        self.no_in = no_in;
        e
    }

    pub(super) fn assign(&mut self) -> PResult<u32> {
        self.enter()?;
        let r = self.assign_inner();
        self.leave();
        r
    }

    fn assign_inner(&mut self) -> PResult<u32> {
        let start = self.start();
        if let Some(arrow) = self.arrow()? {
            return Ok(arrow);
        }
        if self.in_gen && self.is_word("yield") {
            self.bump();
            let mut kids = Kids::default();
            if !self.tok.nl && self.eat("*") {
                let t = self.prev_end - 1;
                let m = self.text_node(t, t + 1);
                kids.add(Field::Op, m);
            }
            if !self.tok.nl && self.starts_expression() {
                let arg = self.assign()?;
                kids.add(Field::Arg, arg);
            }
            return Ok(self.finish(Kind::Yield, start, kids));
        }
        let left = self.conditional()?;
        let op = match self.tok.tok {
            Tok::P(p) if ASSIGN_OPS.contains(&p) => Some(p.len()),
            Tok::P(">") => match self.glued_gt() {
                ">>=" => Some(3),
                ">>>=" => Some(4),
                _ => None,
            },
            _ => None,
        };
        let Some(len) = op else { return Ok(left) };
        let t = if self.is(">") { self.bump_glued(len) } else { self.bump() };
        let op = self.text_node(t.start, t.end);
        let right = self.assign()?;
        let mut kids = Kids::default();
        kids.add(Field::Left, left);
        kids.add(Field::Op, op);
        kids.add(Field::Right, right);
        Ok(self.finish(Kind::Assign, start, kids))
    }

    /// Can the current token start an expression?
    pub(super) fn starts_expression(&self) -> bool {
        match self.tok.tok {
            Tok::Eof => false,
            Tok::P(p) => matches!(
                p,
                "(" | "[" | "{" | "+" | "-" | "!" | "~" | "++" | "--" | "/" | "/=" | "<" | "@" | "#" | "..."
            ),
            Tok::Ident => !matches!(self.cur(), "in" | "of" | "instanceof" | "as" | "satisfies"),
            _ => true,
        }
    }

    // ---- arrows --------------------------------------------------------

    fn arrow(&mut self) -> PResult<Option<u32>> {
        let start = self.start();
        let is_async = self.is_word("async") && {
            let (n, text) = self.peek_text();
            !n.nl && (matches!(n.tok, Tok::P("(" | "<")) || (n.tok == Tok::Ident && text != "function"))
        };
        // `x => ...` and `async x => ...`
        let simple = |p: &Self| {
            if !p.is_binding_ident() {
                return false;
            }
            let n = p.peek();
            matches!(n.tok, Tok::P("=>")) && !n.nl
        };
        if simple(self) {
            let param = self.simple_param()?;
            let mut kids = Kids::default();
            kids.add(Field::Param, param);
            return self.arrow_rest(start, kids).map(Some);
        }
        if is_async {
            let saved_ok = self.try_parse(|p| {
                let a = p.token_text();
                let mut kids = Kids::default();
                kids.add(Field::Modifier, a);
                if simple(p) {
                    let param = p.simple_param()?;
                    kids.add(Field::Param, param);
                } else {
                    p.arrow_head(&mut kids)?;
                }
                p.arrow_rest(start, kids)
            });
            return Ok(saved_ok);
        }
        if self.is("(") || (self.is("<") && !self.jsx) {
            return Ok(self.try_parse(|p| {
                let mut kids = Kids::default();
                p.arrow_head(&mut kids)?;
                p.arrow_rest(start, kids)
            }));
        }
        Ok(None)
    }

    fn simple_param(&mut self) -> PResult<u32> {
        let start = self.start();
        let name = self.leaf(Kind::Ident);
        let mut kids = Kids::default();
        kids.add(Field::Name, name);
        Ok(self.finish(Kind::Param, start, kids))
    }

    /// `<T>(a: A, b): R` before an arrow's `=>`.
    fn arrow_head(&mut self, kids: &mut Kids) -> PResult<()> {
        if self.is("<") {
            self.type_params(kids)?;
        }
        self.params(kids)?;
        if self.eat(":") {
            let r = self.return_type()?;
            kids.add(Field::ReturnType, r);
        }
        if !self.is("=>") || self.tok.nl {
            return self.fail("expected `=>`");
        }
        Ok(())
    }

    fn arrow_rest(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        let at = self.start();
        self.expect("=>")?;
        // The `=>` token: where rules point at an arrow function's head.
        let op = self.text_node(at, self.prev_end);
        kids.add(Field::Op, op);
        let in_gen = std::mem::replace(&mut self.in_gen, false);
        let body = if self.is("{") { self.function_body() } else { self.assign() };
        self.in_gen = in_gen;
        kids.add(Field::Body, body?);
        Ok(self.finish(Kind::Arrow, start, kids))
    }

    // ---- operators -----------------------------------------------------

    fn conditional(&mut self) -> PResult<u32> {
        let start = self.start();
        let test = self.binary(0)?;
        if !self.is("?") {
            return Ok(test);
        }
        self.bump();
        let consequent = self.assign_in()?;
        self.expect(":")?;
        let alternate = self.assign()?;
        let mut kids = Kids::default();
        kids.add(Field::Test, test);
        kids.add(Field::Consequent, consequent);
        kids.add(Field::Alternate, alternate);
        Ok(self.finish(Kind::Conditional, start, kids))
    }

    fn binary(&mut self, min: u8) -> PResult<u32> {
        let start = self.start();
        let mut left = self.unary()?;
        loop {
            // `as` and `satisfies` bind like relational operators.
            if !self.tok.nl && (self.is_word("as") || self.is_word("satisfies")) && min < 8 {
                let kind = if self.is_word("as") { Kind::As } else { Kind::Satisfies };
                self.bump();
                let ty = if self.is_word("const") { self.leaf(Kind::TypeKeyword) } else { self.type_()? };
                let mut kids = Kids::default();
                kids.add(Field::Expr, left);
                kids.add(Field::Type, ty);
                left = self.finish(kind, start, kids);
                continue;
            }
            let op: &str = match self.tok.tok {
                Tok::P(">") => self.glued_gt(),
                Tok::P(p) => p,
                Tok::Ident if matches!(self.cur(), "instanceof") => "instanceof",
                Tok::Ident if self.cur() == "in" && !self.no_in => "in",
                _ => break,
            };
            let Some((prec, kind)) = binary_op(op) else { break };
            if prec <= min {
                break;
            }
            let t = if op.starts_with('>') { self.bump_glued(op.len()) } else { self.bump() };
            let op_node = self.text_node(t.start, t.end);
            // `**` is right-associative.
            let right = if op == "**" { self.binary(prec - 1)? } else { self.binary(prec)? };
            let mut kids = Kids::default();
            kids.add(Field::Left, left);
            kids.add(Field::Op, op_node);
            kids.add(Field::Right, right);
            left = self.finish(kind, start, kids);
        }
        Ok(left)
    }

    fn unary(&mut self) -> PResult<u32> {
        self.enter()?;
        let r = self.unary_inner();
        self.leave();
        r
    }

    fn unary_inner(&mut self) -> PResult<u32> {
        let start = self.start();
        let simple = match self.tok.tok {
            Tok::P("!" | "~" | "+" | "-") => Some(Kind::Unary),
            Tok::P("++" | "--") => Some(Kind::Update),
            Tok::Ident if matches!(self.cur(), "delete" | "void" | "typeof") => Some(Kind::Unary),
            _ => None,
        };
        if let Some(kind) = simple {
            let op = self.token_text();
            let arg = self.unary()?;
            let mut kids = Kids::default();
            kids.add(Field::Op, op);
            kids.add(Field::Arg, arg);
            return Ok(self.finish(kind, start, kids));
        }
        if self.is_word("await") && self.await_is_keyword() {
            self.bump();
            let arg = self.unary()?;
            let mut kids = Kids::default();
            kids.add(Field::Arg, arg);
            return Ok(self.finish(Kind::Await, start, kids));
        }
        if self.is("<") && !self.jsx {
            self.bump();
            let ty = if self.is_word("const") { self.leaf(Kind::TypeKeyword) } else { self.type_()? };
            self.expect(">")?;
            let expr = self.unary()?;
            let mut kids = Kids::default();
            kids.add(Field::Type, ty);
            kids.add(Field::Expr, expr);
            return Ok(self.finish(Kind::TypeAssertion, start, kids));
        }
        let e = self.lhs()?;
        if !self.tok.nl && (self.is("++") || self.is("--")) {
            let op = self.token_text();
            let mut kids = Kids::default();
            kids.add(Field::Arg, e);
            kids.add(Field::Op, op);
            return Ok(self.finish(Kind::Update, start, kids));
        }
        Ok(e)
    }

    fn await_is_keyword(&self) -> bool {
        let n = self.peek();
        !matches!(n.tok, Tok::Eof | Tok::P(")" | "]" | "}" | "," | ";" | ":" | "=" | "." | "?." | "=>" | "?"))
    }

    // ---- calls and members ---------------------------------------------

    pub(super) fn lhs(&mut self) -> PResult<u32> {
        let start = self.start();
        let mut e = if self.is_word("new") { self.new_expr()? } else { self.primary()? };
        loop {
            e = match self.tok.tok {
                Tok::P(".") => {
                    self.bump();
                    let prop = self.property_name()?;
                    self.member(start, e, None, prop)
                }
                Tok::P("?.") => {
                    let q = self.token_text();
                    if self.is("(") {
                        self.call(start, e, Some(q), Kids::default())?
                    } else if self.is("[") {
                        self.index(start, e, Some(q))?
                    } else if self.is("<") {
                        let mut targs = Kids::default();
                        self.type_args(&mut targs, Field::TypeArg)?;
                        self.call(start, e, Some(q), targs)?
                    } else {
                        let prop = self.property_name()?;
                        self.member(start, e, Some(q), prop)
                    }
                }
                Tok::P("[") => self.index(start, e, None)?,
                Tok::P("(") => self.call(start, e, None, Kids::default())?,
                Tok::Template | Tok::TemplateHead => {
                    let quasi = self.template()?;
                    let mut kids = Kids::default();
                    kids.add(Field::Tag, e);
                    kids.add(Field::Quasi, quasi);
                    self.finish(Kind::TaggedTemplate, start, kids)
                }
                Tok::P("!") if !self.tok.nl => {
                    self.bump();
                    let mut kids = Kids::default();
                    kids.add(Field::Expr, e);
                    self.finish(Kind::NonNull, start, kids)
                }
                Tok::P("<") => {
                    let targs = self.try_parse(|p| {
                        let mut targs = Kids::default();
                        p.type_args(&mut targs, Field::TypeArg)?;
                        if matches!(p.tok.tok, Tok::P("(") | Tok::Template | Tok::TemplateHead) {
                            Ok(targs)
                        } else {
                            p.fail("not type arguments")
                        }
                    });
                    match targs {
                        Some(targs) if self.is("(") => self.call(start, e, None, targs)?,
                        Some(mut targs) => {
                            let quasi = self.template()?;
                            targs.0.insert(0, crate::grammar::Edge { label: Field::Tag as u16, node: e });
                            targs.add(Field::Quasi, quasi);
                            self.finish(Kind::TaggedTemplate, start, targs)
                        }
                        None => break,
                    }
                }
                _ => break,
            };
        }
        Ok(e)
    }

    /// The name after `.`: any word, or a private name.
    fn property_name(&mut self) -> PResult<u32> {
        match self.tok.tok {
            Tok::Ident => Ok(self.leaf(Kind::Ident)),
            Tok::PrivateName => Ok(self.leaf(Kind::PrivateName)),
            _ => self.fail("expected a property name"),
        }
    }

    fn member(&mut self, start: u32, object: u32, q: Option<u32>, prop: u32) -> u32 {
        let mut kids = Kids::default();
        kids.add(Field::Object, object);
        kids.opt(Field::Optional, q);
        kids.add(Field::Property, prop);
        self.finish(Kind::Member, start, kids)
    }

    fn index(&mut self, start: u32, object: u32, q: Option<u32>) -> PResult<u32> {
        self.expect("[")?;
        let index = self.expression_in()?;
        self.expect("]")?;
        let mut kids = Kids::default();
        kids.add(Field::Object, object);
        kids.opt(Field::Optional, q);
        kids.add(Field::Index, index);
        Ok(self.finish(Kind::Index, start, kids))
    }

    fn call(&mut self, start: u32, callee: u32, q: Option<u32>, targs: Kids) -> PResult<u32> {
        let mut kids = Kids::default();
        kids.add(Field::Callee, callee);
        kids.opt(Field::Optional, q);
        kids.0.extend(targs.0);
        self.arguments(&mut kids)?;
        Ok(self.finish(Kind::Call, start, kids))
    }

    fn arguments(&mut self, kids: &mut Kids) -> PResult<()> {
        self.expect("(")?;
        let no_in = std::mem::replace(&mut self.no_in, false);
        while !self.is(")") {
            let a = if self.is("...") { self.spread()? } else { self.assign()? };
            kids.add(Field::Arg, a);
            if !self.eat(",") {
                break;
            }
        }
        self.no_in = no_in;
        self.expect(")")?;
        Ok(())
    }

    fn spread(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect("...")?;
        let arg = self.assign_in()?;
        let mut kids = Kids::default();
        kids.add(Field::Arg, arg);
        Ok(self.finish(Kind::Spread, start, kids))
    }

    fn new_expr(&mut self) -> PResult<u32> {
        let start = self.start();
        self.bump();
        if self.eat(".") {
            self.bump();
            return Ok(self.finish(Kind::MetaProperty, start, Kids::default()));
        }
        let cstart = self.start();
        let mut callee = if self.is_word("new") { self.new_expr()? } else { self.primary()? };
        loop {
            callee = match self.tok.tok {
                Tok::P(".") => {
                    self.bump();
                    let prop = self.property_name()?;
                    self.member(cstart, callee, None, prop)
                }
                Tok::P("[") => self.index(cstart, callee, None)?,
                // `new X!(args)`: the arguments still belong to `new`.
                Tok::P("!") if !self.tok.nl => {
                    self.bump();
                    let mut kids = Kids::default();
                    kids.add(Field::Expr, callee);
                    self.finish(Kind::NonNull, cstart, kids)
                }
                Tok::Template | Tok::TemplateHead => {
                    let quasi = self.template()?;
                    let mut kids = Kids::default();
                    kids.add(Field::Tag, callee);
                    kids.add(Field::Quasi, quasi);
                    self.finish(Kind::TaggedTemplate, cstart, kids)
                }
                _ => break,
            };
        }
        let mut kids = Kids::default();
        kids.add(Field::Callee, callee);
        if self.is("<") {
            if let Some(targs) = self.try_parse(|p| {
                let mut k = Kids::default();
                p.type_args(&mut k, Field::TypeArg)?;
                Ok(k)
            }) {
                kids.0.extend(targs.0);
            }
        }
        if self.is("(") {
            self.arguments(&mut kids)?;
        }
        Ok(self.finish(Kind::New, start, kids))
    }

    // ---- primaries -----------------------------------------------------

    fn primary(&mut self) -> PResult<u32> {
        let start = self.start();
        match self.tok.tok {
            Tok::Num => Ok(self.leaf(Kind::Number)),
            Tok::BigInt => Ok(self.leaf(Kind::BigInt)),
            Tok::Str => Ok(self.leaf(Kind::String)),
            Tok::Template | Tok::TemplateHead => self.template(),
            Tok::PrivateName => Ok(self.leaf(Kind::PrivateName)),
            Tok::P("/" | "/=") => {
                self.rescan_regex();
                Ok(self.leaf(Kind::Regex))
            }
            Tok::P("(") => {
                self.bump();
                let e = self.expression_in()?;
                self.expect(")")?;
                let mut kids = Kids::default();
                kids.add(Field::Expr, e);
                Ok(self.finish(Kind::Paren, start, kids))
            }
            Tok::P("[") => self.array(),
            Tok::P("{") => self.object(),
            Tok::P("@") => {
                let mut kids = Kids::default();
                self.decorators(&mut kids)?;
                self.class(start, kids)
            }
            Tok::P("<") if self.jsx => self.fail("JSX is not supported yet"),
            Tok::Ident => match self.cur() {
                "function" => self.function(start, Kids::default()),
                "class" => self.class(start, Kids::default()),
                "this" => Ok(self.leaf(Kind::This)),
                "super" => Ok(self.leaf(Kind::Super)),
                "null" => Ok(self.leaf(Kind::Null)),
                "true" | "false" => Ok(self.leaf(Kind::Bool)),
                "async" if {
                    let (n, t) = self.peek_text();
                    !n.nl && t == "function"
                } =>
                {
                    let a = self.token_text();
                    let mut kids = Kids::default();
                    kids.add(Field::Modifier, a);
                    self.function(start, kids)
                }
                "import" => {
                    self.bump();
                    if self.eat(".") {
                        self.bump();
                        return Ok(self.finish(Kind::MetaProperty, start, Kids::default()));
                    }
                    let mut kids = Kids::default();
                    self.arguments(&mut kids)?;
                    let mut fixed = Kids::default();
                    for e in kids.0 {
                        fixed.add(Field::Arg, e.node);
                    }
                    Ok(self.finish(Kind::ImportCall, start, fixed))
                }
                w if RESERVED.contains(&w) => self.fail("expected an expression"),
                _ => Ok(self.leaf(Kind::Ident)),
            },
            _ => self.fail("expected an expression"),
        }
    }

    pub(super) fn template(&mut self) -> PResult<u32> {
        let start = self.start();
        let mut kids = Kids::default();
        if self.tok.tok == Tok::Template {
            let q = self.token_text();
            kids.add(Field::Quasi, q);
            return Ok(self.finish(Kind::Template, start, kids));
        }
        let q = self.token_text();
        kids.add(Field::Quasi, q);
        loop {
            let e = self.expression_in()?;
            kids.add(Field::Expr, e);
            if !self.is("}") {
                return self.fail("expected `}` closing a template substitution");
            }
            self.rescan_template();
            let tail = self.tok.tok == Tok::TemplateTail;
            let q = self.token_text();
            kids.add(Field::Quasi, q);
            if tail {
                break;
            }
        }
        Ok(self.finish(Kind::Template, start, kids))
    }

    fn array(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect("[")?;
        let no_in = std::mem::replace(&mut self.no_in, false);
        let mut kids = Kids::default();
        while !self.is("]") {
            if self.is(",") {
                let at = self.start();
                let hole = self.finish_at(Kind::Hole, at, at, Kids::default());
                kids.add(Field::Element, hole);
                self.bump();
                continue;
            }
            let e = if self.is("...") { self.spread()? } else { self.assign()? };
            kids.add(Field::Element, e);
            if !self.eat(",") {
                break;
            }
        }
        self.no_in = no_in;
        self.expect("]")?;
        Ok(self.finish(Kind::Array, start, kids))
    }

    fn object(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect("{")?;
        let no_in = std::mem::replace(&mut self.no_in, false);
        let mut kids = Kids::default();
        while !self.is("}") {
            let p = if self.is("...") { self.spread()? } else { self.object_member()? };
            kids.add(Field::Property, p);
            if !self.eat(",") {
                break;
            }
        }
        self.no_in = no_in;
        self.expect("}")?;
        Ok(self.finish(Kind::Object, start, kids))
    }

    /// Is the current word a modifier (followed by a property name) rather
    /// than itself a property name?
    pub(super) fn word_is_modifier(&self) -> bool {
        !self.peek().nl && self.word_is_accessor()
    }

    /// `get` / `set` followed by a property name: an accessor, even across
    /// a line break (only `async` may not have one after it).
    pub(super) fn word_is_accessor(&self) -> bool {
        matches!(self.peek().tok, Tok::Ident | Tok::Str | Tok::Num | Tok::BigInt | Tok::PrivateName | Tok::P("[" | "*" | "{" | "#"))
    }

    fn object_member(&mut self) -> PResult<u32> {
        let start = self.start();
        let mut mods = Kids::default();
        if self.is_word("async") && self.word_is_modifier() {
            let a = self.token_text();
            mods.add(Field::Modifier, a);
        }
        if self.is("*") {
            let s = self.token_text();
            mods.add(Field::Modifier, s);
        }
        if (self.is_word("get") || self.is_word("set")) && self.word_is_accessor() {
            let k = self.token_text();
            mods.add(Field::Kind, k);
        }
        let key = self.property_key()?;
        if self.is("(") || self.is("<") {
            return self.method_rest(start, mods, key, None);
        }
        let mut kids = mods;
        kids.add(Field::Key, key);
        if self.eat(":") {
            let v = self.assign()?;
            kids.add(Field::Value, v);
        } else if self.is("=") {
            // `{ a = 1 } = x`: a default in a destructuring assignment.
            let dstart = self.start();
            self.bump();
            let right = self.assign()?;
            let mut d = Kids::default();
            d.add(Field::Right, right);
            let v = self.finish(Kind::AssignPattern, dstart, d);
            kids.add(Field::Value, v);
        }
        Ok(self.finish(Kind::Prop, start, kids))
    }

    /// A property key: a word, string, number or `[computed]`.
    pub(super) fn property_key(&mut self) -> PResult<u32> {
        match self.tok.tok {
            Tok::Ident => Ok(self.leaf(Kind::Ident)),
            Tok::PrivateName => Ok(self.leaf(Kind::PrivateName)),
            Tok::Str => Ok(self.leaf(Kind::String)),
            Tok::Num => Ok(self.leaf(Kind::Number)),
            Tok::BigInt => Ok(self.leaf(Kind::BigInt)),
            Tok::P("[") => {
                let start = self.start();
                self.bump();
                let e = self.assign_in()?;
                self.expect("]")?;
                let mut kids = Kids::default();
                kids.add(Field::Expr, e);
                Ok(self.finish(Kind::Computed, start, kids))
            }
            _ => self.fail("expected a property name"),
        }
    }

    /// After a method's key: `?`, type parameters, parameters, return type
    /// and body (or `;` for overloads and abstract methods).
    pub(super) fn method_rest(&mut self, start: u32, mut kids: Kids, key: u32, optional: Option<u32>) -> PResult<u32> {
        kids.add(Field::Key, key);
        kids.opt(Field::Optional, optional);
        let is_gen = kids.0.iter().any(|e| e.label == Field::Modifier as u16 && self.node_text(e.node) == "*");
        if self.is("<") {
            self.type_params(&mut kids)?;
        }
        self.params(&mut kids)?;
        if self.eat(":") {
            let r = self.return_type()?;
            kids.add(Field::ReturnType, r);
        }
        if self.is("{") {
            let in_gen = std::mem::replace(&mut self.in_gen, is_gen);
            let body = self.function_body();
            self.in_gen = in_gen;
            kids.add(Field::Body, body?);
        } else {
            self.semi()?;
        }
        Ok(self.finish(Kind::Method, start, kids))
    }

    pub(super) fn node_text(&self, id: u32) -> &'s str {
        let n = &self.nodes[id as usize];
        &self.src[n.start as usize..n.end as usize]
    }

    // ---- functions -----------------------------------------------------

    /// `function` onwards (modifiers like `async` are already in `kids`).
    pub(super) fn function(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        self.expect_word("function")?;
        let mut is_gen = false;
        if self.is("*") {
            let s = self.token_text();
            kids.add(Field::Modifier, s);
            is_gen = true;
        }
        if self.tok.tok == Tok::Ident && !self.is("(") {
            let name = self.leaf(Kind::Ident);
            kids.add(Field::Name, name);
        }
        if self.is("<") {
            self.type_params(&mut kids)?;
        }
        self.params(&mut kids)?;
        if self.eat(":") {
            let r = self.return_type()?;
            kids.add(Field::ReturnType, r);
        }
        if self.is("{") {
            let in_gen = std::mem::replace(&mut self.in_gen, is_gen);
            let body = self.function_body();
            self.in_gen = in_gen;
            kids.add(Field::Body, body?);
        } else {
            // Overloads and `declare function`.
            self.semi()?;
        }
        Ok(self.finish(Kind::Function, start, kids))
    }

    pub(super) fn function_body(&mut self) -> PResult<u32> {
        let no_in = std::mem::replace(&mut self.no_in, false);
        let b = self.block();
        self.no_in = no_in;
        b
    }

    /// `(a, b?: T, ...rest)`
    pub(super) fn params(&mut self, kids: &mut Kids) -> PResult<()> {
        self.expect("(")?;
        while !self.is(")") {
            let p = self.param()?;
            kids.add(Field::Param, p);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(())
    }

    fn param(&mut self) -> PResult<u32> {
        let start = self.start();
        let mut kids = Kids::default();
        self.decorators(&mut kids)?;
        while self.tok.tok == Tok::Ident
            && matches!(self.cur(), "public" | "private" | "protected" | "readonly" | "override")
            && self.word_is_modifier()
        {
            let m = self.token_text();
            kids.add(Field::Modifier, m);
        }
        let name = if self.is("...") {
            let rstart = self.start();
            self.bump();
            let arg = self.binding()?;
            let mut r = Kids::default();
            r.add(Field::Arg, arg);
            self.finish(Kind::Rest, rstart, r)
        } else if self.is_word("this") {
            self.leaf(Kind::This)
        } else {
            self.binding()?
        };
        kids.add(Field::Name, name);
        if self.is("?") {
            let q = self.token_text();
            kids.add(Field::Optional, q);
        }
        if self.eat(":") {
            let t = self.type_()?;
            kids.add(Field::Type, t);
        }
        if self.eat("=") {
            let i = self.assign_in()?;
            kids.add(Field::Init, i);
        }
        Ok(self.finish(Kind::Param, start, kids))
    }

    pub(super) fn decorators(&mut self, kids: &mut Kids) -> PResult<()> {
        while self.is("@") {
            let start = self.start();
            self.bump();
            let e = self.lhs()?;
            let mut d = Kids::default();
            d.add(Field::Expr, e);
            let d = self.finish(Kind::Decorator, start, d);
            kids.add(Field::Decorator, d);
        }
        Ok(())
    }

    // ---- binding patterns ----------------------------------------------

    pub(super) fn binding(&mut self) -> PResult<u32> {
        match self.tok.tok {
            Tok::P("{") => self.object_pattern(),
            Tok::P("[") => self.array_pattern(),
            Tok::Ident if !RESERVED.contains(&self.cur()) => Ok(self.leaf(Kind::Ident)),
            _ => self.fail("expected a binding name"),
        }
    }

    /// A binding with an optional `= default`.
    fn binding_element(&mut self) -> PResult<u32> {
        let start = self.start();
        let b = self.binding()?;
        if !self.is("=") {
            return Ok(b);
        }
        self.bump();
        let right = self.assign_in()?;
        let mut kids = Kids::default();
        kids.add(Field::Left, b);
        kids.add(Field::Right, right);
        Ok(self.finish(Kind::AssignPattern, start, kids))
    }

    fn object_pattern(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect("{")?;
        let mut kids = Kids::default();
        while !self.is("}") {
            if self.is("...") {
                let rstart = self.start();
                self.bump();
                let arg = self.binding()?;
                let mut r = Kids::default();
                r.add(Field::Arg, arg);
                let r = self.finish(Kind::Rest, rstart, r);
                kids.add(Field::Rest, r);
            } else {
                let pstart = self.start();
                let shorthand = self.tok.tok == Tok::Ident && !matches!(self.peek().tok, Tok::P(":"));
                let key = self.property_key()?;
                let mut p = Kids::default();
                p.add(Field::Key, key);
                if self.eat(":") {
                    let v = self.binding_element()?;
                    p.add(Field::Value, v);
                } else if shorthand && self.is("=") {
                    let dstart = self.start();
                    self.bump();
                    let right = self.assign_in()?;
                    let mut d = Kids::default();
                    d.add(Field::Right, right);
                    let v = self.finish(Kind::AssignPattern, dstart, d);
                    p.add(Field::Value, v);
                }
                let p = self.finish(Kind::PatternProp, pstart, p);
                kids.add(Field::Property, p);
            }
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        Ok(self.finish(Kind::ObjectPattern, start, kids))
    }

    fn array_pattern(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect("[")?;
        let mut kids = Kids::default();
        while !self.is("]") {
            if self.is(",") {
                let at = self.start();
                let hole = self.finish_at(Kind::Hole, at, at, Kids::default());
                kids.add(Field::Element, hole);
                self.bump();
                continue;
            }
            let e = if self.is("...") {
                let rstart = self.start();
                self.bump();
                let arg = self.binding()?;
                let mut r = Kids::default();
                r.add(Field::Arg, arg);
                self.finish(Kind::Rest, rstart, r)
            } else {
                self.binding_element()?
            };
            kids.add(Field::Element, e);
            if !self.eat(",") {
                break;
            }
        }
        self.expect("]")?;
        Ok(self.finish(Kind::ArrayPattern, start, kids))
    }
}

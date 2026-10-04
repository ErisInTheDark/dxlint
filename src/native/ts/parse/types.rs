//! Type syntax: annotations, type parameters and arguments, and the type
//! members of interfaces and type literals.

use super::super::kinds::{Field, Kind};
use super::super::lexer::Tok;
use super::{Kids, PResult, Parser};

const KEYWORD_TYPES: &[&str] = &[
    "any", "unknown", "number", "string", "boolean", "void", "never", "object", "symbol", "bigint", "undefined", "null",
    "this", "intrinsic",
];

impl<'s> Parser<'s> {
    pub(super) fn type_(&mut self) -> PResult<u32> {
        self.enter()?;
        let outer = std::mem::replace(&mut self.no_cond, false);
        let r = self.type_inner(false);
        self.no_cond = outer;
        self.leave();
        r
    }

    /// A type that may not be a conditional type (the `extends` clause of a
    /// conditional type, `infer X extends C`).
    fn type_no_conditional(&mut self) -> PResult<u32> {
        self.enter()?;
        let outer = std::mem::replace(&mut self.no_cond, true);
        let r = self.type_inner(true);
        self.no_cond = outer;
        self.leave();
        r
    }

    fn type_inner(&mut self, no_conditional: bool) -> PResult<u32> {
        let start = self.start();
        if self.is("<") || self.is_word("new") || (self.is_word("abstract") && self.peek_text().1 == "new") {
            return self.fn_type();
        }
        if self.is("(") {
            if let Some(f) = self.try_parse(|p| p.fn_type()) {
                return Ok(f);
            }
        }
        let check = self.union_type()?;
        if no_conditional || self.tok.nl || !self.is_word("extends") {
            return Ok(check);
        }
        self.bump();
        let extends = self.type_no_conditional()?;
        self.expect("?")?;
        let consequent = self.type_()?;
        self.expect(":")?;
        let alternate = self.type_()?;
        let mut kids = Kids::default();
        kids.add(Field::Check, check);
        kids.add(Field::Extends, extends);
        kids.add(Field::Consequent, consequent);
        kids.add(Field::Alternate, alternate);
        Ok(self.finish(Kind::ConditionalType, start, kids))
    }

    /// `(a: A) => R`, `new (a: A) => R`, `<T>(a: T) => T`
    fn fn_type(&mut self) -> PResult<u32> {
        let start = self.start();
        let mut kids = Kids::default();
        if self.is_word("abstract") {
            let a = self.token_text();
            kids.add(Field::Modifier, a);
        }
        if self.is_word("new") {
            let n = self.token_text();
            kids.add(Field::Kind, n);
        }
        if self.is("<") {
            self.type_params(&mut kids)?;
        }
        self.params(&mut kids)?;
        self.expect("=>")?;
        let r = self.return_type()?;
        kids.add(Field::ReturnType, r);
        Ok(self.finish(Kind::FnType, start, kids))
    }

    /// A return type, which may be a predicate (`x is T`, `asserts x`).
    pub(super) fn return_type(&mut self) -> PResult<u32> {
        let start = self.start();
        let (next, next_text) = self.peek_text();
        let asserts = self.is_word("asserts") && !next.nl && next.tok == Tok::Ident && next_text != "is";
        let is_pred = self.tok.tok == Tok::Ident && !next.nl && next.tok == Tok::Ident && next_text == "is";
        if !asserts && !is_pred {
            return self.type_();
        }
        let mut kids = Kids::default();
        if asserts {
            let a = self.token_text();
            kids.add(Field::Modifier, a);
        }
        let name = if self.is_word("this") { self.leaf(Kind::This) } else { self.leaf(Kind::Ident) };
        kids.add(Field::Name, name);
        if self.eat_word("is") {
            let t = self.type_()?;
            kids.add(Field::Type, t);
        }
        Ok(self.finish(Kind::TypePredicate, start, kids))
    }

    fn union_type(&mut self) -> PResult<u32> {
        self.list_type("|", Kind::Union, Self::intersection_type)
    }

    fn intersection_type(&mut self) -> PResult<u32> {
        self.list_type("&", Kind::Intersection, Self::operator_type)
    }

    fn list_type(&mut self, sep: &str, kind: Kind, item: fn(&mut Self) -> PResult<u32>) -> PResult<u32> {
        let start = self.start();
        let leading = self.eat(sep);
        let first = item(self)?;
        if !self.is(sep) {
            if leading {
                let mut kids = Kids::default();
                kids.add(Field::Type, first);
                return Ok(self.finish(kind, start, kids));
            }
            return Ok(first);
        }
        let mut kids = Kids::default();
        kids.add(Field::Type, first);
        while self.eat(sep) {
            let t = item(self)?;
            kids.add(Field::Type, t);
        }
        Ok(self.finish(kind, start, kids))
    }

    fn operator_type(&mut self) -> PResult<u32> {
        let start = self.start();
        if self.tok.tok == Tok::Ident && matches!(self.cur(), "keyof" | "unique" | "readonly") && self.word_starts_type_after() {
            let op = self.token_text();
            let t = self.operator_type()?;
            let mut kids = Kids::default();
            kids.add(Field::Op, op);
            kids.add(Field::Type, t);
            return Ok(self.finish(Kind::TypeOperator, start, kids));
        }
        if self.is_word("infer") && self.peek().tok == Tok::Ident {
            self.bump();
            let pstart = self.start();
            let name = self.leaf(Kind::Ident);
            let mut p = Kids::default();
            p.add(Field::Name, name);
            if self.is_word("extends") {
                // In a conditional's `extends` clause the constraint always
                // belongs to `infer`; elsewhere only when no `?` follows.
                let keep = self.no_cond;
                let c = self.try_parse(|p| {
                    p.bump();
                    let c = p.type_no_conditional()?;
                    if p.is("?") && !keep {
                        return p.fail("a conditional type's `extends`");
                    }
                    Ok(c)
                });
                p.opt(Field::Constraint, c);
            }
            let param = self.finish(Kind::TypeParam, pstart, p);
            let mut kids = Kids::default();
            kids.add(Field::TypeParam, param);
            return Ok(self.finish(Kind::Infer, start, kids));
        }
        let mut t = self.primary_type()?;
        while self.is("[") && !self.tok.nl {
            self.bump();
            if self.eat("]") {
                let mut kids = Kids::default();
                kids.add(Field::Type, t);
                t = self.finish(Kind::ArrayType, start, kids);
            } else {
                let index = self.type_()?;
                self.expect("]")?;
                let mut kids = Kids::default();
                kids.add(Field::Object, t);
                kids.add(Field::Index, index);
                t = self.finish(Kind::IndexedAccess, start, kids);
            }
        }
        Ok(t)
    }

    /// `keyof`/`readonly`/`unique` as operators (not type names).
    fn word_starts_type_after(&self) -> bool {
        let n = self.peek();
        !matches!(n.tok, Tok::Eof | Tok::P(")" | "]" | "}" | "," | ";" | ":" | "=" | ">" | "|" | "&" | "?" | "." | "=>"))
    }

    fn primary_type(&mut self) -> PResult<u32> {
        let start = self.start();
        match self.tok.tok {
            Tok::P("(") => {
                self.bump();
                let t = self.type_()?;
                self.expect(")")?;
                let mut kids = Kids::default();
                kids.add(Field::Type, t);
                Ok(self.finish(Kind::ParenType, start, kids))
            }
            Tok::P("{") => {
                if let Some(m) = self.try_parse(|p| p.mapped_type()) {
                    return Ok(m);
                }
                let mut kids = Kids::default();
                self.type_members(&mut kids)?;
                Ok(self.finish(Kind::TypeLit, start, kids))
            }
            Tok::P("[") => self.tuple_type(),
            Tok::Str | Tok::Num | Tok::BigInt => {
                let v = self.leaf(match self.tok.tok {
                    Tok::Str => Kind::String,
                    Tok::Num => Kind::Number,
                    _ => Kind::BigInt,
                });
                let mut kids = Kids::default();
                kids.add(Field::Value, v);
                Ok(self.finish(Kind::LiteralType, start, kids))
            }
            Tok::P("-") if matches!(self.peek().tok, Tok::Num | Tok::BigInt) => {
                self.bump();
                let v = self.leaf(if self.tok.tok == Tok::BigInt { Kind::BigInt } else { Kind::Number });
                let mut kids = Kids::default();
                kids.add(Field::Value, v);
                Ok(self.finish(Kind::LiteralType, start, kids))
            }
            Tok::Template | Tok::TemplateHead => self.template_type(),
            Tok::Ident => match self.cur() {
                "true" | "false" => {
                    let v = self.leaf(Kind::Bool);
                    let mut kids = Kids::default();
                    kids.add(Field::Value, v);
                    Ok(self.finish(Kind::LiteralType, start, kids))
                }
                "typeof" => {
                    self.bump();
                    let name = if self.is_word("import") { self.import_type()? } else { self.entity_name(true)? };
                    let mut kids = Kids::default();
                    kids.add(Field::Name, name);
                    if self.is("<") && !self.tok.nl {
                        self.type_args(&mut kids, Field::TypeArg)?;
                    }
                    Ok(self.finish(Kind::TypeQuery, start, kids))
                }
                "import" => self.import_type(),
                w if KEYWORD_TYPES.contains(&w) && !matches!(self.peek().tok, Tok::P(".")) => Ok(self.leaf(Kind::TypeKeyword)),
                _ => {
                    let name = self.entity_name(false)?;
                    let mut kids = Kids::default();
                    kids.add(Field::Name, name);
                    if self.is("<") && !self.tok.nl {
                        self.type_args(&mut kids, Field::TypeArg)?;
                    }
                    Ok(self.finish(Kind::TypeRef, start, kids))
                }
            },
            _ => self.fail("expected a type"),
        }
    }

    /// `A`, `A.B.C` (`typeof` also allows `this.x` and `#private`).
    fn entity_name(&mut self, in_typeof: bool) -> PResult<u32> {
        let start = self.start();
        let mut name = match self.tok.tok {
            Tok::Ident if self.cur() == "this" => self.leaf(Kind::This),
            Tok::Ident => self.leaf(Kind::Ident),
            _ => return self.fail("expected a type name"),
        };
        while self.is(".") {
            self.bump();
            let right = match self.tok.tok {
                Tok::Ident => self.leaf(Kind::Ident),
                Tok::PrivateName if in_typeof => self.leaf(Kind::PrivateName),
                _ => return self.fail("expected a name after `.`"),
            };
            let mut kids = Kids::default();
            kids.add(Field::Left, name);
            kids.add(Field::Right, right);
            name = self.finish(Kind::QualifiedName, start, kids);
        }
        Ok(name)
    }

    /// `import("mod").Name<T>`
    fn import_type(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect_word("import")?;
        self.expect("(")?;
        let mut kids = Kids::default();
        let arg = self.type_()?;
        kids.add(Field::Arg, arg);
        if self.eat(",") && !self.is(")") {
            // Import attributes: `{ with: { ... } }`.
            self.assign_in()?;
        }
        self.expect(")")?;
        if self.eat(".") {
            let q = self.entity_name(false)?;
            kids.add(Field::Qualifier, q);
        }
        if self.is("<") && !self.tok.nl {
            self.type_args(&mut kids, Field::TypeArg)?;
        }
        Ok(self.finish(Kind::ImportType, start, kids))
    }

    fn template_type(&mut self) -> PResult<u32> {
        let start = self.start();
        let mut kids = Kids::default();
        let single = self.tok.tok == Tok::Template;
        let q = self.token_text();
        kids.add(Field::Quasi, q);
        if !single {
            loop {
                let t = self.type_()?;
                kids.add(Field::Type, t);
                if !self.is("}") {
                    return self.fail("expected `}` in a template literal type");
                }
                self.rescan_template();
                let tail = self.tok.tok == Tok::TemplateTail;
                let q = self.token_text();
                kids.add(Field::Quasi, q);
                if tail {
                    break;
                }
            }
        }
        Ok(self.finish(Kind::TemplateType, start, kids))
    }

    fn tuple_type(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect("[")?;
        let mut kids = Kids::default();
        while !self.is("]") {
            let estart = self.start();
            let rest = self.is("...");
            if rest {
                self.bump();
            }
            let named = self.tok.tok == Tok::Ident && {
                let n = self.peek();
                matches!(n.tok, Tok::P(":")) || (matches!(n.tok, Tok::P("?")) && {
                    let mut lx = self.lx.clone();
                    lx.next_token();
                    matches!(lx.next_token().tok, Tok::P(":"))
                })
            };
            let mut m = Kids::default();
            if named {
                let name = self.leaf(Kind::Ident);
                m.add(Field::Name, name);
                if self.is("?") {
                    let q = self.token_text();
                    m.add(Field::Optional, q);
                }
                self.expect(":")?;
            }
            let t = self.type_()?;
            m.add(Field::Type, t);
            if !named && self.is("?") {
                let q = self.token_text();
                m.add(Field::Optional, q);
            }
            let e = if rest {
                self.finish(Kind::Rest, estart, m)
            } else {
                self.finish(Kind::TupleMember, estart, m)
            };
            kids.add(Field::Element, e);
            if !self.eat(",") {
                break;
            }
        }
        self.expect("]")?;
        Ok(self.finish(Kind::TupleType, start, kids))
    }

    /// `{ readonly [K in T as N]?: V }`
    fn mapped_type(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect("{")?;
        let mut kids = Kids::default();
        if self.is("+") || self.is("-") {
            let s = self.start();
            self.bump();
            self.expect_word("readonly")?;
            let m = self.text_node(s, self.prev_end);
            kids.add(Field::Modifier, m);
        } else if self.is_word("readonly") {
            let m = self.token_text();
            kids.add(Field::Modifier, m);
        }
        self.expect("[")?;
        let pstart = self.start();
        if self.tok.tok != Tok::Ident {
            return self.fail("expected a mapped type parameter");
        }
        let name = self.leaf(Kind::Ident);
        self.expect_word("in")?;
        let constraint = self.type_()?;
        let mut p = Kids::default();
        p.add(Field::Name, name);
        p.add(Field::Constraint, constraint);
        let param = self.finish(Kind::TypeParam, pstart, p);
        kids.add(Field::TypeParam, param);
        if self.eat_word("as") {
            let n = self.type_()?;
            kids.add(Field::Name, n);
        }
        self.expect("]")?;
        if self.is("+") || self.is("-") {
            let s = self.start();
            self.bump();
            self.expect("?")?;
            let q = self.text_node(s, self.prev_end);
            kids.add(Field::Optional, q);
        } else if self.is("?") {
            let q = self.token_text();
            kids.add(Field::Optional, q);
        }
        if self.eat(":") {
            let t = self.type_()?;
            kids.add(Field::Type, t);
        }
        let _ = self.eat(";") || self.eat(",");
        self.expect("}")?;
        Ok(self.finish(Kind::Mapped, start, kids))
    }

    /// `{ a: T; b(): U; [k: string]: V; new (): W }`, as `member` fields.
    pub(super) fn type_members(&mut self, kids: &mut Kids) -> PResult<()> {
        self.expect("{")?;
        while !self.is("}") {
            let m = self.type_member()?;
            kids.add(Field::Member, m);
            let separated = self.eat(";") || self.eat(",");
            if !separated && !self.tok.nl && !self.is("}") {
                return self.fail("expected `;` between members");
            }
        }
        self.expect("}")?;
        Ok(())
    }

    fn type_member(&mut self) -> PResult<u32> {
        let start = self.start();
        if self.is("(") || self.is("<") {
            return self.call_sig(start, Kids::default());
        }
        if self.is_word("new") && matches!(self.peek().tok, Tok::P("(" | "<")) {
            let n = self.token_text();
            let mut kids = Kids::default();
            kids.add(Field::Kind, n);
            return self.call_sig(start, kids);
        }
        let mut mods = Kids::default();
        if self.is_word("readonly") && self.word_is_modifier() {
            let r = self.token_text();
            mods.add(Field::Modifier, r);
        }
        if self.is("[") {
            let m = Kids(mods.0.clone());
            if let Some(sig) = self.try_parse(|p| p.index_sig(start, m)) {
                return Ok(sig);
            }
        }
        let mut kind = None;
        if (self.is_word("get") || self.is_word("set")) && self.word_is_modifier() {
            kind = Some(self.token_text());
        }
        let key = self.property_key()?;
        let optional = if self.is("?") { Some(self.token_text()) } else { None };
        if self.is("(") || self.is("<") {
            let mut kids = mods;
            kids.opt(Field::Kind, kind);
            kids.add(Field::Key, key);
            kids.opt(Field::Optional, optional);
            if self.is("<") {
                self.type_params(&mut kids)?;
            }
            self.params(&mut kids)?;
            if self.eat(":") {
                let r = self.return_type()?;
                kids.add(Field::ReturnType, r);
            }
            return Ok(self.finish(Kind::MethodSig, start, kids));
        }
        let mut kids = mods;
        kids.add(Field::Key, key);
        kids.opt(Field::Optional, optional);
        if self.eat(":") {
            let t = self.type_()?;
            kids.add(Field::Type, t);
        }
        Ok(self.finish(Kind::PropSig, start, kids))
    }

    fn call_sig(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        if self.is("<") {
            self.type_params(&mut kids)?;
        }
        self.params(&mut kids)?;
        if self.eat(":") {
            let r = self.return_type()?;
            kids.add(Field::ReturnType, r);
        }
        Ok(self.finish(Kind::CallSig, start, kids))
    }

    /// `[key: string]: T` (modifiers already in `kids`).
    pub(super) fn index_sig(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        self.expect("[")?;
        let pstart = self.start();
        if self.tok.tok != Tok::Ident {
            return self.fail("expected an index signature parameter");
        }
        let name = self.leaf(Kind::Ident);
        self.expect(":")?;
        let t = self.type_()?;
        let mut p = Kids::default();
        p.add(Field::Name, name);
        p.add(Field::Type, t);
        let param = self.finish(Kind::Param, pstart, p);
        kids.add(Field::Param, param);
        self.expect("]")?;
        if self.eat(":") {
            let t = self.type_()?;
            kids.add(Field::Type, t);
        }
        Ok(self.finish(Kind::IndexSig, start, kids))
    }

    /// `<T extends C = D, const U, in out V>`
    pub(super) fn type_params(&mut self, kids: &mut Kids) -> PResult<()> {
        self.expect("<")?;
        while !self.is(">") {
            let start = self.start();
            let mut p = Kids::default();
            while self.tok.tok == Tok::Ident && matches!(self.cur(), "const" | "in" | "out") && self.peek().tok == Tok::Ident {
                let m = self.token_text();
                p.add(Field::Modifier, m);
            }
            if self.tok.tok != Tok::Ident {
                return self.fail("expected a type parameter");
            }
            let name = self.leaf(Kind::Ident);
            p.add(Field::Name, name);
            if self.eat_word("extends") {
                let c = self.type_()?;
                p.add(Field::Constraint, c);
            }
            if self.eat("=") {
                let d = self.type_()?;
                p.add(Field::Default, d);
            }
            let param = self.finish(Kind::TypeParam, start, p);
            kids.add(Field::TypeParam, param);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(">")?;
        Ok(())
    }

    /// `<A, B>`, each added under `field`.
    pub(super) fn type_args(&mut self, kids: &mut Kids, field: Field) -> PResult<()> {
        self.expect("<")?;
        while !self.is(">") {
            let t = self.type_()?;
            kids.add(field, t);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(">")?;
        Ok(())
    }
}

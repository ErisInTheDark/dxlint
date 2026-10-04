//! JSX in `.tsx`/`.jsx` files: elements, fragments, attributes and
//! children. Text between tags is not JavaScript, so it is scanned raw and
//! the token after a `>` or `}` that opens text is never lexed.

use super::super::kinds::{Field, Kind};
use super::super::lexer::Tok;
use super::{Kids, PResult, Parser};

impl<'s> Parser<'s> {
    /// An element or fragment at the current `<`, as an expression.
    pub(super) fn jsx_element(&mut self) -> PResult<u32> {
        self.jsx_tag(false)
    }

    /// `<T,>(x) => x` and `<T extends U>(x) => x`: how `.tsx` writes a
    /// generic arrow function (any other `<` starts JSX).
    pub(super) fn jsx_generic_arrow_ahead(&self) -> bool {
        let mut lx = self.lx.clone();
        if lx.next_token().tok != Tok::Ident {
            return false;
        }
        let next = lx.next_token();
        next.tok == Tok::P(",") || (next.tok == Tok::Ident && self.text(next) == "extends")
    }

    /// `in_text`: the tag is a child, and its parent's text follows it.
    fn jsx_tag(&mut self, in_text: bool) -> PResult<u32> {
        self.enter()?;
        let r = self.jsx_tag_inner(in_text);
        self.leave();
        r
    }

    fn jsx_tag_inner(&mut self, in_text: bool) -> PResult<u32> {
        let start = self.start();
        self.expect("<")?;
        let mut kids = Kids::default();
        if self.is(">") {
            self.jsx_gt(true);
            self.jsx_children(&mut kids)?;
            self.expect("<")?;
            self.expect("/")?;
            if !self.is(">") {
                return self.fail("expected `>` closing the fragment");
            }
            self.jsx_gt(in_text);
            return Ok(self.finish(Kind::JsxFragment, start, kids));
        }
        let name = self.jsx_element_name()?;
        let src = self.src;
        let name_text = &src[self.nodes[name as usize].start as usize..self.prev_end as usize];
        kids.add(Field::Name, name);
        if self.is("<") {
            self.type_args(&mut kids, Field::TypeArg)?;
        }
        while !self.is("/") && !self.is(">") {
            let a = self.jsx_attribute()?;
            kids.add(Field::Property, a);
        }
        if self.eat("/") {
            if !self.is(">") {
                return self.fail("expected `>`");
            }
            self.jsx_gt(in_text);
            return Ok(self.finish(Kind::JsxElement, start, kids));
        }
        self.jsx_gt(true);
        self.jsx_children(&mut kids)?;
        self.expect("<")?;
        self.expect("/")?;
        let closing_start = self.start();
        if self.tok.tok != Tok::Ident {
            return self.fail("expected a closing tag name");
        }
        // The whole closing name (`a.b`, `a:b`) is one plain name: only the
        // opening tag references a component.
        // Spacing (line breaks too) may surround the `.` / `:`.
        let b = self.src.as_bytes();
        let skip = |mut at: usize| {
            while b.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            at
        };
        let mut end = closing_start as usize;
        loop {
            end = self.lx.jsx_name_end(end);
            let dot = skip(end);
            let next = skip(dot + 1);
            if matches!(b.get(dot), Some(b'.' | b':')) && b.get(next).is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, b'_' | b'$')) {
                end = next;
            } else {
                break;
            }
        }
        let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        if bare(&src[closing_start as usize..end]) != bare(name_text) {
            return self.fail_at(closing_start, &format!("expected corresponding JSX closing tag for `{name_text}`"));
        }
        let closing = self.jsx_name_node(closing_start, end);
        kids.add(Field::Closing, closing);
        if !self.is(">") {
            return self.fail("expected `>`");
        }
        self.jsx_gt(in_text);
        Ok(self.finish(Kind::JsxElement, start, kids))
    }

    /// Consumes the current `>`; when text follows, leaves the lexer right
    /// after it for [`Self::jsx_children`].
    fn jsx_gt(&mut self, text_follows: bool) {
        if text_follows {
            self.prev_end = self.tok.end;
            self.lx.pos = self.tok.end as usize;
        } else {
            self.bump();
        }
    }

    /// Children up to the parent's closing `</`, which is the current token
    /// afterwards.
    fn jsx_children(&mut self, kids: &mut Kids) -> PResult<()> {
        loop {
            let from = self.prev_end;
            let Some(stop) = self.lx.jsx_text_end(from as usize) else {
                return self.fail_at(from, "unterminated JSX contents");
            };
            if stop > from as usize {
                let text = self.finish_at(Kind::JsxText, from, stop as u32, Kids::default());
                kids.add(Field::Body, text);
            }
            self.lx.pos = stop;
            self.tok = self.lx.next_token();
            if self.is("{") {
                let start = self.start();
                self.bump();
                let mut inner = Kids::default();
                if self.is("...") {
                    let s = self.spread()?;
                    inner.add(Field::Expr, s);
                } else if !self.is("}") {
                    let e = self.expression_in()?;
                    inner.add(Field::Expr, e);
                }
                if !self.is("}") {
                    return self.fail("expected `}`");
                }
                self.jsx_gt(true);
                let e = self.finish(Kind::JsxExpression, start, inner);
                kids.add(Field::Body, e);
            } else if self.peek().tok == Tok::P("/") {
                return Ok(());
            } else {
                let e = self.jsx_tag(true)?;
                kids.add(Field::Body, e);
            }
        }
    }

    /// One name part (`div`, `aria-label`, `Foo`) at the current token.
    fn jsx_name_part(&mut self) -> PResult<(u32, u32)> {
        if self.tok.tok != Tok::Ident {
            return self.fail("expected a JSX name");
        }
        let start = self.tok.start;
        let end = self.lx.jsx_name_end(start as usize);
        self.lx.pos = end;
        self.prev_end = end as u32;
        self.tok = self.lx.next_token();
        Ok((start, end as u32))
    }

    fn jsx_name_node(&mut self, start: u32, end: usize) -> u32 {
        self.lx.pos = end;
        self.prev_end = end as u32;
        self.tok = self.lx.next_token();
        self.finish_at(Kind::JsxName, start, end as u32, Kids::default())
    }

    /// An opening tag's name. Like eslint-scope, components (`Foo`, `_x`,
    /// `a.b`, `this`) are references; intrinsic tags (`div`, `a-b`) and
    /// namespaced names (`svg:rect`) are plain names.
    fn jsx_element_name(&mut self) -> PResult<u32> {
        let start = self.start();
        let (s, e) = self.jsx_name_part()?;
        if self.is(":") {
            self.bump();
            let (_, e) = self.jsx_name_part()?;
            return Ok(self.finish_at(Kind::JsxName, start, e, Kids::default()));
        }
        let src = self.src;
        let text = &src[s as usize..e as usize];
        let leaf = |p: &mut Self, kind| p.finish_at(kind, s, e, Kids::default());
        if !self.is(".") {
            let kind = match text {
                "this" => Kind::This,
                _ if text.starts_with(|c: char| c.is_ascii_lowercase()) || text.contains('-') => Kind::JsxName,
                _ => Kind::Ident,
            };
            return Ok(leaf(self, kind));
        }
        let mut object = leaf(self, if text == "this" { Kind::This } else { Kind::Ident });
        while self.eat(".") {
            let (ps, pe) = self.jsx_name_part()?;
            let prop = self.finish_at(Kind::Ident, ps, pe, Kids::default());
            let mut kids = Kids::default();
            kids.add(Field::Object, object);
            kids.add(Field::Property, prop);
            object = self.finish_at(Kind::Member, start, pe, kids);
        }
        Ok(object)
    }

    /// `name`, `name="text"`, `name={expr}`, `name=<el/>` or `{...expr}`.
    fn jsx_attribute(&mut self) -> PResult<u32> {
        let start = self.start();
        if self.eat("{") {
            self.expect("...")?;
            let arg = self.assign_in()?;
            self.expect("}")?;
            let mut kids = Kids::default();
            kids.add(Field::Arg, arg);
            return Ok(self.finish(Kind::JsxSpreadAttribute, start, kids));
        }
        let (_, mut end) = self.jsx_name_part()?;
        if self.is(":") {
            self.bump();
            end = self.jsx_name_part()?.1;
        }
        let name = self.finish_at(Kind::JsxName, start, end, Kids::default());
        let mut kids = Kids::default();
        kids.add(Field::Name, name);
        if self.is("=") {
            let value = self.jsx_attribute_value()?;
            kids.add(Field::Value, value);
        }
        Ok(self.finish(Kind::JsxAttribute, start, kids))
    }

    /// After `=`. Attribute strings have no escapes and may span lines, so
    /// they are scanned raw.
    fn jsx_attribute_value(&mut self) -> PResult<u32> {
        let b = self.src.as_bytes();
        let mut at = self.tok.end as usize;
        while at < b.len() && b[at].is_ascii_whitespace() {
            at += 1;
        }
        if let Some(&quote) = b.get(at).filter(|&&c| c == b'"' || c == b'\'') {
            let Some(close) = memchr::memchr(quote, &b[at + 1..]) else {
                return self.fail_at(at as u32, "unterminated string");
            };
            let end = at + 2 + close;
            self.lx.pos = end;
            self.prev_end = end as u32;
            self.tok = self.lx.next_token();
            return Ok(self.finish_at(Kind::String, at as u32, end as u32, Kids::default()));
        }
        self.bump();
        if self.is("<") {
            return self.jsx_tag(false);
        }
        let start = self.start();
        self.expect("{")?;
        let e = self.assign_in()?;
        self.expect("}")?;
        let mut kids = Kids::default();
        kids.add(Field::Expr, e);
        Ok(self.finish(Kind::JsxExpression, start, kids))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn member_names_may_break_lines_around_their_dots() {
        for src in ["<Form\n.Input />", "<Form.\nInput />", "<Form . Input>x</Form\n.Input>", "<a:b></a : b>"] {
            let r = super::super::super::parse(&format!("const x = {src};"), true);
            assert!(r.is_ok(), "{src}: {:?}", r.err());
        }
        assert!(super::super::super::parse("const x = <A.B></A.C>;", true).is_err());
    }
}

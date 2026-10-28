//! Statements, declarations, classes and modules.

use super::super::kinds::{Field, Kind};
use super::super::lexer::Tok;
use super::{Kids, PResult, Parser};

/// Words that start a declaration after `declare`/`export`.
const DECL_WORDS: &[&str] =
    &["var", "let", "const", "function", "class", "interface", "type", "enum", "namespace", "module", "abstract", "global", "async", "using"];

impl<'s> Parser<'s> {
    pub(super) fn statement(&mut self) -> PResult<u32> {
        self.enter()?;
        let r = self.statement_inner();
        self.leave();
        r
    }

    fn statement_inner(&mut self) -> PResult<u32> {
        let start = self.start();
        match self.tok.tok {
            Tok::P("{") => return self.block(),
            Tok::P(";") => {
                self.bump();
                return Ok(self.finish(Kind::Empty, start, Kids::default()));
            }
            Tok::P("@") => {
                let mut kids = Kids::default();
                self.decorators(&mut kids)?;
                return self.declaration_after_decorators(start, kids);
            }
            Tok::Ident => {}
            _ => return self.expression_statement(),
        }
        let word = self.cur();
        let (next, next_text) = self.peek_text();
        match word {
            "var" | "const" if !(word == "const" && next_text == "enum") => self.var_statement(Kids::default()),
            "let" if matches!(next.tok, Tok::Ident | Tok::P("[" | "{")) => self.var_statement(Kids::default()),
            "using" if next.tok == Tok::Ident && !next.nl && next_text != "in" && next_text != "of" => {
                self.var_statement(Kids::default())
            }
            "await" if next_text == "using" && !next.nl => self.var_statement(Kids::default()),
            "const" => self.enum_(start, Kids::default()),
            "function" => self.function(start, Kids::default()),
            "async" if next_text == "function" && !next.nl => {
                let a = self.token_text();
                let mut kids = Kids::default();
                kids.add(Field::Modifier, a);
                self.function(start, kids)
            }
            "class" => self.class(start, Kids::default()),
            "if" => self.if_(),
            "for" => self.for_(),
            "while" => {
                self.bump();
                let test = self.paren_expr()?;
                let body = self.statement()?;
                let mut kids = Kids::default();
                kids.add(Field::Test, test);
                kids.add(Field::Body, body);
                Ok(self.finish(Kind::While, start, kids))
            }
            "do" => {
                self.bump();
                let body = self.statement()?;
                self.expect_word("while")?;
                let test = self.paren_expr()?;
                self.eat(";");
                let mut kids = Kids::default();
                kids.add(Field::Body, body);
                kids.add(Field::Test, test);
                Ok(self.finish(Kind::DoWhile, start, kids))
            }
            "return" | "throw" => {
                let kind = if word == "return" { Kind::Return } else { Kind::Throw };
                self.bump();
                let mut kids = Kids::default();
                if !self.tok.nl && !self.is(";") && !self.is("}") && self.tok.tok != Tok::Eof {
                    let e = self.expression()?;
                    kids.add(Field::Arg, e);
                }
                self.semi()?;
                Ok(self.finish(kind, start, kids))
            }
            "break" | "continue" => {
                let kind = if word == "break" { Kind::Break } else { Kind::Continue };
                self.bump();
                let mut kids = Kids::default();
                if !self.tok.nl && self.tok.tok == Tok::Ident {
                    let l = self.leaf(Kind::Ident);
                    kids.add(Field::Label, l);
                }
                self.semi()?;
                Ok(self.finish(kind, start, kids))
            }
            "switch" => self.switch(),
            "try" => self.try_(),
            "debugger" => {
                self.bump();
                self.semi()?;
                Ok(self.finish(Kind::Debugger, start, Kids::default()))
            }
            "with" => {
                self.bump();
                let object = self.paren_expr()?;
                let body = self.statement()?;
                let mut kids = Kids::default();
                kids.add(Field::Object, object);
                kids.add(Field::Body, body);
                Ok(self.finish(Kind::With, start, kids))
            }
            "import" if !matches!(next.tok, Tok::P("(" | ".")) => self.import(),
            "export" => self.export(),
            "interface" if next.tok == Tok::Ident && !next.nl => self.interface(start, Kids::default()),
            "type" if next.tok == Tok::Ident && !next.nl => self.type_alias(start, Kids::default()),
            "enum" if next.tok == Tok::Ident => self.enum_(start, Kids::default()),
            "namespace" | "module" if !next.nl && matches!(next.tok, Tok::Ident | Tok::Str) => {
                self.namespace(start, Kids::default())
            }
            "abstract" if next_text == "class" && !next.nl => {
                let a = self.token_text();
                let mut kids = Kids::default();
                kids.add(Field::Modifier, a);
                self.class(start, kids)
            }
            "declare" if !next.nl && next.tok == Tok::Ident && DECL_WORDS.contains(&next_text) => {
                let d = self.token_text();
                let mut kids = Kids::default();
                kids.add(Field::Modifier, d);
                self.declaration(start, kids)
            }
            "global" if next.tok == Tok::P("{") => self.namespace(start, Kids::default()),
            _ if next.tok == Tok::P(":") && self.is_binding_ident() => {
                let label = self.leaf(Kind::Ident);
                self.bump();
                let body = self.statement()?;
                let mut kids = Kids::default();
                kids.add(Field::Label, label);
                kids.add(Field::Body, body);
                Ok(self.finish(Kind::Labeled, start, kids))
            }
            _ => self.expression_statement(),
        }
    }

    /// A declaration after `declare` or `export` (their modifiers in `kids`).
    fn declaration(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        let word = self.cur();
        let next_text = self.peek_text().1;
        match word {
            "var" | "let" | "using" => self.var_statement(kids),
            "await" => self.var_statement(kids),
            "const" if next_text == "enum" => self.enum_(start, kids),
            "const" => self.var_statement(kids),
            "function" => self.function(start, kids),
            "async" => {
                let a = self.token_text();
                kids.add(Field::Modifier, a);
                self.function(start, kids)
            }
            "class" => self.class(start, kids),
            "abstract" => {
                let a = self.token_text();
                kids.add(Field::Modifier, a);
                self.class(start, kids)
            }
            "interface" => self.interface(start, kids),
            "type" => self.type_alias(start, kids),
            "enum" => self.enum_(start, kids),
            "namespace" | "module" | "global" => self.namespace(start, kids),
            "declare" => {
                let d = self.token_text();
                kids.add(Field::Modifier, d);
                self.declaration(start, kids)
            }
            _ => self.fail("expected a declaration"),
        }
    }

    fn declaration_after_decorators(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        if self.is_word("export") {
            // `@dec export class X`: keep the export, decorators on the class;
            // the export spans them too, so it holds its class.
            self.bump();
            let mut ekids = Kids::default();
            if self.is_word("default") {
                let d = self.token_text();
                ekids.add(Field::Kind, d);
            }
            let class = self.declaration_after_decorators(start, kids)?;
            ekids.add(Field::Decl, class);
            return Ok(self.finish(Kind::Export, start, ekids));
        }
        while self.is_word("abstract") || self.is_word("declare") {
            let m = self.token_text();
            kids.add(Field::Modifier, m);
        }
        self.class(start, kids)
    }

    pub(super) fn block(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect("{")?;
        let mut kids = Kids::default();
        while !self.is("}") {
            if self.tok.tok == Tok::Eof {
                return self.fail("expected `}`");
            }
            let s = self.statement()?;
            kids.add(Field::Body, s);
        }
        self.bump();
        Ok(self.finish(Kind::Block, start, kids))
    }

    fn expression_statement(&mut self) -> PResult<u32> {
        let start = self.start();
        let e = self.expression()?;
        self.semi()?;
        let mut kids = Kids::default();
        kids.add(Field::Expr, e);
        Ok(self.finish(Kind::ExprStmt, start, kids))
    }

    fn paren_expr(&mut self) -> PResult<u32> {
        self.expect("(")?;
        let e = self.expression_in()?;
        self.expect(")")?;
        Ok(e)
    }

    // ---- variables -----------------------------------------------------

    /// `var`/`let`/`const`/`using`/`await using` declarations, with `;`.
    fn var_statement(&mut self, kids: Kids) -> PResult<u32> {
        let v = self.var_decl(kids)?;
        self.semi()?;
        self.nodes[v as usize].end = self.prev_end;
        Ok(v)
    }

    /// The declaration list without its `;` (also used in `for` heads).
    fn var_decl(&mut self, mut kids: Kids) -> PResult<u32> {
        let start = kids.0.first().map_or(self.start(), |e| self.nodes[e.node as usize].start);
        let kstart = self.start();
        if self.is_word("await") {
            self.bump();
        }
        self.bump();
        let kind = self.text_node(kstart, self.prev_end);
        kids.add(Field::Kind, kind);
        loop {
            let dstart = self.start();
            let name = self.binding()?;
            let mut d = Kids::default();
            d.add(Field::Name, name);
            if self.is("!") {
                self.bump();
            }
            if self.eat(":") {
                let t = self.type_()?;
                d.add(Field::Type, t);
            }
            if self.eat("=") {
                let i = self.assign()?;
                d.add(Field::Init, i);
            }
            let decl = self.finish(Kind::Declarator, dstart, d);
            kids.add(Field::Decl, decl);
            if !self.eat(",") {
                break;
            }
        }
        Ok(self.finish(Kind::Var, start, kids))
    }

    // ---- control flow --------------------------------------------------

    fn if_(&mut self) -> PResult<u32> {
        let start = self.start();
        self.bump();
        let test = self.paren_expr()?;
        let consequent = self.statement()?;
        let mut kids = Kids::default();
        kids.add(Field::Test, test);
        kids.add(Field::Consequent, consequent);
        if self.eat_word("else") {
            let alternate = self.statement()?;
            kids.add(Field::Alternate, alternate);
        }
        Ok(self.finish(Kind::If, start, kids))
    }

    fn for_(&mut self) -> PResult<u32> {
        let start = self.start();
        self.bump();
        let mut kids = Kids::default();
        if self.is_word("await") {
            let a = self.token_text();
            kids.add(Field::Modifier, a);
        }
        self.expect("(")?;
        let mut init = None;
        if !self.is(";") {
            self.no_in = true;
            let (next, next_text) = self.peek_text();
            let is_decl = matches!(self.cur(), "var" | "const")
                || (self.is_word("let") && matches!(next.tok, Tok::Ident | Tok::P("[" | "{")) && next_text != "of" && next_text != "in")
                || (self.is_word("using") && next.tok == Tok::Ident && next_text != "of" && next_text != "in")
                || (self.is_word("await") && next_text == "using");
            let r = if is_decl { self.var_decl(Kids::default()) } else { self.expression() };
            self.no_in = false;
            init = Some(r?);
        }
        if let Some(left) = init {
            if self.is_word("of") || self.is_word("in") {
                let kind = if self.is_word("of") { Kind::ForOf } else { Kind::ForIn };
                self.bump();
                let right = if kind == Kind::ForOf { self.assign_in()? } else { self.expression_in()? };
                self.expect(")")?;
                let body = self.statement()?;
                kids.add(Field::Left, left);
                kids.add(Field::Right, right);
                kids.add(Field::Body, body);
                return Ok(self.finish(kind, start, kids));
            }
        }
        kids.opt(Field::Init, init);
        self.expect(";")?;
        if !self.is(";") {
            let t = self.expression_in()?;
            kids.add(Field::Test, t);
        }
        self.expect(";")?;
        if !self.is(")") {
            let u = self.expression_in()?;
            kids.add(Field::Update, u);
        }
        self.expect(")")?;
        let body = self.statement()?;
        kids.add(Field::Body, body);
        Ok(self.finish(Kind::For, start, kids))
    }

    fn switch(&mut self) -> PResult<u32> {
        let start = self.start();
        self.bump();
        let disc = self.paren_expr()?;
        let mut kids = Kids::default();
        kids.add(Field::Discriminant, disc);
        self.expect("{")?;
        while !self.is("}") {
            let cstart = self.start();
            let mut c = Kids::default();
            if self.eat_word("case") {
                let t = self.expression_in()?;
                c.add(Field::Test, t);
            } else {
                self.expect_word("default")?;
            }
            self.expect(":")?;
            while !self.is("}") && !self.is_word("case") && !self.is_word("default") {
                if self.tok.tok == Tok::Eof {
                    return self.fail("expected `}`");
                }
                let s = self.statement()?;
                c.add(Field::Body, s);
            }
            let c = self.finish(Kind::Case, cstart, c);
            kids.add(Field::Case, c);
        }
        self.bump();
        Ok(self.finish(Kind::Switch, start, kids))
    }

    fn try_(&mut self) -> PResult<u32> {
        let start = self.start();
        self.bump();
        let mut kids = Kids::default();
        let b = self.block()?;
        kids.add(Field::Block, b);
        let kstart = self.start();
        if self.eat_word("catch") {
            // The `catch` keyword: where the clause starts.
            let k = self.text_node(kstart, self.prev_end);
            kids.add(Field::Kind, k);
            if self.eat("(") {
                let p = self.binding()?;
                kids.add(Field::Param, p);
                if self.eat(":") {
                    let t = self.type_()?;
                    kids.add(Field::Type, t);
                }
                self.expect(")")?;
            }
            let h = self.block()?;
            kids.add(Field::Handler, h);
        }
        if self.eat_word("finally") {
            let f = self.block()?;
            kids.add(Field::Finalizer, f);
        }
        Ok(self.finish(Kind::Try, start, kids))
    }

    // ---- classes -------------------------------------------------------

    /// `class` onwards (decorators and modifiers already in `kids`).
    pub(super) fn class(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        self.expect_word("class")?;
        if self.tok.tok == Tok::Ident && !matches!(self.cur(), "extends" | "implements") {
            let name = self.leaf(Kind::Ident);
            kids.add(Field::Name, name);
        }
        if self.is("<") {
            self.type_params(&mut kids)?;
        }
        if self.eat_word("extends") {
            self.heritage = true;
            let e = self.lhs()?;
            kids.add(Field::Extends, e);
            if self.is("<") {
                self.type_args(&mut kids, Field::TypeArg)?;
            }
        }
        if self.eat_word("implements") {
            loop {
                let t = self.type_()?;
                kids.add(Field::Implements, t);
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect("{")?;
        while !self.is("}") {
            if self.eat(";") {
                continue;
            }
            if self.tok.tok == Tok::Eof {
                return self.fail("expected `}`");
            }
            let m = self.class_member()?;
            kids.add(Field::Member, m);
        }
        self.bump();
        Ok(self.finish(Kind::Class, start, kids))
    }

    fn class_member(&mut self) -> PResult<u32> {
        let start = self.start();
        let mut kids = Kids::default();
        self.decorators(&mut kids)?;
        if self.is_word("static") && self.peek().tok == Tok::P("{") {
            self.bump();
            let b = self.block()?;
            let mut s = Kids::default();
            s.add(Field::Body, b);
            return Ok(self.finish(Kind::StaticBlock, start, s));
        }
        const MODS: &[&str] =
            &["public", "private", "protected", "static", "readonly", "abstract", "override", "declare", "accessor", "async"];
        while self.tok.tok == Tok::Ident
            && MODS.contains(&self.cur())
            && if self.is_word("static") { self.word_is_accessor() } else { self.word_is_modifier() }
        {
            let m = self.token_text();
            kids.add(Field::Modifier, m);
        }
        if self.is("[") {
            let m = super::Kids(kids.0.clone());
            if let Some(sig) = self.try_parse(|p| {
                let s = p.index_sig(start, m)?;
                p.semi()?;
                Ok(s)
            }) {
                return Ok(sig);
            }
        }
        if self.is("*") {
            let s = self.token_text();
            kids.add(Field::Modifier, s);
        }
        if (self.is_word("get") || self.is_word("set")) && self.word_is_accessor() {
            let k = self.token_text();
            kids.add(Field::Kind, k);
        }
        let key = if self.is_word("constructor") && self.peek().tok == Tok::P("(") {
            self.leaf(Kind::Ident)
        } else {
            self.property_key()?
        };
        let optional = if self.is("?") { Some(self.token_text()) } else { None };
        if self.is("(") || self.is("<") {
            return self.method_rest(start, kids, key, optional);
        }
        kids.add(Field::Key, key);
        kids.opt(Field::Optional, optional);
        self.eat("!");
        if self.eat(":") {
            let t = self.type_()?;
            kids.add(Field::Type, t);
        }
        if self.eat("=") {
            let i = self.assign_in()?;
            kids.add(Field::Init, i);
        }
        self.semi()?;
        Ok(self.finish(Kind::ClassProp, start, kids))
    }

    // ---- type declarations ---------------------------------------------

    fn interface(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        self.expect_word("interface")?;
        let name = self.leaf(Kind::Ident);
        kids.add(Field::Name, name);
        if self.is("<") {
            self.type_params(&mut kids)?;
        }
        if self.eat_word("extends") {
            loop {
                let t = self.type_()?;
                kids.add(Field::Extends, t);
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.type_members(&mut kids)?;
        Ok(self.finish(Kind::Interface, start, kids))
    }

    fn type_alias(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        self.expect_word("type")?;
        let name = self.leaf(Kind::Ident);
        kids.add(Field::Name, name);
        if self.is("<") {
            self.type_params(&mut kids)?;
        }
        self.expect("=")?;
        let t = self.type_()?;
        kids.add(Field::Type, t);
        self.semi()?;
        Ok(self.finish(Kind::TypeAlias, start, kids))
    }

    fn enum_(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        if self.is_word("const") {
            let c = self.token_text();
            kids.add(Field::Modifier, c);
        }
        self.expect_word("enum")?;
        let name = self.leaf(Kind::Ident);
        kids.add(Field::Name, name);
        self.expect("{")?;
        while !self.is("}") {
            let mstart = self.start();
            let key = self.property_key()?;
            let mut m = Kids::default();
            m.add(Field::Name, key);
            if self.eat("=") {
                let i = self.assign_in()?;
                m.add(Field::Init, i);
            }
            let m = self.finish(Kind::EnumMember, mstart, m);
            kids.add(Field::Member, m);
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        Ok(self.finish(Kind::Enum, start, kids))
    }

    fn namespace(&mut self, start: u32, mut kids: Kids) -> PResult<u32> {
        let k = self.token_text();
        kids.add(Field::Kind, k);
        let global = self.node_text(k) == "global";
        if !global {
            let nstart = self.start();
            let mut name = match self.tok.tok {
                Tok::Str => self.leaf(Kind::String),
                _ => self.leaf(Kind::Ident),
            };
            while self.eat(".") {
                let right = self.leaf(Kind::Ident);
                let mut q = Kids::default();
                q.add(Field::Left, name);
                q.add(Field::Right, right);
                name = self.finish(Kind::QualifiedName, nstart, q);
            }
            kids.add(Field::Name, name);
        }
        if self.is("{") {
            let b = self.block()?;
            kids.add(Field::Body, b);
        } else {
            // `declare module "x";`
            self.semi()?;
        }
        Ok(self.finish(Kind::Namespace, start, kids))
    }

    // ---- modules -------------------------------------------------------

    fn import(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect_word("import")?;
        let mut kids = Kids::default();
        let (next, next_text) = self.peek_text();
        // `import type from "x"` imports a default named `type`.
        if self.is_word("type") && (matches!(next.tok, Tok::P("{" | "*")) || (next.tok == Tok::Ident && next_text != "from")) {
            let t = self.token_text();
            kids.add(Field::Kind, t);
        }
        if self.tok.tok == Tok::Str {
            let from = self.leaf(Kind::String);
            kids.add(Field::From, from);
            self.import_attributes()?;
            self.semi()?;
            return Ok(self.finish(Kind::Import, start, kids));
        }
        if self.tok.tok == Tok::Ident {
            let name = self.leaf(Kind::Ident);
            if self.eat("=") {
                // `import x = require("m")` / `import x = A.B`
                let value = self.lhs()?;
                self.semi()?;
                let mut k = kids;
                k.add(Field::Name, name);
                k.add(Field::Value, value);
                return Ok(self.finish(Kind::ImportEquals, start, k));
            }
            kids.add(Field::DefaultImport, name);
            self.eat(",");
        }
        if self.eat("*") {
            self.expect_word("as")?;
            let ns = self.leaf(Kind::Ident);
            kids.add(Field::Namespace, ns);
        } else if self.is("{") {
            self.bump();
            while !self.is("}") {
                let sstart = self.start();
                let mut s = Kids::default();
                let (n, nt) = self.peek_text();
                if self.is_word("type") && matches!(n.tok, Tok::Ident | Tok::Str) && nt != "as" {
                    let t = self.token_text();
                    s.add(Field::Kind, t);
                }
                let imported = if self.tok.tok == Tok::Str { self.leaf(Kind::String) } else { self.leaf(Kind::Ident) };
                s.add(Field::Imported, imported);
                if self.eat_word("as") {
                    let local = self.leaf(Kind::Ident);
                    s.add(Field::Local, local);
                }
                let s = self.finish(Kind::ImportSpec, sstart, s);
                kids.add(Field::Specifier, s);
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("}")?;
        }
        self.expect_word("from")?;
        if self.tok.tok != Tok::Str {
            return self.fail("expected a module name");
        }
        let from = self.leaf(Kind::String);
        kids.add(Field::From, from);
        self.import_attributes()?;
        self.semi()?;
        Ok(self.finish(Kind::Import, start, kids))
    }

    /// `with { type: "json" }`, which may follow a line break, or the older
    /// `assert { ... }`, which may not. Their structure is the pattern layer
    /// `ts_import_attributes`.
    fn import_attributes(&mut self) -> PResult<()> {
        if !(self.is_word("with") || self.is_word("assert") && !self.tok.nl) {
            return Ok(());
        }
        self.bump();
        self.expect("{")?;
        let mut depth = 1;
        while depth > 0 {
            match self.tok.tok {
                Tok::P("{") => depth += 1,
                Tok::P("}") => depth -= 1,
                Tok::Eof => return self.fail("expected `}`"),
                _ => {}
            }
            self.bump();
        }
        Ok(())
    }

    fn export(&mut self) -> PResult<u32> {
        let start = self.start();
        self.expect_word("export")?;
        let mut kids = Kids::default();
        if self.eat("=") {
            let e = self.expression()?;
            self.semi()?;
            let mut k = Kids::default();
            k.add(Field::Expr, e);
            return Ok(self.finish(Kind::ExportAssign, start, k));
        }
        if self.is_word("as") && self.peek_text().1 == "namespace" {
            self.bump();
            self.bump();
            let n = self.leaf(Kind::Ident);
            kids.add(Field::Namespace, n);
            self.semi()?;
            return Ok(self.finish(Kind::Export, start, kids));
        }
        if self.is_word("default") {
            let d = self.token_text();
            kids.add(Field::Kind, d);
            let dstart = self.start();
            let (next, next_text) = self.peek_text();
            let decl = match self.cur() {
                "function" => Some(self.function(dstart, Kids::default())?),
                "async" if next_text == "function" && !next.nl => {
                    let a = self.token_text();
                    let mut k = Kids::default();
                    k.add(Field::Modifier, a);
                    Some(self.function(dstart, k)?)
                }
                "class" => Some(self.class(dstart, Kids::default())?),
                "abstract" if next_text == "class" => {
                    let a = self.token_text();
                    let mut k = Kids::default();
                    k.add(Field::Modifier, a);
                    Some(self.class(dstart, k)?)
                }
                "interface" if next.tok == Tok::Ident => Some(self.interface(dstart, Kids::default())?),
                _ => None,
            };
            match decl {
                Some(d) => kids.add(Field::Decl, d),
                None => {
                    let e = self.assign()?;
                    self.semi()?;
                    kids.add(Field::Expr, e);
                }
            }
            return Ok(self.finish(Kind::Export, start, kids));
        }
        if self.is_word("import") {
            let i = self.import()?;
            kids.add(Field::Decl, i);
            return Ok(self.finish(Kind::Export, start, kids));
        }
        let type_only = self.is_word("type") && matches!(self.peek().tok, Tok::P("{" | "*"));
        if type_only {
            let t = self.token_text();
            kids.add(Field::Kind, t);
        }
        if self.is("*") {
            let operator = self.token_text();
            kids.add(Field::Op, operator);
            if self.eat_word("as") {
                let n = if self.tok.tok == Tok::Str { self.leaf(Kind::String) } else { self.leaf(Kind::Ident) };
                kids.add(Field::Namespace, n);
            }
            self.expect_word("from")?;
            let from = self.leaf(Kind::String);
            kids.add(Field::From, from);
            self.import_attributes()?;
            self.semi()?;
            return Ok(self.finish(Kind::Export, start, kids));
        }
        if self.eat("{") {
            while !self.is("}") {
                let sstart = self.start();
                let mut s = Kids::default();
                if self.is_word("type") && matches!(self.peek().tok, Tok::Ident | Tok::Str) && self.peek_text().1 != "as" {
                    let t = self.token_text();
                    s.add(Field::Kind, t);
                }
                let local = if self.tok.tok == Tok::Str { self.leaf(Kind::String) } else { self.leaf(Kind::Ident) };
                s.add(Field::Local, local);
                if self.eat_word("as") {
                    let e = if self.tok.tok == Tok::Str { self.leaf(Kind::String) } else { self.leaf(Kind::Ident) };
                    s.add(Field::Exported, e);
                }
                let s = self.finish(Kind::ExportSpec, sstart, s);
                kids.add(Field::Specifier, s);
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("}")?;
            if self.eat_word("from") {
                let from = self.leaf(Kind::String);
                kids.add(Field::From, from);
                self.import_attributes()?;
            }
            self.semi()?;
            return Ok(self.finish(Kind::Export, start, kids));
        }
        let dstart = self.start();
        let decl = if self.is("@") {
            let mut k = Kids::default();
            self.decorators(&mut k)?;
            self.declaration_after_decorators(dstart, k)?
        } else {
            self.declaration(dstart, Kids::default())?
        };
        kids.add(Field::Decl, decl);
        Ok(self.finish(Kind::Export, start, kids))
    }
}

#[cfg(test)]
mod tests {
    use crate::native::ts::{kinds::{Field, Kind}, parse};

    #[test]
    fn export_wildcards_keep_type_and_default_kinds() {
        for (source, wildcard, kind) in [
            ("export * from 'mod';", true, None),
            ("export /* wildcard */ * from 'mod';", true, None),
            ("export type * from 'mod';", true, Some("type")),
            ("export * as ns from 'mod';", true, None),
            ("export type * as ns from 'mod';", true, Some("type")),
            ("export {} from 'mod';", false, None),
            ("export type {} from 'mod';", false, Some("type")),
            ("export { name } from 'mod';", false, None),
            ("export default value;", false, Some("default")),
            ("export const product = left * right;", false, None),
            ("export as namespace ns;", false, None),
        ] {
            let parsed = parse(source, false).unwrap_or_else(|error| panic!("{source}: {error:?}"));
            let export = parsed.nodes.iter().find(|node| node.matcher == Kind::Export as u32).expect("export");
            let word = |field| export.children.iter().find(|edge| edge.label == field as u16)
                .map(|edge| {
                    let node = &parsed.nodes[edge.node as usize];
                    &source[node.start as usize..node.end as usize]
                });
            assert_eq!(word(Field::Op), wildcard.then_some("*"), "{source}");
            assert_eq!(word(Field::Kind), kind, "{source}");
        }
    }

    #[test]
    fn nodes_hold_their_children() {
        for src in ["@dec\nexport class A {}", "@a() @b\nexport default class {}", "export @dec class C {}", "@dec class D {}"] {
            let parsed = parse(src, false).unwrap_or_else(|error| panic!("{src}: {error:?}"));
            for node in &parsed.nodes {
                for edge in &node.children {
                    let child = &parsed.nodes[edge.node as usize];
                    assert!(node.start <= child.start && child.end <= node.end, "{src}: {}..{} outside {}..{}", child.start, child.end, node.start, node.end);
                }
            }
        }
    }

    #[test]
    fn class_modifiers_preserve_line_break_semantics() {
        let members = |src: &str| {
            let parsed = parse(src, false).unwrap_or_else(|error| panic!("{src}: {error:?}"));
            let class = parsed.nodes.iter().find(|node| node.matcher == Kind::Class as u32).expect("class");
            let ids: Vec<_> = class.children.iter().filter(|edge| edge.label == Field::Member as u16).map(|edge| edge.node).collect();
            (parsed, ids)
        };
        for newline in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            for (member, kind) in [("method() {}", Kind::Method), ("[key]() {}", Kind::Method), ("field;", Kind::ClassProp)] {
                let src = format!("class C {{ static{newline}{member} }}");
                let (parsed, ids) = members(&src);
                assert_eq!(ids.len(), 1, "{src}");
                let node = &parsed.nodes[ids[0] as usize];
                assert_eq!(node.matcher, kind as u32, "{src}");
                assert_eq!(node.start as usize, src.find("static").unwrap(), "{src}");
                let modifier = node.children.iter().find(|edge| edge.label == Field::Modifier as u16).expect("static modifier");
                let modifier = &parsed.nodes[modifier.node as usize];
                assert_eq!(&src[modifier.start as usize..modifier.end as usize], "static");
            }
        }
        for modifier in ["async", "public", "private", "protected", "readonly", "abstract", "override", "declare", "accessor"] {
            let src = format!("class C {{ {modifier}\nmethod() {{}} }}");
            let (parsed, ids) = members(&src);
            assert_eq!(ids.len(), 2, "{src}");
            assert_eq!(parsed.nodes[ids[0] as usize].matcher, Kind::ClassProp as u32, "{src}");
            assert_eq!(parsed.nodes[ids[1] as usize].matcher, Kind::Method as u32, "{src}");
            assert!(ids.iter().all(|id| parsed.nodes[*id as usize].children.iter().all(|edge| edge.label != Field::Modifier as u16)), "{src}");
        }
        for member in ["static;", "static() {}", "static = 1;"] {
            let src = format!("class C {{ {member} }}");
            let (parsed, ids) = members(&src);
            assert_eq!(ids.len(), 1, "{src}");
            assert!(parsed.nodes[ids[0] as usize].children.iter().all(|edge| edge.label != Field::Modifier as u16), "{src}");
        }
    }

    #[test]
    fn import_attributes_may_follow_a_line_break_but_assert_may_not() {
        // The statements of the program, as source text.
        let statements = |src: &str| {
            let parsed = parse(src, false).unwrap_or_else(|error| panic!("{src}: {error:?}"));
            let program = parsed.nodes.iter().find(|node| node.matcher == Kind::Program as u32).expect("program");
            program.children.iter().filter(|edge| edge.label == Field::Body as u16)
                .map(|edge| src[parsed.nodes[edge.node as usize].start as usize..parsed.nodes[edge.node as usize].end as usize].to_owned())
                .collect::<Vec<_>>()
        };
        for src in ["import a from 'a.json'\n  with { type: 'json' };", "export * from 'a'\nwith { type: 'json' };", "export { a } from 'a' assert { type: 'json' };"] {
            assert_eq!(statements(src), [src], "{src}");
        }
        assert_eq!(statements("import a from 'a.json'\nassert\n"), ["import a from 'a.json'", "assert"]);
    }
}

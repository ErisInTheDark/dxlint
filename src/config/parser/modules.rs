use super::super::ast::modules::{Asking, Definition, Item, Mode, Setting, SettingValue};
use super::super::ast::{Expr, ExprKind, Name};
use super::super::lexer::{decode_escapes, Tk};
use super::Parser;

impl Parser<'_> {
    pub(super) fn module_item(&mut self, what: &str) -> Option<Item> {
        if what == "merges" {
            let classes = self.name_list("a declaration class");
            return (!classes.is_empty()).then_some(Item::Merges { classes });
        }
        if what == "uses" {
            if !self.word("module_resolution") {
                self.error_here("expected `uses module_resolution <name>`");
                return None;
            }
            return Some(Item::Uses(self.ident("a module resolution name")?));
        }
        let first = what == "globals" && self.word("first");
        let all = self.word("all");
        let classes = if all && self.ts.peek_raw(0).is_some_and(|t| t.is_ident("from")) {
            Vec::new()
        } else {
            self.name_list("a declaration class")
        };
        if !all && classes.is_empty() {
            return None;
        }
        let target = if all { None } else { Some(self.module_operand()?) };
        let module = if self.word("from") {
            if !self.word("module") {
                self.error_here("expected `from module <source>`");
                return None;
            }
            Some(self.module_operand()?)
        } else {
            None
        };
        if all && module.is_none() {
            self.error_here(format!("expected `{what} all [classes] from module <source>`"));
            return None;
        }
        let named = if what == "globals" && self.word("named") { Some(self.module_operand()?) } else { None };
        let alias = if self.word("as") { Some(self.module_operand()?) } else { None };
        let asking = self.module_asking()?;
        if what == "globals" {
            if alias.is_some() {
                self.error_here("globals do not take an alias");
                return None;
            }
            return Some(Item::Globals { first, classes, target, module, named, asking });
        }
        Some(Item::Exports { classes, target, module, alias, asking })
    }

    pub(super) fn module_scope(&mut self, classes: Vec<Name>) -> Option<Item> {
        let name = self.module_operand()?;
        let module = if self.word("from") {
            if !self.word("module") {
                self.error_here("expected `from module <source>`");
                return None;
            }
            Some(self.module_operand()?)
        } else {
            None
        };
        Some(Item::Scope { classes, name, module })
    }

    pub(super) fn module_resolves(&mut self, classes: Vec<Name>) -> Option<Item> {
        let binding = self.module_operand()?;
        if !self.word("in") || !self.word("module") {
            self.error_here("expected `resolves <classes> <binding> in module <source>`");
            return None;
        }
        let module = self.module_operand()?;
        let imported = if self.word("as") { Some(self.module_operand()?) } else { None };
        let asking = self.module_asking()?;
        Some(Item::Resolves { classes, binding, module, imported, asking })
    }

    fn module_asking(&mut self) -> Option<Option<Asking>> {
        if !self.word("asking") { return Some(None) }
        if self.word("all") { return Some(Some(Asking::All)) }
        let classes = self.name_list("a source declaration class");
        if classes.is_empty() { return None }
        Some(Some(Asking::Classes(classes)))
    }

    pub(super) fn module_resolves_ahead(&mut self) -> bool {
        let mut index = 0;
        while let Some(token) = self.ts.peek_raw(index) {
            if matches!(token.kind, Tk::Newline | Tk::RBrace) {
                break;
            }
            if token.is_ident("in") && self.ts.peek_raw(index + 1).is_some_and(|next| next.is_ident("module")) {
                return true;
            }
            index += 1;
        }
        false
    }

    fn module_operand(&mut self) -> Option<Expr> {
        let left = self.operand()?;
        self.skip(false, true);
        if !self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Nullish) {
            return Some(left);
        }
        self.ts.next_raw();
        let right = self.module_operand()?;
        let span = left.span.to(right.span);
        Some(Expr { kind: ExprKind::Nullish(Box::new(left), Box::new(right)), span })
    }

    pub(super) fn module_resolution(&mut self) -> Option<Definition> {
        let name = self.ident("a module resolution name")?;
        self.expect(Tk::LBrace, "`{` after the module resolution name")?;
        let mut settings = Vec::new();
        let mut modes = Vec::new();
        loop {
            let Some(token) = self.peek() else {
                self.error_here("unclosed module resolution block");
                return None;
            };
            if token.kind == Tk::RBrace {
                self.bump();
                break;
            }
            if self.word("when") {
                let when = self.expr()?;
                self.expect(Tk::LBrace, "`{` after the mode condition")?;
                modes.push(Mode { when, settings: self.module_settings()? });
            } else {
                settings.push(self.module_setting()?);
            }
        }
        Some(Definition { name, settings, modes })
    }

    fn module_settings(&mut self) -> Option<Vec<Setting>> {
        let mut settings = Vec::new();
        loop {
            let Some(token) = self.peek() else {
                self.error_here("unclosed module settings block");
                return None;
            };
            if token.kind == Tk::RBrace {
                self.bump();
                return Some(settings);
            }
            settings.push(self.module_setting()?);
        }
    }

    fn module_setting(&mut self) -> Option<Setting> {
        let token = self.bump()?;
        let key = match token.kind {
            Tk::Ident => Name { text: token.text.to_string(), span: token.span },
            Tk::Str => match decode_escapes(&token.text, token.span) {
                Ok(text) => Name { text, span: token.span },
                Err(error) => {
                    self.errors.push(error);
                    return None;
                }
            },
            _ => {
                self.error_here("expected a module resolution setting");
                return None;
            },
        };
        self.skip(false, true);
        let value = if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::LBrace) {
            self.bump();
            SettingValue::Block(self.module_settings()?)
        } else {
            let value = SettingValue::Literal(self.expr()?);
            self.end_of_item();
            value
        };
        Some(Setting { key, value })
    }
}

use super::{Parser, Tk};
use crate::config::ast::effects::{Flow, Item, Path};

impl Parser<'_> {
    pub(super) fn execution_item(&mut self, keyword: &str) -> Option<Item> {
        match keyword {
            "assigned" => {
                let target = self.name_path("an assignment target")?;
                let value = if self.word("with") { Some(self.unwrap_value()?) } else { None };
                Some(Item::Assigned { target, value })
            }
            "runs" => self.flow_or().map(Item::Runs),
            "labels" => self.name_path("a label field").map(Item::Labels),
            "in" => {
                let field = self.name_path("an execution field")?;
                self.execution_field(field)
            }
            _ => None,
        }
    }

    pub(super) fn execution_field(&mut self, field: Path) -> Option<Item> {
        if self.word("catches") {
            let effects = self.name_list("an effect");
            if effects.is_empty() { return None }
            if !self.word("from") {
                self.error_here("expected `from`: `in handler catches thrown from block`");
                return None;
            }
            let source = self.name_path("the source field")?;
            Some(Item::Catches { handler: field, effects, source })
        } else if self.word("runs") {
            if self.word("separately") {
                Some(Item::Entry(field))
            } else if self.word("after") {
                let mut sources = Vec::new();
                loop {
                    sources.push(self.name_path("a field whose exits run the finalizer")?);
                    if !self.on_line(Tk::Comma) { break }
                    self.ts.next_raw();
                }
                Some(Item::After { finalizer: field, sources })
            } else {
                self.error_here("expected `separately` or `after` following a field's `runs`");
                None
            }
        } else {
            self.error_here("expected `catches` or `runs` following an execution field");
            None
        }
    }

    fn flow_or(&mut self) -> Option<Flow> {
        let first = self.flow_then()?;
        if !self.word("or") { return Some(first) }
        let mut parts = vec![first];
        loop {
            parts.push(self.flow_then()?);
            if !self.word("or") { break }
        }
        Some(Flow::Or(parts))
    }

    fn flow_then(&mut self) -> Option<Flow> {
        let first = self.flow_atom()?;
        if !self.word("then") { return Some(first) }
        let mut parts = vec![first];
        loop {
            parts.push(self.flow_atom()?);
            if !self.word("then") { break }
        }
        Some(Flow::Then(parts))
    }

    fn flow_atom(&mut self) -> Option<Flow> {
        self.skip(false, true);
        if self.on_line(Tk::LParen) {
            self.ts.next_raw();
            let flow = self.flow_or()?;
            self.expect(Tk::RParen, "`)` closing the execution flow")?;
            return Some(flow);
        }
        if self.word("nothing") { return Some(Flow::Nothing) }
        if self.word("maybe") { return self.flow_atom().map(|flow| Flow::Maybe(Box::new(flow))) }
        if self.word("repeat") {
            let body = Box::new(self.flow_atom()?);
            let test = if self.word("while") { Some(self.name_path("a loop test field")?) } else { None };
            let mut first = false;
            let mut next = None;
            loop {
                if self.word("first") {
                    if first {
                        self.error_here("an execution repetition has only one first round");
                        return None;
                    }
                    first = true;
                } else if self.word("next") {
                    if next.is_some() {
                        self.error_here("an execution repetition has only one update flow");
                        return None;
                    }
                    next = Some(Box::new(self.flow_atom()?));
                } else {
                    break;
                }
            }
            return Some(Flow::Repeat { body, test, next, first });
        }
        if self.word("enter") {
            if !self.word("one") || !self.word("of") {
                self.error_here("expected `one of` after `enter`");
                return None;
            }
            let entries = self.name_path("the selection list")?;
            if !self.word("testing") {
                self.error_here("expected `testing` after the selection list");
                return None;
            }
            let test = self.name_path("each entry's selection test")?;
            if !self.word("then") {
                self.error_here("expected `then` after the selection test");
                return None;
            }
            let body = self.name_path("each entry's body")?;
            return Some(Flow::Select { entries, test, body });
        }
        self.name_path("an execution field").map(Flow::Field)
    }
}

use std::sync::OnceLock;

use crate::config::ast::modules::Item as ParsedItem;
use crate::config::ast::{Config, Expr};

#[derive(Clone, Copy, Debug)]
pub enum Part {
    Target,
    Module,
    Alias,
    Binding,
    Imported,
    ScopeName,
}

pub struct Operand {
    pub item: usize,
    pub matcher: u32,
    pub part: Part,
    pub expr: OnceLock<crate::expr::CExpr>,
    pub lowered: OnceLock<crate::vm::VExpr>,
}

#[derive(Clone, Debug)]
pub enum Action {
    Exports { classes: u64, target: Option<u32>, module: Option<u32>, alias: Option<u32> },
    Globals { classes: u64, target: u32 },
    Resolves { classes: u64, binding: u32, module: u32, imported: Option<u32> },
    Scope { classes: u64, name: u32 },
    Uses(Box<str>),
}

#[derive(Clone, Debug)]
pub struct Conditional {
    pub action: Action,
    pub when: Option<u32>,
}

#[derive(Default, Clone, Debug)]
pub struct KindModules {
    pub items: Vec<Conditional>,
}

#[derive(Default)]
pub struct Tables {
    pub operands: Vec<Operand>,
}

impl Tables {
    pub fn lower(&mut self, item: &ParsedItem, index: usize, matcher: u32, classes: u64, when: Option<u32>) -> Conditional {
        let action = match item {
            ParsedItem::Exports { target, module, alias, .. } => Action::Exports {
                classes,
                target: target.as_ref().map(|_| self.operand(index, matcher, Part::Target)),
                module: module.as_ref().map(|_| self.operand(index, matcher, Part::Module)),
                alias: alias.as_ref().map(|_| self.operand(index, matcher, Part::Alias)),
            },
            ParsedItem::Globals { .. } => Action::Globals { classes, target: self.operand(index, matcher, Part::Target) },
            ParsedItem::Resolves { imported, .. } => Action::Resolves {
                classes,
                binding: self.operand(index, matcher, Part::Binding),
                module: self.operand(index, matcher, Part::Module),
                imported: imported.as_ref().map(|_| self.operand(index, matcher, Part::Imported)),
            },
            ParsedItem::Scope { .. } => Action::Scope { classes, name: self.operand(index, matcher, Part::ScopeName) },
            ParsedItem::Uses(name) => Action::Uses(name.text.clone().into()),
        };
        Conditional { action, when }
    }

    fn operand(&mut self, item: usize, matcher: u32, part: Part) -> u32 {
        let index = self.operands.len() as u32;
        self.operands.push(Operand { item, matcher, part, expr: OnceLock::new(), lowered: OnceLock::new() });
        index
    }
}

impl Operand {
    pub fn source<'a>(&self, config: &'a Config) -> Option<&'a Expr> {
        let crate::config::ast::NameItem::Module(item) = &config.names.get(self.item)?.item else { return None };
        match (item, self.part) {
            (ParsedItem::Exports { target, .. }, Part::Target) => target.as_ref(),
            (ParsedItem::Exports { module, .. }, Part::Module) => module.as_ref(),
            (ParsedItem::Exports { alias, .. }, Part::Alias) => alias.as_ref(),
            (ParsedItem::Globals { target, .. }, Part::Target) => Some(target),
            (ParsedItem::Resolves { binding, .. }, Part::Binding) => Some(binding),
            (ParsedItem::Resolves { module, .. }, Part::Module) => Some(module),
            (ParsedItem::Resolves { imported, .. }, Part::Imported) => imported.as_ref(),
            (ParsedItem::Scope { name, .. }, Part::ScopeName) => Some(name),
            _ => None,
        }
    }
}

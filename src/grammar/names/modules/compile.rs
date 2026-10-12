use crate::config::ast::modules::{operand, Item};

#[derive(Clone, Debug)]
pub enum Action {
    Exports { classes: u64, asks: u64, target: Option<u32>, module: Option<u32>, alias: Option<u32> },
    Globals { first: bool, classes: u64, asks: u64, target: Option<u32>, module: Option<u32> },
    Resolves { classes: u64, asks: u64, binding: u32, module: u32, imported: Option<u32> },
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

pub fn lower(item: &Item, classes: u64, asks: u64, when: Option<u32>, operand: &mut dyn FnMut(u8) -> u32) -> Conditional {
    let action = match item {
        Item::Exports { target, module, alias, .. } => Action::Exports {
            classes,
            asks,
            target: target.as_ref().map(|_| operand(operand::TARGET)),
            module: module.as_ref().map(|_| operand(operand::MODULE)),
            alias: alias.as_ref().map(|_| operand(operand::ALIAS)),
        },
        Item::Globals { first, target, module, .. } => Action::Globals {
            first: *first,
            classes,
            asks,
            target: target.as_ref().map(|_| operand(operand::TARGET)),
            module: module.as_ref().map(|_| operand(operand::MODULE)),
        },
        Item::Resolves { imported, .. } => Action::Resolves {
            classes,
            asks,
            binding: operand(operand::BINDING),
            module: operand(operand::MODULE),
            imported: imported.as_ref().map(|_| operand(operand::IMPORTED)),
        },
        Item::Scope { .. } => Action::Scope { classes, name: operand(operand::SCOPE_NAME) },
        Item::Uses(name) => Action::Uses(name.text.clone().into()),
    };
    Conditional { action, when }
}

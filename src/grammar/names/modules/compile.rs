use crate::config::ast::modules::{operand, Item};
pub use super::profiles::compile as profiles;

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: Box<str>,
    pub settings: super::resolution::Settings,
    pub modes: Vec<Mode>,
}

#[derive(Clone, Debug)]
pub struct Mode {
    pub when: u32,
    pub settings: super::resolution::Settings,
}

#[derive(Clone, Debug)]
pub enum Action {
    Exports { classes: u64, asks: u64, target: Option<u32>, module: Option<u32>, alias: Option<u32> },
    Globals { first: bool, classes: u64, asks: u64, target: Option<u32>, module: Option<u32>, named: Option<u32> },
    Resolves { classes: u64, asks: u64, binding: u32, module: u32, imported: Option<u32> },
    Scope { classes: u64, name: u32, module: Option<u32> },
    Uses(Box<str>),
    Merges { classes: u64 },
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
        Item::Globals { first, target, module, named, .. } => Action::Globals {
            first: *first,
            classes,
            asks,
            target: target.as_ref().map(|_| operand(operand::TARGET)),
            module: module.as_ref().map(|_| operand(operand::MODULE)),
            named: named.as_ref().map(|_| operand(operand::NAMED)),
        },
        Item::Resolves { imported, .. } => Action::Resolves {
            classes,
            asks,
            binding: operand(operand::BINDING),
            module: operand(operand::MODULE),
            imported: imported.as_ref().map(|_| operand(operand::IMPORTED)),
        },
        Item::Scope { module, .. } => Action::Scope {
            classes,
            name: operand(operand::SCOPE_NAME),
            module: module.as_ref().map(|_| operand(operand::MODULE)),
        },
        Item::Uses(name) => Action::Uses(name.text.clone().into()),
        Item::Merges { .. } => Action::Merges { classes },
    };
    Conditional { action, when }
}

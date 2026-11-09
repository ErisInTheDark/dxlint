use super::{Expr, Name};

#[derive(Clone, Debug)]
pub enum Item {
    Exports {
        classes: Vec<Name>,
        target: Option<Expr>,
        module: Option<Expr>,
        alias: Option<Expr>,
        asking: Option<Asking>,
    },
    Globals {
        first: bool,
        classes: Vec<Name>,
        target: Option<Expr>,
        module: Option<Expr>,
        /// The names a computed target can publish, so lookups of other names skip it.
        named: Option<Expr>,
        asking: Option<Asking>,
    },
    Resolves {
        classes: Vec<Name>,
        binding: Expr,
        module: Expr,
        imported: Option<Expr>,
        asking: Option<Asking>,
    },
    Scope {
        classes: Vec<Name>,
        name: Expr,
        module: Option<Expr>,
    },
    Uses(Name),
    Merges { classes: Vec<Name> },
}

impl Item {
    /// The expressions the item evaluates on its nodes.
    pub fn exprs(&self) -> impl Iterator<Item = &Expr> {
        let (a, b, c) = match self {
            Item::Exports { target, module, alias, .. } => (target.as_ref(), module.as_ref(), alias.as_ref()),
            Item::Globals { target, module, named, .. } => (target.as_ref(), module.as_ref(), named.as_ref()),
            Item::Resolves { binding, module, imported, .. } => (Some(binding), Some(module), imported.as_ref()),
            Item::Scope { name, module, .. } => (Some(name), module.as_ref(), None),
            Item::Uses(_) | Item::Merges { .. } => (None, None, None),
        };
        [a, b, c].into_iter().flatten()
    }

    pub fn classes(&self) -> &[Name] {
        match self {
            Item::Exports { classes, .. } | Item::Globals { classes, .. } | Item::Resolves { classes, .. } | Item::Scope { classes, .. } | Item::Merges { classes } => classes,
            Item::Uses(_) => &[],
        }
    }

    pub fn asking(&self) -> Option<&Asking> {
        match self {
            Item::Exports { asking, .. } | Item::Globals { asking, .. } | Item::Resolves { asking, .. } => asking.as_ref(),
            _ => None,
        }
    }

    pub fn operand(&self, part: u8) -> Option<&Expr> {
        match (self, part) {
            (Item::Exports { target, .. } | Item::Globals { target, .. }, operand::TARGET) => target.as_ref(),
            (Item::Exports { module, .. } | Item::Globals { module, .. }, operand::MODULE) => module.as_ref(),
            (Item::Exports { alias, .. }, operand::ALIAS) => alias.as_ref(),
            (Item::Globals { named, .. }, operand::NAMED) => named.as_ref(),
            (Item::Resolves { binding, .. }, operand::BINDING) => Some(binding),
            (Item::Resolves { module, .. }, operand::MODULE) => Some(module),
            (Item::Resolves { imported, .. }, operand::IMPORTED) => imported.as_ref(),
            (Item::Scope { name, .. }, operand::SCOPE_NAME) => Some(name),
            (Item::Scope { module, .. }, operand::MODULE) => module.as_ref(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Asking {
    All,
    Classes(Vec<Name>),
}

pub mod operand {
    pub const TARGET: u8 = 0;
    pub const MODULE: u8 = 1;
    pub const ALIAS: u8 = 2;
    pub const BINDING: u8 = 3;
    pub const IMPORTED: u8 = 4;
    pub const SCOPE_NAME: u8 = 5;
    pub const NAMED: u8 = 6;
}

#[derive(Clone, Debug)]
pub struct Definition {
    pub name: Name,
    pub settings: Vec<Setting>,
    pub modes: Vec<Mode>,
}

#[derive(Clone, Debug)]
pub struct Setting {
    pub key: Name,
    pub value: SettingValue,
}

#[derive(Clone, Debug)]
pub enum SettingValue {
    Literal(Expr),
    Block(Vec<Setting>),
}

#[derive(Clone, Debug)]
pub struct Mode {
    pub when: Expr,
    pub settings: Vec<Setting>,
}

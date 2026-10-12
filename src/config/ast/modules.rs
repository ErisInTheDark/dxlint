use super::{Expr, Name};

#[derive(Clone, Debug)]
pub enum Item {
    Exports {
        classes: Vec<Name>,
        target: Option<Expr>,
        module: Option<Expr>,
        alias: Option<Expr>,
    },
    Globals {
        classes: Vec<Name>,
        target: Expr,
    },
    Resolves {
        classes: Vec<Name>,
        binding: Expr,
        module: Expr,
        imported: Option<Expr>,
    },
    Scope {
        classes: Vec<Name>,
        name: Expr,
    },
    Uses(Name),
}

impl Item {
    /// The expressions the item evaluates on its nodes.
    pub fn exprs(&self) -> impl Iterator<Item = &Expr> {
        let (a, b, c) = match self {
            Item::Exports { target, module, alias, .. } => (target.as_ref(), module.as_ref(), alias.as_ref()),
            Item::Globals { target, .. } => (Some(target), None, None),
            Item::Resolves { binding, module, imported, .. } => (Some(binding), Some(module), imported.as_ref()),
            Item::Scope { name, .. } => (Some(name), None, None),
            Item::Uses(_) => (None, None, None),
        };
        [a, b, c].into_iter().flatten()
    }

    pub fn classes(&self) -> &[Name] {
        match self {
            Item::Exports { classes, .. } | Item::Globals { classes, .. } | Item::Resolves { classes, .. } | Item::Scope { classes, .. } => classes,
            Item::Uses(_) => &[],
        }
    }
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

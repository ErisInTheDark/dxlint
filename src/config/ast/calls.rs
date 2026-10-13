//! Call declarations and structural comparison metadata.

use super::{Expr, Name, Unwrap};

pub type Path = Vec<Name>;

#[derive(Clone, Debug)]
pub enum Item {
    Signature { parameters: Path, receiver: Option<Path>, result: Unwrap },
    Sources(Unwrap),
    Invoke { callee: Unwrap, arguments: Path, construct: bool },
    Expected { field: Path, value: Unwrap, at: ExpectedAt },
    DiscardsResult,
    Identity(Expr),
    Optional,
    ParameterVariance(Variance),
    InferLastOverload,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExpectedAt {
    #[default]
    Owner,
    Subject,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variance {
    #[default]
    Contravariant,
    Bivariant,
}

impl Item {
    pub fn operand(&self, part: u8) -> Option<&Expr> {
        match (self, part) {
            (Item::Identity(value), 0) => Some(value),
            _ => None,
        }
    }
}

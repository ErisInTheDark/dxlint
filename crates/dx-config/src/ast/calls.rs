//! Call declarations and structural comparison metadata.

use super::{Expr, Name, Unwrap};

pub type Path = Vec<Name>;

#[derive(Clone, Debug)]
pub enum Item {
    Signature { parameters: Path, receiver: Option<Path>, result: Unwrap },
    /// `signatures <value> [returns <result>]`: the call signatures of the
    /// value's declarations, each giving `result` (evaluated at the owner)
    /// instead of its own when present (a derived class's inherited
    /// constructors make the derived instance).
    Sources(Unwrap, Option<Unwrap>),
    /// `calls callee with arguments [this receiver]`: without `this`, the
    /// receiver is not checked against a signature's declared one.
    Invoke { callee: Unwrap, arguments: Path, construct: bool, receiver: Option<Unwrap> },
    Expected { field: Path, value: Unwrap, at: ExpectedAt },
    DiscardsResult,
    Identity(Expr),
    Optional,
    /// `flag <name>`: a named fact about the declaration (such as `readonly`),
    /// reported with member sources.
    Flag(Name),
    ParameterVariance(Variance),
    InferLastOverload,
    /// `overloads exact first` (on an invocation): pick among overloads
    /// first with dynamic (`any`) arguments fitting only dynamic or top
    /// parameters, then with ordinary acceptance.
    ExactFirstOverload,
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

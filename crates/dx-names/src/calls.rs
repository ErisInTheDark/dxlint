//! Calls and signatures: the recipes `signatures`, `invokes`, `expected` and
//! their comparison metadata are declared with. Selection and inference over
//! them lives in `dx-semantic`.

use dx_config::ast::calls::{ExpectedAt, Variance};

use super::Value;

pub mod compile;

pub type Path = Vec<u16>;

#[derive(Clone, Debug)]
pub struct Conditional<T> {
    pub item: T,
    pub when: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Signature {
    pub parameters: Path,
    pub receiver: Option<Path>,
    pub result: Value,
}

#[derive(Clone, Debug)]
pub struct Invoke {
    pub callee: Value,
    pub arguments: Path,
    pub construct: bool,
    /// The value a call passes as its receiver; `None` leaves declared
    /// receivers unchecked.
    pub receiver: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct Expected {
    pub field: Path,
    pub value: Value,
    pub at: ExpectedAt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvokeKind { Call, Construct }

#[derive(Clone, Debug, Default)]
pub struct KindCalls {
    pub signatures: Vec<Conditional<Signature>>,
    pub sources: Vec<Conditional<Value>>,
    /// Per source (same index), the `returns` result overriding its
    /// signatures' own; missing entries have none.
    pub source_results: Vec<Option<Value>>,
    pub invokes: Vec<Conditional<Invoke>>,
    pub expected: Vec<Conditional<Expected>>,
    pub discards: Vec<Option<u32>>,
    pub identities: Vec<Conditional<u32>>,
    pub optional: Vec<Option<u32>>,
    pub flags: Vec<Conditional<Box<str>>>,
    pub variances: Vec<Conditional<Variance>>,
    pub infer_last: Vec<Option<u32>>,
    pub exact_first: Vec<Option<u32>>,
}

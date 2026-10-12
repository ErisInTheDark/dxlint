use super::Name;
use crate::config::source::Span;

/// Traits that cannot occur together in one possible value.
#[derive(Clone, Debug)]
pub struct Exclusive {
    pub traits: Vec<Name>,
    pub span: Span,
}

/// Declarative value expansion, member projection and truth constraints.
#[derive(Clone, Debug)]
pub enum Unwrap {
    Path(Vec<Name>),
    Text(String),
    /// The first value that exists; unknown is not absence.
    Either(Box<Unwrap>, Box<Unwrap>),
    Union(Vec<Unwrap>),
    Intersection(Vec<Unwrap>),
    Spread { intersection: bool, path: Vec<Name> },
    From(Box<Unwrap>, Box<Unwrap>, Vec<Name>),
    /// Project owned scopes without unwrapping the source nodes.
    Scope { classes: Vec<Name>, from: Box<Unwrap> },
    Not(Box<Unwrap>),
    Excludes { traits: Vec<Name>, from: Option<Box<Unwrap>> },
    Includes { value: Box<Unwrap>, from: Option<Box<Unwrap>> },
    Has { key: Box<Unwrap>, from: Option<Box<Unwrap>> },
    Ordinal(u32),
    /// An ordinary global declaration with ordered evaluated arguments.
    Instance { name: Name, class: Name, arguments: Vec<Unwrap> },
}

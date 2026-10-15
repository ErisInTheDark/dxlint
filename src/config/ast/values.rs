use super::Name;
use crate::config::source::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyDomain { Text, Ordinal, Symbol }

impl KeyDomain {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "text" => Some(Self::Text),
            "ordinal" => Some(Self::Ordinal),
            "symbol" => Some(Self::Symbol),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum IndexedMode {
    Slots { repeated: bool, strings: bool },
    Domain(KeyDomain),
    Keyed(Unwrap),
}

/// Traits that cannot occur together in one possible value.
#[derive(Clone, Debug)]
pub struct Exclusive {
    pub traits: Vec<Name>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ProductShape {
    pub name: Name,
    pub items: Name,
    pub traits: Vec<Name>,
    pub description: crate::expr::Value<'static>,
    pub inherits: Option<Unwrap>,
    pub readonly_inherits: Option<Unwrap>,
    pub optional: Option<Unwrap>,
}

/// `apparent <trait> inherits <value>`: the members values with the trait
/// read through (a primitive's object wrapper), without becoming their own.
#[derive(Clone, Debug)]
pub struct Apparent {
    pub name: Name,
    pub inherits: Unwrap,
}

/// Declarative value expansion, member projection and truth constraints.
#[derive(Clone, Debug)]
pub enum Unwrap {
    Current,
    Unknown,
    Path(Vec<Name>),
    Text(String),
    Traits(Vec<Name>),
    Returned { default: Box<Unwrap> },
    /// The first value that exists; unknown is not absence.
    Either(Box<Unwrap>, Box<Unwrap>),
    Union(Vec<Unwrap>),
    Intersection(Vec<Unwrap>),
    Spread { intersection: bool, path: Vec<Name>, value: Option<Box<Unwrap>> },
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
    Select {
        check: Box<Unwrap>,
        pattern: Box<Unwrap>,
        yes: Box<Unwrap>,
        no: Box<Unwrap>,
        distribute: Option<Vec<Name>>,
        dynamic_both: bool,
    },
    Product {
        shape: Name,
        sites: Vec<Name>,
        value: Option<Box<Unwrap>>,
        optional: Option<Vec<Name>>,
        repeated: Option<Vec<Name>>,
        /// `repeated values <flag>`: the element itself repeats.
        values: Option<Vec<Name>>,
        readonly: Option<Vec<Name>>,
    },
}

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
    /// One member per key the path's values hold (an indexer per key
    /// domain), with `bind`'s formal bound to that key.
    Keys { bind: Option<Vec<Name>> },
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
    /// `count "<key>"`: the member a fixed product reads its element count
    /// from (a tuple's `length`).
    pub count: Option<String>,
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
    /// `keys of <value>`: the value's member keys, each a literal (`keyof`).
    Keys { of: Box<Unwrap> },
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
    /// `modified <value> [optional add|remove] [readonly add|remove]
    /// [flag <name> add|remove]...`: the value with its members'
    /// requiredness and flags (and its products' readonly flag) changed.
    Modified { source: Box<Unwrap>, required: Option<bool>, readonly: Option<bool>, flags: Vec<(Name, bool)> },
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

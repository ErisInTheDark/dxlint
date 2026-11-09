//! Semantic values: what an `unwraps` value means, its alternatives and the
//! frames they are read through. Evaluation stays in the names layer.

/// A node in some file: `file` indexes the question's file table (0 is the
/// file being linted).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeRef {
    pub file: u32,
    pub node: u32,
}

impl NodeRef {
    pub fn here(node: u32) -> NodeRef {
        NodeRef { file: 0, node }
    }
}

/// A generic instantiation context: the chain of references a node was
/// reached `through` (`Box<string>` while unwrapping `Box`'s members).
/// Frames live in the question context; `FrameId::ROOT` is no instantiation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(pub u32);

impl FrameId {
    pub const ROOT: FrameId = FrameId(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProductShapeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProductId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModifiedId(pub u32);

/// A property key as a value of its own (`keyof`'s members): no syntax node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(pub u32);

/// A scope in some file (a module's root scope, an ambient module, a
/// namespace body): `scope` indexes that file's resolved scopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeRef {
    pub file: u32,
    pub scope: u32,
}

/// What a leaf stands for: a node, or a scope (a module namespace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    Node(NodeRef),
    Scope(ScopeRef),
    Traits(u64),
    Product(ProductId),
    Modified(ModifiedId),
    Key(KeyId),
}

/// A leaf of a value: a node that fits traits (or a namespace scope), seen
/// through a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Leaf {
    pub target: Target,
    pub frame: FrameId,
}

/// What a value unwraps to, in disjunctive form: one of the alternatives
/// (`|`), each all of its leaves at once (`&`). Unknown when the budget ran
/// out or something could not be read.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeValue {
    Alternatives(Rows),
    Unknown,
}

impl NodeValue {
    pub fn never() -> NodeValue {
        NodeValue::rows(Vec::new())
    }
    pub fn leaf(leaf: Leaf) -> NodeValue {
        NodeValue::rows(vec![vec![leaf]])
    }
    pub fn rows(rows: Vec<Vec<Leaf>>) -> NodeValue {
        NodeValue::Alternatives(Rows::from(rows))
    }
}

/// A value's alternatives, shared: one declared type (a 2,000-member enum)
/// reaches every node that reads it, so passing it on copies a pointer.
/// Remembers how far it is already normalized, so passing it on also
/// never normalizes it again.
#[derive(Clone, Debug)]
pub struct Rows {
    rows: std::sync::Arc<Vec<Vec<Leaf>>>,
    normal: Normal,
}

/// How much of normalization a value's rows already went through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Normal {
    Raw,
    /// Duplicates and absorbing tops settled.
    Union,
    /// Also contradictory intersections removed.
    Full,
}

impl Rows {
    pub fn normal(&self) -> Normal {
        self.normal
    }

    /// Union-normal rows of one leaf each that is no trait set are fully
    /// normal: one node site can't contradict itself.
    pub fn fully_normal(&self) -> Option<Rows> {
        match self.normal {
            Normal::Full => Some(self.clone()),
            Normal::Union if self.rows.iter().all(|row| row.len() <= 1 && !row.iter().any(|leaf| matches!(leaf.target, Target::Traits(_)))) =>
                Some(Rows { rows: self.rows.clone(), normal: Normal::Full }),
            _ => None,
        }
    }

    pub fn normalized(rows: Vec<Vec<Leaf>>, normal: Normal) -> Rows {
        Rows { rows: std::sync::Arc::new(rows), normal }
    }

    /// The same rows, known to be normalized at least to `normal`.
    pub fn settled(self, normal: Normal) -> Rows {
        Rows { normal: self.normal.max(normal), ..self }
    }

    pub fn into_vec(self) -> Vec<Vec<Leaf>> {
        std::sync::Arc::unwrap_or_clone(self.rows)
    }
}

impl From<Vec<Vec<Leaf>>> for Rows {
    fn from(rows: Vec<Vec<Leaf>>) -> Rows {
        Rows::normalized(rows, Normal::Raw)
    }
}

impl FromIterator<Vec<Leaf>> for Rows {
    fn from_iter<I: IntoIterator<Item = Vec<Leaf>>>(rows: I) -> Rows {
        Rows::from(rows.into_iter().collect::<Vec<_>>())
    }
}

impl std::ops::Deref for Rows {
    type Target = Vec<Vec<Leaf>>;
    fn deref(&self) -> &Vec<Vec<Leaf>> {
        &self.rows
    }
}

impl PartialEq for Rows {
    fn eq(&self, other: &Rows) -> bool {
        std::sync::Arc::ptr_eq(&self.rows, &other.rows) || self.rows == other.rows
    }
}

impl Eq for Rows {}

impl std::hash::Hash for Rows {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.rows.hash(state)
    }
}

impl IntoIterator for Rows {
    type Item = Vec<Leaf>;
    type IntoIter = std::vec::IntoIter<Vec<Leaf>>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a Rows {
    type Item = &'a Vec<Leaf>;
    type IntoIter = std::slice::Iter<'a, Vec<Leaf>>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductRecipe {
    pub shape: ProductShapeId,
    pub sites: Vec<u16>,
    pub value: Option<Box<Value>>,
    pub optional: Option<Vec<u16>>,
    pub repeated: Option<Vec<u16>>,
    pub values: Option<Vec<u16>>,
    pub readonly: Option<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub check: Value,
    pub pattern: Value,
    pub yes: Value,
    pub no: Value,
    pub distribute: Option<Vec<u16>>,
    pub dynamic_both: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Current,
    Unknown,
    Path(Vec<u16>),
    Text(Box<str>),
    Traits(u64),
    Returned { default: Box<Value> },
    Either(Box<Value>, Box<Value>),
    Union(Vec<Value>),
    Intersection(Vec<Value>),
    Spread { intersection: bool, path: Vec<u16>, value: Option<Box<Value>> },
    /// Scope mask zero uses the source kind's member defaults.
    From(Box<Value>, Box<Value>, u64),
    /// Project scopes without expanding the source values.
    Scope { classes: u64, from: Box<Value> },
    Not(Box<Value>),
    Excludes { traits: u64, from: Option<Box<Value>> },
    Includes { value: Box<Value>, from: Option<Box<Value>> },
    Has { key: Box<Value>, from: Option<Box<Value>> },
    /// The value's member keys, one literal leaf each (`keyof`).
    Keys { of: Box<Value> },
    /// The nth type argument of the value's carriers.
    Argument { index: u32, of: Box<Value> },
    Ordinal(u32),
    Instance { name: Box<str>, classes: u64, arguments: Vec<Value> },
    Select(Box<Selection>),
    Product(ProductRecipe),
    Captured(Box<NodeValue>),
    /// See the names layer's `Modified` value.
    Modified { source: Box<Value>, required: Option<bool>, readonly: Option<bool>, flags: Box<[(Box<str>, bool)]> },
}

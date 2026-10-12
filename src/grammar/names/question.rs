//! What every type-layer question shares (docs/design/type-layers.md
//! "Evaluation"): one work budget per question, so cycles and runaway
//! unwrapping end in `unknown` the same way everywhere, and node references
//! that may point into other files.

use std::cell::Cell;

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

/// An answer that may be unknown: the budget ran out, a cycle was cut, or
/// a declaration could not be read. Rules treat unknown as "cannot tell"
/// (`could` / `always` are false on it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Known<T> {
    Yes(T),
    Unknown,
}

impl<T> Known<T> {
    pub fn known(self) -> Option<T> {
        match self {
            Known::Yes(v) => Some(v),
            Known::Unknown => None,
        }
    }
}

/// One question's work budget: every step (an unwrap, a lookup, a graph
/// edge) spends one unit; when it is gone the question's answer is
/// [`Known::Unknown`]. One owner: questions never nest budgets, a nested
/// step spends from the same one.
pub struct Budget {
    left: Cell<u32>,
}

impl Budget {
    /// The work one question may do. Generous for real code; it exists to
    /// stop cycles and pathological types, not to tune results.
    pub const DEFAULT: u32 = 100_000;

    pub fn new(units: u32) -> Budget {
        Budget { left: Cell::new(units) }
    }

    /// Spends one unit; `false` when the budget is gone.
    pub fn step(&self) -> bool {
        match self.left.get() {
            0 => false,
            n => {
                self.left.set(n - 1);
                true
            }
        }
    }

    pub fn exhausted(&self) -> bool {
        self.left.get() == 0
    }
}

impl Default for Budget {
    fn default() -> Budget {
        Budget::new(Budget::DEFAULT)
    }
}

/// A generic instantiation context: the chain of references a node was
/// reached `through` (`Box<string>` while unwrapping `Box`'s members).
/// Frames live in a [`QueryContext`]; `FrameId::ROOT` is no instantiation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(pub u32);

impl FrameId {
    pub const ROOT: FrameId = FrameId(0);
}

/// Arguments a frame binds that were already evaluated (inferred call
/// arguments, library instantiations): interned in a [`QueryContext`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArgsId(pub u32);

/// One instantiation of a generic declaration (`owner`): through a
/// reference whose ordered type arguments are read in `parent`, or with
/// evaluated `args` (no syntax: inferred or library instantiations).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    pub parent: FrameId,
    pub through: Option<NodeRef>,
    pub owner: NodeRef,
    pub args: Option<ArgsId>,
}

/// What a leaf stands for: a node, or a scope (a module namespace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    Node(NodeRef),
    Scope(ScopeRef),
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
    Alternatives(Vec<Vec<Leaf>>),
    Unknown,
}

impl NodeValue {
    pub fn never() -> NodeValue {
        NodeValue::Alternatives(Vec::new())
    }
    pub fn leaf(leaf: Leaf) -> NodeValue {
        NodeValue::Alternatives(vec![vec![leaf]])
    }
}

/// The files a question can see, for every type-layer engine: file 0 is
/// the linted file; others come in as declarations or module resolution
/// reach them (the engine owns reading, parsing and recorded reads).
pub trait World {
    fn grammar(&self) -> &crate::grammar::Grammar;
    fn tree(&self, file: u32) -> Option<&crate::grammar::Tree>;
    fn text(&self, file: u32) -> Option<&str>;
    /// The file's resolved names (scopes, declarations, references).
    fn names(&self, file: u32) -> Option<&super::Resolved>;
    /// The scope a module specifier names from `importer` (a file's root
    /// scope or an ambient module); `Yes(None)` when nothing matches.
    fn module(&self, importer: u32, specifier: &str, question: &QueryContext) -> Known<Option<ScopeRef>>;
    /// Whether item condition `condition` ([`super::Names::conditions`])
    /// holds at `at` (run on the VM in that node's file; nested questions
    /// see its frame and view).
    fn condition(&self, at: Leaf, condition: u32, semantic: &super::semantic::Semantic<'_>) -> Known<bool>;
    /// The value of module item operand `operand`
    /// ([`super::modules::compile::Tables::operands`]) at `at`, run on the
    /// VM in that node's file like [`World::condition`].
    fn operand(&self, at: Leaf, operand: u32, semantic: &super::semantic::Semantic<'_>) -> Known<crate::expr::Value<'static>>;
    /// The `let` attribute `name` on `node` (`flow_truth`), run on the VM in
    /// that node's file; `Yes(Null)` when the node's layer has none.
    fn attribute(&self, node: NodeRef, name: &str, question: &QueryContext) -> Known<crate::expr::Value<'static>>;
}

/// A scope in some file (a module's root scope, an ambient module, a
/// namespace body): `scope` indexes that file's [`super::Resolved::scopes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeRef {
    pub file: u32,
    pub scope: u32,
}

/// One linted file's question state, shared by every type-layer engine
/// (values, calls, effects, narrowing, modules): the frame arena and the
/// work budget of the question being answered. Memos keyed by
/// (question, [`Leaf`]) live with their engine but are reset with this
/// context; there is no second budget or cycle guard anywhere.
#[derive(Default)]
pub struct QueryContext {
    frames: std::cell::RefCell<Interner<Frame>>,
    args: std::cell::RefCell<Interner<Vec<NodeValue>>>,
    pub budget: Budget,
}

/// Values numbered in arrival order, each once.
struct Interner<T> {
    items: Vec<T>,
    ids: crate::fx_hash::FxHashMap<T, u32>,
}

impl<T> Default for Interner<T> {
    fn default() -> Self {
        Interner { items: Vec::new(), ids: crate::fx_hash::FxHashMap::default() }
    }
}

impl<T: Clone + Eq + std::hash::Hash> Interner<T> {
    fn intern(&mut self, item: T) -> u32 {
        if let Some(&id) = self.ids.get(&item) {
            return id;
        }
        let id = self.items.len() as u32;
        self.items.push(item.clone());
        self.ids.insert(item, id);
        id
    }
}

impl QueryContext {
    /// The frame instantiating `owner` through `through` (syntax arguments
    /// read in `parent`) or with evaluated `args`; interned.
    pub fn bind(&self, parent: FrameId, owner: NodeRef, through: Option<NodeRef>, args: Option<Vec<NodeValue>>) -> FrameId {
        let args = args.map(|args| ArgsId(self.args.borrow_mut().intern(args)));
        FrameId(self.frames.borrow_mut().intern(Frame { parent, through, owner, args }) + 1)
    }

    /// The frame `id` stands for; `None` for [`FrameId::ROOT`].
    pub fn get(&self, id: FrameId) -> Option<Frame> {
        id.0.checked_sub(1).and_then(|at| self.frames.borrow().items.get(at as usize).copied())
    }

    /// A frame's evaluated arguments.
    pub fn arguments(&self, id: ArgsId) -> Option<Vec<NodeValue>> {
        self.args.borrow().items.get(id.0 as usize).cloned()
    }
}

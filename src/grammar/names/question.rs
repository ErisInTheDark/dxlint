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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    pub parent: FrameId,
    /// The reference (`Box<string>`, a call) this frame instantiates through.
    pub through: NodeRef,
}

/// A leaf of a value: a node that fits traits, seen through a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Leaf {
    pub node: NodeRef,
    pub frame: FrameId,
}

/// What a value unwraps to, in disjunctive form: one of the alternatives
/// (`|`), each all of its leaves at once (`&`). Unknown when the budget ran
/// out or something could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// One linted file's question state, shared by every type-layer engine
/// (values, calls, effects, narrowing, modules): the frame arena and the
/// work budget of the question being answered. Memos keyed by
/// (question, [`Leaf`]) live with their engine but are reset with this
/// context; there is no second budget or cycle guard anywhere.
#[derive(Default)]
pub struct QueryContext {
    frames: std::cell::RefCell<Vec<Frame>>,
    pub budget: Budget,
}

impl QueryContext {
    /// The frame for reaching through `through` from `parent` (interned).
    pub fn frame(&self, parent: FrameId, through: NodeRef) -> FrameId {
        let mut frames = self.frames.borrow_mut();
        let frame = Frame { parent, through };
        if let Some(at) = frames.iter().position(|f| *f == frame) {
            return FrameId(at as u32 + 1);
        }
        frames.push(frame);
        FrameId(frames.len() as u32)
    }

    /// The frame `id` stands for; `None` for [`FrameId::ROOT`].
    pub fn get(&self, id: FrameId) -> Option<Frame> {
        id.0.checked_sub(1).and_then(|at| self.frames.borrow().get(at as usize).copied())
    }
}

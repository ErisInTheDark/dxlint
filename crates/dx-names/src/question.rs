//! What every type-layer question shares: one work budget per question, so cycles and runaway
//! unwrapping end in `unknown` the same way everywhere, and node references
//! that may point into other files.

use std::cell::Cell;

pub use dx_ir::semantic::{FrameId, KeyId, Leaf, ModifiedId, NodeRef, NodeValue, Normal, ProductId, ProductShapeId, Rows, ScopeRef, Target};

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

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Known<U> {
        match self {
            Known::Yes(v) => Known::Yes(f(v)),
            Known::Unknown => Known::Unknown,
        }
    }
}

/// One question's work budget: every step (an unwrap, a lookup, a graph
/// edge) spends one unit; when it is gone the question's answer is
/// [`Known::Unknown`]. Each memoised fact a question needs is computed on
/// an allowance of its own ([`Budget::fact`]), so what a fact answers never
/// depends on how much its first asker had left. It also bounds how deeply
/// engines call into each other ([`Budget::nest`]), so a long chain of
/// results through distinct frames ends in unknown instead of exhausting
/// the Rust stack.
pub struct Budget {
    left: Cell<u32>,
    depth: Cell<u32>,
    limited: Cell<bool>,
    /// Whether steps are attributed for `--profile` ([`dx_core::profile::Profile::count_step`]).
    traced: Cell<bool>,
    /// Refusals to go deeper ([`Budget::nest`]): how deep a fact is reached
    /// depends on who asks, so an unknown computed after one must not be
    /// kept ([`Budget::depth_cuts`]).
    depth_cuts: Cell<u64>,
    /// Facts being computed ([`Budget::begin_fact`]), outermost first.
    facts: Cell<u32>,
    /// How many outermost facts have ended: provisional answers kept during
    /// one ([`super::values::Values::keep_provisional`]) expire with it.
    fact_epoch: Cell<u64>,
    /// Graph hops since the last unit they paid ([`Budget::hop`]).
    hops: Cell<u32>,
    /// What the last fact to end spent of its own allowance.
    #[cfg(feature = "work")]
    spent: Cell<u32>,
    /// What this fact had left when a fact it asked cut it.
    #[cfg(feature = "work")]
    left_when_cut: Cell<Option<u32>>,
}

/// Why a fact ([`Budget::fact`]) gave no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// It spent its own allowance: its unknown answer is kept, and its
    /// asker is cut too.
    RanOut,
    /// Its asker had no step left to start it: nothing to keep.
    Unpaid,
    /// It met the depth limit, which depends on its asker: nothing to keep.
    Deep,
}

/// The asker's allowance while a fact runs on its own ([`Budget::begin_fact`]).
#[derive(Clone, Copy, Debug)]
pub struct Asker {
    left: u32,
    limited: bool,
    /// [`Budget::depth_cuts`] when the fact began.
    depth_cuts: u64,
    /// [`Budget::fact_epoch`] when the fact began.
    pub epoch: u64,
    hops: u32,
    #[cfg(feature = "work")]
    units: u32,
    #[cfg(feature = "work")]
    left_when_cut: Option<u32>,
}

/// Holds one level of [`Budget::nest`]; leaving it returns the level.
pub struct Nested<'b>(&'b Cell<u32>);

impl Drop for Nested<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl Budget {
    /// The floor of the allowances once-per-file and once-per-graph facts
    /// run on.
    pub const DEFAULT: u32 = 100_000;
    /// The work one root question may do by default: a question past it is
    /// waste, and speed ranks before parity.
    pub const QUESTION: u32 = dx_core::levers::SEMANTIC_BUDGET_DEFAULT;
    /// How deeply engines may call into each other in one question.
    pub const DEPTH: u32 = 256;

    pub fn new(units: u32) -> Budget {
        Budget { left: Cell::new(units), depth: Cell::new(0), limited: Cell::new(false), traced: Cell::new(false), depth_cuts: Cell::new(0),
            facts: Cell::new(0), fact_epoch: Cell::new(0), hops: Cell::new(0),
            #[cfg(feature = "work")]
            spent: Cell::new(0),
            #[cfg(feature = "work")]
            left_when_cut: Cell::new(None) }
    }

    /// What the last fact to end spent of its own allowance.
    #[cfg(feature = "work")]
    pub fn last_spent(&self) -> u32 {
        self.spent.get()
    }

    /// What the root question has spent so far of its own allowance.
    #[cfg(feature = "work")]
    pub fn root_spent(&self) -> u32 {
        Self::per_question() - self.left_when_cut.get().unwrap_or(self.left.get())
    }

    pub fn trace(&self, on: bool) {
        self.traced.set(on);
    }

    fn limit(&self) {
        self.limited.set(true);
    }

    /// Enters one more level of engine-to-engine calls; `None` (answer
    /// unknown) past [`Budget::DEPTH`]. Engines call it where they recurse
    /// into another engine's question, never per step.
    pub fn nest(&self) -> Option<Nested<'_>> {
        let depth = self.depth.get();
        if depth >= Self::DEPTH {
            self.depth_cuts.set(self.depth_cuts.get() + 1);
            self.limit();
            return None;
        }
        self.depth.set(depth + 1);
        Some(Nested(&self.depth))
    }

    /// Spends one unit; `false` when the budget is gone.
    #[cfg_attr(feature = "work", track_caller)]
    pub fn step(&self) -> bool {
        #[cfg(feature = "work")]
        dx_core::work::step_at(std::panic::Location::caller());
        if self.traced.get() { dx_core::profile::Profile::count_step() }
        match self.left.get() {
            0 => { self.limit(); false }
            n => {
                self.left.set(n - 1);
                true
            }
        }
    }

    /// A step of a walk or a lookup (a built graph, a tree, a kept answer),
    /// nowhere near a semantic step: many pay for one unit.
    pub fn hop(&self) -> bool {
        const HOPS_PER_UNIT: u32 = 32;
        let hops = self.hops.get().wrapping_add(1);
        self.hops.set(hops);
        if hops % HOPS_PER_UNIT == 0 { return self.step() }
        if self.left.get() == 0 { self.limit(); return false }
        true
    }

    /// Spends the rest of the budget: the question ends as one that ran out.
    pub fn exhaust(&self) {
        #[cfg(feature = "work")]
        if self.left_when_cut.get().is_none() && self.left.get() > 0 { self.left_when_cut.set(Some(self.left.get())) }
        self.left.set(0);
        self.limit();
    }

    /// See [`Budget::depth_cuts`] (the field).
    pub fn depth_cuts(&self) -> u64 {
        self.depth_cuts.get()
    }

    pub fn exhausted(&self) -> bool {
        self.left.get() == 0
    }

    pub fn remaining(&self) -> u32 {
        self.left.get()
    }

    /// Computes one memoised fact on an allowance of its own: the facts it
    /// asks have their own, so what it answers never depends on who asked
    /// it first or what they had left. One that ran out cuts its asker, and
    /// every later reader of its kept answer
    /// ([`super::values::Values::read_kept`]).
    pub fn fact<R>(&self, units: u32, work: impl FnOnce() -> R) -> Result<R, Cut> {
        if self.exhausted() {
            self.limit();
            return Err(Cut::Unpaid);
        }
        let result = self.own_allowance(units, work);
        if result.as_ref().err() == Some(&Cut::RanOut) { self.exhaust() }
        result
    }

    /// Computes work sized by what it covers rather than by a question
    /// (module answers, whole-file walks, effects graphs and their walks,
    /// values of very many parts) on an allowance for that size. It never
    /// cuts its asker, cold or kept: its answer is the same whoever meets
    /// it first and whatever they have left.
    pub fn sized<R>(&self, units: u32, work: impl FnOnce() -> R) -> Result<R, Cut> {
        self.own_allowance(units, work)
    }

    fn own_allowance<R>(&self, units: u32, work: impl FnOnce() -> R) -> Result<R, Cut> {
        let asker = self.begin_fact(units);
        let scope = self.traced.get().then(dx_core::profile::Profile::detached_scope);
        let result = work();
        drop(scope);
        match self.end_fact(asker) {
            None => Ok(result),
            Some(cut) => Err(cut),
        }
    }

    /// [`Budget::fact`] for work that can't be one call (the value
    /// evaluator's task loop): starts the fact's allowance, returning the
    /// asker's to hand to [`Budget::end_fact`].
    pub fn begin_fact(&self, units: u32) -> Asker {
        #[cfg(feature = "work")]
        dx_core::work::fact_steps_open();
        self.facts.set(self.facts.get() + 1);
        Asker { left: self.left.replace(units), limited: self.limited.replace(false), depth_cuts: self.depth_cuts.get(), epoch: self.fact_epoch.get(), hops: self.hops.replace(0),
            #[cfg(feature = "work")]
            units,
            #[cfg(feature = "work")]
            left_when_cut: self.left_when_cut.replace(None) }
    }

    /// See [`Budget::fact_epoch`] (the field).
    pub fn fact_epoch(&self) -> u64 {
        self.fact_epoch.get()
    }

    /// Ends a fact begun with [`Budget::begin_fact`]: the asker's allowance
    /// is back as it was (cutting it is the caller's). Why the fact stopped
    /// short, if it did.
    pub fn end_fact(&self, asker: Asker) -> Option<Cut> {
        let limited = self.limited();
        #[cfg(feature = "work")]
        {
            dx_core::work::fact_steps_close();
            self.spent.set(asker.units - self.left_when_cut.get().unwrap_or(self.left.get()));
            self.left_when_cut.set(asker.left_when_cut);
        }
        self.left.set(asker.left);
        self.limited.set(asker.limited);
        self.hops.set(asker.hops);
        self.facts.set(self.facts.get() - 1);
        if self.facts.get() == 0 { self.fact_epoch.set(self.fact_epoch.get() + 1) }
        match (limited, self.depth_cuts.get() != asker.depth_cuts) {
            (false, _) => None,
            (true, true) => Some(Cut::Deep),
            (true, false) => Some(Cut::RanOut),
        }
    }

    /// The work each root question gets in this process: [`Budget::QUESTION`]
    /// unless `--semantic-budget` set it (a diagnostic ruler: questions
    /// doing more than it give up and show in the profile).
    pub fn per_question() -> u32 {
        dx_core::levers::semantic_budget()
    }

    pub fn set_per_question(units: u32) {
        dx_core::levers::set_semantic_budget(units);
    }

    pub fn restart(&self) {
        assert_eq!(self.depth.get(), 0);
        debug_assert_eq!(self.facts.get(), 0, "a fact left open past its root");
        self.left.set(Self::per_question());
        self.limited.set(false);
        #[cfg(feature = "work")]
        self.left_when_cut.set(None);
    }

    pub fn limited(&self) -> bool {
        self.limited.get() || self.exhausted()
    }
}

impl Default for Budget {
    fn default() -> Budget {
        Budget::new(Budget::per_question())
    }
}

/// Arguments a frame binds that were already evaluated (inferred call
/// arguments, library instantiations): interned in a [`QueryContext`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArgsId(pub u32);

/// A value read as `source` with its members' requiredness and declaration
/// flags changed (`unwraps modified`): mapped types keeping their source's
/// keys. The members' own values stay the source's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Modified {
    pub source: Leaf,
    /// `Some(false)`: every member optional; `Some(true)`: every member required.
    pub required: Option<bool>,
    /// Flag names added (`true`) or removed (`false`) on every member.
    pub flags: Box<[(Box<str>, bool)]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProductElement {
    pub site: Leaf,
    pub value: NodeValue,
    pub optional: bool,
    /// A spread collection: its ordinal members repeat here.
    pub repeated: bool,
    /// A directly repeated element: the value itself repeats here
    /// (`[1, 2]` as an array of its elements), never spliced or unpacked.
    pub values: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Product {
    pub shape: ProductShapeId,
    pub source: Leaf,
    /// Built by a recipe with directly repeated elements (an array, even
    /// when empty or only spreads): one carrier argument, the item union.
    pub array: bool,
    pub elements: Vec<ProductElement>,
    pub readonly: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Args {
    Complete(Vec<Option<NodeValue>>),
    Overlay(Vec<Option<NodeValue>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Argument {
    Bound(NodeValue),
    Unbound,
    Inherited,
}

/// One instantiation of a generic declaration (`owner`): through a
/// reference whose ordered type arguments are read in its own frame, or with
/// evaluated `args` (no syntax: inferred or library instantiations).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    pub classes: u64,
    pub parent: FrameId,
    pub through: Option<Leaf>,
    pub owner: NodeRef,
    pub args: Option<ArgsId>,
}

/// A derived fact about values: what a world of a run can reuse from another
/// when no part of it depends on which file is being linted.
#[derive(Clone)]
pub enum FactKey {
    Trait(Leaf, u16),
    Optional(Leaf),
    Closed(Leaf),
    MemberKey(Leaf, super::values::Key),
    Members(Leaf),
    Accepts(NodeValue, NodeValue),
}

#[derive(Clone)]
pub enum Fact {
    Flag(bool),
    Member(Option<super::values::Member>),
    Members(Vec<super::values::Member>),
}

/// One linted file's question state, shared by every type-layer engine
/// (values, calls, effects, narrowing, modules): the frame arena and the
/// work budget of the question being answered. Memos keyed by
/// (question, [`Leaf`]) live with their engine but are reset with this
/// context; there is no second budget or cycle guard anywhere.
#[derive(Default)]
pub struct QueryContext {
    frames: std::cell::RefCell<Interner<Frame>>,
    args: std::cell::RefCell<Interner<Args>>,
    products: std::cell::RefCell<Interner<Product>>,
    modified: std::cell::RefCell<Interner<Modified>>,
    keys: std::cell::RefCell<Interner<super::values::Key>>,
    handles: std::cell::RefCell<Interner<NodeValue>>,
    /// This context's handle namespace, assigned with its first handle.
    owner: std::cell::Cell<u32>,
    /// Frames by the id the world shares them under across contexts
    /// (`None`: not shareable), both ways.
    world_frames: std::cell::RefCell<dx_core::fx_hash::FxHashMap<FrameId, Option<u64>>>,
    frames_by_world: std::cell::RefCell<dx_core::fx_hash::FxHashMap<u64, FrameId>>,
    pub budget: Budget,
}

/// A value handed to rules: valid only in the [`QueryContext`] that made
/// it (as long as that lives), so a handle from another file or thread
/// reads as nothing rather than as another value. Fits an f64 exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(u64);

impl Handle {
    const OWNER_BITS: u32 = 21;

    pub fn number(self) -> f64 { self.0 as f64 }

    pub fn from_number(number: f64) -> Option<Self> {
        let max = ((1u64 << (32 + Self::OWNER_BITS)) - 1) as f64;
        (number >= 0.0 && number <= max && number.fract() == 0.0).then_some(Handle(number as u64))
    }
}

/// Values numbered in arrival order, each once.
struct Interner<T> {
    items: Vec<T>,
    ids: dx_core::fx_hash::FxHashMap<T, u32>,
}

impl<T> Default for Interner<T> {
    fn default() -> Self {
        Interner { items: Vec::new(), ids: dx_core::fx_hash::FxHashMap::default() }
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

/// What [`QueryContext::interned`] moves: every id a session's answers use.
pub struct Interned {
    frames: Interner<Frame>,
    args: Interner<Args>,
    products: Interner<Product>,
    modified: Interner<Modified>,
    keys: Interner<super::values::Key>,
    handles: Interner<NodeValue>,
    owner: u32,
    world_frames: dx_core::fx_hash::FxHashMap<FrameId, Option<u64>>,
    frames_by_world: dx_core::fx_hash::FxHashMap<u64, FrameId>,
}

impl QueryContext {
    /// Moves out everything ids answers keep refer to (frames, keys,
    /// handles), leaving this context empty.
    pub fn interned(&self) -> Interned {
        Interned {
            frames: self.frames.take(),
            args: self.args.take(),
            products: self.products.take(),
            modified: self.modified.take(),
            keys: self.keys.take(),
            handles: self.handles.take(),
            owner: self.owner.replace(0),
            world_frames: self.world_frames.take(),
            frames_by_world: self.frames_by_world.take(),
        }
    }

    /// Takes back what [`QueryContext::interned`] moved out.
    pub fn restore(&self, interned: Interned) {
        self.frames.replace(interned.frames);
        self.args.replace(interned.args);
        self.products.replace(interned.products);
        self.modified.replace(interned.modified);
        self.keys.replace(interned.keys);
        self.handles.replace(interned.handles);
        self.owner.set(interned.owner);
        self.world_frames.replace(interned.world_frames);
        self.frames_by_world.replace(interned.frames_by_world);
    }

    /// A context for once-per-run work (program selection, inputs, roots):
    /// on the detached floor, as a question's budget would leave it unknown
    /// and every later question would redo it.
    pub fn once() -> QueryContext {
        QueryContext { budget: Budget::new(Budget::DEFAULT), ..QueryContext::default() }
    }

    /// One handle per distinct value; callers canonicalise frames first.
    pub fn handle(&self, value: NodeValue) -> Handle {
        static OWNERS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        if self.owner.get() == 0 {
            let owner = OWNERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % ((1 << Handle::OWNER_BITS) - 1) + 1;
            self.owner.set(owner);
        }
        let id = self.handles.borrow_mut().intern(value);
        Handle(u64::from(self.owner.get()) << 32 | u64::from(id))
    }

    pub fn handle_value(&self, handle: Handle) -> Option<NodeValue> {
        if self.owner.get() == 0 || handle.0 >> 32 != u64::from(self.owner.get()) { return None }
        self.handles.borrow().items.get((handle.0 & u64::from(u32::MAX)) as usize).cloned()
    }

    pub fn intern_product(&self, product: Product) -> ProductId {
        ProductId(self.products.borrow_mut().intern(product))
    }

    pub fn product(&self, id: ProductId) -> Option<Product> {
        self.products.borrow().items.get(id.0 as usize).cloned()
    }

    pub fn intern_modified(&self, modified: Modified) -> ModifiedId {
        ModifiedId(self.modified.borrow_mut().intern(modified))
    }

    pub fn modified(&self, id: ModifiedId) -> Option<Modified> {
        self.modified.borrow().items.get(id.0 as usize).cloned()
    }

    pub fn intern_key(&self, key: super::values::Key) -> KeyId {
        KeyId(self.keys.borrow_mut().intern(key))
    }

    pub fn key(&self, id: KeyId) -> Option<super::values::Key> {
        self.keys.borrow().items.get(id.0 as usize).cloned()
    }

    /// The frame instantiating `owner` through `through` (syntax arguments
    /// read in that leaf's frame) or with evaluated `args`; interned.
    pub fn bind(&self, parent: FrameId, owner: NodeRef, through: Option<Leaf>, args: Option<Vec<NodeValue>>) -> FrameId {
        self.bind_definition(parent, owner, through, args, 0)
    }

    pub fn bind_definition(&self, parent: FrameId, owner: NodeRef, through: Option<Leaf>, args: Option<Vec<NodeValue>>, classes: u64) -> FrameId {
        let args = args.map(|args| Args::Complete(args.into_iter().map(Some).collect()));
        self.bind_arguments(parent, owner, through, args, classes)
    }

    pub fn bind_slots(&self, parent: FrameId, owner: NodeRef, through: Option<Leaf>, args: Vec<Option<NodeValue>>, classes: u64) -> FrameId {
        self.bind_arguments(parent, owner, through, Some(Args::Complete(args)), classes)
    }

    pub fn bind_overlay(&self, parent: FrameId, owner: NodeRef, args: Vec<Option<NodeValue>>, classes: u64) -> FrameId {
        self.bind_arguments(parent, owner, None, Some(Args::Overlay(args)), classes)
    }

    fn bind_arguments(&self, parent: FrameId, owner: NodeRef, through: Option<Leaf>, args: Option<Args>, classes: u64) -> FrameId {
        let args = args.map(|args| ArgsId(self.args.borrow_mut().intern(args)));
        FrameId(self.frames.borrow_mut().intern(Frame { parent, through, owner, args, classes }) + 1)
    }

    pub fn owner_classes(&self, mut parent: FrameId, owner: NodeRef) -> Known<u64> {
        while parent != FrameId::ROOT {
            if !self.budget.step() { return Known::Unknown }
            let Some(frame) = self.get(parent) else { return Known::Unknown };
            if frame.owner == owner { return Known::Yes(frame.classes) }
            parent = frame.parent;
        }
        Known::Yes(0)
    }

    /// The frame `id` stands for; `None` for [`FrameId::ROOT`].
    pub fn get(&self, id: FrameId) -> Option<Frame> {
        id.0.checked_sub(1).and_then(|at| self.frames.borrow().items.get(at as usize).copied())
    }

    pub fn argument_slot(&self, id: ArgsId, position: usize) -> Known<Argument> {
        let args = self.args.borrow();
        match args.items.get(id.0 as usize) {
            Some(Args::Complete(args)) => Known::Yes(args.get(position).cloned().flatten()
                .map_or(Argument::Unbound, Argument::Bound)),
            Some(Args::Overlay(args)) => Known::Yes(args.get(position).cloned().flatten()
                .map_or(Argument::Inherited, Argument::Bound)),
            None => Known::Unknown,
        }
    }

    /// `frame`'s world-wide id, computed once per context.
    pub fn world_frame(&self, frame: FrameId, compute: impl FnOnce() -> Option<u64>) -> Option<u64> {
        if let Some(&known) = self.world_frames.borrow().get(&frame) { return known }
        let id = compute();
        self.world_frames.borrow_mut().insert(frame, id);
        if let Some(id) = id { self.frames_by_world.borrow_mut().entry(id).or_insert(frame); }
        id
    }

    /// The frame world-wide `id` stands for here, built once per context.
    pub fn frame_for_world(&self, id: u64, build: impl FnOnce() -> Option<FrameId>) -> Option<FrameId> {
        if let Some(&frame) = self.frames_by_world.borrow().get(&id) { return Some(frame) }
        let frame = build()?;
        self.frames_by_world.borrow_mut().insert(id, frame);
        self.world_frames.borrow_mut().insert(frame, Some(id));
        Some(frame)
    }

    /// Evaluated arguments `id`: whether they overlay a parent's (unbound
    /// slots inherit), and the slots.
    pub fn arguments(&self, id: ArgsId) -> Option<(bool, Vec<Option<NodeValue>>)> {
        match self.args.borrow().items.get(id.0 as usize)? {
            Args::Complete(args) => Some((false, args.clone())),
            Args::Overlay(args) => Some((true, args.clone())),
        }
    }

    /// Whether evaluated arguments `id` bind nothing at all.
    pub fn binds_nothing(&self, id: ArgsId) -> bool {
        match self.args.borrow().items.get(id.0 as usize) {
            Some(Args::Complete(args) | Args::Overlay(args)) => args.iter().all(Option::is_none),
            None => false,
        }
    }

    /// One of a frame's evaluated arguments: `Yes(None)` past the last,
    /// unknown for an id this context never interned.
    pub fn argument(&self, id: ArgsId, position: usize) -> Known<Option<NodeValue>> {
        match self.argument_slot(id, position) {
            Known::Yes(Argument::Bound(value)) => Known::Yes(Some(value)),
            Known::Yes(Argument::Unbound | Argument::Inherited) => Known::Yes(None),
            Known::Unknown => Known::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Handle, NodeValue, QueryContext};

    #[test]
    fn handles_name_one_value_and_only_in_their_own_context() {
        let first = QueryContext::default();
        let second = QueryContext::default();
        let never = first.handle(NodeValue::never());
        assert_eq!(first.handle(NodeValue::never()), never);
        assert_eq!(first.handle_value(never), Some(NodeValue::never()));
        second.handle(NodeValue::Unknown);
        assert_eq!(second.handle_value(never), None);
        assert_eq!(Handle::from_number(never.number()), Some(never));
    }
}

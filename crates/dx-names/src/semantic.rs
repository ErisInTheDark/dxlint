//! One root question's engines. Borrowed handles share its memo state.

use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;

use super::question::{FrameId, Known, Leaf, NodeRef, QueryContext, Target, World};
use super::values::Values;
use super::Names;

pub struct Semantic<'a> {
    pub world: &'a dyn World,
    pub tables: &'a Names,
    pub question: &'a QueryContext,
    state: Rc<State>,
}

#[derive(Default)]
pub struct State {
    pub(crate) values: super::values::State,
    pub(crate) calls: super::calls::State,
    pub(crate) narrowing_places: super::narrowing::State,
    pub(crate) effects: Rc<super::effects::State>,
    pub name_queries: Rc<super::Memo>,
    pub(crate) modules: super::modules::queries::State,
    /// Semantic attributes read inside questions, by what their answer can
    /// depend on, and whether the answer read something unknown.
    attributes: super::memo::Memo<AttributeKey, (dx_ir::vm::Packed, bool)>,
    defects: RefCell<Vec<Defect>>,
    /// [`Semantic::generic_scope`] by owner: syntax alone decides it, and
    /// frames ask it for every leaf they reach.
    generic_scopes: super::memo::Memo<NodeRef, Option<u32>>,
    /// [`Semantic::shares_one_frame`] by owner.
    single_frames: super::memo::Memo<NodeRef, bool>,
    /// Attributes (keyed at the root frame) whose answer asked no value
    /// question: the same in every frame.
    frame_free: RefCell<dx_core::fx_hash::FxHashSet<AttributeKey>>,
}

/// A semantic attribute on a node, read where a question's frame (and a
/// scope of the node's file the question stands at) applies: everything its
/// answer can see. Kept compact: big files keep hundreds of thousands.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttributeKey {
    pub file: u32,
    pub node: u32,
    pub frame: FrameId,
    pub scope: Option<u32>,
    pub attr: u16,
}

impl std::ops::Deref for Semantic<'_> {
    type Target = State;
    fn deref(&self) -> &State { &self.state }
}

/// One file's semantic answers and frame arena, with a fresh budget per
/// root question. Every memoised answer is a fact computed on its own
/// allowance, so a root that runs out answers unknown itself but keeps
/// every fact it computed (frames are interned, so their keys mean the same
/// in every root).
#[derive(Default)]
pub struct Session {
    question: QueryContext,
    state: RefCell<Rc<State>>,
    revision: Cell<u64>,
}

impl Session {
    pub fn run<R>(&self, world: &dyn World, tables: &Names, run: impl FnOnce(&Semantic<'_>) -> Known<R>) -> Known<R> {
        let _owner = dx_core::alloc::heap::Owner::enter(dx_core::alloc::heap::SEMANTIC);
        let revision = world.revision();
        if self.revision.replace(revision) != revision { self.state.replace(Rc::default()); }
        self.question.budget.restart();
        self.question.budget.trace(world.profile().is_some());
        let semantic = Semantic { world, tables, question: &self.question, state: self.state.borrow().clone() };
        let outer = dx_tree::footprint::begin();
        let answer = run(&semantic);
        dx_tree::footprint::resume(outer);
        if let Some(profile) = world.profile() {
            profile.record_semantic_budget(
                u64::from(super::question::Budget::per_question() - self.question.budget.remaining()),
                u64::from(super::question::Budget::per_question()),
                self.question.budget.limited(),
            );
            profile.flush_steps(self.question.budget.limited());
        }
        let failed = self.question.budget.limited();
        if let (true, Some(profile)) = (failed, world.profile()) {
            let inner = semantic.values.innermost().and_then(|leaf| match leaf.target {
                Target::Node(node) => world.tree(node.file).zip(world.text(node.file))
                    .and_then(|(tree, text)| { let n = tree.node(node.node); text.get(n.start as usize..n.end as usize).map(str::to_owned) }),
                _ => None,
            });
            profile.note_cut_inner(inner);
        }
        semantic.values.end_root();
        if failed {
            if let Some(profile) = world.profile() {
                dx_core::profile::Profile::add(&profile.semantic_budget_limits, 1);
                profile.note_budget_limit();
            }
            semantic.defects.borrow_mut().clear();
            // Whatever was half built when the budget ran out is not an answer.
            return Known::Unknown;
        }
        answer
    }
}

impl Session {
    /// The session's answers so far, with the ids they use, to carry into
    /// the next lint of the same file ([`Session::carry_in`]).
    pub fn carry_out(&self) -> (Rc<State>, super::question::Interned) {
        (self.state.replace(Rc::default()), self.question.interned())
    }

    /// Drops answers that read something `dirty` changed or sit at a place
    /// `touched` says is gone (nothing when a question is still running).
    pub fn forget(&self, dirty: &dx_tree::footprint::Dirty, touched: &dyn Fn(&super::question::Target) -> bool) {
        if let Some(state) = Rc::get_mut(&mut self.state.borrow_mut()) { state.forget(dirty, touched) }
    }

    /// Starts from carried answers that hold at world revision `revision`.
    pub fn carry_in(&self, (state, interned): (Rc<State>, super::question::Interned), revision: u64) {
        self.state.replace(state);
        self.question.restore(interned);
        self.revision.set(revision);
    }
}

impl State {
    /// Drops what an edit of the linted file may change: answers whose
    /// footprint it meets, and every per-lint cache.
    pub fn forget(&mut self, dirty: &dx_tree::footprint::Dirty, touched: &dyn Fn(&super::question::Target) -> bool) {
        self.values.forget(dirty);
        self.calls.forget(dirty);
        self.narrowing_places.forget(dirty);
        match Rc::get_mut(&mut self.effects) {
            Some(effects) => effects.forget(dirty, touched),
            None => self.effects = Rc::default(),
        }
        // Lists, records and built texts live in the lint's own value arena,
        // which the next lint does not have.
        self.attributes.forget_unless(dirty, |(packed, _)| !matches!(packed.get(), dx_ir::vm::V::List(_) | dx_ir::vm::V::Record(..) | dx_ir::vm::V::Text(_)));
        self.generic_scopes.forget(dirty);
        self.single_frames.forget(dirty);
        self.name_queries = Rc::default();
        self.modules = Default::default();
        self.defects.get_mut().clear();
        self.frame_free.get_mut().clear();
    }
}

pub struct Defect {
    pub item: u32,
    pub at: Leaf,
    pub message: Box<str>,
}

impl<'a> Semantic<'a> {
    pub fn new(world: &'a dyn World, tables: &'a Names, question: &'a QueryContext) -> Self {
        Self { world, tables, question, state: Rc::default() }
    }

    pub fn leaf(&self, mut at: Leaf) -> Known<Leaf> {
        let Target::Node(mut node) = at.target else { return Known::Yes(at) };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(canonical) = tree.semantic_node_with(node.node, &mut || self.question.budget.step()) else { return Known::Unknown };
        node.node = canonical;
        at.target = Target::Node(node);
        at.frame = self.relevant_frame(at.frame, node);
        Known::Yes(at)
    }

    /// The innermost frame in `frame`'s chain that can matter to `node`: one
    /// whose owner's formals `node` can see. Formals are visible only inside
    /// the declarations (around the owner) that declare generics, so frames
    /// of owners elsewhere bind nothing the node reads; dropping
    /// them keeps one memo entry per node (enum members, literals, library
    /// types) instead of one per unrelated instantiation.
    fn relevant_frame(&self, mut frame: super::question::FrameId, node: NodeRef) -> super::question::FrameId {
        while let Some(binding) = self.question.get(frame) {
            if binding.owner.file == node.file && self.sees_formals(binding.owner, node) { return frame }
            frame = binding.parent;
        }
        frame
    }

    /// Whether `node` lies inside the outermost ancestor-or-self of `owner`
    /// declaring generics: the owner's formals may be declared by an
    /// enclosing declaration (a constructor instantiates its class's
    /// parameters). True when there is none.
    fn sees_formals(&self, owner: NodeRef, node: NodeRef) -> bool {
        let Some(tree) = self.world.tree(owner.file) else { return true };
        let Some(scope) = self.generic_scope(owner) else { return true };
        let (Some(outer), Some(inner)) = (tree.nodes.get(scope as usize), tree.nodes.get(node.node as usize)) else { return true };
        outer.start <= inner.start && inner.end <= outer.end
    }

    /// The outermost ancestor-or-self of `owner` declaring generics: where
    /// the formals an instantiation of `owner` binds are visible. `None`
    /// when there is none (or the tree is not loaded).
    pub(crate) fn generic_scope(&self, owner: NodeRef) -> Option<u32> {
        if let Some(scope) = self.generic_scopes.get(&owner) { return scope }
        let outer = dx_tree::footprint::begin();
        let scope = self.find_generic_scope(owner);
        // Unloaded trees aren't remembered: they may load later.
        if self.world.tree(owner.file).is_some() { self.generic_scopes.insert(owner, scope); }
        dx_tree::footprint::resume(outer);
        scope
    }

    fn find_generic_scope(&self, owner: NodeRef) -> Option<u32> {
        let tree = self.world.tree(owner.file)?;
        let mut scope = None;
        let mut cursor = Some(owner.node);
        while let Some(id) = cursor {
            let declares = tree.nodes.get(id as usize)
                .and_then(|data| self.tables.kinds.get(data.matcher as usize))
                .is_some_and(|kind| !kind.generics.is_empty());
            if declares { scope = Some(id) }
            cursor = tree.parent(id);
        }
        scope
    }

    /// Whether `node` declares formals in its syntax: its kind's generics
    /// paths find some (paths through attributes or value steps can't be
    /// told from syntax: they may).
    pub(crate) fn declares_formals(&self, node: NodeRef) -> bool {
        let Some(tree) = self.world.tree(node.file) else { return true };
        let Some(kind) = tree.nodes.get(node.node as usize).and_then(|data| self.tables.kinds.get(data.matcher as usize)) else { return false };
        kind.generics.iter().any(|(path, _)| {
            path.iter().any(|&label| label >= super::ATTR_STEP && label != super::PARENT && label != super::IT)
                || tree.semantic_follow_with(node.node, path, &mut || true).is_none_or(|nodes| !nodes.is_empty())
        })
    }

    /// Whether every reference to `owner` reads one value: a plain binding
    /// (neither it nor its parent can be instantiated, unlike callables,
    /// parameters and members, whose values frames or context decide) with
    /// no ancestor declaring formals in syntax (a local of a non-generic
    /// function or class).
    pub(crate) fn shares_one_frame(&self, owner: NodeRef) -> bool {
        if let Some(shared) = self.single_frames.get(&owner) { return shared }
        let outer = dx_tree::footprint::begin();
        let shared = self.find_one_frame(owner);
        self.single_frames.insert(owner, shared);
        dx_tree::footprint::resume(outer);
        shared
    }

    fn find_one_frame(&self, owner: NodeRef) -> bool {
        let Some(tree) = self.world.tree(owner.file) else { return false };
        let instantiable = |id: u32| tree.nodes.get(id as usize)
            .and_then(|data| self.tables.kinds.get(data.matcher as usize))
            .is_none_or(|kind| !kind.generics.is_empty());
        let mut shared = !instantiable(owner.node) && tree.parent(owner.node).is_none_or(|parent| !instantiable(parent));
        let mut cursor = Some(owner.node);
        while let (true, Some(id)) = (shared, cursor) {
            if self.declares_formals(NodeRef { node: id, ..owner }) { shared = false }
            cursor = tree.parent(id);
        }
        shared
    }

    pub fn parent(&self, at: Leaf) -> Known<Option<Leaf>> {
        let Known::Yes(node) = self.capture_owner(at) else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(parent) = tree.parent(node.node) else { return Known::Yes(None) };
        match self.leaf(Leaf { target: Target::Node(NodeRef { node: parent, ..node }), ..at }) {
            Known::Yes(parent) => Known::Yes(Some(parent)),
            Known::Unknown => Known::Unknown,
        }
    }

    pub fn position(&self, at: Leaf) -> Known<Option<usize>> {
        let Known::Yes(node) = self.capture_owner(at) else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        Known::Yes(tree.field_index(node.node))
    }

    pub fn follow(&self, at: Leaf, path: &[u16]) -> Known<Vec<Leaf>> {
        let Known::Yes(at) = self.leaf(at) else { return Known::Unknown };
        if !matches!(at.target, Target::Node(_)) && path.iter().all(|&label| label == super::IT) {
            for _ in path {
                if !self.question.budget.step() { return Known::Unknown }
            }
            return Known::Yes(vec![at]);
        }
        let node = match at.target {
            Target::Node(node) => node,
            Target::Scope(scope) => {
                let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else { return Known::Unknown };
                NodeRef { file: scope.file, node: info.node }
            }
            _ => return Known::Unknown,
        };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(nodes) = tree.semantic_follow_with(node.node, path, &mut || self.question.budget.step()) else { return Known::Unknown };
        Known::Yes(nodes.into_iter().map(|child| Leaf { target: Target::Node(NodeRef { node: child, ..node }), ..at }).collect())
    }

    fn capture_owner(&self, at: Leaf) -> Known<NodeRef> {
        let Known::Yes(at) = self.leaf(at) else { return Known::Unknown };
        let Target::Node(mut node) = at.target else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(owner) = tree.capture_owner_with(node.node, &mut || self.question.budget.step()) else { return Known::Unknown };
        node.node = owner;
        Known::Yes(node)
    }

    pub fn values(&self) -> Values<'_> {
        Values { semantic: self }
    }

    pub fn calls(&self) -> super::calls::Calls<'_> {
        super::calls::Calls { semantic: self }
    }

    pub fn effects(&self) -> super::effects::Effects<'_> {
        super::effects::Effects { semantic: self }
    }

    pub fn modules(&self) -> super::modules::queries::Queries<'_> {
        super::modules::queries::Queries::new(self)
    }

    pub fn narrowing(&self) -> super::narrowing::Narrowing<'_> {
        super::narrowing::Narrowing { semantic: self }
    }

    /// A kept attribute answer, and whether it read something unknown.
    pub fn attribute(&self, key: &AttributeKey) -> Option<(dx_ir::vm::V, bool)> {
        self.attributes.get(key).map(|(value, unknown)| (value.get(), unknown))
    }

    /// Keeps an attribute answer with what computing it read.
    pub fn remember_attribute(&self, key: AttributeKey, value: dx_ir::vm::V, unknown: bool, read: dx_tree::footprint::Footprint) {
        self.attributes.insert_read(key, (dx_ir::vm::Packed::new(value), unknown), read);
    }

    /// Whether the attribute at `key` (whatever its frame) was found to ask
    /// no value question.
    pub fn frame_free_attribute(&self, key: AttributeKey) -> bool {
        self.frame_free.borrow().contains(&AttributeKey { frame: FrameId::ROOT, ..key })
    }

    /// Keeps an attribute that asked no value question for every frame.
    pub fn remember_frame_free(&self, key: AttributeKey, value: dx_ir::vm::V, unknown: bool, read: dx_tree::footprint::Footprint) {
        let root = AttributeKey { frame: FrameId::ROOT, ..key };
        self.frame_free.borrow_mut().insert(root);
        if key.frame != FrameId::ROOT { self.remember_attribute(root, value, unknown, read) }
    }

    pub fn defect(&self, item: u32, at: Leaf, message: impl Into<Box<str>>) {
        Defect::record(&mut self.defects.borrow_mut(), Defect { item, at, message: message.into() });
    }

    pub fn defects(&self) -> Ref<'_, [Defect]> {
        Ref::map(self.defects.borrow(), Vec::as_slice)
    }
}

impl Defect {
    pub fn file(&self) -> Option<u32> {
        match self.at.target {
            super::question::Target::Node(node) => Some(node.file),
            super::question::Target::Scope(scope) => Some(scope.file),
            _ => None,
        }
    }

    fn record(defects: &mut Vec<Defect>, defect: Defect) {
        if !defects.iter().any(|previous| previous.item == defect.item && previous.file() == defect.file()) {
            defects.push(defect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::question::{FrameId, NodeRef, ScopeRef, Target};

    #[test]
    fn defects_deduplicate_per_item_and_file_not_reference_or_view() {
        let first = Leaf { target: Target::Node(NodeRef::here(0)), frame: FrameId::ROOT };
        let sites = [
            (7, first),
            (7, Leaf { target: Target::Node(NodeRef::here(1)), frame: FrameId::ROOT }),
            (7, Leaf { target: Target::Scope(ScopeRef { file: 0, scope: 0 }), frame: FrameId::ROOT }),
            (8, first),
            (7, Leaf { target: Target::Node(NodeRef { file: 1, node: 0 }), frame: FrameId::ROOT }),
        ];
        let mut defects = Vec::new();
        for (item, at) in sites {
            Defect::record(&mut defects, Defect { item, at, message: "conflict".into() });
        }
        assert_eq!(defects.len(), 3);
        assert_eq!(defects[0].at, first);
        assert_eq!(defects.iter().filter(|defect| defect.item == 7).count(), 2);
    }
}

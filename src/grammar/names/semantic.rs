//! One root question's engines. Borrowed handles share its memo state.

use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;

use super::memo::Epoch;
use super::question::{Known, Leaf, NodeRef, QueryContext, Target, World};
use super::values::Values;
use super::Names;

pub struct Semantic<'a> {
    pub world: &'a dyn World,
    pub tables: &'a Names,
    pub question: &'a QueryContext,
    pub(super) epoch: Epoch,
    state: Rc<State>,
}

#[derive(Default)]
pub struct State {
    pub(crate) values: super::values::State,
    pub(crate) calls: super::calls::State,
    pub(crate) effects: Rc<super::effects::State>,
    pub(crate) name_queries: Rc<crate::expr::queries::NamesMemo>,
    pub(crate) modules: super::modules::queries::State,
    pub(crate) narrowing: super::narrowing::State,
    /// Semantic attributes read inside questions, by what their answer can
    /// depend on: complete answers only.
    attributes: super::memo::Memo<AttributeKey, crate::vm::V>,
    defects: RefCell<Vec<Defect>>,
}

/// A semantic attribute on a node, read where a question's frame (and a
/// scope standing for that node) applies: everything its answer can see.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AttributeKey {
    pub file: u32,
    pub node: u32,
    pub attr: u16,
    pub at: Leaf,
}

impl std::ops::Deref for Semantic<'_> {
    type Target = State;
    fn deref(&self) -> &State { &self.state }
}

/// One file's semantic answers and frame arena, with a fresh budget per
/// root question. Answers a root's limit cut short never survive into the
/// next root; answers it completed before the cut do (frames are interned,
/// so their keys mean the same in every root).
#[derive(Default)]
pub(crate) struct Session {
    question: QueryContext,
    state: RefCell<Rc<State>>,
    revision: Cell<u64>,
    epoch: Cell<Epoch>,
}

impl Session {
    pub(crate) fn run<R>(&self, world: &dyn World, tables: &Names, run: impl FnOnce(&Semantic<'_>) -> Known<R>) -> Known<R> {
        let revision = world.revision();
        if self.revision.replace(revision) != revision { self.state.replace(Rc::default()); }
        self.question.budget.restart();
        let epoch = self.epoch.get().next();
        self.epoch.set(epoch);
        let semantic = Semantic { world, tables, question: &self.question, epoch, state: self.state.borrow().clone() };
        let answer = run(&semantic);
        if let Some(profile) = world.profile() {
            profile.record_semantic_budget(
                u64::from(super::question::Budget::DEFAULT - self.question.budget.remaining()),
                u64::from(super::question::Budget::DEFAULT),
            );
        }
        let failed = self.question.budget.limited();
        let cut = self.question.budget.cut();
        let discarded = semantic.values.finish(epoch, failed, cut) + semantic.calls.finish(epoch, failed, cut)
            + semantic.narrowing.finish(epoch, failed, cut)
            + semantic.attributes.finish(epoch, failed, cut, |_| true);
        if failed {
            if let Some(profile) = world.profile() {
                crate::engine::Profile::add(&profile.semantic_budget_limits, 1);
                profile.note_budget_limit();
                crate::engine::Profile::add(&profile.semantic_state_discards, 1);
                crate::engine::Profile::add(&profile.semantic_memo_discards, discarded);
            }
            semantic.effects.rollback();
            semantic.defects.borrow_mut().clear();
        }
        answer
    }
}

pub struct Defect {
    pub item: u32,
    pub at: Leaf,
    pub message: Box<str>,
}

impl<'a> Semantic<'a> {
    pub fn new(world: &'a dyn World, tables: &'a Names, question: &'a QueryContext) -> Self {
        Self { world, tables, question, epoch: Epoch::default(), state: Rc::default() }
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
    /// instantiating a declaration of the node's own file. Formals are only
    /// visible in their own file, so frames of other files' owners bind
    /// nothing the node reads; dropping them keeps one memo entry per node
    /// (library types especially) instead of one per unrelated instantiation.
    /// Owners in the same file are kept: their formals may be declared
    /// around the owner (a constructor instantiates its class's parameters).
    fn relevant_frame(&self, mut frame: super::question::FrameId, node: NodeRef) -> super::question::FrameId {
        while let Some(binding) = self.question.get(frame) {
            if binding.owner.file == node.file { return frame }
            frame = binding.parent;
        }
        frame
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

    pub(crate) fn attribute(&self, key: &AttributeKey) -> Option<crate::vm::V> {
        self.attributes.get(key)
    }

    pub(crate) fn remember_attribute(&self, key: AttributeKey, value: crate::vm::V) {
        self.attributes.insert(self.epoch, key, value);
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
    use crate::grammar::names::question::{FrameId, NodeRef, ScopeRef, Target};
    use std::cell::Cell;

    #[test]
    fn semantic_capture_items_keep_their_parent_and_order() {
        use crate::grammar::parse::{Edge, Node, TEXT};
        use crate::pattern::compile::NO_LABEL;
        let linter = crate::engine::tests::linter("in paren fits marked");
        let tree = crate::grammar::Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 1, children: Vec::new() },
            Node { matcher: TEXT, start: 0, end: 1, children: vec![Edge { label: NO_LABEL, node: 0 }] },
            Node { matcher: 0, start: 1, end: 2, children: Vec::new() },
            Node { matcher: TEXT, start: 1, end: 2, children: vec![Edge { label: NO_LABEL, node: 2 }] },
            Node { matcher: 0, start: 0, end: 2, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 3 }] },
        ], 4);
        let world = CountingWorld { grammar: &linter.grammar, tree: &tree, conditions: Cell::new(0), cut: Cell::new(0), revision: Cell::new(0), profile: None };
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &linter.grammar.names, &question);
        let frame = question.bind(FrameId::ROOT, NodeRef::here(4), None, None);
        let parent = Leaf { target: Target::Node(NodeRef::here(4)), frame };
        for (node, position) in [(0, 0), (1, 0), (2, 1), (3, 1)] {
            let at = Leaf { target: Target::Node(NodeRef::here(node)), frame };
            assert_eq!(semantic.position(at), Known::Yes(Some(position)));
            assert_eq!(semantic.parent(at), Known::Yes(Some(parent)));
        }
        assert_eq!(semantic.parent(parent), Known::Yes(None));
        assert_eq!(semantic.leaf(parent), Known::Yes(parent));
        let children: Vec<_> = [0, 2].into_iter().map(|node| Leaf { target: Target::Node(NodeRef::here(node)), frame }).collect();
        assert_eq!(semantic.follow(parent, &[0]), Known::Yes(children.clone()));
        assert_eq!(semantic.follow(children[0], &[super::super::PARENT, 0]), Known::Yes(children));
    }

    #[test]
    fn semantic_captures_preserve_views_and_reject_broken_chains() {
        use crate::grammar::parse::{Edge, Node, TEXT};
        use crate::pattern::compile::NO_LABEL;
        let linter = crate::engine::tests::linter("in paren fits marked");
        let tree = crate::grammar::Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 2, children: Vec::new() },
            Node { matcher: TEXT, start: 0, end: 2, children: vec![Edge { label: NO_LABEL, node: 0 }] },
            Node { matcher: TEXT, start: 0, end: 2, children: vec![Edge { label: NO_LABEL, node: 1 }] },
            Node { matcher: TEXT, start: 0, end: 2, children: Vec::new() },
            Node { matcher: TEXT, start: 0, end: 2, children: vec![Edge { label: 0, node: 0 }] },
            Node { matcher: TEXT, start: 0, end: 2, children: vec![Edge { label: NO_LABEL, node: 0 }, Edge { label: NO_LABEL, node: 3 }] },
        ], 2);
        let world = CountingWorld { grammar: &linter.grammar, tree: &tree, conditions: Cell::new(0), cut: Cell::new(0), revision: Cell::new(0), profile: None };
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &linter.grammar.names, &question);
        let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
        let enclosing = question.bind(FrameId::ROOT, NodeRef { file: 7, node: 0 }, None, None);
        let at = Leaf { target: Target::Node(NodeRef { file: 7, node: 2 }), frame: enclosing };
        assert_eq!(semantic.leaf(at), Known::Yes(Leaf { target: Target::Node(NodeRef { file: 7, node: 0 }), frame: enclosing }));
        // A frame instantiating a declaration that does not enclose the node
        // binds nothing it can read: the leaf drops it.
        let unrelated = Leaf { target: Target::Node(NodeRef { file: 7, node: 2 }), frame };
        assert_eq!(semantic.leaf(unrelated), Known::Yes(Leaf { target: Target::Node(NodeRef { file: 7, node: 0 }), frame: FrameId::ROOT }));
        for node in [0, 3, 4, 5] {
            let at = Leaf { target: Target::Node(NodeRef::here(node)), frame };
            assert_eq!(semantic.leaf(at), Known::Yes(at));
        }
        for target in [Target::Scope(ScopeRef { file: 7, scope: 2 }), Target::Traits(1)] {
            let at = Leaf { target, frame };
            assert_eq!(semantic.leaf(at), Known::Yes(at));
        }
        assert_eq!(semantic.leaf(Leaf { target: Target::Node(NodeRef::here(99)), frame }), Known::Unknown);
        let mut bounded = QueryContext::default();
        bounded.budget = super::super::question::Budget::new(1);
        let semantic = Semantic::new(&world, &linter.grammar.names, &bounded);
        assert_eq!(semantic.leaf(at), Known::Unknown);
    }

    struct CountingWorld<'a> {
        grammar: &'a crate::grammar::Grammar,
        tree: &'a crate::grammar::Tree,
        conditions: Cell<u32>,
        cut: Cell<u8>,
        revision: Cell<u64>,
        profile: Option<&'a crate::engine::Profile>,
    }

    impl World for CountingWorld<'_> {
        fn profile(&self) -> Option<&crate::engine::Profile> { self.profile }
        fn revision(&self) -> u64 { self.revision.get() }
        fn grammar(&self) -> &crate::grammar::Grammar { self.grammar }
        fn tree(&self, _: u32) -> Option<&crate::grammar::Tree> { Some(self.tree) }
        fn text(&self, _: u32) -> Option<&str> { Some("()") }
        fn names(&self, _: u32) -> Option<&super::super::Resolved> { None }
        fn index(&self, _: u32) -> Option<&super::super::ScopeIndex> { None }
        fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> { text.into() }
        fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Unknown }
        fn operand(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> { Known::Unknown }
        fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> { Known::Unknown }
        fn condition(&self, at: Leaf, _: u32, semantic: &Semantic<'_>) -> Known<bool> {
            self.conditions.set(self.conditions.get() + 1);
            match self.cut.get() {
                1 => {
                    while semantic.question.budget.step() {}
                    return Known::Unknown;
                }
                2 => {
                    let mut guards = Vec::new();
                    while let Some(guard) = semantic.question.budget.nest() { guards.push(guard); }
                    return Known::Unknown;
                }
                _ => {}
            }
            let answer = semantic.question.get(at.frame).and_then(|frame| frame.args)
                .map(|args| semantic.question.argument(args, 0) == Known::Yes(Some(super::super::question::NodeValue::never())))
                .unwrap_or(true);
            Known::Yes(answer && self.revision.get() % 2 == 0)
        }
    }

    #[test]
    fn root_sessions_reuse_answers_without_mixing_frames_or_retaining_limit_cuts() {
        let linter = crate::engine::tests::linter("in paren fits marked when text == \"()\"");
        let tree = crate::grammar::Tree::parse_with(&linter.grammar, 0, "()", None, None);
        let matcher = linter.grammar.matcher("paren").unwrap();
        let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
        let profile = crate::engine::Profile::default();
        let world = CountingWorld { grammar: &linter.grammar, tree: &tree, conditions: Cell::new(0), cut: Cell::new(0), revision: Cell::new(0), profile: Some(&profile) };
        let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
        let trait_id = linter.grammar.names.vocabulary.traits.iter().position(|name| name == "marked").unwrap() as u16;
        let session = Session::default();
        for _ in 0..2 {
            assert_eq!(session.run(&world, &linter.grammar.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
        }
        assert_eq!(world.conditions.get(), 1);
        for (argument, expected) in [
            (super::super::question::NodeValue::never(), true),
            (super::super::question::NodeValue::leaf(at), false),
            (super::super::question::NodeValue::never(), true),
        ] {
            assert_eq!(session.run(&world, &linter.grammar.names, |semantic| {
                let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None, Some(vec![argument]));
                semantic.values().fits(Leaf { frame, ..at }, trait_id)
            }), Known::Yes(expected));
        }
        assert_eq!(world.conditions.get(), 3);
        for cut in [1, 2] {
            let session = Session::default();
            assert_eq!(session.run(&world, &linter.grammar.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
            let before = world.conditions.get();
            let discarded = crate::engine::Profile::get(&profile.semantic_memo_discards);
            assert_eq!(session.run(&world, &linter.grammar.names, |semantic| {
                let complete = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None,
                    Some(vec![super::super::question::NodeValue::leaf(at)]));
                assert_eq!(semantic.values().fits(Leaf { frame: complete, ..at }, trait_id), Known::Yes(false));
                world.cut.set(cut);
                let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None,
                    Some(vec![super::super::question::NodeValue::never()]));
                semantic.values().fits(Leaf { frame, ..at }, trait_id)
            }), Known::Unknown);
            assert_eq!(crate::engine::Profile::get(&profile.semantic_memo_discards), discarded + 1, "only the answer cut short is dropped");
            world.cut.set(0);
            assert_eq!(session.run(&world, &linter.grammar.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
            assert_eq!(world.conditions.get(), before + 2, "another root's limit discarded a completed answer");
            assert_eq!(session.run(&world, &linter.grammar.names, |semantic| {
                let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None,
                    Some(vec![super::super::question::NodeValue::leaf(at)]));
                semantic.values().fits(Leaf { frame, ..at }, trait_id)
            }), Known::Yes(false));
            assert_eq!(world.conditions.get(), before + 2, "a root cut short dropped an answer it completed before the cut");
            assert_eq!(session.run(&world, &linter.grammar.names, |semantic| {
                let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None,
                    Some(vec![super::super::question::NodeValue::never()]));
                semantic.values().fits(Leaf { frame, ..at }, trait_id)
            }), Known::Yes(true));
            assert_eq!(world.conditions.get(), before + 3, "the answer cut short is computed again");
        }
        let before = world.conditions.get();
        world.revision.set(1);
        for _ in 0..2 {
            assert_eq!(session.run(&world, &linter.grammar.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(false));
        }
        assert_eq!(world.conditions.get(), before + 1);
        world.revision.set(2);
        assert_eq!(session.run(&world, &linter.grammar.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
        assert_eq!(world.conditions.get(), before + 2);
    }

    #[test]
    fn root_budget_profile_counts_known_unknown_and_limit_work_before_restart() {
        use super::super::question::Budget;
        use std::sync::atomic::Ordering;
        let linter = crate::engine::tests::linter("");
        let tree = crate::grammar::Tree::parse_with(&linter.grammar, 0, "()", None, None);
        let profile = crate::engine::Profile::default();
        let world = CountingWorld {
            grammar: &linter.grammar, tree: &tree, conditions: Cell::new(0),
            cut: Cell::new(0), revision: Cell::new(0), profile: Some(&profile),
        };
        let session = Session::default();
        let half = Budget::DEFAULT / 2;
        for (units, known) in [(half, true), (half + 1, false), (Budget::DEFAULT, false), (3, true)] {
            session.run(&world, &linter.grammar.names, |semantic| {
                for _ in 0..units { assert!(semantic.question.budget.step()); }
                if known { Known::Yes(()) } else { Known::Unknown }
            });
        }
        assert_eq!(profile.semantic_budget_steps_total.load(Ordering::Relaxed), u64::from(half * 2 + 1 + Budget::DEFAULT + 3));
        assert_eq!(profile.semantic_budget_steps_max.load(Ordering::Relaxed), u64::from(Budget::DEFAULT));
        assert_eq!(profile.semantic_budget_steps_over_half.load(Ordering::Relaxed), 2);
        assert_eq!(profile.semantic_budget_limits.load(Ordering::Relaxed), 1);
        assert_eq!(profile.semantic_state_discards.load(Ordering::Relaxed), 1);
    }

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

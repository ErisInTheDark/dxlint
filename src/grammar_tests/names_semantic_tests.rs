use std::cell::Cell;

use crate::engine::tests::linter as make_linter;
use crate::names::question::{
    Budget, FrameId, Known, Leaf, NodeRef, NodeValue, QueryContext, ScopeRef, Target,
};
use crate::semantic::question::World;
use crate::semantic::semantic::{Semantic, Session};
use crate::names::{Resolved, ScopeIndex, PARENT};
use crate::grammar::{Edge, Node, TEXT};
use crate::pattern::compile::NO_LABEL;

#[test]
fn semantic_capture_items_keep_their_parent_and_order() {
    let linter = make_linter("in paren fits marked");
    let tree = crate::grammar::Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 1, children: Vec::new() },
        Node { matcher: TEXT, start: 0, end: 1, children: vec![Edge { label: NO_LABEL, node: 0 }] },
        Node { matcher: 0, start: 1, end: 2, children: Vec::new() },
        Node { matcher: TEXT, start: 1, end: 2, children: vec![Edge { label: NO_LABEL, node: 2 }] },
        Node { matcher: 0, start: 0, end: 2, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 3 }] },
    ], 4);
    let world = CountingWorld { grammar: &linter.grammar, tables: &linter.names, tree: &tree, conditions: Cell::new(0), cut: Cell::new(0), revision: Cell::new(0), profile: None };
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &linter.names, &question);
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
    assert_eq!(semantic.follow(children[0], &[PARENT, 0]), Known::Yes(children));
}

#[test]
fn semantic_captures_preserve_views_and_reject_broken_chains() {
    let linter = make_linter("in paren fits marked");
    let tree = crate::grammar::Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 2, children: Vec::new() },
        Node { matcher: TEXT, start: 0, end: 2, children: vec![Edge { label: NO_LABEL, node: 0 }] },
        Node { matcher: TEXT, start: 0, end: 2, children: vec![Edge { label: NO_LABEL, node: 1 }] },
        Node { matcher: TEXT, start: 0, end: 2, children: Vec::new() },
        Node { matcher: TEXT, start: 0, end: 2, children: vec![Edge { label: 0, node: 0 }] },
        Node { matcher: TEXT, start: 0, end: 2, children: vec![Edge { label: NO_LABEL, node: 0 }, Edge { label: NO_LABEL, node: 3 }] },
    ], 2);
    let world = CountingWorld { grammar: &linter.grammar, tables: &linter.names, tree: &tree, conditions: Cell::new(0), cut: Cell::new(0), revision: Cell::new(0), profile: None };
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &linter.names, &question);
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
    bounded.budget = Budget::new(1);
    let semantic = Semantic::new(&world, &linter.names, &bounded);
    assert_eq!(semantic.leaf(at), Known::Unknown);
}

struct CountingWorld<'a> {
    grammar: &'a crate::grammar::Grammar,
    tables: &'a crate::names::Names,
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
    fn names(&self, _: u32) -> Option<&Resolved> { None }
    fn index(&self, _: u32) -> Option<&ScopeIndex> { None }
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
            // Real work: more than a nearly spent asker has left.
            3 => {
                for _ in 0..50 {
                    if !semantic.question.budget.step() { return Known::Unknown }
                }
            }
            // A cycle: the condition asks the fact it decides.
            4 => {
                let marked = self.tables.vocabulary.traits.iter().position(|name| name == "marked").unwrap() as u16;
                return semantic.values().fits(at, marked);
            }
            _ => {}
        }
        let answer = semantic.question.get(at.frame).and_then(|frame| frame.args)
            .map(|args| semantic.question.argument(args, 0) == Known::Yes(Some(NodeValue::never())))
            .unwrap_or(true);
        Known::Yes(answer && self.revision.get() % 2 == 0)
    }
}

#[test]
fn root_sessions_reuse_answers_without_mixing_frames_or_retaining_limit_cuts() {
    let linter = make_linter("in paren fits marked when text == \"()\"");
    let tree = linter.grammar.parse_tree_with(0, "()", None, None);
    let matcher = linter.grammar.matcher("paren").unwrap();
    let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
    let profile = crate::engine::Profile::default();
    let world = CountingWorld { grammar: &linter.grammar, tables: &linter.names, tree: &tree, conditions: Cell::new(0), cut: Cell::new(0), revision: Cell::new(0), profile: Some(&profile) };
    let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
    let trait_id = linter.names.vocabulary.traits.iter().position(|name| name == "marked").unwrap() as u16;
    let session = Session::default();
    for _ in 0..2 {
        assert_eq!(session.run(&world, &linter.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
    }
    assert_eq!(world.conditions.get(), 1);
    for (argument, expected) in [
        (NodeValue::never(), true),
        (NodeValue::leaf(at), false),
        (NodeValue::never(), true),
    ] {
        assert_eq!(session.run(&world, &linter.names, |semantic| {
            let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None, Some(vec![argument]));
            semantic.values().fits(Leaf { frame, ..at }, trait_id)
        }), Known::Yes(expected));
    }
    assert_eq!(world.conditions.get(), 3);
    for cut in [1, 2] {
        let session = Session::default();
        assert_eq!(session.run(&world, &linter.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
        let before = world.conditions.get();
        assert_eq!(session.run(&world, &linter.names, |semantic| {
            let complete = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None,
                Some(vec![NodeValue::leaf(at)]));
            assert_eq!(semantic.values().fits(Leaf { frame: complete, ..at }, trait_id), Known::Yes(false));
            world.cut.set(cut);
            let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None,
                Some(vec![NodeValue::never()]));
            semantic.values().fits(Leaf { frame, ..at }, trait_id)
        }), Known::Unknown);
        world.cut.set(0);
        assert_eq!(session.run(&world, &linter.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
        assert_eq!(session.run(&world, &linter.names, |semantic| {
            let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None,
                Some(vec![NodeValue::leaf(at)]));
            semantic.values().fits(Leaf { frame, ..at }, trait_id)
        }), Known::Yes(false));
        assert_eq!(world.conditions.get(), before + 2, "a fact computed beside one that ran out is kept");
        // A fact that ran out of its own allowance stays unknown without
        // being redone; one cut by nesting depth (which depends on who
        // asked) is computed again.
        let (answer, conditions) = if cut == 1 { (Known::Unknown, before + 2) } else { (Known::Yes(true), before + 3) };
        assert_eq!(session.run(&world, &linter.names, |semantic| {
            let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(node), None,
                Some(vec![NodeValue::never()]));
            semantic.values().fits(Leaf { frame, ..at }, trait_id)
        }), answer);
        assert_eq!(world.conditions.get(), conditions);
    }
    let before = world.conditions.get();
    world.revision.set(1);
    for _ in 0..2 {
        assert_eq!(session.run(&world, &linter.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(false));
    }
    assert_eq!(world.conditions.get(), before + 1);
    world.revision.set(2);
    assert_eq!(session.run(&world, &linter.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
    assert_eq!(world.conditions.get(), before + 2);
}

#[test]
fn a_fact_answers_the_same_whichever_root_reaches_it_first() {
    let linter = make_linter("in paren fits marked when text == \"()\"");
    let tree = linter.grammar.parse_tree_with(0, "()", None, None);
    let matcher = linter.grammar.matcher("paren").unwrap();
    let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
    let world = CountingWorld { grammar: &linter.grammar, tables: &linter.names, tree: &tree, conditions: Cell::new(0), cut: Cell::new(3), revision: Cell::new(0), profile: None };
    let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
    let trait_id = linter.names.vocabulary.traits.iter().position(|name| name == "marked").unwrap() as u16;
    // The fact spends 50 steps of its own allowance: a root with 20 left
    // gets it as a fresh root does, whichever of them computes it.
    let ask = |session: &Session, late: bool| session.run(&world, &linter.names, |semantic| {
        while late && semantic.question.budget.remaining() > 20 { semantic.question.budget.step(); }
        Known::Yes(semantic.values().fits(at, trait_id))
    });
    for late_first in [true, false] {
        let session = Session::default();
        assert_eq!(ask(&session, late_first), Known::Yes(Known::Yes(true)));
        assert_eq!(ask(&session, !late_first), Known::Yes(Known::Yes(true)));
    }
    assert_eq!(world.conditions.get(), 2, "each session computed the fact once");
}

#[test]
fn an_unknown_resting_on_an_unfinished_cycle_expires_with_the_fact_computing_it() {
    let linter = make_linter("in paren fits marked when text == \"()\"");
    let tree = linter.grammar.parse_tree_with(0, "()", None, None);
    let matcher = linter.grammar.matcher("paren").unwrap();
    let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
    let world = CountingWorld { grammar: &linter.grammar, tables: &linter.names, tree: &tree, conditions: Cell::new(0), cut: Cell::new(4), revision: Cell::new(0), profile: None };
    let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
    let trait_id = linter.names.vocabulary.traits.iter().position(|name| name == "marked").unwrap() as u16;
    let session = Session::default();
    for _ in 0..2 {
        assert_eq!(session.run(&world, &linter.names, |semantic| semantic.values().fits(at, trait_id)), Known::Unknown);
    }
    // Never kept for good: each root computes it afresh (a cycle head
    // once more after its cycle), and a world where the cycle is gone
    // answers.
    assert_eq!(world.conditions.get(), 4);
    world.cut.set(0);
    assert_eq!(session.run(&world, &linter.names, |semantic| semantic.values().fits(at, trait_id)), Known::Yes(true));
}

#[test]
fn root_questions_cut_short_answer_unknown_whatever_they_built() {
    let linter = make_linter("");
    let tree = linter.grammar.parse_tree_with(0, "()", None, None);
    let world = CountingWorld { grammar: &linter.grammar, tables: &linter.names, tree: &tree, conditions: Cell::new(0), cut: Cell::new(0), revision: Cell::new(0), profile: None };
    let session = Session::default();
    assert_eq!(session.run(&world, &linter.names, |semantic| {
        while semantic.question.budget.step() {}
        Known::Yes(0)
    }), Known::Unknown);
    assert_eq!(session.run(&world, &linter.names, |semantic| {
        let mut guards = Vec::new();
        while let Some(guard) = semantic.question.budget.nest() { guards.push(guard); }
        drop(guards);
        Known::Yes(0)
    }), Known::Unknown);
    assert_eq!(session.run(&world, &linter.names, |semantic| {
        assert!(semantic.question.budget.step());
        Known::Yes(0)
    }), Known::Yes(0));
}

#[test]
fn root_budget_profile_counts_known_unknown_and_limit_work_before_restart() {
    use std::sync::atomic::Ordering;
    let linter = make_linter("");
    let tree = linter.grammar.parse_tree_with(0, "()", None, None);
    let profile = crate::engine::Profile::default();
    let world = CountingWorld {
        grammar: &linter.grammar, tables: &linter.names, tree: &tree, conditions: Cell::new(0),
        cut: Cell::new(0), revision: Cell::new(0), profile: Some(&profile),
    };
    let session = Session::default();
    let budget = Budget::per_question();
    let half = budget / 2;
    for (units, known) in [(half, true), (half + 1, false), (budget, false), (3, true)] {
        session.run(&world, &linter.names, |semantic| {
            for _ in 0..units { assert!(semantic.question.budget.step()); }
            if known { Known::Yes(()) } else { Known::Unknown }
        });
    }
    assert_eq!(profile.semantic_budget_steps_total.load(Ordering::Relaxed), u64::from(half * 2 + 1 + budget + 3));
    assert_eq!(profile.semantic_budget_steps_max.load(Ordering::Relaxed), u64::from(budget));
    assert_eq!(profile.semantic_budget_steps_over_half.load(Ordering::Relaxed), 2);
    assert_eq!(profile.semantic_budget_limits.load(Ordering::Relaxed), 1);
}

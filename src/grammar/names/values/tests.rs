use super::*;
use crate::config::ast::Config;
use crate::grammar::{Edge, Grammar, Node, Tree};
use super::super::{KindNames, Names, Resolved};
use super::super::question::{Budget, FrameId, QueryContext, ScopeRef, World};

struct TestWorld {
    grammar: Grammar,
    tree: Tree,
    names: Resolved,
}

impl TestWorld {
    fn new() -> Self {
        Self {
            grammar: Grammar::build(&Config::default(), &mut Vec::new()),
            tree: Tree::from_native(
                (0..3).map(|matcher| Node { matcher, start: 0, end: 0, children: Vec::new() }).collect(),
                0,
            ),
            names: Resolved::default(),
        }
    }
}

impl World for TestWorld {
    fn grammar(&self) -> &Grammar { &self.grammar }
    fn tree(&self, file: u32) -> Option<&Tree> { (file == 0).then_some(&self.tree) }
    fn text(&self, file: u32) -> Option<&str> { (file == 0).then_some("") }
    fn names(&self, file: u32) -> Option<&Resolved> { (file == 0).then_some(&self.names) }
    fn index(&self, _: u32) -> Option<&super::super::ScopeIndex> { None }
    fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> {
        std::borrow::Cow::Borrowed(text)
    }
    fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Yes(None) }
    fn condition(&self, at: Leaf, condition: u32, semantic: &Semantic<'_>) -> Known<bool> {
        match condition {
            1 => semantic.values().fits(at, 0),
            2 => Known::Yes(matches!(at.target, Target::Scope(_))),
            _ => Known::Unknown,
        }
    }
    fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
        Known::Yes(crate::expr::Value::Null)
    }
    fn operand(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
        Known::Unknown
    }
}

fn leaf(node: u32) -> Leaf {
    Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT }
}

fn tables() -> Names {
    Names {
        kinds: vec![
            KindNames { fits: vec![(1, None)], ..KindNames::default() },
            KindNames { fits: vec![(2, None)], ..KindNames::default() },
            KindNames { fits: vec![(1, Some(0))], ..KindNames::default() },
        ],
        ..Names::default()
    }
}

#[test]
fn alternatives_require_every_branch_but_intersections_can_prove_a_trait() {
    let world = TestWorld::new();
    let tables = tables();
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let union = NodeValue::Alternatives(vec![vec![leaf(0)], vec![leaf(1)]]);
    assert_eq!(values.could(&union, 1), Known::Yes(true));
    assert_eq!(values.always(&union, 1), Known::Yes(false));
    let intersection = NodeValue::Alternatives(vec![vec![leaf(0), leaf(1)]]);
    assert_eq!(values.always(&intersection, 1 | 2), Known::Yes(true));
    assert_eq!(values.could(&intersection, 1 | 2), Known::Yes(true));
}

#[test]
fn unknown_is_not_a_negative_proof_and_never_has_no_possible_value() {
    let world = TestWorld::new();
    let tables = tables();
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    assert_eq!(values.could(&NodeValue::leaf(leaf(2)), 1), Known::Unknown);
    assert_eq!(values.always(&NodeValue::Unknown, 1), Known::Unknown);
    assert_eq!(values.could(&NodeValue::never(), 1), Known::Yes(false));
}

#[test]
fn exhausting_the_shared_question_budget_does_not_return_a_false_answer() {
    let world = TestWorld::new();
    let tables = tables();
    let mut question = QueryContext::default();
    question.budget = Budget::new(0);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    assert_eq!(values.could(&NodeValue::leaf(leaf(0)), 1), Known::Unknown);
}

#[test]
fn unwrapping_a_list_keeps_its_union_alternatives() {
    let mut world = TestWorld::new();
    world.tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 7, node: 1 }, Edge { label: 7, node: 2 }] },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
        Node { matcher: 2, start: 0, end: 0, children: Vec::new() },
    ], 0);
    let mut tables = tables();
    tables.kinds[0].unwraps.push(super::super::Unwrapping {
        effect: None, possible: false, value: Some(super::super::Value::Spread { intersection: false, path: vec![7] }),
        nearest: 0, when: None,
    });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    assert_eq!(values.values(leaf(0)), NodeValue::Alternatives(vec![vec![leaf(1)], vec![leaf(2)]]));
}

#[test]
fn cyclic_unwrapping_returns_unknown_instead_of_a_partly_resolved_leaf() {
    let mut world = TestWorld::new();
    world.tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 0, children: Vec::new() },
    ], 0);
    let mut tables = tables();
    tables.kinds[0].unwraps.push(super::super::Unwrapping {
        effect: None, possible: false, value: Some(super::super::Value::Path(Vec::new())),
        nearest: 0, when: None,
    });
    let question = QueryContext::default();
    assert_eq!(Semantic::new(&world, &tables, &question).values().values(leaf(0)), NodeValue::Unknown);
}

#[test]
fn conditions_reenter_the_same_question_memo_through_borrowed_handles() {
    let world = TestWorld::new();
    let mut tables = tables();
    tables.kinds[0].fits = vec![(1, Some(1))];
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().fits(leaf(0), 0), Known::Unknown);
    assert_eq!(semantic.values().fits(leaf(0), 0), Known::Unknown);
}


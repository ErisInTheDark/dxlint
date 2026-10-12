use crate::config::ast::Config;
use crate::grammar::{Edge, Grammar, Node, Tree};
use crate::grammar::names::{KindNames, Names, Resolved};
use crate::grammar::names::question::{Budget, FrameId, Known, Leaf, NodeRef, NodeValue, QueryContext, ScopeRef, Target, World};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::values::Values;
use super::{Conditional, Invoke, InvokeKind, Signature};

struct TestWorld {
    grammar: Grammar,
    tree: Tree,
    names: Resolved,
}

impl TestWorld {
    fn new() -> Self {
        Self {
            grammar: Grammar::build(&Config::default(), &mut Vec::new()),
            tree: Tree::from_native((0..4).map(|matcher| Node {
                matcher, start: 0, end: 0, children: Vec::new(),
            }).collect(), 0),
            names: Resolved::default(),
        }
    }
}

impl World for TestWorld {
    fn grammar(&self) -> &Grammar { &self.grammar }
    fn tree(&self, file: u32) -> Option<&Tree> { (file == 0).then_some(&self.tree) }
    fn text(&self, file: u32) -> Option<&str> { (file == 0).then_some("") }
    fn names(&self, file: u32) -> Option<&Resolved> { (file == 0).then_some(&self.names) }
    fn index(&self, _: u32) -> Option<&crate::grammar::names::ScopeIndex> { None }
    fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> { text.into() }
    fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Yes(None) }
    fn condition(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<bool> { Known::Unknown }
    fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> { Known::Unknown }
    fn operand(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> { Known::Unknown }
}

fn accepts(expected: &NodeValue, actual: &NodeValue, values: &Values<'_>) -> Known<bool> {
    values.calls().accepts(expected, actual)
}

fn leaf(node: u32) -> Leaf {
    Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT }
}

fn value(node: u32) -> NodeValue {
    NodeValue::leaf(leaf(node))
}

fn tables() -> Names {
    Names {
        kinds: vec![
            KindNames { fits: vec![(1, 0, None)], ..KindNames::default() },
            KindNames { fits: vec![(2, 0, None)], ..KindNames::default() },
            KindNames { fits: vec![(1, 0, Some(0))], ..KindNames::default() },
            KindNames::default(),
        ],
        exclusive: vec![1 | 2],
        ..Names::default()
    }
}

#[test]
fn categories_accept_matching_values_and_reject_incompatible_union_arms() {
    let world = TestWorld::new();
    let tables = tables();
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    assert_eq!(accepts(&value(0), &value(0), &values), Known::Yes(true));
    assert_eq!(accepts(&value(0), &value(1), &values), Known::Yes(false));
    let union = NodeValue::Alternatives(vec![vec![leaf(0)], vec![leaf(1)]]);
    assert_eq!(accepts(&value(0), &union, &values), Known::Yes(false));
    assert_eq!(accepts(&union, &value(1), &values), Known::Yes(true));
}

#[test]
fn missing_descriptors_and_unknown_conditions_do_not_become_acceptance_proofs() {
    let world = TestWorld::new();
    let mut tables = tables();
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(accepts(&value(0), &value(2), &semantic.values()), Known::Unknown);
    assert_eq!(accepts(&value(3), &value(0), &semantic.values()), Known::Unknown);
    tables.exclusive.clear();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(accepts(&value(0), &value(0), &semantic.values()), Known::Unknown);
}

#[test]
fn never_and_budget_exhaustion_keep_their_logical_meanings() {
    let world = TestWorld::new();
    let tables = tables();
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(accepts(&value(0), &NodeValue::never(), &semantic.values()), Known::Yes(true));
    assert_eq!(accepts(&NodeValue::never(), &value(0), &semantic.values()), Known::Yes(false));
    let mut question = QueryContext::default();
    question.budget = Budget::new(0);
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(accepts(&value(0), &value(0), &semantic.values()), Known::Unknown);
}

#[test]
fn overload_sets_preserve_union_alternatives_and_intersection_declarations() {
    let world = TestWorld::new();
    let mut tables = tables();
    std::sync::Arc::get_mut(&mut tables.vocabulary).unwrap().traits =
        vec!["first".into(), "second".into(), "callable".into(), "constructible".into()];
    for kind in &mut tables.kinds[..2] {
        kind.fits.push((4, 0, None));
        kind.calls.signatures.push(Conditional {
            item: Signature { parameters: vec![0], result: super::Value::Path(vec![0]) },
            when: None,
        });
    }
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let union = NodeValue::Alternatives(vec![vec![leaf(0)], vec![leaf(1)]]);
    let Known::Yes(sets) = semantic.calls().headers(&union, InvokeKind::Call) else { panic!("known overload sets") };
    assert_eq!(sets.len(), 2);
    assert!(sets[0].iter().all(|header| header.site == leaf(0)));
    assert!(sets[1].iter().all(|header| header.site == leaf(1)));
    assert!(!sets[0].is_empty() && !sets[1].is_empty());

    tables.exclusive.clear();
    let semantic = Semantic::new(&world, &tables, &question);
    let intersection = NodeValue::Alternatives(vec![vec![leaf(0), leaf(1)]]);
    let Known::Yes(sets) = semantic.calls().headers(&intersection, InvokeKind::Call) else { panic!("known combined overloads") };
    assert_eq!(sets.len(), 1);
    assert!(sets[0].iter().any(|header| header.site == leaf(0)));
    assert!(sets[0].iter().any(|header| header.site == leaf(1)));
    tables.kinds[1].fits.pop();
    tables.kinds[1].fits.push((8, 0, None));
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(calls) = semantic.calls().headers(&intersection, InvokeKind::Call) else { panic!("known call headers") };
    let Known::Yes(constructs) = semantic.calls().headers(&intersection, InvokeKind::Construct) else { panic!("known construct headers") };
    assert!(calls[0].iter().all(|header| header.site == leaf(0)));
    assert!(constructs[0].iter().all(|header| header.site == leaf(1)));
    assert!(!calls[0].is_empty() && !constructs[0].is_empty());
}

#[test]
fn uncertain_signature_conditions_do_not_publish_complete_overload_sets() {
    let world = TestWorld::new();
    let mut tables = tables();
    std::sync::Arc::get_mut(&mut tables.vocabulary).unwrap().traits =
        vec!["first".into(), "second".into(), "callable".into()];
    tables.kinds[0].fits.push((4, 0, None));
    tables.kinds[0].calls.signatures.push(Conditional {
        item: Signature { parameters: vec![0], result: super::Value::Path(vec![0]) },
        when: Some(0),
    });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.calls().headers(&value(0), InvokeKind::Call), Known::Unknown);
}

fn call_fixture() -> (TestWorld, Names) {
    let mut world = TestWorld::new();
    let mut nodes: Vec<_> = [0, 1, 2, 3, 4, 5, 0, 1, 0, 1, 1].into_iter().map(|matcher| Node {
        matcher, start: 0, end: 0, children: Vec::new(),
    }).collect();
    nodes[4].children = [(10, 6), (11, 7), (12, 8), (13, 9)].into_iter()
        .map(|(label, node)| Edge { label, node }).collect();
    nodes[5].children = [(14, 4), (15, 10)].into_iter()
        .map(|(label, node)| Edge { label, node }).collect();
    world.tree = Tree::from_native(nodes, 5);
    let mut tables = tables();
    std::sync::Arc::get_mut(&mut tables.vocabulary).unwrap().traits =
        vec!["first".into(), "second".into(), "callable".into(), "rest".into()];
    tables.kinds.extend([KindNames::default(), KindNames::default()]);
    tables.kinds[4].fits.push((4, 0, None));
    for (parameters, result) in [(10, 12), (11, 13)] {
        tables.kinds[4].calls.signatures.push(Conditional {
            item: Signature { parameters: vec![parameters], result: super::Value::Path(vec![result]) },
            when: None,
        });
    }
    tables.kinds[5].calls.invokes.push(Conditional {
        item: Invoke { callee: super::Value::Path(vec![14]), arguments: vec![15], construct: false },
        when: None,
    });
    (world, tables)
}

#[test]
fn selection_uses_compatible_headers_and_preserves_caller_argument_frames() {
    let (world, tables) = call_fixture();
    let question = QueryContext::default();
    let caller_frame = question.bind(FrameId::ROOT, NodeRef::here(3), None, None);
    let semantic = Semantic::new(&world, &tables, &question);
    let call = Leaf { frame: caller_frame, ..leaf(5) };
    let Known::Yes(Some(selected)) = semantic.calls().selection(call) else { panic!("compatible overload selected") };
    assert!(!selected.uncertain);
    assert_eq!(selected.choices.len(), 1);
    let chosen = &selected.choices[0];
    assert_eq!(chosen.header.signature, 1);
    assert_eq!(chosen.mapping.len(), 1);
    assert_eq!(chosen.mapping[0].0.target, leaf(7).target);
    assert_eq!(chosen.mapping[0].0.frame, chosen.header.site.frame);
    assert_eq!(chosen.mapping[0].1, vec![Leaf { frame: caller_frame, ..leaf(10) }]);
    let binding = question.get(chosen.header.site.frame).expect("callee substitution frame");
    assert_eq!(binding.owner, NodeRef::here(4));
    assert_eq!(binding.through, Some(call));
}

#[test]
fn selected_header_through_keeps_caller_generics_separate_from_callee_environment() {
    let (mut world, mut tables) = call_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes.extend([6, 3, 7, 3, 3, 8].into_iter().map(|matcher| Node {
        matcher, start: 0, end: 0, children: Vec::new(),
    }));
    nodes[4].children.extend([Edge { label: 31, node: 13 }, Edge { label: 50, node: 12 }]);
    nodes[5].children[0].node = 11;
    nodes[5].children.push(Edge { label: 40, node: 16 });
    nodes[13].children.push(Edge { label: 42, node: 15 });
    nodes[16].children.push(Edge { label: 42, node: 14 });
    world.tree = Tree::from_native(nodes, 5);
    world.names.declaration.extend([(11, 12), (14, 15)]);
    world.names.declared.extend([(12, (0, 4, 1, false)), (15, (0, 13, 1, false))]);
    tables.kinds[4].generics.push((vec![31], None));
    for path in [Some(vec![crate::grammar::names::DECLARATION]), None, Some(vec![42, crate::grammar::names::DECLARATION])] {
        let mut kind = KindNames::default();
        if let Some(path) = path {
            kind.unwraps.push(crate::grammar::names::Unwrapping {
                value: Some(super::Value::Path(path)),
                ..Default::default()
            });
        }
        tables.kinds.push(kind);
    }
    let question = QueryContext::default();
    let caller_frame = question.bind(FrameId::ROOT, NodeRef::here(4), None, Some(vec![value(1)]));
    let call = Leaf { frame: caller_frame, ..leaf(5) };
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(selected)) = semantic.calls().selection(call) else { panic!("generic candidate retained") };
    let header = selected.choices[0].header.site;
    let binding = question.get(header.frame).expect("selected callee environment");
    assert_ne!(binding.parent, caller_frame, "declaration keeps its own environment");
    assert_eq!(semantic.values().follow(header, &[crate::grammar::names::THROUGH]), Known::Yes(vec![call]));
    let Known::Yes(formals) = semantic.values().follow(header, &[crate::grammar::names::THROUGH, 40, 42, crate::grammar::names::DECLARATION])
        else { panic!("caller type argument declaration") };
    assert_eq!(formals.len(), 1);
    assert_eq!(semantic.values().bound_argument(formals[0]), Known::Yes(Some(value(1))));
}

#[test]
fn plain_parameter_binding_maps_to_raw_caller_arguments_not_type_slots() {
    let (mut world, mut tables) = call_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes.push(Node { matcher: 3, start: 0, end: 0, children: Vec::new() });
    nodes[7].children.push(Edge { label: 80, node: 11 });
    world.tree = Tree::from_native(nodes, 5);
    world.names.declared.insert(11, (0, 4, 1, false));
    tables.kinds[1].targets.push((vec![vec![80]], None));
    let question = QueryContext::default();
    let caller_frame = question.bind(FrameId::ROOT, NodeRef::here(3), None, Some(vec![value(0)]));
    let call = Leaf { frame: caller_frame, ..leaf(5) };
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(selected)) = semantic.calls().selection(call) else { panic!("selected header") };
    let binding = Leaf { frame: selected.choices[0].header.site.frame, ..leaf(11) };
    assert_eq!(semantic.calls().argument_sites(binding),
        Known::Yes(Some(vec![Leaf { frame: caller_frame, ..leaf(10) }])));
    assert_eq!(semantic.calls().argument_sites(leaf(11)), Known::Yes(None));
}

#[test]
fn indirect_and_uncertain_parameter_bindings_never_project_whole_arguments() {
    let (mut world, mut tables) = call_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes.push(Node { matcher: 3, start: 0, end: 0, children: Vec::new() });
    nodes.push(Node { matcher: 6, start: 0, end: 0, children: vec![Edge { label: 81, node: 11 }] });
    nodes[7].children.push(Edge { label: 80, node: 12 });
    world.tree = Tree::from_native(nodes, 5);
    world.names.declared.insert(11, (0, 4, 1, false));
    tables.kinds[1].targets.push((vec![vec![80]], None));
    tables.kinds.push(KindNames { targets: vec![(vec![vec![81]], None)], ..KindNames::default() });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(selected)) = semantic.calls().selection(leaf(5)) else { panic!("selected header") };
    let binding = Leaf { frame: selected.choices[0].header.site.frame, ..leaf(11) };
    assert_eq!(semantic.calls().argument_sites(binding), Known::Unknown);

    world.tree.nodes[7].children[0].node = 11;
    tables.kinds.push(KindNames { fits: vec![(2, 0, Some(0))], ..KindNames::default() });
    world.tree.nodes[10].matcher = 7;
    tables.kinds[4].calls.signatures.remove(0);
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(selected)) = semantic.calls().selection(leaf(5)) else { panic!("uncertain candidate retained") };
    assert!(selected.uncertain);
    let binding = Leaf { frame: selected.choices[0].header.site.frame, ..leaf(11) };
    assert_eq!(semantic.calls().argument_sites(binding), Known::Unknown);
}

#[test]
fn selection_prefers_proof_over_unknown_and_never_invents_invalid_call_results() {
    let (mut world, tables) = call_fixture();
    world.tree.nodes[6].matcher = 2;
    world.tree.nodes[7].matcher = 0;
    world.tree.nodes[10].matcher = 0;
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(selected)) = semantic.calls().selection(leaf(5)) else { panic!("proven overload selected") };
    assert_eq!(selected.choices[0].header.signature, 1);
    assert!(!selected.uncertain);

    world.tree.nodes[7].matcher = 1;
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(selected)) = semantic.calls().selection(leaf(5)) else { panic!("uncertain candidate retained") };
    assert_eq!(selected.choices[0].header.signature, 0);
    assert!(selected.uncertain);

    world.tree.nodes[6].matcher = 0;
    world.tree.nodes[10].matcher = 3;
    let semantic = Semantic::new(&world, &tables, &question);
    assert!(matches!(semantic.calls().selection(leaf(5)), Known::Unknown));
}

#[test]
fn declaration_results_do_not_require_callable_value_unwrapping() {
    let (world, mut tables) = call_fixture();
    tables.kinds[4].fits.clear();
    tables.kinds[4].unwraps.push(crate::grammar::names::Unwrapping {
        value: Some(super::Value::Path(vec![99])),
        ..Default::default()
    });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(result)) = semantic.calls().result(leaf(4)) else { panic!("declared returns are available") };
    assert_eq!(semantic.values().could(&result, 1), Known::Yes(true));
    assert_eq!(semantic.values().could(&result, 2), Known::Yes(true));
    assert_eq!(semantic.values().always(&result, 1), Known::Yes(false));
}

#[test]
fn invocation_results_use_only_selected_headers_and_keep_uncertainty() {
    let (mut world, mut tables) = call_fixture();
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(result)) = semantic.calls().result(leaf(5)) else { panic!("selected result is available") };
    assert_eq!(semantic.values().always(&result, 2), Known::Yes(true));
    assert_eq!(semantic.values().could(&result, 1), Known::Yes(false));
    tables.kinds.push(KindNames { fits: vec![(1, 0, Some(0)), (2, 0, Some(1))], ..KindNames::default() });
    world.tree.nodes[10].matcher = 6;
    let semantic = Semantic::new(&world, &tables, &question);
    assert!(matches!(semantic.calls().result(leaf(5)), Known::Unknown));
}

#[test]
fn uncertain_candidates_can_prove_a_result_only_when_all_possible_outputs_agree() {
    let (mut world, mut tables) = call_fixture();
    tables.kinds.push(KindNames { fits: vec![(1, 0, Some(0)), (2, 0, Some(1))], ..KindNames::default() });
    world.tree.nodes[10].matcher = 6;
    world.tree.nodes[9].matcher = 0;
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(result)) = semantic.calls().result(leaf(5)) else { panic!("all possible outputs agree") };
    assert_eq!(semantic.values().always(&result, 1), Known::Yes(true));
    world.tree.nodes[9].matcher = 1;
    let semantic = Semantic::new(&world, &tables, &question);
    assert!(matches!(semantic.calls().result(leaf(5)), Known::Unknown));
}

#[test]
fn unavailable_parameter_projection_does_not_hide_a_later_proven_overload() {
    let (world, mut tables) = call_fixture();
    tables.kinds[4].calls.signatures[0].item.parameters = vec![crate::grammar::names::ATTR_STEP];
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(selected)) = semantic.calls().selection(leaf(5)) else { panic!("later known candidate wins") };
    assert_eq!(selected.choices[0].header.signature, 1);
    assert!(!selected.uncertain);
}

#[test]
fn container_results_follow_declared_header_sources() {
    let (mut world, mut tables) = call_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes[4].children = vec![Edge { label: 20, node: 11 }];
    nodes.push(Node { matcher: 6, start: 0, end: 0, children: vec![Edge { label: 21, node: 8 }] });
    world.tree = Tree::from_native(nodes, 5);
    tables.kinds[4].calls.signatures.clear();
    tables.kinds[4].calls.sources.push(Conditional { item: super::Value::Path(vec![20]), when: None });
    tables.kinds.push(KindNames {
        fits: vec![(4, 0, None)],
        calls: super::KindCalls {
            signatures: vec![Conditional {
                item: Signature { parameters: vec![10], result: super::Value::Path(vec![21]) },
                when: None,
            }],
            ..super::KindCalls::default()
        },
        ..KindNames::default()
    });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(result)) = semantic.calls().result(leaf(4)) else { panic!("sourced result is available") };
    assert_eq!(semantic.values().always(&result, 1), Known::Yes(true));
    assert_eq!(semantic.values().could(&result, 2), Known::Yes(false));
}

#[test]
fn exhausted_engine_nesting_does_not_enter_queries_or_poison_later_answers() {
    let (world, tables) = call_fixture();
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let mut active = Vec::new();
    while let Some(nest) = question.budget.nest() { active.push(nest) }
    assert!(matches!(semantic.calls().argument_sites(leaf(7)), Known::Unknown));
    assert!(matches!(semantic.calls().result(leaf(5)), Known::Unknown));
    assert!(matches!(semantic.calls().selection(leaf(5)), Known::Unknown));
    assert!(matches!(semantic.calls().headers(&value(4), InvokeKind::Call), Known::Unknown));
    assert_eq!(semantic.calls().accepts(&value(0), &value(0)), Known::Unknown);
    drop(active);
    assert!(matches!(semantic.calls().result(leaf(5)), Known::Yes(Some(_))));
    assert_eq!(semantic.calls().accepts(&value(0), &value(0)), Known::Yes(true));
}

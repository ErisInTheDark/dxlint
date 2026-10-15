use crate::config::ast::Config;
use crate::grammar::{Edge, Grammar, Node, Tree};
use crate::grammar::names::{KindNames, Names, Resolved};
use crate::grammar::names::question::{Budget, FrameId, Known, Leaf, NodeRef, NodeValue, QueryContext, ScopeRef, Target, World};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::values::Values;
use super::{Conditional, Expected, ExpectedAt, Invoke, InvokeKind, Signature};

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
    fn index(&self, file: u32) -> Option<&crate::grammar::names::ScopeIndex> {
        (file == 0).then(|| self.names.index(&self.tree, "", &|key| key.into()))
    }
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
fn expected_recipes_keep_owner_context_or_use_each_real_subject() {
    let mut world = TestWorld::new();
    let mut nodes = world.tree.nodes.clone();
    nodes[3].children = vec![Edge { label: 40, node: 0 }, Edge { label: 40, node: 1 }];
    world.tree = Tree::from_native(nodes, 3);
    for at in [ExpectedAt::Owner, ExpectedAt::Subject] {
        let mut tables = tables();
        tables.kinds[3].calls.expected.push(Conditional {
            item: Expected { field: vec![40], value: super::Value::Path(vec![super::super::IT]), at },
            when: None,
        });
        let question = QueryContext::default();
        let frame = question.bind(FrameId::ROOT, NodeRef::here(3), None, Some(vec![value(2)]));
        let semantic = Semantic::new(&world, &tables, &question);
        for node in [0, 1] {
            let subject = Leaf { frame, ..leaf(node) };
            let expected = Leaf { frame, ..leaf(if at == ExpectedAt::Owner { 3 } else { node }) };
            assert_eq!(semantic.calls().expected(subject), Known::Yes(Some(NodeValue::leaf(expected))));
        }
    }
}

#[test]
fn expected_recipes_distinguish_missing_context_from_failed_and_empty_values() {
    for at in [ExpectedAt::Owner, ExpectedAt::Subject] {
        for own in [false, true] {
            let mut world = TestWorld::new();
            let mut nodes = world.tree.nodes.clone();
            nodes[3].children.push(Edge { label: 40, node: 0 });
            world.tree = Tree::from_native(nodes, 3);
            let owner = if own { 0 } else { 3 };
            let field = if own { vec![super::super::IT] } else { vec![40] };
            let absent = super::Value::Either(
                Box::new(super::Value::Path(vec![41])),
                Box::new(super::Value::Path(vec![42])),
            );
            let failed = super::Value::From(
                Box::new(super::Value::Current), Box::new(super::Value::Unknown), 0,
            );
            let empty = super::Value::Spread { intersection: false, path: vec![41], value: None };
            for (recipes, expected) in [
                (vec![absent.clone()], Known::Yes(None)),
                (vec![failed], Known::Unknown),
                (vec![empty], Known::Yes(Some(NodeValue::never()))),
                (vec![absent.clone(), super::Value::Current], Known::Yes(Some(value(if at == ExpectedAt::Owner { owner } else { 0 })))),
                (vec![super::Value::Current, absent], Known::Yes(Some(value(if at == ExpectedAt::Owner { owner } else { 0 })))),
            ] {
                let mut tables = tables();
                for recipe in recipes {
                    tables.kinds[owner as usize].calls.expected.push(Conditional {
                        item: Expected { field: field.clone(), value: recipe, at }, when: None,
                    });
                }
                let question = QueryContext::default();
                let semantic = Semantic::new(&world, &tables, &question);
                assert_eq!(semantic.calls().expected(leaf(0)), expected, "{at:?}, own={own}");
            }
        }
    }
}

#[test]
fn unannotated_returns_have_no_expected_type_when_both_context_sources_are_absent() {
    let mut world = TestWorld::new();
    let mut nodes = world.tree.nodes.clone();
    nodes[3].children.push(Edge { label: 40, node: 0 });
    world.tree = Tree::from_native(nodes, 3);
    let mut tables = tables();
    tables.kinds[3].calls.expected.push(Conditional {
        item: Expected {
            field: vec![40],
            value: super::Value::Either(
                Box::new(super::Value::Path(vec![41])),
                Box::new(super::Value::Path(vec![super::super::EXPECTED, super::super::RESULT])),
            ),
            at: ExpectedAt::Owner,
        },
        when: None,
    });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.calls().expected(leaf(0)), Known::Yes(None));
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
fn structural_members_compare_values_and_required_presence() {
    use crate::grammar::names::values::{IndexedMode, IndexedRecipe};
    for (actual_type, expected_optional, actual_optional, absent, closed, expected) in [
        (0, false, false, false, true, Known::Yes(true)),
        (1, false, false, false, true, Known::Yes(false)),
        (0, false, true, false, true, Known::Yes(false)),
        (0, true, true, false, true, Known::Yes(true)),
        (0, true, false, true, true, Known::Yes(true)),
        (0, false, false, true, true, Known::Yes(false)),
        (0, false, false, true, false, Known::Unknown),
    ] {
        let mut world = TestWorld::new();
        let mut tables = tables();
        let mut nodes = world.tree.nodes.clone();
        nodes.extend([4, 5, 6, 7].into_iter().map(|matcher| Node {
            matcher, start: 0, end: 0, children: Vec::new(),
        }));
        nodes[4].children.push(Edge { label: 40, node: 6 });
        if !absent { nodes[5].children.push(Edge { label: 40, node: 7 }) }
        nodes[6].children.push(Edge { label: 41, node: 0 });
        nodes[7].children.push(Edge { label: 41, node: actual_type });
        world.tree = Tree::from_native(nodes, 4);
        for _ in 0..2 {
            tables.kinds.push(KindNames {
                member_defaults: 1,
                member_closed: vec![(1, None)],
                indexed: vec![IndexedRecipe {
                    path: vec![40], mode: IndexedMode::Keyed(super::Value::Text("x".into())),
                    views: 1, value: None, when: None,
                }],
                ..KindNames::default()
            });
        }
        if !closed { tables.kinds[5].member_closed.clear() }
        for optional in [expected_optional, actual_optional] {
            let mut member = KindNames {
                unwraps: vec![crate::grammar::names::Unwrapping {
                    value: Some(super::Value::Path(vec![41])), ..Default::default()
                }],
                ..KindNames::default()
            };
            if optional { member.calls.optional.push(None) }
            tables.kinds.push(member);
        }
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(accepts(&value(4), &value(5), &semantic.values()), expected);
    }
}

#[test]
fn recursive_structures_relate_coinductively_but_still_fail_on_real_mismatches() {
    use crate::grammar::names::values::{IndexedMode, IndexedRecipe};
    // Shapes 4 and 5 are `{ x: Self, y: _ }`; y is category 0 on 4 and
    // `y_actual` on 5, so only x's comparison is recursive.
    for (y_actual, expected) in [(0, Known::Yes(true)), (1, Known::Yes(false))] {
        let mut world = TestWorld::new();
        let mut tables = tables();
        let mut nodes = world.tree.nodes.clone();
        nodes.extend((4..10).map(|matcher| Node { matcher, start: 0, end: 0, children: Vec::new() }));
        nodes[4].children.extend([Edge { label: 40, node: 6 }, Edge { label: 42, node: 8 }]);
        nodes[5].children.extend([Edge { label: 40, node: 7 }, Edge { label: 42, node: 9 }]);
        for (member, target) in [(6, 4), (7, 5), (8, 0), (9, y_actual)] {
            nodes[member].children.push(Edge { label: 41, node: target });
        }
        world.tree = Tree::from_native(nodes, 4);
        for _ in 0..2 {
            let keyed = |key: &str, path| IndexedRecipe {
                path: vec![path], mode: IndexedMode::Keyed(super::Value::Text(key.into())),
                views: 1, value: None, when: None,
            };
            tables.kinds.push(KindNames {
                member_defaults: 1,
                member_closed: vec![(1, None)],
                indexed: vec![keyed("x", 40), keyed("y", 42)],
                ..KindNames::default()
            });
        }
        for _ in 6..10 {
            tables.kinds.push(KindNames {
                unwraps: vec![crate::grammar::names::Unwrapping {
                    value: Some(super::Value::Path(vec![41])), ..Default::default()
                }],
                ..KindNames::default()
            });
        }
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(accepts(&value(4), &value(5), &semantic.values()), expected, "y_actual={y_actual}");
        assert_eq!(accepts(&value(4), &value(5), &semantic.values()), expected, "memoised, y_actual={y_actual}");
    }
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
            item: Signature { parameters: vec![0], receiver: None, result: super::Value::Path(vec![0]) },
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
        item: Signature { parameters: vec![0], receiver: None, result: super::Value::Path(vec![0]) },
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
            item: Signature { parameters: vec![parameters], receiver: None, result: super::Value::Path(vec![result]) },
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

fn generic_fixture() -> (TestWorld, Names) {
    let (mut world, mut tables) = call_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes.extend([6, 7, 3, 3, 5, 0, 0].into_iter().map(|matcher| Node {
        matcher, start: 0, end: 0, children: Vec::new(),
    }));
    nodes[4].children.push(Edge { label: 31, node: 11 });
    nodes[6].matcher = 8;
    nodes[6].children.push(Edge { label: 61, node: 12 });
    nodes[11].children.extend([Edge { label: 42, node: 14 }, Edge { label: 66, node: 17 }]);
    nodes[12].children.push(Edge { label: 42, node: 13 });
    nodes[15].children = vec![Edge { label: 14, node: 4 }, Edge { label: 15, node: 16 }];
    world.tree = Tree::from_native(nodes, 5);
    world.names.declaration.insert(13, 14);
    world.names.declared.insert(14, (0, 11, 1, false));
    tables.kinds[4].generics.push((vec![31], None));
    tables.kinds[4].calls.signatures.truncate(1);
    for path in [vec![66], vec![42, crate::grammar::names::DECLARATION], vec![61]] {
        tables.kinds.push(KindNames {
            unwraps: vec![crate::grammar::names::Unwrapping {
                value: Some(if path == [66] {
                    super::Value::Either(
                        Box::new(super::Value::Path(vec![crate::grammar::names::INFERRED])),
                        Box::new(super::Value::Path(path)),
                    )
                } else { super::Value::Path(path) }),
                ..Default::default()
            }],
            ..KindNames::default()
        });
    }
    (world, tables)
}

fn matching_signature_fixture() -> (TestWorld, Names) {
    let (mut world, mut tables) = call_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes.extend([3, 3].into_iter().map(|matcher| Node {
        matcher, start: 0, end: 0, children: Vec::new(),
    }));
    nodes[5].children = vec![Edge { label: 10, node: 11 }, Edge { label: 12, node: 12 }];
    world.tree = Tree::from_native(nodes, 5);
    tables.kinds[4].calls.signatures.truncate(1);
    tables.kinds[4].calls.signatures[0].item.receiver = Some(vec![99]);
    tables.kinds[5].fits.push((4, 0, None));
    tables.kinds[5].calls.signatures = tables.kinds[4].calls.signatures.clone();
    (world, tables)
}

#[test]
fn product_patterns_capture_recipe_holes_without_replacing_them_with_defaults() {
    use crate::grammar::names::question::ProductShapeId;
    use crate::grammar::names::values::products::{ProductElement, Shape};
    let (mut world, mut tables) = matching_signature_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes.extend([11, 12].into_iter().map(|node| Node {
        matcher: 1, start: 0, end: 0, children: vec![Edge { label: 22, node }],
    }));
    nodes[5].children = vec![Edge { label: 10, node: 13 }, Edge { label: 10, node: 14 }];
    world.tree = Tree::from_native(nodes, 5);
    tables.kinds[3].unwraps.push(crate::grammar::names::Unwrapping {
        value: Some(super::Value::Traits(1)), ..Default::default()
    });
    tables.products.push(Some(Shape {
        traits: 0, description: crate::expr::Value::Null,
        inherits: None, readonly_inherits: None, optional: None,
    }));
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(3), None, Some(vec![value(1)]));
    let semantic = Semantic::new(&world, &tables, &question);
    let elements: Vec<_> = [6, 7].into_iter().map(|node| {
        let site = Leaf { frame, ..leaf(node) };
        ProductElement { site, value: NodeValue::leaf(site), optional: false, repeated: false, values: false }
    }).collect();
    let actual = semantic.values().product(ProductShapeId(0), leaf(4), elements.clone(), false);
    let pattern = super::Value::Product(crate::grammar::names::values::compile::ProductRecipe {
        shape: ProductShapeId(0), sites: vec![10], value: Some(Box::new(super::Value::Path(vec![22]))),
        optional: None, repeated: None, values: None, readonly: None,
    });
    let Known::Yes(result) = semantic.calls().match_value(leaf(5), &pattern, &actual, &[leaf(11), leaf(12)])
        else { panic!("known product inference") };
    assert!(result.accepted);
    assert_eq!(result.arguments, elements.into_iter().map(|element| Some(element.value)).collect::<Vec<_>>());
}

#[test]
fn rest_inference_keeps_order_optional_markers_and_source_frames() {
    use crate::grammar::names::question::ProductShapeId;
    for length in [0, 2] {
        let (mut world, mut tables) = matching_signature_fixture();
        let mut nodes = world.tree.nodes.clone();
        nodes[4].children.retain(|edge| edge.label != 10);
        if length != 0 {
            nodes[4].children.extend([Edge { label: 10, node: 6 }, Edge { label: 10, node: 7 }]);
        }
        nodes[5].children = vec![Edge { label: 10, node: 11 }, Edge { label: 12, node: 8 }];
        world.tree = Tree::from_native(nodes, 5);
        tables.kinds[3].fits.push((8, 0, None));
        tables.kinds[3].packs.push((ProductShapeId(0), None));
        tables.kinds[1].calls.optional.push(None);
        tables.products.push(Some(crate::grammar::names::values::products::Shape {
            traits: 0, description: crate::expr::Value::Null,
            inherits: None, readonly_inherits: None, optional: None,
        }));
        let question = QueryContext::default();
        let frame = question.bind(FrameId::ROOT, NodeRef::here(3), None, Some(vec![value(1)]));
        let semantic = Semantic::new(&world, &tables, &question);
        let actual = NodeValue::leaf(Leaf { frame, ..leaf(4) });
        let Known::Yes(result) = semantic.calls().match_value(leaf(5), &super::Value::Current, &actual, &[leaf(11)])
            else { panic!("known ordered parameter inference") };
        assert!(result.accepted);
        let NodeValue::Alternatives(arms) = result.arguments[0].as_ref().expect("rest slot") else { panic!("known parameter product") };
        let Known::Yes(Some(product)) = semantic.values().product_info(arms[0][0]) else { panic!("real product") };
        assert_eq!(product.elements.len(), length);
        for (position, element) in product.elements.iter().enumerate() {
            let site = Leaf { frame, ..leaf(6 + position as u32) };
            assert_eq!(element.site, site);
            assert_eq!(element.value, NodeValue::leaf(site));
            assert_eq!(element.optional, position == 1);
            assert!(!element.repeated);
        }
    }
}

#[test]
fn matching_captures_signature_parameters_and_results_in_the_source_frame() {
    let (mut world, mut tables) = matching_signature_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes[4].children.push(Edge { label: 98, node: 3 });
    world.tree = Tree::from_native(nodes, 5);
    tables.kinds[4].generics.push((vec![98], None));
    let question = QueryContext::default();
    let source_frame = question.bind(FrameId::ROOT, NodeRef::here(3), None, Some(vec![value(1)]));
    let semantic = Semantic::new(&world, &tables, &question);
    let actual = NodeValue::leaf(Leaf { frame: source_frame, ..leaf(4) });
    let Known::Yes(result) = semantic.calls().match_value(
        leaf(5), &super::Value::Current, &actual, &[leaf(11), leaf(12)],
    ) else { panic!("known callback inference") };
    assert!(result.accepted);
    assert_eq!(result.arguments, vec![
        Some(NodeValue::leaf(Leaf { frame: source_frame, ..leaf(6) })),
        Some(NodeValue::leaf(Leaf { frame: source_frame, ..leaf(8) })),
    ]);
}

#[test]
fn matching_mixed_variance_selects_candidates_before_checking_compatibility() {
    for result_node in [8, 9] {
        let (mut world, tables) = matching_signature_fixture();
        let mut nodes = world.tree.nodes.clone();
        nodes[4].children.iter_mut().find(|edge| edge.label == 12).unwrap().node = result_node;
        nodes[5].children.iter_mut().find(|edge| edge.label == 12).unwrap().node = 11;
        world.tree = Tree::from_native(nodes, 5);
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let Known::Yes(result) = semantic.calls().match_value(leaf(5), &super::Value::Current, &value(4), &[leaf(11)])
            else { panic!("known mixed inference") };
        assert_eq!(result.accepted, result_node == 8);
        assert_eq!(result.arguments, vec![Some(value(if result_node == 8 { 8 } else { 6 }))]);
    }
}

#[test]
fn matching_parameter_variance_uses_intersection_or_bivariant_union() {
    for bivariant in [false, true] {
        let (mut world, mut tables) = matching_signature_fixture();
        let mut nodes = world.tree.nodes.clone();
        nodes[4].children.push(Edge { label: 10, node: 7 });
        nodes[5].children = vec![Edge { label: 10, node: 11 }, Edge { label: 10, node: 11 }, Edge { label: 12, node: 8 }];
        world.tree = Tree::from_native(nodes, 5);
        if bivariant {
            tables.kinds[5].calls.variances.push(Conditional {
                item: crate::config::ast::calls::Variance::Bivariant, when: None,
            });
        }
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let Known::Yes(result) = semantic.calls().match_value(leaf(5), &super::Value::Current, &value(4), &[leaf(11)])
            else { panic!("known parameter inference") };
        assert!(result.accepted);
        let inferred = result.arguments[0].as_ref().expect("parameter slot");
        if bivariant {
            assert_eq!(semantic.values().could(inferred, 1), Known::Yes(true));
            assert_eq!(semantic.values().could(inferred, 2), Known::Yes(true));
        } else {
            assert_eq!(inferred, &NodeValue::never());
        }
    }
}

#[test]
fn inference_obeys_the_declared_last_overload_without_changing_acceptance() {
    for infer in [false, true] {
        let (mut world, mut tables) = matching_signature_fixture();
        let mut nodes = world.tree.nodes.clone();
        nodes[5].children = vec![
            Edge { label: 10, node: 6 }, Edge { label: 12, node: if infer { 12 } else { 8 } },
        ];
        world.tree = Tree::from_native(nodes, 5);
        let mut last = tables.kinds[4].calls.signatures[0].clone();
        last.item.result = super::Value::Path(vec![13]);
        tables.kinds[4].calls.signatures.push(last);
        tables.kinds[4].calls.infer_last.push(None);
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        if infer {
            let Known::Yes(result) = semantic.calls().match_value(
                leaf(5), &super::Value::Current, &value(4), &[leaf(12)],
            ) else { panic!("known overload inference") };
            assert!(result.accepted);
            let inferred = result.arguments[0].as_ref().expect("return slot");
            assert_eq!(semantic.values().could(inferred, 2), Known::Yes(true));
            assert_eq!(semantic.values().could(inferred, 1), Known::Yes(false));
        } else {
            assert_eq!(semantic.calls().accepts(&value(5), &value(4)), Known::Yes(true));
        }
    }
}

#[test]
fn matching_raw_patterns_uses_the_real_owners_qualified_view() {
    let mut world = TestWorld::new();
    let mut tables = tables();
    let mut nodes = world.tree.nodes.clone();
    nodes.push(Node { matcher: 4, start: 0, end: 0, children: vec![Edge { label: 40, node: 2 }] });
    world.tree = Tree::from_native(nodes, 4);
    tables.kinds.push(KindNames {
        unwraps: vec![
            crate::grammar::names::Unwrapping {
                value: Some(super::Value::Traits(2)), ..Default::default()
            },
            crate::grammar::names::Unwrapping {
                classes: 1, value: Some(super::Value::Path(vec![40])), ..Default::default()
            },
        ],
        ..KindNames::default()
    });
    let question = QueryContext::default();
    let qualified = question.bind_definition(FrameId::ROOT, NodeRef::here(4), None, None, 1);
    let unrelated = question.bind_definition(FrameId::ROOT, NodeRef::here(3), None, None, 1);
    let semantic = Semantic::new(&world, &tables, &question);
    for (frame, accepted) in [(qualified, true), (FrameId::ROOT, false), (unrelated, false)] {
        let Known::Yes(result) = semantic.calls().match_value(
            Leaf { frame, ..leaf(4) }, &super::Value::Current, &value(0), &[leaf(2)],
        ) else { panic!("known declaration view") };
        assert_eq!(result.accepted, accepted);
        assert_eq!(result.arguments, vec![accepted.then(|| value(0))]);
    }
}

#[test]
fn captured_arguments_fall_back_to_known_constraints_and_defer_unknown_bounds() {
    for constraint in [
        super::Value::Union(Vec::new()), super::Value::Traits(1), super::Value::Path(vec![99]),
    ] {
        let mut world = TestWorld::new();
        let mut tables = tables();
        let mut nodes = world.tree.nodes.clone();
        nodes.push(Node { matcher: 4, start: 0, end: 0, children: vec![Edge { label: 40, node: 2 }] });
        world.tree = Tree::from_native(nodes, 4);
        tables.kinds.push(KindNames { generics: vec![(vec![40], None)], ..KindNames::default() });
        tables.kinds[2].constraints.push((constraint.clone(), None));
        tables.dynamic = 4;
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let actual = if matches!(constraint, super::Value::Union(_)) {
            NodeValue::leaf(Leaf { target: Target::Traits(4), frame: FrameId::ROOT })
        } else { value(1) };
        let result = semantic.calls().match_value(leaf(4), &super::Value::Path(vec![40]), &actual, &[leaf(2)]);
        if matches!(constraint, super::Value::Path(_)) {
            assert_eq!(result, Known::Unknown);
        } else {
            let Known::Yes(result) = result else { panic!("known constrained candidate") };
            assert!(!result.accepted);
            let inferred = result.arguments[0].as_ref().expect("captured slot");
            if matches!(constraint, super::Value::Union(_)) { assert_eq!(inferred, &NodeValue::never()) }
            else { assert_eq!(semantic.values().could(inferred, 1), Known::Yes(true)) }
        }
    }
}

#[test]
fn constrained_validation_does_not_invent_bindings_from_another_alternative() {
    let mut world = TestWorld::new();
    let mut tables = tables();
    let mut nodes = world.tree.nodes.clone();
    nodes.push(Node { matcher: 4, start: 0, end: 0, children: vec![
        Edge { label: 40, node: 2 }, Edge { label: 40, node: 3 },
        Edge { label: 41, node: 2 }, Edge { label: 42, node: 3 },
    ] });
    world.tree = Tree::from_native(nodes, 4);
    tables.kinds.push(KindNames { generics: vec![(vec![40], None)], ..KindNames::default() });
    tables.kinds[2].constraints.push((super::Value::Union(Vec::new()), None));
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let pattern = super::Value::Union(vec![super::Value::Path(vec![41]), super::Value::Path(vec![42])]);
    assert_eq!(semantic.calls().match_value(leaf(4), &pattern, &value(1), &[leaf(2), leaf(3)]), Known::Unknown);
}

#[test]
fn dependent_constraints_use_the_complete_frame_and_reject_unstable_replacements() {
    for (bound, first, locked, expected) in [
        (2, 0, false, Known::Yes(Some(vec![Some(value(1)), Some(value(1))]))),
        (1, 0, false, Known::Unknown),
        (1, 1, true, Known::Yes(None)),
    ] {
        let mut world = TestWorld::new();
        let mut tables = tables();
        let mut nodes = world.tree.nodes.clone();
        nodes.push(Node { matcher: 4, start: 0, end: 0, children: vec![
            Edge { label: 40, node: 2 }, Edge { label: 40, node: 3 }, Edge { label: 42, node: 3 },
        ] });
        world.tree = Tree::from_native(nodes, 4);
        tables.kinds.push(KindNames { generics: vec![(vec![40], None)], ..KindNames::default() });
        tables.kinds[2].constraints.push((super::Value::Path(vec![crate::grammar::names::PARENT, 42, crate::grammar::names::INFERRED]), None));
        tables.kinds[3].constraints.push((super::Value::Traits(bound), None));
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.calls().constrained_arguments(
            leaf(4), None, &[leaf(2), leaf(3)], vec![Some(value(first)), Some(value(1))], &[locked, false],
        ), expected);
    }
}

#[test]
fn explicit_arguments_that_violate_constraints_do_not_prove_a_call_result() {
    let (mut world, mut tables) = generic_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes[5].children.iter_mut().find(|edge| edge.label == 15).unwrap().node = 0;
    nodes[5].children.push(Edge { label: 70, node: 0 });
    world.tree = Tree::from_native(nodes, 5);
    tables.kinds[5].arguments.push((vec![70], None));
    tables.kinds[6].constraints.push((super::Value::Traits(2), None));
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.calls().result(leaf(5)), Known::Unknown);
}

#[test]
fn matching_member_holes_keeps_failed_alternatives_out_of_chosen_bindings() {
    use crate::grammar::names::values::{IndexedMode, IndexedRecipe};
    let mut world = TestWorld::new();
    let mut tables = tables();
    let mut nodes = world.tree.nodes.clone();
    nodes.extend([4, 5, 6, 7].into_iter().map(|matcher| Node {
        matcher, start: 0, end: 0, children: Vec::new(),
    }));
    nodes[4].children = vec![Edge { label: 40, node: 2 }, Edge { label: 41, node: 1 }];
    nodes[5].children = vec![Edge { label: 40, node: 0 }, Edge { label: 41, node: 0 }];
    nodes[6].children = vec![Edge { label: 40, node: 3 }];
    nodes[7].children = vec![Edge { label: 80, node: 4 }, Edge { label: 81, node: 6 }];
    world.tree = Tree::from_native(nodes, 7);
    tables.kinds[2].unwraps.push(crate::grammar::names::Unwrapping {
        value: Some(super::Value::Traits(2)), ..Default::default()
    });
    for keys in [vec![(40, "x"), (41, "y")], vec![(40, "x"), (41, "y")], vec![(40, "x")]] {
        tables.kinds.push(KindNames {
            indexed: keys.into_iter().map(|(label, key)| IndexedRecipe {
                path: vec![label], mode: IndexedMode::Keyed(super::Value::Text(key.into())),
                views: 0, value: None, when: None,
            }).collect(),
            ..KindNames::default()
        });
    }
    tables.kinds.push(KindNames::default());
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(7), None, Some(vec![value(1)]));
    let semantic = Semantic::new(&world, &tables, &question);
    let actual = NodeValue::leaf(Leaf { frame, ..leaf(5) });
    let first = super::Value::Path(vec![80]);
    let Known::Yes(failed) = semantic.calls().match_value(leaf(7), &first, &actual, &[leaf(2), leaf(3)])
        else { panic!("known mismatch retains inference") };
    assert!(!failed.accepted);
    assert_eq!(failed.arguments[0], Some(NodeValue::leaf(Leaf { frame, ..leaf(0) })));
    let pattern = super::Value::Union(vec![first, super::Value::Path(vec![81])]);
    let Known::Yes(chosen) = semantic.calls().match_value(leaf(7), &pattern, &actual, &[leaf(2), leaf(3)])
        else { panic!("known union inference") };
    assert!(chosen.accepted);
    assert_eq!(chosen.arguments, vec![None, Some(NodeValue::leaf(Leaf { frame, ..leaf(0) }))]);
}

#[test]
fn explicit_type_argument_arity_selects_a_header_that_can_hold_every_slot() {
    for construct in [false, true] {
        for optional in [false, true] {
            for count in 0..=3 {
                let (mut world, mut tables) = call_fixture();
                let mut nodes = world.tree.nodes.clone();
                nodes.extend([6, 7, 3, 8].into_iter().map(|matcher| Node {
                    matcher, start: 0, end: 0, children: Vec::new(),
                }));
                nodes[5].children = vec![Edge { label: 14, node: 11 }];
                nodes[11].children = vec![Edge { label: 81, node: 4 }, Edge { label: 82, node: 12 }];
                nodes[4].children = vec![Edge { label: 20, node: 1 }];
                nodes[12].children = vec![
                    Edge { label: 20, node: 0 }, Edge { label: 31, node: 13 }, Edge { label: 31, node: 14 },
                ];
                for _ in 0..count { nodes[5].children.push(Edge { label: 70, node: 0 }) }
                world.tree = Tree::from_native(nodes, 5);
                tables.kinds[4].calls.signatures = vec![Conditional {
                    item: Signature { parameters: vec![99], receiver: None, result: super::Value::Path(vec![20]) },
                    when: None,
                }];
                tables.kinds[5].calls.invokes[0].item.construct = construct;
                tables.kinds[5].arguments.push((vec![70], None));
                tables.kinds.push(KindNames {
                    unwraps: vec![crate::grammar::names::Unwrapping {
                        value: Some(super::Value::Intersection(vec![
                            super::Value::Path(vec![81]), super::Value::Path(vec![82]),
                        ])),
                        ..Default::default()
                    }],
                    ..KindNames::default()
                });
                let mut generic = tables.kinds[4].clone();
                generic.generics.push((vec![31], None));
                tables.kinds.push(generic);
                let mut default = KindNames::default();
                if optional { default.calls.optional.push(None) }
                tables.kinds.push(default);
                std::sync::Arc::get_mut(&mut tables.vocabulary).unwrap().traits.push("constructible".into());
                for kind in [4, 7] { tables.kinds[kind].fits.push((16, 0, None)) }
                let question = QueryContext::default();
                let semantic = Semantic::new(&world, &tables, &question);
                let result = semantic.calls().result(leaf(5));
                match count {
                    0 => assert!(matches!(result, Known::Yes(Some(NodeValue::Alternatives(ref arms))) if arms[0][0].target == leaf(1).target)),
                    1 if !optional => assert_eq!(result, Known::Unknown),
                    1 | 2 => assert!(matches!(result, Known::Yes(Some(NodeValue::Alternatives(ref arms))) if arms[0][0].target == leaf(0).target)),
                    _ => assert_eq!(result, Known::Unknown),
                }
            }
        }
    }
}

#[test]
fn explicit_call_arguments_win_over_inference_and_keep_unsupplied_defaults() {
    for construct in [false, true] {
        for unknown in [false, true] {
            let (mut world, mut tables) = generic_fixture();
            let mut nodes = world.tree.nodes.clone();
            nodes.extend([6, 0, if unknown { 9 } else { 1 }].into_iter().map(|matcher| Node {
                matcher, start: 0, end: 0, children: Vec::new(),
            }));
            nodes[4].children.push(Edge { label: 31, node: 18 });
            nodes[18].children.push(Edge { label: 66, node: 19 });
            nodes[5].children.push(Edge { label: 70, node: 20 });
            world.tree = Tree::from_native(nodes, 5);
            tables.kinds[5].arguments.push((vec![70], None));
            tables.kinds[5].calls.invokes[0].item.construct = construct;
            tables.kinds[4].calls.signatures[0].item.result = super::Value::Path(vec![10, 61]);
            tables.kinds[6].unwraps[0].value = Some(super::Value::Either(
                Box::new(super::Value::Path(vec![crate::grammar::names::INFERRED])),
                Box::new(super::Value::Path(vec![66])),
            ));
            tables.kinds[6].calls.optional.push(None);
            std::sync::Arc::get_mut(&mut tables.vocabulary).unwrap().traits.push("constructible".into());
            tables.kinds[4].fits.push((16, 0, None));
            tables.kinds.push(KindNames {
                unwraps: vec![crate::grammar::names::Unwrapping {
                    value: Some(super::Value::Path(vec![99])),
                    ..Default::default()
                }],
                ..KindNames::default()
            });
            let question = QueryContext::default();
            let caller = question.bind(FrameId::ROOT, NodeRef::here(3), None, Some(vec![value(2)]));
            let semantic = Semantic::new(&world, &tables, &question);
            let call = Leaf { frame: caller, ..leaf(5) };
            let Known::Yes(Some(selected)) = semantic.calls().selection(call) else { panic!("generic header selected") };
            let header = selected.choices[0].header;
            let first = Leaf { frame: header.site.frame, ..leaf(11) };
            let second = Leaf { frame: header.site.frame, ..leaf(18) };
            let explicit = if unknown { NodeValue::Unknown } else { NodeValue::leaf(Leaf { frame: caller, ..leaf(20) }) };
            assert_eq!(semantic.values().bound_argument(first), Known::Yes(Some(explicit.clone())));
            assert_eq!(semantic.calls().header_result(header), if unknown { Known::Unknown } else { Known::Yes(explicit.clone()) });
            assert_eq!(semantic.calls().result(call), if unknown { Known::Unknown } else { Known::Yes(Some(explicit)) });
            let Known::Yes(Some(default)) = semantic.values().bound_argument(second) else { panic!("unsupplied formal uses its default") };
            assert!(matches!(default, NodeValue::Alternatives(ref arms) if arms.len() == 1 && arms[0].len() == 1 && arms[0][0].target == leaf(19).target));
        }
    }
}

#[test]
fn generic_inference_matches_raw_formals_before_defaults_and_isolates_callers() {
    let (world, tables) = generic_fixture();
    let question = QueryContext::default();
    let first_caller = question.bind(FrameId::ROOT, NodeRef::here(3), None, Some(vec![value(0)]));
    let second_caller = question.bind(FrameId::ROOT, NodeRef::here(3), None, Some(vec![value(1)]));
    let semantic = Semantic::new(&world, &tables, &question);
    for (call, actual, caller) in [(5, 10, first_caller), (15, 16, second_caller)] {
        let call = Leaf { frame: caller, ..leaf(call) };
        let Known::Yes(Some(selected)) = semantic.calls().selection(call) else { panic!("generic candidate retained") };
        let formal = Leaf { frame: selected.choices[0].header.site.frame, ..leaf(11) };
        assert_eq!(semantic.calls().inferred(formal),
            Known::Yes(Some(NodeValue::leaf(Leaf { frame: caller, ..leaf(actual) }))));
    }
}

#[test]
fn generic_contributions_combine_all_inputs_without_skipping_unknowns() {
    let (mut world, mut tables) = generic_fixture();
    let mut nodes = world.tree.nodes.clone();
    nodes.extend([7, 3, 1, 1].into_iter().map(|matcher| Node {
        matcher, start: 0, end: 0, children: Vec::new(),
    }));
    nodes[4].children.push(Edge { label: 10, node: 7 });
    nodes[7].matcher = 8;
    nodes[7].children.push(Edge { label: 61, node: 18 });
    nodes[18].children.push(Edge { label: 42, node: 19 });
    nodes[5].children.push(Edge { label: 15, node: 20 });
    nodes[15].children.push(Edge { label: 15, node: 21 });
    nodes[10].matcher = 9;
    world.tree = Tree::from_native(nodes, 5);
    world.names.declaration.insert(19, 14);
    tables.kinds.push(KindNames {
        unwraps: vec![crate::grammar::names::Unwrapping {
            value: Some(super::Value::Path(vec![99])),
            ..Default::default()
        }],
        ..KindNames::default()
    });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    for call in [5, 15] {
        let Known::Yes(Some(selected)) = semantic.calls().selection(leaf(call)) else { panic!("generic candidate retained") };
        let formal = Leaf { frame: selected.choices[0].header.site.frame, ..leaf(11) };
        let Known::Yes(Some(inferred)) = semantic.calls().inferred(formal) else { panic!("inferred input") };
        if call == 5 {
            assert_eq!(inferred, NodeValue::Unknown);
        } else {
            assert_eq!(semantic.values().could(&inferred, 1), Known::Yes(true));
            assert_eq!(semantic.values().could(&inferred, 2), Known::Yes(true));
        }
    }
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
    // Two overloads neither ruled out (a lone one would be resolved).
    let overload = tables.kinds[4].calls.signatures[0].clone();
    tables.kinds[4].calls.signatures.push(overload);
    let semantic = Semantic::new(&world, &tables, &question);
    let Known::Yes(Some(selected)) = semantic.calls().selection(leaf(5)) else { panic!("uncertain candidate selected") };
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
    let Known::Yes(Some(selected)) = semantic.calls().selection(leaf(5)) else { panic!("unproven candidate retained") };
    assert_eq!(selected.choices[0].header.signature, 0);
    assert!(!selected.uncertain, "the only candidate not ruled out is the resolved signature");

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
                item: Signature { parameters: vec![10], receiver: None, result: super::Value::Path(vec![21]) },
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

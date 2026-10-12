use super::*;
use crate::grammar::names::effects::{EventKind, Phase, Point, Step, Target};
use crate::grammar::names::question::{Budget, NodeRef};

fn entry(node: u32) -> Step {
    Step { node: NodeRef::here(node), point: Point::Enter }
}

fn field(node: u32) -> Step {
    Step { node: NodeRef::here(node), point: Point::Field(vec![7]) }
}

fn write(node: u32, binding: u32) -> Step {
    Step {
        node: NodeRef::here(node),
        point: Point::Event(Event {
            kind: EventKind::Write,
            site: NodeRef::here(node),
            target: Some(Target { value: NodeRef::here(binding), binding: Some(NodeRef::here(binding)) }),
            value: None,
            phase: Phase::Leave,
        }),
    }
}

fn graph(steps: Vec<Step>, edges: &[&[StepId]]) -> Execution {
    Execution::new(0, steps, edges.iter().map(|edges| edges.to_vec()).collect())
}

fn writes_binding(binding: u32) -> impl Fn(&Event) -> bool {
    move |event| event.kind == EventKind::Write
        && event.target.is_some_and(|target| target.binding == Some(NodeRef::here(binding)))
}

#[test]
fn branch_proof_does_not_leak_through_a_join_with_an_unguarded_path() {
    let flow = graph(
        vec![entry(0), field(0), entry(1), entry(2)],
        &[&[1, 3], &[2], &[3], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1], 2, writes_binding(10), &question), Known::Yes(true));
    assert_eq!(guarded(&flow, &[1], 3, writes_binding(10), &question), Known::Yes(false));
}

#[test]
fn every_normal_path_must_be_free_of_writes_to_the_tested_binding() {
    let flow = graph(
        vec![entry(0), field(0), write(1, 10), write(2, 11), entry(3)],
        &[&[1], &[2, 3], &[4], &[4], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1], 4, writes_binding(10), &question), Known::Yes(false));
    assert_eq!(guarded(&flow, &[1], 4, writes_binding(12), &question), Known::Yes(true));
}

#[test]
fn repeated_guard_tests_refresh_proofs_without_hiding_writes_in_the_current_iteration() {
    let flow = graph(
        vec![entry(0), field(0), entry(1), write(2, 10), entry(3), entry(4)],
        &[&[1], &[2, 5], &[3], &[4], &[1], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1], 2, writes_binding(10), &question), Known::Yes(true));
    assert_eq!(guarded(&flow, &[1], 4, writes_binding(10), &question), Known::Yes(false));
    assert_eq!(guarded(&flow, &[1], 5, writes_binding(10), &question), Known::Yes(true));
}

#[test]
fn duplicated_branch_entries_can_jointly_prove_the_same_guard() {
    let flow = graph(
        vec![entry(0), field(0), field(0), entry(1)],
        &[&[1, 2], &[3], &[3], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1, 2], 3, writes_binding(10), &question), Known::Yes(true));
    assert_eq!(guarded(&flow, &[1], 3, writes_binding(10), &question), Known::Yes(false));
}

#[test]
fn unreachable_guard_entries_cannot_invalidate_a_reachable_proof() {
    let flow = graph(
        vec![entry(0), field(0), field(0), write(1, 10), entry(2)],
        &[&[1], &[4], &[3], &[4], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1, 2], 4, writes_binding(10), &question), Known::Yes(true));
}

#[test]
fn missing_paths_and_exhaustion_do_not_create_a_guard_proof() {
    let flow = graph(
        vec![entry(0), field(0), entry(1), entry(2)],
        &[&[1], &[2], &[], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1], 3, writes_binding(10), &question), Known::Yes(false));
    let mut exhausted = QueryContext::default();
    exhausted.budget = Budget::new(0);
    assert_eq!(guarded(&flow, &[1], 2, writes_binding(10), &exhausted), Known::Unknown);
    assert_eq!(guarded(&flow, &[1], 99, writes_binding(10), &question), Known::Unknown);
}

#[test]
fn writes_invalidate_only_the_same_runtime_root_and_descendant_member_paths() {
    use crate::grammar::names::question::{FrameId, Target as ValueTarget};
    let root = Leaf { target: ValueTarget::Node(NodeRef::here(10)), frame: FrameId::ROOT };
    let base = Place { root, path: vec![] };
    let member = Place { root, path: vec![Key::Text("payload".into())] };
    let child = Place { root, path: vec![Key::Text("payload".into()), Key::Text("kind".into())] };
    let sibling = Place { root, path: vec![Key::Text("other".into())] };
    let separate = Place {
        root: Leaf { target: ValueTarget::Node(NodeRef::here(11)), ..root },
        ..child.clone()
    };
    assert!(base.invalidates(&child));
    assert!(member.invalidates(&child));
    assert!(child.invalidates(&child));
    assert!(!child.invalidates(&member));
    assert!(!sibling.invalidates(&child));
    assert!(!separate.invalidates(&child));
}

mod places {
    use super::super::*;
    use crate::config::ast::Config;
    use crate::grammar::{Edge, Grammar, Node, Tree};
    use crate::grammar::names::{KindNames, Names, Resolved, Unwrapping, Value};
    use crate::grammar::names::question::{FrameId, NodeRef, ScopeRef, Target, World};

    struct TestWorld {
        grammar: Grammar,
        tree: Tree,
        names: Resolved,
    }

    fn leaf(node: u32) -> Leaf {
        Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT }
    }

    impl TestWorld {
        fn new() -> Self {
            let mut nodes = vec![Node { matcher: 0, start: 0, end: 0, children: vec![] }; 15];
            nodes[0].children = [1, 2, 7, 8, 9, 10].map(|node| Edge { label: 0, node }).to_vec();
            for (node, matcher, object) in [(5, 1, 3), (6, 1, 4), (7, 2, 5), (8, 3, 6)] {
                nodes[node].matcher = matcher;
                nodes[node].children.push(Edge { label: 1, node: object });
            }
            for (node, matcher) in [(11, 4), (12, 5), (13, 6), (14, 7)] {
                nodes[node].matcher = matcher;
            }
            nodes[3].children.extend([Edge { label: 2, node: 11 }, Edge { label: 3, node: 14 }]);
            let mut names = Resolved::default();
            names.declaration.extend([(3, 1), (4, 2)]);
            names.merged.insert(10, 1);
            names.declared.extend([1, 2, 10].map(|node| (node, (0, node, 1, false))));
            Self {
                grammar: Grammar::build(&Config::default(), &mut Vec::new()),
                tree: Tree::from_native(nodes, 0),
                names,
            }
        }

        fn branches() -> Self {
            let mut world = Self::new();
            let mut nodes = world.tree.nodes.clone();
            nodes[0].matcher = 9;
            nodes[0].children = [1, 2, 15, 18].map(|node| Edge { label: 0, node }).to_vec();
            for node in &mut nodes[5..11] { node.children.clear(); }
            nodes.push(Node {
                matcher: 8, start: 0, end: 0,
                children: vec![Edge { label: 4, node: 3 }, Edge { label: 5, node: 16 }, Edge { label: 6, node: 17 }],
            });
            nodes.extend([16, 17, 18].map(|_| Node { matcher: 0, start: 0, end: 0, children: vec![] }));
            world.names.declaration.extend([16, 17, 18].map(|node| (node, 1)));
            world.tree = Tree::from_native(nodes, 0);
            world
        }
    }

    impl World for TestWorld {
        fn grammar(&self) -> &Grammar { &self.grammar }
        fn tree(&self, file: u32) -> Option<&Tree> { (file == 0).then_some(&self.tree) }
        fn text(&self, file: u32) -> Option<&str> { (file == 0).then_some("") }
        fn names(&self, file: u32) -> Option<&Resolved> { (file == 0).then_some(&self.names) }
        fn index(&self, _: u32) -> Option<&crate::grammar::names::ScopeIndex> { None }
        fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> {
            std::borrow::Cow::Borrowed(text)
        }
        fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Yes(None) }
        fn condition(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<bool> { Known::Unknown }
        fn operand(&self, _: Leaf, operand: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
            if operand == 0 { Known::Yes(crate::expr::Value::from("ready")) } else { Known::Unknown }
        }
        fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> { Known::Unknown }
    }

    fn tables() -> Names {
        let mut tables = Names { kinds: vec![KindNames::default(); 8], ..Names::default() };
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["truthy".into()],
            traits: ["nullable", "string", "falsy", "literal"].map(str::to_owned).to_vec(),
            ..Default::default()
        });
        tables.exclusive.push((1 << 0) | (1 << 1));
        for traits in [1 << 0, 1 << 2] {
            tables.kinds[0].unwraps.push(Unwrapping {
                effect: Some(0), value: Some(Value::Excludes { traits, from: None }), ..Default::default()
            });
        }
        tables.kinds[4].fits.push((1 << 1, 0, None));
        tables.kinds[5].fits.push(((1 << 0) | (1 << 2), 0, None));
        tables.kinds[6].fits.push(((1 << 1) | (1 << 2), 0, None));
        tables.kinds[7].fits.push(((1 << 1) | (1 << 3), 0, None));
        tables.kinds[7].calls.identities.push(crate::grammar::names::calls::Conditional { item: 0, when: None });
        for (kind, value) in [
            (1, Value::From(Box::new(Value::Path(vec![1])), Box::new(Value::Text("payload".into())), 0)),
            (2, Value::From(Box::new(Value::Path(vec![1])), Box::new(Value::Text("kind".into())), 0)),
            (3, Value::Path(vec![1])),
        ] {
            tables.kinds[kind].unwraps.push(Unwrapping {
                value: Some(value), ..Default::default()
            });
        }
        tables
    }

    #[test]
    fn binding_identity_keeps_shadowed_names_separate_and_merged_declarations_together() {
        let world = TestWorld::new();
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let narrowing = semantic.narrowing();
        assert_eq!(narrowing.place(leaf(3)), Known::Yes(Place { root: leaf(1), path: vec![] }));
        assert_eq!(narrowing.place(leaf(4)), Known::Yes(Place { root: leaf(2), path: vec![] }));
        assert_eq!(narrowing.place(leaf(10)), Known::Yes(Place { root: leaf(1), path: vec![] }));
        assert_eq!(narrowing.place(leaf(9)), Known::Unknown);
    }

    #[test]
    #[ignore = "RED until Values.key is implemented"]
    fn declared_member_recipes_keep_runtime_roots_and_paths_through_wrappers() {
        let world = TestWorld::new();
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let narrowing = semantic.narrowing();
        let payload = Key::Text("payload".into());
        assert_eq!(narrowing.place(leaf(5)), Known::Yes(Place { root: leaf(1), path: vec![payload.clone()] }));
        assert_eq!(narrowing.place(leaf(7)), Known::Yes(Place {
            root: leaf(1), path: vec![payload.clone(), Key::Text("kind".into())],
        }));
        assert_eq!(narrowing.place(leaf(8)), Known::Yes(Place { root: leaf(2), path: vec![payload] }));
    }

    fn candidates() -> NodeValue {
        NodeValue::Alternatives([11, 12, 13].map(|node| vec![leaf(node)]).to_vec())
    }

    fn branch_tables() -> Names {
        use crate::grammar::names::effects::{Conditional, Flow};
        let mut tables = tables();
        tables.kinds.resize_with(10, KindNames::default);
        tables.kinds[0].unwraps.clear();
        let span = crate::config::source::Span::new(crate::config::source::FileId(0), 0, 1);
        tables.kinds[0].execution.runs.push(Conditional {
            item: Flow::Nothing, when: None, item_id: 1, span,
        });
        tables.kinds[8].execution.runs.push(Conditional {
            item: Flow::Then(vec![Flow::Field(vec![4]), Flow::Or(vec![Flow::Field(vec![5]), Flow::Field(vec![6])])]),
            when: None, item_id: 2, span,
        });
        let proof = Value::Excludes { traits: 1 << 0, from: Some(Box::new(Value::Path(vec![4]))) };
        tables.kinds[8].propagates.extend([
            (vec![5], proof.clone(), None),
            (vec![6], Value::Not(Box::new(proof)), None),
        ]);
        tables
    }

    #[test]
    fn only_a_definite_early_exit_carries_the_surviving_branch_proof_after_the_join() {
        for possible in [false, true] {
            let mut world = TestWorld::branches();
            let mut nodes = world.tree.nodes.clone();
            nodes[17].matcher = 10;
            world.tree = Tree::from_native(nodes, 0);
            let mut tables = branch_tables();
            tables.kinds.resize_with(11, KindNames::default);
            tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
                effects: vec!["truthy".into(), "returned".into()],
                traits: tables.vocabulary.traits.clone(),
                ..Default::default()
            });
            tables.controls = crate::grammar::names::effects::compile::controls(&tables.vocabulary);
            tables.kinds[10].unwraps.push(Unwrapping {
                effect: Some(1), possible, ..Default::default()
            });
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            assert!(semantic.effects().execution(leaf(0)).known().is_some());
            let after = semantic.narrowing().refine(leaf(18), candidates());
            assert_eq!(semantic.values().could(&after, 1 << 0), Known::Yes(possible));
            assert_eq!(semantic.values().could(&after, 1 << 1), Known::Yes(true));
        }
    }

    #[test]
    fn exhausted_runtime_refinement_returns_unknown_instead_of_a_known_base() {
        let world = TestWorld::branches();
        let tables = branch_tables();
        let mut question = QueryContext::default();
        question.budget = crate::grammar::names::question::Budget::new(0);
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.narrowing().refine(leaf(16), candidates()), NodeValue::Unknown);
    }

    #[test]
    fn runtime_refinement_uses_actual_branch_entries_and_does_not_leak_across_the_join() {
        let world = TestWorld::branches();
        let tables = branch_tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert!(semantic.effects().execution(leaf(0)).known().is_some());
        let consequent = semantic.narrowing().refine(leaf(16), candidates());
        let alternate = semantic.narrowing().refine(leaf(17), candidates());
        let after = semantic.narrowing().refine(leaf(18), candidates());
        assert_eq!(semantic.values().could(&consequent, 1 << 0), Known::Yes(false));
        assert_eq!(semantic.values().always(&alternate, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().could(&after, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().could(&after, 1 << 1), Known::Yes(true));
    }

    #[test]
    fn excluded_traits_mean_any_listed_trait_and_negation_keeps_the_other_side() {
        let world = TestWorld::new();
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let subject = Place { root: leaf(1), path: vec![] };
        let proof = Value::Excludes { traits: (1 << 0) | (1 << 2), from: None };
        let positive = semantic.narrowing().apply(leaf(3), &proof, &subject, candidates());
        let negative = semantic.narrowing().apply(leaf(3), &Value::Not(Box::new(proof)), &subject, candidates());
        assert_eq!(semantic.values().could(&positive, 1 << 0), Known::Yes(false));
        assert_eq!(semantic.values().could(&positive, 1 << 2), Known::Yes(false));
        assert_eq!(semantic.values().always(&positive, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().could(&negative, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().always(&negative, 1 << 2), Known::Yes(true));
    }

    #[test]
    fn negation_pushes_through_alternative_proofs_without_turning_opaque_tests_into_proof() {
        let world = TestWorld::new();
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let subject = Place { root: leaf(1), path: vec![] };
        let proof = Value::Union(vec![
            Value::Excludes { traits: 1 << 0, from: None },
            Value::Excludes { traits: 1 << 2, from: None },
        ]);
        let positive = semantic.narrowing().apply(leaf(3), &proof, &subject, candidates());
        let negative = semantic.narrowing().apply(leaf(3), &Value::Not(Box::new(proof)), &subject, candidates());
        assert_eq!(semantic.values().could(&positive, 1 << 0), Known::Yes(false));
        assert_eq!(semantic.values().could(&positive, 1 << 2), Known::Yes(true));
        assert_eq!(semantic.values().always(&negative, (1 << 0) | (1 << 2)), Known::Yes(true));
        assert_eq!(semantic.values().could(&negative, 1 << 1), Known::Yes(false));
        let opaque = Value::Not(Box::new(Value::Path(vec![])));
        let unresolved = semantic.narrowing().apply(leaf(9), &opaque, &subject, candidates());
        assert_eq!(semantic.values().could(&unresolved, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().could(&unresolved, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().could(&unresolved, 1 << 2), Known::Yes(true));
        assert_eq!(semantic.narrowing().apply(leaf(3), &opaque, &subject, NodeValue::Unknown), NodeValue::Unknown);
    }

    #[test]
    fn truthy_effects_compose_as_one_proof_before_the_test_is_negated() {
        let world = TestWorld::new();
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let subject = Place { root: leaf(1), path: vec![] };
        let proof = Value::Path(vec![]);
        let positive = semantic.narrowing().apply(leaf(3), &proof, &subject, candidates());
        let negative = semantic.narrowing().apply(leaf(3), &Value::Not(Box::new(proof)), &subject, candidates());
        assert_eq!(semantic.values().always(&positive, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().could(&positive, 1 << 2), Known::Yes(false));
        assert_eq!(semantic.values().could(&negative, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().always(&negative, 1 << 2), Known::Yes(true));
    }

    #[test]
    fn includes_removes_disjoint_categories_and_negation_removes_only_fully_included_values() {
        let world = TestWorld::new();
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let subject = Place { root: leaf(1), path: vec![] };
        let proof = Value::Includes { value: Box::new(Value::Path(vec![2])), from: None };
        let positive = semantic.narrowing().apply(leaf(3), &proof, &subject, candidates());
        let negative = semantic.narrowing().apply(leaf(3), &Value::Not(Box::new(proof)), &subject, candidates());
        assert_eq!(semantic.values().always(&positive, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().could(&positive, 1 << 0), Known::Yes(false));
        assert_eq!(semantic.values().always(&negative, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().could(&negative, 1 << 1), Known::Yes(false));
    }

    #[test]
    fn literal_overlap_refines_a_broad_positive_type_but_does_not_exclude_it_on_the_negative_side() {
        let world = TestWorld::new();
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let subject = Place { root: leaf(1), path: vec![] };
        let before = NodeValue::Alternatives(vec![vec![leaf(11)], vec![leaf(12)]]);
        let proof = Value::Includes { value: Box::new(Value::Path(vec![3])), from: None };
        let positive = semantic.narrowing().apply(leaf(3), &proof, &subject, before.clone());
        let negative_proof = Value::Not(Box::new(proof.clone()));
        let negative = semantic.narrowing().apply(leaf(3), &negative_proof, &subject, before);
        assert_eq!(semantic.values().always(&positive, 1 << 3), Known::Yes(true));
        assert_eq!(semantic.values().could(&positive, 1 << 0), Known::Yes(false));
        assert_eq!(semantic.values().could(&negative, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().could(&negative, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().always(&negative, 1 << 3), Known::Yes(false));
        let unknown_positive = semantic.narrowing().apply(leaf(3), &proof, &subject, NodeValue::Unknown);
        assert_eq!(semantic.values().always(&unknown_positive, 1 << 3), Known::Yes(true));
        assert_eq!(semantic.narrowing().apply(leaf(3), &negative_proof, &subject, NodeValue::Unknown), NodeValue::Unknown);
    }
}

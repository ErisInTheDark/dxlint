use super::*;
use crate::grammar::names::effects::{EventKind, Phase, Point, Step, Target};
use crate::grammar::names::question::{Budget, NodeRef};

fn entry(node: u32) -> Step {
    Step { node: NodeRef::here(node), point: Point::Enter }
}

fn field(node: u32) -> Step {
    Step { node: NodeRef::here(node), point: Point::Field(Box::new([7])) }
}

fn write(node: u32, binding: u32) -> Step {
    Step {
        node: NodeRef::here(node),
        point: Point::Event(Box::new(Event {
            kind: EventKind::Write,
            site: NodeRef::here(node),
            target: Some(Target { value: NodeRef::here(binding), binding: Some(NodeRef::here(binding)) }),
            value: None,
            written: None,
            phase: Phase::Leave,
        })),
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

    #[test]
    fn atomic_guards_track_writes_to_the_tested_place_not_the_queried_root() {
        let world = TestWorld::new();
        let tables = tables();
        for (binding, valid) in [(1, false), (5, false), (6, true)] {
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            let narrowing = semantic.narrowing();
            let Known::Yes(tested) = narrowing.place(leaf(7)) else { panic!("nested member place is known") };
            let flow = super::graph(
                vec![super::entry(0), super::field(0), super::write(8, binding), super::entry(3)],
                &[&[1], &[2], &[3], &[]],
            );
            let region = Region { execution: &flow, entry: leaf(0), entries: &[1], uses: &[3], captured: false };
            assert_eq!(region.valid(&narrowing, &tested), Known::Yes(valid));
            let Known::Yes(root) = narrowing.place(leaf(3)) else { panic!("binding place is known") };
            assert_eq!(region.valid(&narrowing, &root), Known::Yes(binding != 1));
            let captured = Region { captured: true, ..region };
            assert_eq!(captured.valid(&narrowing, &tested), Known::Yes(false));
        }
    }

    #[test]
    fn mixed_member_key_forms_invalidate_guards_without_guessing_a_conversion() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[1].unwraps[0].value = Some(Value::From(
            Box::new(Value::Path(vec![1])), Box::new(Value::Ordinal(0)), 0,
        ));
        for (root, key, valid) in [
            (leaf(1), Key::Text("0".into()), false),
            (leaf(1), Key::Ordinal(1), true),
            (leaf(2), Key::Text("0".into()), true),
        ] {
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            let flow = super::graph(
                vec![super::entry(0), super::field(0), super::write(8, 5), super::entry(3)],
                &[&[1], &[2], &[3], &[]],
            );
            let region = Region { execution: &flow, entry: leaf(0), entries: &[1], uses: &[3], captured: false };
            assert_eq!(region.valid(&semantic.narrowing(), &Place { root, path: vec![key] }), Known::Yes(valid));
        }
    }

    #[test]
    fn assignment_narrowing_requires_valued_last_writes_on_every_reaching_route() {
        let world = TestWorld::new();
        let tables = tables();
        let mut valued = super::write(8, 1);
        let Point::Event(event) = &mut valued.point else { unreachable!() };
        event.written = Some(crate::grammar::names::effects::Written {
            owner: leaf(3), recipe: std::sync::Arc::new(Value::Path(vec![3])),
        });
        for (flow, narrowed) in [
            (super::graph(vec![super::entry(0), valued.clone(), super::entry(3)], &[&[1], &[2], &[]]), true),
            (super::graph(vec![super::entry(0), valued.clone(), super::entry(3)], &[&[1, 2], &[2], &[]]), false),
            (super::graph(vec![super::entry(0), valued.clone(), super::write(9, 1), super::entry(3)], &[&[1], &[2], &[3], &[]]), false),
        ] {
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            let narrowing = semantic.narrowing();
            let Known::Yes(subject) = narrowing.place(leaf(3)) else { panic!("binding place is known") };
            let base = NodeValue::Alternatives(vec![vec![leaf(14)], vec![leaf(12)]]);
            let use_site = flow.steps().last().unwrap().0;
            let result = narrowing.assigned(&subject, &flow, leaf(0), &[use_site], base.clone());
            assert_eq!(result, if narrowed { NodeValue::leaf(leaf(14)) } else { base });
            // A lone declared alternative is what every write picks.
            let lone = NodeValue::leaf(leaf(12));
            assert_eq!(narrowing.assigned(&subject, &flow, leaf(0), &[use_site], lone.clone()), lone);
        }
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let narrowing = semantic.narrowing();
        let Known::Yes(member) = narrowing.place(leaf(7)) else { panic!("member place is known") };
        let flow = super::graph(vec![super::entry(0), valued, super::entry(7)], &[&[1], &[2], &[]]);
        let base = NodeValue::Alternatives(vec![vec![leaf(14)], vec![leaf(12)]]);
        assert_eq!(narrowing.assigned(&member, &flow, leaf(0), &[2], base.clone()), base);
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
    fn anonymous_capture_wrappers_do_not_hide_binding_places_or_guard_recipes() {
        use crate::grammar::parse::TEXT;
        use crate::pattern::compile::NO_LABEL;
        let mut world = TestWorld::new();
        let mut nodes = world.tree.nodes.clone();
        for target in [3, 15] {
            nodes.push(Node {
                matcher: TEXT, start: 0, end: 0,
                children: vec![Edge { label: NO_LABEL, node: target }],
            });
        }
        world.tree = Tree::from_native(nodes, 0);
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let narrowing = semantic.narrowing();
        let subject = Place { root: leaf(1), path: vec![] };
        assert_eq!(narrowing.place(leaf(16)), Known::Yes(subject.clone()));
        let positive = narrowing.apply(leaf(16), &Value::Path(vec![]), &subject, candidates());
        assert_eq!(semantic.values().always(&positive, 1 << 1), Known::Yes(true));
        let negative = narrowing.apply(leaf(16), &Value::Not(Box::new(Value::Path(vec![]))), &subject, candidates());
        assert_eq!(semantic.values().could(&negative, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().could(&negative, 1 << 2), Known::Yes(true));
    }

    #[test]
    fn directional_guard_atoms_do_not_weaken_negation_but_unrelated_or_branches_do() {
        let world = TestWorld::new();
        let mut tables = tables();
        let forward = Value::Includes {
            value: Box::new(Value::Path(vec![2])), from: None,
        };
        let reverse = Value::Includes {
            value: Box::new(Value::Path(vec![])), from: Some(Box::new(Value::Path(vec![2]))),
        };
        tables.kinds[0].unwraps = [forward.clone(), reverse.clone()].into_iter().map(|value| Unwrapping {
            effect: Some(0), value: Some(value), ..Default::default()
        }).collect();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let narrowing = semantic.narrowing();
        let subject = Place { root: leaf(1), path: vec![] };
        let base = NodeValue::Alternatives(vec![vec![leaf(11)], vec![leaf(12)]]);
        let positive = narrowing.apply(leaf(3), &Value::Path(vec![]), &subject, base.clone());
        let negative = narrowing.apply(leaf(3), &Value::Not(Box::new(Value::Path(vec![]))), &subject, base.clone());
        assert_eq!(semantic.values().always(&positive, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().always(&negative, 1 << 0), Known::Yes(true));
        let unrelated_or = Value::Union(vec![forward, reverse]);
        let unrelated = narrowing.apply(leaf(3), &unrelated_or, &subject, base);
        assert_eq!(semantic.values().could(&unrelated, 1 << 0), Known::Yes(true));
        assert_eq!(semantic.values().could(&unrelated, 1 << 1), Known::Yes(true));
    }

    #[test]
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

    fn predicate_call() -> (TestWorld, Names) {
        use crate::grammar::names::calls::{Conditional, Invoke, Signature};
        let mut world = TestWorld::new();
        let mut nodes = world.tree.nodes.clone();
        nodes.extend([8, 9, 10, 0, 0, 11].map(|matcher| Node { matcher, start: 0, end: 0, children: vec![] }));
        nodes[15].children = vec![Edge { label: 8, node: 16 }, Edge { label: 9, node: 17 }];
        nodes[16].children.push(Edge { label: 10, node: 18 });
        nodes[17].children = vec![Edge { label: 11, node: 19 }, Edge { label: 12, node: 11 }];
        nodes[18].children.push(Edge { label: 2, node: 11 });
        nodes[20].children = vec![Edge { label: 13, node: 15 }, Edge { label: 14, node: 3 }];
        world.tree = Tree::from_native(nodes, 20);
        world.names.declaration.insert(19, 18);
        world.names.declared.insert(18, (0, 15, 1, false));
        let mut tables = tables();
        tables.kinds.resize_with(12, KindNames::default);
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["truthy".into()],
            traits: ["nullable", "string", "falsy", "literal", "callable"].map(str::to_owned).to_vec(),
            ..Default::default()
        });
        tables.effect_roles = crate::grammar::names::effects::compile::effect_roles(&tables.vocabulary);
        tables.kinds[0].unwraps.push(Unwrapping { value: Some(Value::Path(vec![2])), ..Default::default() });
        tables.kinds[8].fits.push((1 << 4, 0, None));
        tables.kinds[8].calls.signatures.push(Conditional {
            item: Signature { parameters: vec![8], receiver: None, result: Value::Path(vec![9]) }, when: None,
        });
        tables.kinds[9].targets.push((vec![vec![10]], None));
        tables.kinds[9].unwraps.push(Unwrapping { value: Some(Value::Path(vec![10, 2])), ..Default::default() });
        tables.kinds[10].unwraps.push(Unwrapping {
            effect: Some(0),
            value: Some(Value::Includes { value: Box::new(Value::Path(vec![12])), from: Some(Box::new(Value::Path(vec![11]))) }),
            ..Default::default()
        });
        tables.kinds[11].calls.invokes.push(Conditional {
            item: Invoke { callee: Value::Path(vec![13]), arguments: vec![14], construct: false, receiver: None }, when: None,
        });
        (world, tables)
    }

    #[test]
    fn selected_predicate_parameters_resolve_to_the_caller_place_not_type_argument_slots() {
        let (world, tables) = predicate_call();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let Known::Yes(Some(selection)) = semantic.calls().selection(leaf(20)) else { panic!("selected predicate header") };
        assert!(!selection.uncertain);
        let frame = selection.choices[0].header.site.frame;
        assert_eq!(semantic.calls().argument_sites(Leaf { frame, ..leaf(18) }), Known::Yes(Some(vec![leaf(3)])));
        assert_eq!(semantic.narrowing().place(Leaf { frame, ..leaf(19) }), Known::Yes(Place { root: leaf(1), path: vec![] }));
    }

    #[test]
    fn selected_signature_truthy_proofs_refine_both_sides_of_a_predicate_call() {
        let (world, tables) = predicate_call();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let Known::Yes(Some(selection)) = semantic.calls().selection(leaf(20)) else { panic!("selected predicate header") };
        assert!(!selection.uncertain);
        let subject = Place { root: leaf(1), path: vec![] };
        let proof = Value::Path(vec![]);
        let positive = semantic.narrowing().apply(leaf(20), &proof, &subject, candidates());
        let negative = semantic.narrowing().apply(leaf(20), &Value::Not(Box::new(proof)), &subject, candidates());
        assert_eq!(semantic.values().always(&positive, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().could(&positive, 1 << 0), Known::Yes(false));
        assert_eq!(semantic.values().always(&negative, 1 << 0), Known::Yes(true));
    }

    #[test]
    fn possible_predicate_signatures_remain_alternatives_on_both_sides() {
        let (mut world, mut tables) = predicate_call();
        let mut nodes = world.tree.nodes.clone();
        nodes.extend([8, 9, 10, 0, 0].map(|matcher| Node { matcher, start: 0, end: 0, children: vec![] }));
        nodes[21].children = vec![Edge { label: 8, node: 22 }, Edge { label: 9, node: 23 }];
        nodes[22].children.push(Edge { label: 10, node: 24 });
        nodes[23].children = vec![Edge { label: 11, node: 25 }, Edge { label: 12, node: 12 }];
        nodes[24].children.push(Edge { label: 2, node: 11 });
        nodes[20].children.push(Edge { label: 15, node: 21 });
        world.tree = Tree::from_native(nodes, 20);
        world.names.declaration.insert(25, 24);
        world.names.declared.insert(24, (0, 21, 1, false));
        tables.kinds[11].calls.invokes[0].item.callee = Value::Union(vec![Value::Path(vec![13]), Value::Path(vec![15])]);
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let Known::Yes(Some(selection)) = semantic.calls().selection(leaf(20)) else { panic!("both predicate alternatives selected") };
        assert!(!selection.uncertain);
        assert_eq!(selection.choices.len(), 2);
        let subject = Place { root: leaf(1), path: vec![] };
        for proof in [Value::Path(vec![]), Value::Not(Box::new(Value::Path(vec![])))] {
            let refined = semantic.narrowing().apply(leaf(20), &proof, &subject, candidates());
            assert_eq!(semantic.values().could(&refined, 1 << 0), Known::Yes(true));
            assert_eq!(semantic.values().could(&refined, 1 << 1), Known::Yes(true));
        }
    }

    #[test]
    fn uncertain_selected_signatures_do_not_prove_positive_or_negative_guards() {
        let (world, mut tables) = predicate_call();
        tables.kinds[9].unwraps.clear();
        tables.kinds[9].fits.push((1 << 1, 0, Some(0)));
        // Two overloads neither ruled out: a lone one would be the
        // resolved signature, and its guard would hold.
        let overload = tables.kinds[8].calls.signatures[0].clone();
        tables.kinds[8].calls.signatures.push(overload);
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let Known::Yes(Some(selection)) = semantic.calls().selection(leaf(20)) else { panic!("uncertain signature retained") };
        assert!(selection.uncertain);
        let subject = Place { root: leaf(1), path: vec![] };
        for proof in [Value::Path(vec![]), Value::Not(Box::new(Value::Path(vec![])))] {
            let refined = semantic.narrowing().apply(leaf(20), &proof, &subject, candidates());
            assert_eq!(semantic.values().could(&refined, 1 << 0), Known::Yes(true));
            assert_eq!(semantic.values().could(&refined, 1 << 1), Known::Yes(true));
        }
    }

    #[test]
    fn only_binding_replacements_block_guards_in_a_separate_body() {
        use crate::grammar::names::effects::{Conditional, Creation};
        // 0: the binding itself; 1: a property of it; 2: another binding;
        // 3: a name nothing declares; 4: a target that is no name at all.
        for (write, captured_guard) in [(None, true), (Some(0), false), (Some(1), true), (Some(2), true), (Some(3), false), (Some(4), true)] {
            let mut world = TestWorld::branches();
            let mut nodes = world.tree.nodes.clone();
            nodes[16].matcher = 10;
            nodes[16].children.push(Edge { label: 7, node: 19 });
            nodes.push(Node { matcher: 0, start: 0, end: 0, children: vec![] });
            world.tree = Tree::from_native(nodes, 0);
            world.names.declaration.insert(19, 1);
            let mut tables = branch_tables();
            tables.kinds.resize_with(11, KindNames::default);
            tables.kinds[10].execution.entries.push(Conditional {
                item: vec![7], when: None, item_id: 3,
                span: crate::config::source::Span::new(crate::config::source::FileId(0), 0, 1),
            });
            if let Some(write) = write {
                let mut nodes = world.tree.nodes.clone();
                nodes[0].children.push(Edge { label: 0, node: 20 });
                nodes.push(Node { matcher: 11, start: 0, end: 0, children: vec![Edge { label: 8, node: 21 }] });
                nodes.push(Node { matcher: if write == 1 { 1 } else { 0 }, start: 0, end: 0,
                    children: if write == 1 { vec![Edge { label: 1, node: 22 }] } else { vec![] } });
                nodes.push(Node { matcher: 0, start: 0, end: 0, children: vec![] });
                match write {
                    0 => { world.names.declaration.insert(21, 1); }
                    1 => { world.names.declaration.insert(22, 1); }
                    2 => { world.names.declaration.insert(21, 2); }
                    3 => { world.names.keys.insert(21, None); }
                    _ => {}
                }
                world.tree = Tree::from_native(nodes, 0);
                tables.kinds.resize_with(12, KindNames::default);
                tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
                    effects: vec!["truthy".into(), "assigned".into()],
                    traits: tables.vocabulary.traits.clone(),
                    ..Default::default()
                });
                tables.effect_roles = crate::grammar::names::effects::compile::effect_roles(&tables.vocabulary);
                tables.kinds[11].effects.push(crate::grammar::names::Effect {
                    item_id: Some(4), effect: 1, path: vec![8], when: None, value: None, possible: false,
                });
            }
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            assert_eq!(semantic.effects().entry_for(leaf(19)), Known::Yes(leaf(19)));
            assert_eq!(semantic.effects().enclosing(leaf(19)), Known::Yes(Some(Creation { entry: leaf(0), site: leaf(16) })));
            let captured = semantic.narrowing().refine(leaf(19), candidates());
            assert_eq!(semantic.values().could(&captured, 1 << 0), Known::Yes(!captured_guard));
            assert_eq!(semantic.values().always(&captured, 1 << 1), Known::Yes(captured_guard));
        }
    }

    #[test]
    fn runtime_branch_proofs_read_truthy_views_without_turning_them_into_events() {
        let world = TestWorld::branches();
        let truthy = tables().kinds[0].unwraps.clone();
        let mut tables = branch_tables();
        tables.kinds[0].unwraps = truthy;
        tables.effect_roles = crate::grammar::names::effects::compile::effect_roles(&tables.vocabulary);
        tables.kinds[8].propagates = vec![
            (vec![5], Value::Path(vec![4]), None),
            (vec![6], Value::Not(Box::new(Value::Path(vec![4]))), None),
        ];
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert!(semantic.effects().execution(leaf(0)).known().is_some());
        let consequent = semantic.narrowing().refine(leaf(16), candidates());
        let alternate = semantic.narrowing().refine(leaf(17), candidates());
        assert_eq!(semantic.values().always(&consequent, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().could(&consequent, 1 << 2), Known::Yes(false));
        assert_eq!(semantic.values().always(&alternate, 1 << 2), Known::Yes(true));
        assert_eq!(semantic.values().could(&alternate, 1 << 0), Known::Yes(true));
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
            tables.effect_roles = crate::grammar::names::effects::compile::effect_roles(&tables.vocabulary);
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
    fn positive_inclusion_refines_open_values_without_losing_existing_constraints() {
        let world = TestWorld::new();
        let mut tables = tables();
        std::sync::Arc::get_mut(&mut tables.vocabulary).unwrap().traits.push("marker".into());
        tables.kinds[5].fits = vec![(1 << 4, 0, None)];
        tables.kinds[5].open_fits.push((0, None));
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let subject = Place { root: leaf(1), path: vec![] };
        let proof = Value::Includes { value: Box::new(Value::Path(vec![2])), from: None };
        let before = NodeValue::leaf(leaf(12));
        let positive = semantic.narrowing().apply(leaf(3), &proof, &subject, before.clone());
        assert_eq!(semantic.values().always(&positive, 1 << 1), Known::Yes(true));
        assert_eq!(semantic.values().always(&positive, 1 << 4), Known::Yes(true));
        let negative = semantic.narrowing().apply(leaf(3), &Value::Not(Box::new(proof)), &subject, before);
        assert_eq!(semantic.values().always(&negative, 1 << 4), Known::Yes(true));
        assert_eq!(semantic.values().could(&negative, 1 << 1), Known::Unknown);
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

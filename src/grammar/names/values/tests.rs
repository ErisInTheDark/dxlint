use super::*;
use crate::config::ast::Config;
use crate::grammar::{Edge, Grammar, Node, Tree};
use super::super::{KindNames, Names, Resolved, ScopeInfo};
use super::super::question::{Budget, FrameId, QueryContext, ScopeRef, World};
use std::cell::RefCell;

struct TestWorld {
    grammar: Grammar,
    tree: Tree,
    names: Resolved,
    attributes: Vec<Known<crate::expr::Value<'static>>>,
    conditions: RefCell<Vec<Leaf>>,
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
            attributes: Vec::new(),
            conditions: RefCell::default(),
        }
    }
}

impl World for TestWorld {
    fn grammar(&self) -> &Grammar { &self.grammar }
    fn tree(&self, file: u32) -> Option<&Tree> { (file == 0).then_some(&self.tree) }
    fn text(&self, file: u32) -> Option<&str> { (file == 0).then_some("") }
    fn names(&self, file: u32) -> Option<&Resolved> { (file == 0).then_some(&self.names) }
    fn index(&self, file: u32) -> Option<&super::super::ScopeIndex> {
        (file == 0).then(|| self.names.index(&self.tree, "", &|key| key.into()))
    }
    fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> {
        std::borrow::Cow::Borrowed(text)
    }
    fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Yes(None) }
    fn condition(&self, at: Leaf, condition: u32, semantic: &Semantic<'_>) -> Known<bool> {
        self.conditions.borrow_mut().push(at);
        match condition {
            1 => semantic.values().fits(at, 0),
            2 => Known::Yes(matches!(at.target, Target::Scope(_))),
            3 => Known::Yes(false),
            _ => Known::Unknown,
        }
    }
    fn attribute(&self, _: Leaf, attr: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
        self.attributes.get(attr as usize).cloned().unwrap_or(Known::Yes(crate::expr::Value::Null))
    }
    fn operand(&self, _: Leaf, operand: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
        self.attributes.get(operand as usize).cloned().unwrap_or(Known::Unknown)
    }
}

#[test]
fn attributes_project_ordered_nodes_without_losing_the_binding_frame() {
    use crate::expr::Value as Expression;
    let mut world = TestWorld::new();
    world.attributes = vec![Known::Yes(Expression::List(vec![
        Expression::Node(2, "".into()), Expression::Node(1, "".into()),
    ].into()))];
    let tables = tables();
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(2)), None);
    let at = Leaf { frame, ..leaf(0) };
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().follow(at, &[super::super::ATTR_STEP]), Known::Yes(vec![
        Leaf { frame, ..leaf(2) }, Leaf { frame, ..leaf(1) },
    ]));
}

#[test]
fn absent_attributes_are_empty_but_failed_or_non_node_projections_are_unknown() {
    use crate::expr::Value as Expression;
    for (attribute, expected) in [
        (Known::Yes(Expression::Null), Known::Yes(Vec::new())),
        (Known::Unknown, Known::Unknown),
        (Known::Yes(Expression::Num(1.0)), Known::Unknown),
        (Known::Yes(Expression::List(vec![Expression::Node(1, "".into()), Expression::Bool(false)].into())), Known::Unknown),
        (Known::Yes(Expression::Node(99, "".into())), Known::Unknown),
    ] {
        let mut world = TestWorld::new();
        world.attributes = vec![attribute];
        let tables = tables();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.values().follow(leaf(0), &[super::super::ATTR_STEP]), expected);
    }
}

fn leaf(node: u32) -> Leaf {
    Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT }
}

fn tables() -> Names {
    Names {
        kinds: vec![
            KindNames { fits: vec![(1, 0, None)], ..KindNames::default() },
            KindNames { fits: vec![(2, 0, None)], ..KindNames::default() },
            KindNames { fits: vec![(1, 0, Some(0))], ..KindNames::default() },
        ],
        ..Names::default()
    }
}

#[test]
fn declared_universal_values_absorb_joins_and_leave_refinements_intact() {
    let world = TestWorld::new();
    let mut tables = tables();
    tables.top = 1;
    tables.dynamic = 2;
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let top = Leaf { frame, ..leaf(0) };
    let dynamic = Leaf { frame, ..leaf(1) };
    let scalar = Leaf { target: Target::Traits(8), frame };
    for universal in [top, dynamic] {
        let refined = eval::combine(&values, [NodeValue::leaf(universal), NodeValue::leaf(scalar)], true);
        assert_eq!(refined, NodeValue::leaf(scalar));
        assert_eq!(values.could(&refined, 4), Known::Yes(false));
        assert_eq!(eval::combine(&values, [NodeValue::leaf(scalar), NodeValue::leaf(universal)], false), NodeValue::leaf(universal));
        assert_eq!(eval::combine(&values, [NodeValue::Unknown, NodeValue::leaf(universal)], false), NodeValue::leaf(universal));
        assert_eq!(eval::combine(&values, [NodeValue::Unknown, NodeValue::leaf(universal)], true), NodeValue::Unknown);
        assert_eq!(values.expand(&NodeValue::rows(vec![vec![leaf(99)], vec![universal]])), NodeValue::leaf(universal));
    }
    for inputs in [[top, dynamic], [dynamic, top]] {
        assert_eq!(eval::combine(&values, inputs.map(NodeValue::leaf), false), NodeValue::leaf(dynamic));
    }
    assert_eq!(eval::combine(&values, [NodeValue::Unknown, NodeValue::never()], true), NodeValue::never());
    assert_eq!(values.top(scalar), Known::Yes(false));
    assert_eq!(values.dynamic(top), Known::Yes(false));
    assert_eq!(values.dynamic(dynamic), Known::Yes(true));
}

#[test]
fn uncertain_universal_traits_do_not_erase_other_constraints() {
    let world = TestWorld::new();
    let mut tables = tables();
    tables.top = 1;
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let scalar = Leaf { target: Target::Traits(8), frame: FrameId::ROOT };
    let unknown = leaf(2);
    let refined = eval::combine(&values, [NodeValue::leaf(unknown), NodeValue::leaf(scalar)], true);
    assert_eq!(values.always(&refined, 8), Known::Yes(true));
    assert_eq!(values.could(&refined, 1), Known::Unknown);
    assert_eq!(values.top(unknown), Known::Unknown);
}

#[test]
fn raw_self_keeps_the_owner_and_frame_beside_its_expanded_constraint() {
    let mut world = TestWorld::new();
    world.tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 7, node: 1 }] },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
    ], 0);
    let mut tables = tables();
    tables.kinds[0].unwraps.push(super::super::Unwrapping {
        value: Some(Value::Intersection(vec![Value::Current, Value::Path(vec![7])])),
        ..super::super::Unwrapping::default()
    });
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let at = Leaf { frame, ..leaf(0) };
    let expanded = values.values(at);
    assert_eq!(values.always(&expanded, 1 | 2), Known::Yes(true));
    assert_eq!(values.sites(at, &Value::Current), Known::Yes(vec![at]));
    assert_eq!(values.expand(&expanded), expanded);
    let NodeValue::Alternatives(rows) = expanded else { panic!("raw owner and constraint must remain known") };
    assert!(rows.iter().flatten().all(|leaf| leaf.frame == frame));
}

fn generic_world() -> (TestWorld, Names) {
    let mut world = TestWorld::new();
    world.tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 7, node: 1 }, Edge { label: 7, node: 2 }] },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
        Node { matcher: 2, start: 0, end: 0, children: Vec::new() },
    ], 0);
    let mut tables = tables();
    tables.kinds[0].generics.push((vec![7], None));
    (world, tables)
}

#[test]
fn open_traits_keep_positive_knowledge_without_proving_other_traits_absent() {
    let world = TestWorld::new();
    for (when, missing) in [
        (None, Known::Unknown),
        (Some(0), Known::Unknown),
        (Some(3), Known::Yes(false)),
    ] {
        let mut tables = tables();
        tables.kinds[0].open_fits.push((0, when));
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let values = semantic.values();
        assert_eq!(values.fits(leaf(0), 0), Known::Yes(true));
        assert_eq!(values.could(&NodeValue::leaf(leaf(0)), 2), missing);
        assert_eq!(values.always(&NodeValue::leaf(leaf(0)), 2), missing);
    }
}

#[test]
fn arguments_preserve_slot_order_and_caller_frames() {
    let (world, tables) = generic_world();
    let question = QueryContext::default();
    let caller = question.bind(FrameId::ROOT, NodeRef::here(3), None, None);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let at = Leaf { frame: caller, ..leaf(0) };
    assert_eq!(values.arguments(at, &[7]), Known::Yes(vec![
        NodeValue::leaf(Leaf { frame: caller, ..leaf(1) }),
        NodeValue::leaf(Leaf { frame: caller, ..leaf(2) }),
    ]));
    assert_eq!(values.arguments(at, &[8]), Known::Yes(Vec::new()));
}

#[test]
fn finite_indices_and_opted_in_text_labels_keep_real_sites_and_optional_presence() {
    let (world, mut tables) = generic_world();
    tables.kinds[0].indexed.push(IndexedRecipe {
        path: vec![7], mode: super::IndexedMode::Slots { repeated: false, strings: true }, views: 0, value: None, when: None, runtime: false,
    });
    tables.kinds[1].calls.optional.push(None);
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let value = NodeValue::leaf(Leaf { frame, ..leaf(0) });
    let expected = NodeValue::leaf(Leaf { frame, ..leaf(2) });
    assert_eq!(values.member(&value, &Key::Ordinal(1)), expected);
    assert_eq!(values.member(&value, &Key::Text("1".into())), expected);
    assert_eq!(values.member(&value, &Key::Text("01".into())), NodeValue::Unknown);
    let Known::Yes(rows) = values.members(&value) else { panic!("finite members were lost") };
    assert!(rows.iter().flatten().all(|member| member.required == Known::Yes(false)));
    assert_eq!(values.key(Leaf { frame, ..leaf(2) }, &Value::Path(vec![super::super::POSITION])),
        Known::Yes(Key::Ordinal(1)));
}

#[test]
fn nullary_carriers_keep_identity_without_a_self_argument() {
    let mut world = TestWorld::new();
    world.attributes.push(Known::Yes(crate::expr::Value::Str("nullary".into())));
    for (arguments, expected) in [
        (None, Vec::new()),
        (Some(Vec::new()), vec![NodeValue::leaf(leaf(0))]),
        (Some(vec![super::super::IT]), vec![NodeValue::leaf(leaf(0))]),
    ] {
        let mut tables = tables();
        tables.kinds[0].carriers.push(CarrierRecipe {
            description: 0, arguments, value: None, when: None,
        });
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let Known::Yes(Some(carrier)) = semantic.values().carrier(leaf(0)) else { panic!("carrier was lost") };
        assert_eq!(carrier.declaration, NodeRef::here(0));
        assert_eq!(carrier.arguments, expected);
    }
}

#[test]
fn declaration_views_keep_the_requested_class_at_the_real_owner_only() {
    let mut world = TestWorld::new();
    world.names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
    world.names.declared.insert(0, (0, 0, 3, true));
    for (reference, classes) in [(1, 1), (2, 2)] {
        world.names.asked.insert(reference, super::super::ReferenceLookup { scope: 0, classes, outside: None });
        world.names.declaration.insert(reference, 0);
    }
    let mut tables = tables();
    for kind in &mut tables.kinds {
        kind.unwraps = vec![
            super::super::Unwrapping { value: Some(Value::Traits(1)), ..Default::default() },
            super::super::Unwrapping { classes: 2, value: Some(Value::Traits(2)), ..Default::default() },
        ];
    }
    let question = QueryContext::default();
    let type_frame = question.bind_definition(FrameId::ROOT, NodeRef::here(0), Some(leaf(2)), None, 2);
    let value_frame = question.bind_definition(FrameId::ROOT, NodeRef::here(0), Some(leaf(2)), None, 1);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    assert_eq!(values.values(Leaf { frame: type_frame, ..leaf(0) }),
        NodeValue::leaf(Leaf { target: Target::Traits(2), frame: type_frame }));
    assert_eq!(values.values(Leaf { frame: value_frame, ..leaf(0) }),
        NodeValue::leaf(Leaf { target: Target::Traits(1), frame: value_frame }));
    assert_eq!(values.values(Leaf { frame: type_frame, ..leaf(1) }),
        NodeValue::leaf(Leaf { target: Target::Traits(1), frame: type_frame }));
    assert_eq!(values.values(leaf(0)), NodeValue::leaf(Leaf { target: Target::Traits(1), frame: FrameId::ROOT }));
    for (reference, traits) in [(1, 1), (2, 2)] {
        let Known::Yes(declarations) = values.follow(leaf(reference), &[super::super::DECLARATION]) else {
            panic!("real declaration reference was lost");
        };
        let [declaration] = declarations.as_slice() else { panic!("declaration was duplicated") };
        assert_eq!(values.values(*declaration),
            NodeValue::leaf(Leaf { target: Target::Traits(traits), frame: declaration.frame }));
    }
}

#[test]
fn named_keys_win_over_indexers_and_unbounded_keys_keep_the_source_frame() {
    use crate::config::ast::values::KeyDomain;
    let (mut world, mut tables) = generic_world();
    world.tree.edit(|nodes| nodes[0].children.truncate(1));
    for (mode, value) in [
        (IndexedMode::Domain(KeyDomain::Text), Value::Traits(8)),
        (IndexedMode::Keyed(Value::Text("named".into())), Value::Traits(4)),
    ] {
        tables.kinds[0].indexed.push(IndexedRecipe {
            path: vec![7], mode, views: 0, value: Some(value), when: None, runtime: false,
        });
    }
    tables.kinds[2].indexed.push(IndexedRecipe {
        path: vec![super::super::IT], mode: IndexedMode::Domain(KeyDomain::Text),
        views: 0, value: Some(Value::Traits(8)), when: None, runtime: false,
    });
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let value = NodeValue::leaf(Leaf { frame, ..leaf(0) });
    let named = Key::Text("named".into());
    assert_eq!(values.member(&value, &named), NodeValue::leaf(Leaf { target: Target::Traits(4), frame }));
    assert_eq!(values.has_member(&value, &named), Known::Yes(true));
    for row in [
        vec![Leaf { frame, ..leaf(0) }, Leaf { frame, ..leaf(3) }],
        vec![Leaf { frame, ..leaf(3) }, Leaf { frame, ..leaf(0) }],
    ] {
        let intersection = NodeValue::rows(vec![row]);
        assert_eq!(values.member(&intersection, &named), NodeValue::leaf(Leaf { target: Target::Traits(4), frame }));
        let Known::Yes(rows) = values.keyed_members(&intersection, &named) else { panic!("named member was lost") };
        let member = rows[0].as_ref().expect("named member");
        assert_eq!(member.required, Known::Yes(true));
        assert!(member.sources.iter().all(|source| source.origin == MemberOrigin::Named));
    }
    assert_eq!(values.has_member(&NodeValue::rows(vec![vec![leaf(99)], vec![Leaf { frame, ..leaf(0) }]]), &named),
        Known::Yes(true));
    for key in [Key::Text("other".into()), Key::Domain(KeyDomain::Text), Key::Domain(KeyDomain::Ordinal)] {
        assert_eq!(values.member(&value, &key), NodeValue::leaf(Leaf { target: Target::Traits(8), frame }));
        assert_eq!(values.has_member(&value, &key), Known::Unknown);
        let Known::Yes(rows) = values.keyed_members(&value, &key) else { panic!("indexer lookup was lost") };
        let sources = &rows[0].as_ref().expect("matching indexer").sources;
        assert!(sources.iter().all(|source| source.site.frame == frame && source.origin == MemberOrigin::Indexer(KeyDomain::Text)));
    }
    assert_eq!(values.members(&value), Known::Unknown);
    assert_eq!(values.member(&value, &Key::Symbol(NodeRef::here(1))), NodeValue::Unknown);
}

#[test]
fn absent_members_are_false_only_when_the_inventory_is_proven_closed() {
    let mut world = TestWorld::new();
    world.names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
    let mut tables = tables();
    tables.kinds[0].member_defaults = 1;
    tables.kinds[0].member_closed.push((1, None));
    tables.kinds[1].member_defaults = 1;
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let key = Key::Text("missing".into());
    assert_eq!(values.has_member(&NodeValue::leaf(leaf(0)), &key), Known::Yes(false));
    assert_eq!(values.has_member(&NodeValue::leaf(leaf(1)), &key), Known::Unknown);
    assert_eq!(values.has_member(&NodeValue::Unknown, &key), Known::Unknown);
}

#[test]
fn closed_indexer_domains_prove_disjoint_absence_without_listing_infinite_members() {
    use crate::config::ast::values::KeyDomain;
    for domain in [KeyDomain::Text, KeyDomain::Ordinal, KeyDomain::Symbol] {
        for inherited in [false, true] {
            let mut world = TestWorld::new();
            world.tree.edit(|nodes| nodes[0].children.push(Edge { label: 7, node: 1 }));
            let mut tables = tables();
            for node in [0, 1] {
                world.names.scopes.push(ScopeInfo { node, classes: 1, parent: None });
                tables.kinds[node as usize].member_defaults = 1;
                tables.kinds[node as usize].member_closed.push((1, None));
            }
            let owner = usize::from(inherited);
            tables.kinds[owner].indexed.push(IndexedRecipe {
                path: vec![super::super::IT], mode: IndexedMode::Domain(domain),
                views: 1, value: Some(Value::Traits(8)), when: None, runtime: false,
            });
            if inherited { tables.kinds[0].member_bases.push((1, Value::Path(vec![7]), None)); }
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            let values = semantic.values();
            let value = NodeValue::leaf(leaf(0));
            assert_eq!(values.members(&value), Known::Unknown);
            assert_eq!(values.closed(&value), Known::Yes(true), "{domain:?}, inherited={inherited}");
            let expected = if domain == KeyDomain::Text { Known::Unknown } else { Known::Yes(false) };
            assert_eq!(values.has_member(&value, &Key::Text("then".into())), expected);
            assert_eq!(values.has_member(&value, &Key::Domain(domain)), Known::Unknown);
            assert_eq!(values.members(&value), Known::Unknown);
        }
    }
}

#[test]
fn closure_keeps_failed_indexer_conditions_and_unresolved_named_keys_unknown() {
    use crate::config::ast::values::KeyDomain;
    for (mode, when) in [
        (IndexedMode::Domain(KeyDomain::Ordinal), Some(0)),
        (IndexedMode::Keyed(Value::Unknown), None),
    ] {
        let mut world = TestWorld::new();
        world.names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
        let mut tables = tables();
        tables.kinds[0].member_defaults = 1;
        tables.kinds[0].member_closed.push((1, None));
        tables.kinds[0].indexed.push(IndexedRecipe {
            path: vec![super::super::IT], mode, views: 1, value: None, when, runtime: false,
        });
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.values().closed(&NodeValue::leaf(leaf(0))), Known::Unknown);
    }
}

#[test]
fn symbol_keys_need_the_declared_domain_and_keep_real_declaration_identity() {
    use crate::config::ast::values::KeyDomain;
    use crate::grammar::names::calls::Conditional;
    use crate::expr::Value as Expression;
    let mut world = TestWorld::new();
    world.attributes = vec![
        Known::Yes(Expression::Node(1, "".into())),
        Known::Yes(Expression::Node(2, "".into())),
    ];
    let mut tables = tables();
    tables.key_domains = vec![(KeyDomain::Text, 1), (KeyDomain::Symbol, 2)];
    tables.kinds[0].calls.identities.push(Conditional { item: 0, when: None });
    tables.kinds[1].calls.identities.push(Conditional { item: 0, when: None });
    tables.kinds[2].fits = vec![(2, 0, None)];
    tables.kinds[2].calls.identities.push(Conditional { item: 1, when: None });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    assert_eq!(values.value_key(&NodeValue::leaf(leaf(0))), Known::Unknown);
    assert_eq!(values.value_key(&NodeValue::leaf(leaf(1))), Known::Yes(Key::Symbol(NodeRef::here(1))));
    assert_eq!(values.value_key(&NodeValue::leaf(leaf(2))), Known::Yes(Key::Symbol(NodeRef::here(2))));
    assert_eq!(values.value_key(&NodeValue::rows(vec![vec![leaf(1)], vec![leaf(2)]])),
        Known::Yes(Key::Domain(KeyDomain::Symbol)));
    assert_eq!(values.value_key(&NodeValue::leaf(Leaf { target: Target::Traits(2), frame: FrameId::ROOT })),
        Known::Yes(Key::Domain(KeyDomain::Symbol)));
}

#[test]
fn repeated_indices_answer_individual_reads_without_inventing_a_finite_inventory() {
    let (mut world, mut tables) = generic_world();
    world.tree.edit(|nodes| nodes[0].children.truncate(1));
    tables.kinds[0].indexed.push(IndexedRecipe {
        path: vec![7], mode: super::IndexedMode::Slots { repeated: true, strings: false }, views: 0, value: None, when: None, runtime: false,
    });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().members(&NodeValue::leaf(leaf(0))), Known::Unknown);
    assert_eq!(semantic.values().member(&NodeValue::leaf(leaf(0)), &Key::Ordinal(100)), NodeValue::leaf(leaf(1)));
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
    assert_eq!(semantic.values().member(&NodeValue::leaf(Leaf { frame, ..leaf(0) }),
        &Key::Domain(crate::config::ast::values::KeyDomain::Ordinal)),
        NodeValue::leaf(Leaf { frame, ..leaf(1) }));
}

#[test]
fn indexed_value_recipes_keep_source_frames_and_presence_metadata() {
    let (world, mut tables) = generic_world();
    tables.kinds[0].indexed.push(IndexedRecipe {
        path: vec![7], mode: super::IndexedMode::Slots { repeated: false, strings: false }, views: 0,
        value: Some(Value::Path(vec![super::super::WIDENED])), when: None, runtime: false,
    });
    tables.kinds[1].widens.push((Value::Traits(4), None));
    tables.kinds[1].calls.optional.push(None);
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
    let semantic = Semantic::new(&world, &tables, &question);
    let source = Leaf { frame, ..leaf(0) };
    let value = NodeValue::leaf(source);
    assert_eq!(semantic.values().member(&value, &Key::Ordinal(0)),
        NodeValue::leaf(Leaf { frame, target: Target::Traits(4) }));
    assert_eq!(semantic.values().member_sites(&value, &Key::Ordinal(0)),
        Known::Yes(vec![Leaf { frame, ..leaf(1) }]));
    let Known::Yes(rows) = semantic.values().members(&value) else { panic!("finite member inventory") };
    assert!(rows.iter().flatten().all(|member| member.required == Known::Yes(false)));
}

#[test]
fn spread_recipes_transform_values_without_changing_source_inventory() {
    let (world, mut tables) = generic_world();
    tables.kinds[1].widens.push((Value::Traits(4), None));
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
    let semantic = Semantic::new(&world, &tables, &question);
    let source = Leaf { frame, ..leaf(0) };
    let recipe = Value::Spread {
        intersection: false, path: vec![7], value: Some(Box::new(Value::Path(vec![super::super::WIDENED]))),
    };
    assert_eq!(semantic.values().evaluate(source, &recipe),
        NodeValue::leaf(Leaf { frame, target: Target::Traits(4) }));
    assert_eq!(semantic.values().sites(source, &recipe), Known::Yes(vec![
        Leaf { frame, ..leaf(1) }, Leaf { frame, ..leaf(2) },
    ]));
    let absent = Value::Spread { intersection: false, path: vec![8], value: Some(Box::new(Value::Path(vec![9]))) };
    assert_eq!(semantic.values().evaluate(source, &absent), NodeValue::never());
    let unresolved = Value::Spread { intersection: false, path: vec![7], value: Some(Box::new(Value::Path(vec![9]))) };
    assert_eq!(semantic.values().evaluate(source, &unresolved), NodeValue::Unknown);
}

#[test]
fn reference_arguments_bind_formals_without_leaking_the_declaration_frame() {
    let (mut world, mut tables) = generic_world();
    world.tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 7, node: 1 }, Edge { label: 7, node: 2 }] },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
        Node { matcher: 2, start: 0, end: 0, children: vec![Edge { label: 8, node: 4 }, Edge { label: 8, node: 5 }] },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
    ], 0);
    tables.kinds[2].arguments.push((vec![8], None));
    tables.kinds[1].unwraps.push(super::super::Unwrapping {
        value: Some(Value::Current), when: Some(3), ..Default::default()
    });
    let question = QueryContext::default();
    let caller = question.bind(FrameId::ROOT, NodeRef::here(3), None, None);
    let through = Leaf { frame: caller, ..leaf(3) };
    let binding = question.bind(FrameId::ROOT, NodeRef::here(0), Some(through), None);
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().bound_argument(Leaf { frame: binding, ..leaf(2) }),
        Known::Yes(Some(NodeValue::leaf(Leaf { frame: caller, ..leaf(5) }))));
    assert!(!world.conditions.borrow().is_empty());
    assert!(world.conditions.borrow().iter().all(|site| *site == Leaf { frame: caller, ..leaf(5) }),
        "reading one formal evaluated an unrelated argument");
    world.conditions.borrow_mut().clear();
    assert_eq!(semantic.values().declared_arguments(through), Known::Yes(Some(vec![
        NodeValue::leaf(Leaf { frame: caller, ..leaf(4) }),
        NodeValue::leaf(Leaf { frame: caller, ..leaf(5) }),
    ])));
    assert!(world.conditions.borrow().contains(&Leaf { frame: caller, ..leaf(4) }));
    assert!(world.conditions.borrow().iter().all(|site| site.frame == caller
        && [leaf(4).target, leaf(5).target].contains(&site.target)));
    drop(semantic);
    for when in [None, Some(0)] {
        tables.kinds[2].arguments.push((vec![8], when));
        world.conditions.borrow_mut().clear();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.values().bound_argument(Leaf { frame: binding, ..leaf(2) }), Known::Unknown);
        assert!(world.conditions.borrow().iter().all(|site| *site == through),
            "invalid argument declarations evaluated payloads");
        drop(semantic);
        tables.kinds[2].arguments.pop();
    }
    world.tree.edit(|nodes| nodes[3].children.truncate(1));
    world.conditions.borrow_mut().clear();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().bound_argument(Leaf { frame: binding, ..leaf(2) }), Known::Yes(None));
    assert!(world.conditions.borrow().is_empty());
}

#[test]
fn conditional_recipes_choose_only_proven_branches() {
    let loaded = crate::config::load_text("conditional.zlint", r#"
        lang demo "*.demo"
        exclusive scalar, other
        in .demo match scalar /s/ { fits scalar }
        in .demo match other /o/ { fits other }
        in .demo match opaque /u/ { fits unreadable open }
        in .demo match missing /m/ { unwraps unknown }
        in .demo match choice check:(scalar or other or opaque or missing) {
            unwraps select check against trait scalar then trait yes else trait no
        }
        in .demo match outcomes /x/ { fits yes, no }
    "#, &crate::config::source::Disk);
    let mut errors = loaded.errors;
    let grammar = Grammar::build(&loaded.config, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    let mut world = TestWorld::new();
    let choice = grammar.matcher("choice").expect("choice");
    let check = grammar.labels.get("check").expect("check");
    world.tree = Tree::from_native(vec![
        Node { matcher: choice, start: 0, end: 0, children: vec![Edge { label: check, node: 1 }] },
        Node { matcher: grammar.matcher("scalar").expect("scalar"), start: 0, end: 0, children: Vec::new() },
        Node { matcher: choice, start: 0, end: 0, children: vec![Edge { label: check, node: 3 }] },
        Node { matcher: grammar.matcher("other").expect("other"), start: 0, end: 0, children: Vec::new() },
        Node { matcher: choice, start: 0, end: 0, children: vec![Edge { label: check, node: 5 }] },
        Node { matcher: grammar.matcher("opaque").expect("opaque"), start: 0, end: 0, children: Vec::new() },
        Node { matcher: choice, start: 0, end: 0, children: vec![Edge { label: check, node: 7 }] },
        Node { matcher: grammar.matcher("missing").expect("missing"), start: 0, end: 0, children: Vec::new() },
    ], 0);
    let yes = grammar.names.vocabulary.trait_bit("yes");
    let no = grammar.names.vocabulary.trait_bit("no");
    world.grammar = grammar;
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &world.grammar.names, &question);
    assert_eq!(semantic.values().values(leaf(0)),
        NodeValue::leaf(Leaf { target: Target::Traits(yes), frame: FrameId::ROOT }));
    assert_eq!(semantic.values().values(leaf(2)),
        NodeValue::leaf(Leaf { target: Target::Traits(no), frame: FrameId::ROOT }));
    assert_eq!(semantic.values().values(leaf(4)), NodeValue::Unknown);
    assert_eq!(semantic.values().values(leaf(6)), NodeValue::Unknown);
    assert_eq!(semantic.values().values(leaf(7)), NodeValue::Unknown);
}

#[test]
fn generic_slots_skip_unrelated_owners_and_keep_the_actual_value_frames() {
    let (world, tables) = generic_world();
    let question = QueryContext::default();
    let actual = NodeValue::rows(vec![vec![leaf(0), leaf(3)], vec![leaf(2)]]);
    let owner = question.bind(FrameId::ROOT, NodeRef::here(0), None, Some(vec![NodeValue::Unknown, actual.clone()]));
    let unrelated = question.bind(owner, NodeRef::here(3), None, Some(vec![NodeValue::never()]));
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().bound_argument(Leaf { frame: unrelated, ..leaf(2) }), Known::Yes(Some(actual)));
    assert_eq!(semantic.values().bound_argument(Leaf {
        target: Target::Node(NodeRef { file: 1, node: 2 }), frame: unrelated,
    }), Known::Unknown);
}

#[test]
fn distribution_overlays_preserve_other_slots_but_inference_frames_do_not_inherit_holes() {
    let (world, tables) = generic_world();
    let question = QueryContext::default();
    let owner = NodeRef::here(0);
    let first = NodeValue::leaf(leaf(3));
    let second = NodeValue::rows(vec![vec![leaf(0), leaf(3)], vec![leaf(2)]]);
    let outer = question.bind_definition(FrameId::ROOT, owner, None,
        Some(vec![first.clone(), second.clone()]), 2);
    let unrelated = question.bind_definition(outer, NodeRef::here(3), None, None, 4);
    let overlay = question.bind_overlay(unrelated, owner, vec![Some(NodeValue::never())], 2);
    let inference = question.bind_slots(overlay, owner, None, vec![None, Some(first.clone())], 2);
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().bound_argument(Leaf { frame: overlay, ..leaf(1) }), Known::Yes(Some(NodeValue::never())));
    assert_eq!(semantic.values().bound_argument(Leaf { frame: overlay, ..leaf(2) }), Known::Yes(Some(second)));
    assert_eq!(semantic.values().bound_argument(Leaf { frame: inference, ..leaf(1) }), Known::Yes(None));
    assert_eq!(semantic.values().bound_argument(Leaf { frame: inference, ..leaf(2) }), Known::Yes(Some(first)));
    assert_eq!(question.owner_classes(unrelated, owner), Known::Yes(2));
    assert_eq!(question.owner_classes(unrelated, NodeRef::here(2)), Known::Yes(0));
    let plain = question.bind(unrelated, owner, None, None);
    assert_eq!(question.owner_classes(plain, owner), Known::Yes(0));
}

#[test]
fn conditional_distribution_rebinds_each_whole_arm_and_keeps_never_empty() {
    let (mut world, mut tables) = generic_world();
    world.attributes = vec![Known::Yes(crate::expr::Value::Node(1, "".into()))];
    tables.exclusive = vec![3];
    let selection = Value::Select(Box::new(super::compile::Selection {
        check: Value::Path(vec![super::super::ATTR_STEP, super::super::INFERRED]),
        pattern: Value::Traits(1),
        yes: Value::Path(vec![super::super::ATTR_STEP, super::super::INFERRED]),
        no: Value::Traits(4),
        distribute: Some(vec![super::super::ATTR_STEP]),
        dynamic_both: false,
    }));
    for input in [
        NodeValue::rows(vec![
            vec![
                Leaf { target: Target::Traits(1), frame: FrameId::ROOT },
                Leaf { target: Target::Traits(8), frame: FrameId::ROOT },
            ],
            vec![Leaf { target: Target::Traits(2), frame: FrameId::ROOT }],
        ]),
        NodeValue::never(),
    ] {
        let empty = input == NodeValue::never();
        let question = QueryContext::default();
        let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, Some(vec![input]));
        let semantic = Semantic::new(&world, &tables, &question);
        let values = semantic.values();
        let output = values.evaluate(Leaf { frame, ..leaf(3) }, &selection);
        if empty {
            assert_eq!(output, NodeValue::never());
        } else {
            assert_eq!(values.could(&output, 1), Known::Yes(true));
            assert_eq!(values.could(&output, 2), Known::Yes(false));
            assert_eq!(values.could(&output, 4), Known::Yes(true));
            assert_eq!(values.could(&output, 8), Known::Yes(true));
        }
    }
}

#[test]
fn dynamic_conditionals_use_the_bound_inferred_pattern_before_choosing_branches() {
    let (mut world, mut tables) = generic_world();
    world.tree.edit(|nodes| nodes[0].children.truncate(1));
    tables.dynamic = 2;
    tables.kinds[1].fits.clear();
    tables.kinds[1].constraints.push((Value::Union(Vec::new()), None));
    tables.kinds[1].unwraps.push(super::super::Unwrapping {
        value: Some(Value::Path(vec![super::super::INFERRED])),
        ..super::super::Unwrapping::default()
    });
    let selection = Value::Select(Box::new(super::compile::Selection {
        check: Value::Traits(2),
        pattern: Value::Path(vec![7]),
        yes: Value::Path(vec![7, super::super::INFERRED]),
        no: Value::Traits(4),
        distribute: None,
        dynamic_both: true,
    }));
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let output = semantic.values().evaluate(leaf(0), &selection);
    assert_eq!(semantic.values().could(&output, 2), Known::Yes(false));
    assert_eq!(semantic.values().always(&output, 4), Known::Yes(true));
}

#[test]
fn language_products_keep_ordered_values_and_member_frames_without_nominal_identity() {
    let loaded = crate::config::load_text("products.zlint", r#"
        lang demo "*.demo"
        product duo items fits object as { kind: "duo" } inherits ...items readonly inherits trait frozen optional trait absent
        in .demo match marker /m/ { fits frozen, absent }
        in .demo match item /i/ {
            fits scalar
            scope members
            members members closed
            member members "extra" unwraps it
        }
        in .demo match pack (element:item)+ { unwraps product duo of element }
    "#, &crate::config::source::Disk);
    let mut errors = loaded.errors;
    let grammar = Grammar::build(&loaded.config, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    let mut world = TestWorld::new();
    let item = grammar.matcher("item").expect("item");
    let element = grammar.labels.get("element").expect("element");
    world.tree = Tree::from_native(vec![
        Node { matcher: grammar.matcher("pack").expect("pack"), start: 0, end: 0, children: vec![
            Edge { label: element, node: 1 }, Edge { label: element, node: 2 },
        ] },
        Node { matcher: item, start: 0, end: 0, children: Vec::new() },
        Node { matcher: item, start: 0, end: 0, children: Vec::new() },
    ], 0);
    world.grammar = grammar;
    let members = world.grammar.names.vocabulary.scope_class("members");
    for node in [1, 2] {
        world.names.scopes.push(ScopeInfo { node, classes: members, parent: None });
    }
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, None);
    let semantic = Semantic::new(&world, &world.grammar.names, &question);
    let values = semantic.values();
    let output = values.values(Leaf { frame, ..leaf(0) });
    let NodeValue::Alternatives(arms) = &output else { panic!("product was unreadable") };
    let product = *arms.iter().flatten().next().expect("product");
    let Known::Yes(Some(carrier)) = values.carrier(product) else { panic!("product carrier was lost") };
    let first = values.values(Leaf { frame, ..leaf(1) });
    let second = values.values(Leaf { frame, ..leaf(2) });
    assert_eq!(carrier.arguments, vec![first.clone(), second.clone()]);
    assert_eq!(values.member(&output, &Key::Ordinal(0)), first);
    assert_eq!(values.member(&output, &Key::Ordinal(1)), second);
    assert_eq!(values.identity(product), Known::Yes(None));
    assert_eq!(values.closed(&output), Known::Yes(true));
    let scalar = world.grammar.names.vocabulary.trait_bit("scalar");
    assert_eq!(values.always(&values.member(&output, &Key::Text("extra".into())), scalar), Known::Yes(true));
    let Known::Yes(Some(info)) = values.product_info(product) else { panic!("product payload was lost") };
    let readonly = values.product(info.shape, info.source, info.elements, true);
    assert_eq!(values.always(&readonly, world.grammar.names.vocabulary.trait_bit("frozen")), Known::Yes(true));
    let empty = values.product(super::products::ProductShapeId(0), Leaf { frame, ..leaf(0) }, Vec::new(), false);
    assert_eq!(values.member(&empty, &Key::Domain(crate::config::ast::values::KeyDomain::Ordinal)), NodeValue::never());
}

#[test]
fn product_index_domains_join_slots_and_known_prefixes_ignore_unreadable_rest() {
    use super::products::{ProductElement, ProductShapeId, Shape};
    let world = TestWorld::new();
    let mut tables = tables();
    tables.products.push(Some(Shape {
        traits: 0, description: crate::expr::Value::Null,
        inherits: Some(Value::Path(vec![999])), readonly_inherits: None, optional: None, count: None,
    }));
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let first = NodeValue::leaf(Leaf { frame, ..leaf(0) });
    let second = NodeValue::leaf(Leaf { frame, ..leaf(1) });
    let element = |site, value, repeated| ProductElement { site, value, optional: false, repeated, values: false };
    let product = values.product(ProductShapeId(0), leaf(2), vec![
        element(leaf(0), first.clone(), false),
        element(leaf(1), second.clone(), false),
    ], false);
    assert_eq!(values.member(&product, &Key::Domain(crate::config::ast::values::KeyDomain::Ordinal)),
        values.combine([first.clone(), second], false));
    let broad_text = values.member(&product, &Key::Domain(crate::config::ast::values::KeyDomain::Text));
    let product = values.product(ProductShapeId(0), leaf(2), vec![
        element(leaf(0), first.clone(), false),
        element(leaf(1), NodeValue::Unknown, true),
    ], false);
    assert_eq!(values.member(&product, &Key::Ordinal(0)), first);
    let Known::Yes(rows) = values.keyed_members(&product, &Key::Ordinal(0)) else { panic!("missing prefix") };
    let prefix_is_fixed = rows.into_iter().flatten().flat_map(|member| member.sources)
        .all(|source| source.origin == MemberOrigin::Ordinal { repeated: false });
    assert_eq!(values.member(&product, &Key::Ordinal(1)), NodeValue::Unknown);
    assert_eq!((broad_text, prefix_is_fixed), (NodeValue::Unknown, true));
}

#[test]
fn product_optional_elements_keep_whole_values_and_nested_rest_order() {
    use super::products::{ProductElement, ProductShapeId, Shape};
    let world = TestWorld::new();
    let mut tables = tables();
    tables.products.push(Some(Shape {
        traits: 0, description: crate::expr::Value::Null,
        inherits: None, readonly_inherits: None, optional: Some(Value::Traits(8)), count: None,
    }));
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None);
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let first = NodeValue::rows(vec![vec![
        Leaf { frame, ..leaf(0) }, Leaf { target: Target::Traits(16), frame },
    ], vec![Leaf { frame, ..leaf(1) }]]);
    let second = NodeValue::leaf(Leaf { frame, ..leaf(1) });
    let element = |site, value, optional, repeated| ProductElement { site, value, optional, repeated, values: false };
    let nested = values.product(ProductShapeId(0), leaf(2), vec![
        element(leaf(0), first.clone(), true, false),
        element(leaf(1), second.clone(), false, false),
    ], true);
    let product = values.product(ProductShapeId(0), leaf(2), vec![
        element(leaf(2), nested, false, true),
    ], false);
    let expected = values.combine([first, NodeValue::leaf(Leaf { target: Target::Traits(8), frame: FrameId::ROOT })], false);
    assert_eq!(values.member(&product, &Key::Ordinal(0)), expected);
    assert_eq!(values.member(&product, &Key::Ordinal(1)), second);
    let NodeValue::Alternatives(arms) = product else { panic!("product unreadable") };
    let Known::Yes(Some(info)) = values.product_info(arms[0][0]) else { panic!("missing product") };
    assert!(!info.readonly);
    assert!(info.elements[0].optional);
    assert!(!info.elements[0].repeated);
}

#[test]
fn product_producer_flags_distinguish_missing_from_unreadable_values() {
    use super::products::{ProductShapeId, Shape};
    use crate::expr::Value as Expression;
    for (flag, expected) in [
        (Known::Yes(Expression::Null), Some(false)),
        (Known::Yes(Expression::Bool(false)), Some(false)),
        (Known::Yes(Expression::Bool(true)), Some(true)),
        (Known::Yes(Expression::Num(1.0)), None),
        (Known::Unknown, None),
    ] {
        let mut world = TestWorld::new();
        world.attributes.push(flag);
        let mut tables = tables();
        tables.products.push(Some(Shape {
            traits: 0, description: Expression::Null,
            inherits: None, readonly_inherits: None, optional: None, count: None,
        }));
        let question = QueryContext::default();
        let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None);
        let semantic = Semantic::new(&world, &tables, &question);
        let values = semantic.values();
        let recipe = super::compile::ProductRecipe {
            shape: ProductShapeId(0), sites: Vec::new(), value: None,
            optional: Some(vec![super::super::ATTR_STEP]),
            repeated: None,
            values: None,
            readonly: Some(vec![super::super::ATTR_STEP]),
        };
        let payload = NodeValue::leaf(Leaf { frame, ..leaf(1) });
        let result = values.produce_product(leaf(0), &recipe, |_| payload.clone());
        let Some(expected) = expected else {
            assert_eq!(result, NodeValue::Unknown);
            continue;
        };
        let NodeValue::Alternatives(arms) = result else { panic!("missing product") };
        let Known::Yes(Some(info)) = values.product_info(arms[0][0]) else { panic!("missing product info") };
        assert_eq!(info.readonly, expected);
        assert_eq!(info.elements[0].optional, expected);
        assert_eq!(info.elements[0].value, payload);
    }
}

#[test]
fn variadic_product_inventory_keeps_rest_and_suffix_values_as_alternatives() {
    use super::products::{ProductElement, ProductShapeId, Shape};
    use crate::config::ast::values::KeyDomain;
    let world = TestWorld::new();
    let mut tables = tables();
    tables.kinds[1].indexed.push(IndexedRecipe {
        path: Vec::new(), mode: IndexedMode::Domain(KeyDomain::Ordinal),
        views: 0, value: Some(Value::Traits(2)), when: None, runtime: false,
    });
    tables.products.push(Some(Shape {
        traits: 0, description: crate::expr::Value::Null,
        inherits: None, readonly_inherits: None, optional: None, count: None,
    }));
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let product = values.product(ProductShapeId(0), leaf(2), vec![
        ProductElement { site: leaf(1), value: NodeValue::leaf(leaf(1)), optional: false, repeated: true, values: false },
        ProductElement { site: leaf(0), value: NodeValue::leaf(leaf(0)), optional: false, repeated: false, values: false },
    ], false);
    let expected = values.combine([values.evaluate(leaf(1), &Value::Traits(2)), NodeValue::leaf(leaf(0))], false);
    assert_eq!(values.member(&product, &Key::Domain(KeyDomain::Ordinal)), expected);
    let Known::Yes(rows) = values.members(&product) else { panic!("unreadable inventory") };
    let actual = values.combine(rows.into_iter().flatten()
        .filter(|member| member.key == Key::Domain(KeyDomain::Ordinal))
        .flat_map(|member| member.sources).map(|source| {
            source.value.as_ref().map_or_else(|| values.values(source.site), |recipe| values.evaluate(source.site, recipe))
        }), true);
    assert_eq!(actual, expected);
}

#[test]
fn product_flags_check_every_selected_source_before_proving_absence() {
    use super::products::{ProductShapeId, Shape};
    let mut world = TestWorld::new();
    world.tree.edit(|nodes| {
        nodes[0].children = vec![Edge { label: 7, node: 1 }, Edge { label: 7, node: 2 }];
        nodes[2].children = vec![Edge { label: 8, node: 1 }];
    });
    let mut tables = tables();
    tables.products.push(Some(Shape {
        traits: 0, description: crate::expr::Value::Null,
        inherits: None, readonly_inherits: None, optional: None, count: None,
    }));
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let recipe = super::compile::ProductRecipe {
        shape: ProductShapeId(0), sites: Vec::new(), value: None,
        optional: Some(vec![7, 8]), repeated: None, values: None, readonly: None,
    };
    assert_eq!(semantic.values().produce_product(leaf(0), &recipe, NodeValue::leaf), NodeValue::Unknown);
}

#[test]
fn product_producers_apply_element_recipes_in_each_source_frame() {
    let loaded = crate::config::load_text("product-recipes.zlint", r#"
        lang demo "*.demo"
        product duo items fits object as { kind: "duo" }
        in .demo match item /i/ { fits scalar }
        in .demo match pack (element:item)+ { unwraps product duo of element using trait scalar }
    "#, &crate::config::source::Disk);
    let mut errors = loaded.errors;
    let grammar = Grammar::build(&loaded.config, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    let mut world = TestWorld::new();
    world.tree = Tree::from_native(vec![
        Node { matcher: grammar.matcher("pack").expect("pack"), start: 0, end: 0, children: vec![
            Edge { label: grammar.labels.get("element").expect("element"), node: 1 },
        ] },
        Node { matcher: grammar.matcher("item").expect("item"), start: 0, end: 0, children: Vec::new() },
    ], 0);
    world.grammar = grammar;
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None);
    let semantic = Semantic::new(&world, &world.grammar.names, &question);
    let values = semantic.values();
    let output = values.values(Leaf { frame, ..leaf(0) });
    let trait_id = world.grammar.names.vocabulary.traits.iter().position(|name| name == "scalar").expect("scalar");
    assert_eq!(values.member(&output, &Key::Ordinal(0)), NodeValue::leaf(Leaf {
        target: Target::Traits(1 << trait_id), frame,
    }));
}

#[test]
fn a_new_instantiation_never_steals_an_older_matching_owners_arguments() {
    let (world, tables) = generic_world();
    for args in [None, Some(Vec::new()), Some(vec![NodeValue::Unknown])] {
        let question = QueryContext::default();
        let outer = question.bind(FrameId::ROOT, NodeRef::here(0), None, Some(vec![NodeValue::leaf(leaf(3))]));
        let expected = match &args {
            Some(args) if !args.is_empty() => Known::Yes(Some(NodeValue::Unknown)),
            _ => Known::Yes(None),
        };
        let inner = question.bind(outer, NodeRef::here(0), Some(leaf(3)), args);
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.values().bound_argument(Leaf { frame: inner, ..leaf(1) }), expected);
    }
}

#[test]
fn generic_slot_lookup_uses_the_shared_budget_and_conditional_owner_role() {
    let (world, mut tables) = generic_world();
    tables.kinds[0].generics[0].1 = Some(0);
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, Some(vec![NodeValue::never()]));
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().bound_argument(Leaf { frame, ..leaf(1) }), Known::Unknown);
    tables.kinds[0].generics[0].1 = Some(3);
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), None, Some(vec![NodeValue::never()]));
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().bound_argument(Leaf { frame, ..leaf(1) }), Known::Yes(None));
    let mut question = QueryContext::default();
    question.budget = Budget::new(0);
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().bound_argument(leaf(1)), Known::Unknown);
}

#[test]
fn selected_definitions_keep_raw_members_and_the_requested_view() {
    let source = r#"
        lang demo "*.demo"
        in .demo match item /i/ {
            fits scalar, typeview
            unwraps trait scalar
            unwraps trait typeview for type
        }
        in .demo match owner target:item {
            scope members
            members members closed
            declares type it in nearest members hoisted
            member members "selected" unwraps target
        }
        in .demo match reference owner:owner {
            defines from owner at "selected"
            unwraps definition
        }
    "#;
    let loaded = crate::config::load_text("definitions.zlint", source, &crate::config::source::Disk);
    let mut errors = loaded.errors;
    let grammar = Grammar::build(&loaded.config, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    let mut world = TestWorld::new();
    let owner = grammar.matcher("owner").expect("owner");
    let item = grammar.matcher("item").expect("item");
    let reference = grammar.matcher("reference").expect("reference");
    let target = grammar.labels.get("target").expect("target field");
    let receiver = grammar.labels.get("owner").expect("owner field");
    let view = grammar.names.vocabulary.declaration_class("type");
    world.tree = Tree::from_native(vec![
        Node { matcher: owner, start: 0, end: 0, children: vec![Edge { label: target, node: 1 }] },
        Node { matcher: item, start: 0, end: 0, children: Vec::new() },
        Node { matcher: reference, start: 0, end: 0, children: vec![Edge { label: receiver, node: 0 }] },
    ], 2);
    world.names.scopes.push(ScopeInfo {
        node: 0, classes: grammar.names.vocabulary.scope_class("members"), parent: None,
    });
    world.names.declared.insert(0, (0, 0, view, true));
    world.grammar = grammar;
    let question = QueryContext::default();
    let caller = question.bind_definition(FrameId::ROOT, NodeRef::here(2), Some(leaf(2)),
        Some(vec![NodeValue::leaf(leaf(0))]), view);
    let at = Leaf { frame: caller, ..leaf(2) };
    let semantic = Semantic::new(&world, &world.grammar.names, &question);
    let values = semantic.values();
    let Known::Yes(definitions) = values.follow(at, &[super::super::DECLARATION]) else {
        panic!("selected definition was unknown");
    };
    let [selected] = definitions.as_slice() else { panic!("selected definition was lost or duplicated") };
    assert_eq!(selected.target, Target::Node(NodeRef::here(1)));
    let frame = question.get(selected.frame).expect("selected frame");
    assert_eq!(frame.owner, NodeRef::here(1));
    assert_eq!(frame.classes, 0);
    let scalar = world.grammar.names.vocabulary.trait_bit("scalar");
    assert_eq!(values.values(*selected),
        NodeValue::leaf(Leaf { target: Target::Traits(scalar), frame: selected.frame }));
    let Known::Yes(binding) = values.follow(leaf(0), &[super::super::DECLARATION]) else {
        panic!("ordinary binding fallback was lost");
    };
    assert_eq!(binding[0].target, Target::Node(NodeRef::here(0)));

    let (mut world, mut tables) = generic_world();
    world.tree.edit(|nodes| nodes[3].children.push(Edge { label: 8, node: 2 }));
    tables.kinds[2].arguments.push((vec![8], None));
    tables.kinds[0].defines.push((Value::Current, None));
    for explicit in [false, true] {
        let question = QueryContext::default();
        let actual = NodeValue::leaf(leaf(2));
        let bound = question.bind_definition(FrameId::ROOT, NodeRef::here(0), Some(leaf(3)),
            explicit.then(|| vec![actual.clone()]), view);
        let at = Leaf { frame: bound, ..leaf(0) };
        let semantic = Semantic::new(&world, &tables, &question);
        let Known::Yes(selected) = semantic.values().follow(at, &[super::super::DECLARATION]) else {
            panic!("bound definition was unknown");
        };
        let [selected] = selected.as_slice() else { panic!("bound definition was lost or duplicated") };
        assert_eq!(semantic.values().bound_argument(Leaf { frame: selected.frame, ..leaf(1) }),
            Known::Yes(Some(actual)));
        assert_eq!(semantic.values().origin(*selected), Known::Yes(Some(leaf(3))));
        assert_eq!(question.get(selected.frame).expect("bound frame").classes, view);
    }
}

#[test]
fn selected_definitions_fallback_only_on_proven_closed_absence() {
    let (mut world, mut tables) = generic_world();
    world.names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
    tables.kinds[0].member_defaults = 1;
    let missing = Value::From(Box::new(Value::Current), Box::new(Value::Text("missing".into())), 0);
    let fallback = Value::Either(Box::new(missing.clone()), Box::new(Value::Path(vec![7])));
    for closed in [false, true] {
        tables.kinds[0].member_closed = if closed { vec![(1, None)] } else { Vec::new() };
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.values().sites(leaf(0), &fallback),
            if closed { Known::Yes(vec![leaf(1), leaf(2)]) } else { Known::Unknown });
        assert_eq!(semantic.values().evaluate(leaf(0), &missing), NodeValue::Unknown);
    }
    world.attributes = vec![Known::Unknown];
    let failed = Value::Either(Box::new(Value::From(
        Box::new(Value::Path(vec![super::super::ATTR_STEP])),
        Box::new(Value::Text("missing".into())), 0,
    )), Box::new(Value::Path(vec![7])));
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().sites(leaf(0), &failed), Known::Unknown);
    let mut question = QueryContext::default();
    question.budget = Budget::new(0);
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().sites(leaf(0), &fallback), Known::Unknown);
}

#[test]
fn raw_sites_do_not_expand_the_reached_declarations() {
    let (world, mut tables) = generic_world();
    tables.kinds[1].unwraps.push(super::super::Unwrapping {
        value: Some(Value::Path(vec![8])), ..Default::default()
    });
    let question = QueryContext::default();
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(3)), None);
    let semantic = Semantic::new(&world, &tables, &question);
    let at = Leaf { frame, ..leaf(0) };
    assert_eq!(semantic.values().sites(at, &Value::Path(vec![7])), Known::Yes(vec![
        Leaf { frame, ..leaf(1) }, Leaf { frame, ..leaf(2) },
    ]));
}

#[test]
fn raw_sites_fallback_distinguishes_missing_paths_from_unknown_and_empty_spreads() {
    let (mut world, tables) = generic_world();
    world.attributes = vec![Known::Unknown];
    for (left, expected) in [
        (Value::Path(vec![8]), Known::Yes(vec![leaf(1), leaf(2)])),
        (Value::Path(vec![super::super::ATTR_STEP]), Known::Unknown),
        (Value::Spread { intersection: false, path: vec![8], value: None }, Known::Yes(Vec::new())),
    ] {
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.values().sites(leaf(0), &Value::Either(Box::new(left), Box::new(Value::Path(vec![7])))), expected);
    }
}

#[test]
fn excluding_traits_preserves_absence_without_hiding_failed_or_empty_values() {
    let mut world = TestWorld::new();
    world.attributes = vec![Known::Unknown];
    let tables = tables();
    for (source, expected, fallback) in [
        (Value::Path(vec![8]), Known::Yes(None), NodeValue::leaf(leaf(1))),
        (Value::Path(vec![super::super::ATTR_STEP]), Known::Unknown, NodeValue::Unknown),
        (Value::Spread { intersection: false, path: vec![8], value: None }, Known::Yes(Some(NodeValue::never())), NodeValue::never()),
        (Value::Current, Known::Yes(Some(NodeValue::never())), NodeValue::never()),
    ] {
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let expression = Value::Excludes { traits: 1, from: Some(Box::new(source)) };
        assert_eq!(semantic.values().evaluate_optional(leaf(0), &expression), expected);
        let expression = Value::Either(Box::new(expression), Box::new(Value::Captured(Box::new(NodeValue::leaf(leaf(1))))));
        assert_eq!(semantic.values().evaluate(leaf(0), &expression), fallback);
    }
}

#[test]
fn value_refinement_respects_the_shared_engine_nesting_limit() {
    let world = TestWorld::new();
    let tables = tables();
    let question = QueryContext::default();
    let guards: Vec<_> = std::iter::from_fn(|| question.budget.nest()).collect();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().values(leaf(0)), NodeValue::Unknown);
    drop(guards);
    let fresh = Semantic::new(&world, &tables, &question);
    assert_eq!(fresh.values().values(leaf(0)), NodeValue::leaf(leaf(0)));
}

#[test]
fn alternatives_require_every_branch_but_intersections_can_prove_a_trait() {
    let world = TestWorld::new();
    let tables = tables();
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    let union = NodeValue::rows(vec![vec![leaf(0)], vec![leaf(1)]]);
    assert_eq!(values.could(&union, 1), Known::Yes(true));
    assert_eq!(values.always(&union, 1), Known::Yes(false));
    let intersection = NodeValue::rows(vec![vec![leaf(0), leaf(1)]]);
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
        value: Some(super::super::Value::Spread { intersection: false, path: vec![7], value: None }),
        ..Default::default()
    });
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let values = semantic.values();
    assert_eq!(values.values(leaf(0)), NodeValue::rows(vec![vec![leaf(1)], vec![leaf(2)]]));
}

#[test]
fn cyclic_unwrapping_returns_unknown_instead_of_a_partly_resolved_leaf() {
    let mut world = TestWorld::new();
    world.tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 0, children: Vec::new() },
    ], 0);
    let mut tables = tables();
    tables.kinds[0].unwraps.push(super::super::Unwrapping {
        value: Some(super::super::Value::Path(Vec::new())),
        ..Default::default()
    });
    let question = QueryContext::default();
    assert_eq!(Semantic::new(&world, &tables, &question).values().values(leaf(0)), NodeValue::Unknown);
}

#[test]
fn conditions_reenter_the_same_question_memo_through_borrowed_handles() {
    let world = TestWorld::new();
    let mut tables = tables();
    tables.kinds[0].fits = vec![(1, 0, Some(1))];
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    assert_eq!(semantic.values().fits(leaf(0), 0), Known::Unknown);
    assert_eq!(semantic.values().fits(leaf(0), 0), Known::Unknown);
}

#[test]
fn scope_views_keep_the_opening_nodes_conditional_traits() {
    let mut world = TestWorld::new();
    world.names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
    let mut tables = tables();
    tables.kinds[0].fits = vec![(1, 0, Some(2))];
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    let scoped = Leaf { target: Target::Scope(ScopeRef { file: 0, scope: 0 }), frame: FrameId::ROOT };
    assert_eq!(semantic.values().fits(scoped, 0), Known::Yes(true));
    assert_eq!(semantic.values().fits(leaf(0), 0), Known::Yes(false));
}

#[test]
fn traits_restricted_to_one_view_do_not_leak_to_instances_or_nodes() {
    let mut world = TestWorld::new();
    world.names.scopes.extend([
        ScopeInfo { node: 0, classes: 1, parent: None },
        ScopeInfo { node: 0, classes: 2, parent: None },
    ]);
    let mut tables = tables();
    tables.kinds[0].fits = vec![(1, 1, None)];
    let question = QueryContext::default();
    let semantic = Semantic::new(&world, &tables, &question);
    for (scope, expected) in [(0, true), (1, false)] {
        let view = Leaf { target: Target::Scope(ScopeRef { file: 0, scope }), frame: FrameId::ROOT };
        assert_eq!(semantic.values().fits(view, 0), Known::Yes(expected));
    }
    assert_eq!(semantic.values().fits(leaf(0), 0), Known::Yes(false));
}

#[test]
fn exclusive_groups_are_compiled_from_fitted_traits_not_queries() {
    let sources = [
        ("lang demo \"*.demo\"\nin .demo match leaf /a/ { fits first, second }\nexclusive first, second\n", true),
        ("lang demo \"*.demo\"\nin .demo match leaf /a/ { fits first; unwraps truthy excludes missing }\nexclusive first, missing\n", false),
        ("exclusive first, missing\n", false),
    ];
    for (source, valid) in sources {
        let loaded = crate::config::load_text("values.zlint", source, &crate::config::source::Disk);
        let config = loaded.config;
        let mut errors = loaded.errors;
        let grammar = Grammar::build(&config, &mut errors);
        let names = &grammar.names;
        if valid {
            assert!(errors.is_empty(), "{errors:?}");
            assert_eq!(names.exclusive.len(), 1);
            assert_eq!(names.exclusive[0].count_ones(), 2);
        } else {
            assert!(!errors.is_empty(), "query-only domains were treated as definitions");
        }
    }
}

#[test]
fn intersections_of_exclusive_domains_are_never() {
    let mut world = TestWorld::new();
    world.tree = Tree::from_native(vec![
        Node { matcher: 3, start: 0, end: 0, children: vec![Edge { label: 7, node: 1 }, Edge { label: 8, node: 2 }] },
        Node { matcher: 0, start: 0, end: 0, children: Vec::new() },
        Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
    ], 0);
    let mut tables = tables();
    tables.exclusive = vec![1 | 2];
    tables.kinds.push(KindNames {
        unwraps: vec![super::super::Unwrapping {
            value: Some(Value::Intersection(vec![Value::Path(vec![7]), Value::Path(vec![8])])),
            ..Default::default()
        }],
        ..KindNames::default()
    });
    let question = QueryContext::default();
    assert_eq!(Semantic::new(&world, &tables, &question).values().values(leaf(0)), NodeValue::never());
}

#[test]
fn instance_classes_must_have_real_declarations_not_only_queries() {
    use crate::config::ast::{Name, NameDef, NameItem, Unwrap};
    for (class, valid) in [("type", true), ("missing", false)] {
        let loaded = crate::config::load_text(
            "values.zlint",
            "lang demo \"*.demo\"\nin .demo match leaf /a/ { scope members; declares type it in nearest members }\n",
            &crate::config::source::Disk,
        );
        let mut config = loaded.config;
        let mut errors = loaded.errors;
        let target = config.matchers[0].name.clone();
        config.names.push(NameDef {
            targets: vec![target.clone()],
            when: None,
            item: NameItem::Unwraps {
                effect: None, possible: false, nearest: Vec::new(), classes: Vec::new(),
                value: Some(Unwrap::Instance {
                    name: Name { text: "LibraryTemplate".into(), span: target.span },
                    class: Name { text: class.into(), span: target.span },
                    arguments: Vec::new(),
                }),
            },
        });
        Grammar::build(&config, &mut errors);
        assert_eq!(errors.is_empty(), valid, "{errors:?}");
    }
}

use super::*;
use crate::config::ast::Config;
use crate::grammar::{Edge, Grammar, Node, Tree};
use super::super::{KindNames, Names, Resolved, ScopeInfo};
use super::super::question::{Budget, FrameId, QueryContext, ScopeRef, World};

struct TestWorld {
    grammar: Grammar,
    tree: Tree,
    names: Resolved,
    attributes: Vec<Known<crate::expr::Value<'static>>>,
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
    fn attribute(&self, _: Leaf, attr: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
        self.attributes.get(attr as usize).cloned().unwrap_or(Known::Yes(crate::expr::Value::Null))
    }
    fn operand(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
        Known::Unknown
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
    let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(NodeRef::here(2)), None);
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
            effect: None, possible: false,
            value: Some(Value::Intersection(vec![Value::Path(vec![7]), Value::Path(vec![8])])),
            nearest: 0, when: None,
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
                effect: None, possible: false, nearest: Vec::new(),
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

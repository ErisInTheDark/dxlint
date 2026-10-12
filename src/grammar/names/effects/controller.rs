use std::cell::RefCell;
use std::rc::Rc;

use crate::fx_hash::FxHashMap;

use super::{EffectId, Execution};
use crate::grammar::names::question::{Known, Leaf, NodeRef, Target};
use crate::grammar::names::semantic::Semantic;

#[derive(Default)]
pub(crate) struct State {
    graphs: RefCell<FxHashMap<Leaf, Known<Rc<Execution>>>>,
}

pub struct Effects<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredEffect {
    pub site: Leaf,
    pub target: Option<NodeRef>,
    pub value: Option<Leaf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutedEffect {
    pub effect: EffectId,
    pub site: Leaf,
    pub target: Option<NodeRef>,
    pub value: Option<Leaf>,
}

impl Effects<'_> {
    pub fn entry_for(&self, at: Leaf) -> Known<Leaf> {
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        if tree.nodes.get(node.node as usize).is_none() { return Known::Unknown }
        let runtime = super::source::Runtime { semantic: self.semantic };
        let mut ancestry = Vec::new();
        let mut owner = Some(node.node);
        while let Some(id) = owner {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            ancestry.push(id);
            let owner_leaf = Leaf { target: Target::Node(NodeRef { node: id, ..node }), ..at };
            if let Some(kind) = self.semantic.tables.kinds.get(tree.node(id).matcher as usize) {
                let Some(entries) = runtime.enabled(owner_leaf, &kind.execution.entries).known() else { return Known::Unknown };
                let closest = entries.into_iter().flat_map(|entry| {
                    crate::grammar::names::follow(tree, id, &entry.item)
                }).filter_map(|entry| ancestry.iter().position(|&ancestor| ancestor == entry).map(|distance| (distance, entry)))
                    .min_by_key(|&(distance, _)| distance);
                if let Some((_, entry)) = closest {
                    return Known::Yes(Leaf { target: Target::Node(NodeRef { node: entry, ..node }), ..at });
                }
            }
            owner = tree.parent(id);
        }
        Known::Yes(Leaf { target: Target::Node(NodeRef { node: tree.root, ..node }), ..at })
    }

    pub fn execution(&self, entry: Leaf) -> Known<Rc<Execution>> {
        let state = &self.semantic.effects.graphs;
        if let Some(graph) = state.borrow().get(&entry) { return graph.clone() }
        state.borrow_mut().insert(entry, Known::Unknown);
        let source = super::source::Runtime { semantic: self.semantic };
        let graph = match super::build::execution(&source, entry, self.semantic.question) {
            Known::Yes(graph) => Known::Yes(Rc::new(graph)),
            Known::Unknown => Known::Unknown,
        };
        state.borrow_mut().insert(entry, graph.clone());
        graph
    }

    pub fn normal(&self, entry: Leaf) -> Known<bool> {
        match self.execution(entry) {
            Known::Yes(graph) => graph.normal(&self.semantic.question.budget),
            Known::Unknown => Known::Unknown,
        }
    }

    pub fn declared(&self, entry: Leaf, effect: EffectId) -> Known<Vec<DeclaredEffect>> {
        let Target::Node(node) = entry.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        if tree.nodes.get(node.node as usize).is_none() { return Known::Unknown }
        let runtime = super::source::Runtime { semantic: self.semantic };
        let mut pending = vec![node.node];
        let mut declared = Vec::new();
        let mut unique = crate::fx_hash::FxHashSet::default();
        while let Some(id) = pending.pop() {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let at = Leaf { target: Target::Node(NodeRef { node: id, ..node }), ..entry };
            if at != entry {
                match self.entry_for(at) {
                    Known::Yes(owner) if owner != entry => continue,
                    Known::Yes(_) => {}
                    Known::Unknown => return Known::Unknown,
                }
            }
            let Some(effects) = runtime.declarations(at, Some(effect)).known() else { return Known::Unknown };
            for emission in effects {
                let record = emission.declared;
                if unique.insert((record.site, record.target, record.value)) { declared.push(record) }
            }
            pending.extend(tree.node(id).children.iter().rev().map(|edge| edge.node));
        }
        Known::Yes(declared)
    }

    pub fn exits(&self, entry: Leaf) -> Known<Vec<ExecutedEffect>> {
        let Some(graph) = self.execution(entry).known() else { return Known::Unknown };
        let Some(exits) = graph.exits(&self.semantic.question.budget).known() else { return Known::Unknown };
        Known::Yes(exits.into_iter().map(|exit| ExecutedEffect {
            effect: exit.effect,
            site: Leaf { target: Target::Node(exit.site), ..entry },
            target: exit.target,
            value: exit.value.map(|value| Leaf { target: Target::Node(value), ..entry }),
        }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ast::Config;
    use crate::grammar::{Edge, Grammar, Node, Tree};
    use crate::grammar::names::{KindNames, Names, Resolved};
    use crate::grammar::names::effects::{Conditional, Flow};
    use crate::grammar::names::question::{FrameId, QueryContext, ScopeRef, Target, World};

    struct TestWorld {
        grammar: Grammar,
        tree: Tree,
        names: Resolved,
    }

    impl TestWorld {
        fn new() -> Self {
            Self {
                grammar: Grammar::build(&Config::default(), &mut Vec::new()),
                tree: Tree::from_native(vec![
                    Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 1, node: 1 }] },
                    Node { matcher: 1, start: 0, end: 0, children: vec![Edge { label: 2, node: 2 }] },
                    Node { matcher: 2, start: 0, end: 0, children: vec![Edge { label: 3, node: 3 }] },
                    Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
                ], 0),
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
        fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> {
            std::borrow::Cow::Borrowed(text)
        }
        fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Yes(None) }
        fn condition(&self, at: Leaf, _: u32, _: &Semantic<'_>) -> Known<bool> {
            Known::Yes(at.frame != FrameId::ROOT)
        }
        fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
            Known::Unknown
        }
        fn operand(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
            Known::Unknown
        }
    }

    fn leaf(node: u32) -> Leaf {
        Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT }
    }

    fn tables() -> Names {
        Names { kinds: vec![KindNames::default(); 4], ..Names::default() }
    }

    fn conditional<T>(item: T, when: Option<u32>) -> Conditional<T> {
        Conditional {
            item, when, item_id: 0,
            span: crate::config::source::Span::new(crate::config::source::FileId(0), 0, 1),
        }
    }

    #[test]
    fn nearest_separate_entry_does_not_run_in_its_declaration() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[1].execution.runs.push(conditional(Flow::Nothing, None));
        tables.kinds[1].execution.entries.push(conditional(vec![2], None));
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let effects = semantic.effects();
        assert_eq!(effects.entry_for(leaf(3)), Known::Yes(leaf(2)));
        assert_eq!(effects.entry_for(leaf(1)), Known::Yes(leaf(0)));
        let outer = effects.execution(leaf(0)).known().expect("outer execution must be known");
        assert_eq!(outer.reachable(NodeRef::here(3), &question.budget), Known::Yes(false));
        let body = effects.execution(leaf(2)).known().expect("separate execution must be known");
        assert_eq!(body.reachable(NodeRef::here(3), &question.budget), Known::Yes(true));
    }

    #[test]
    fn conditional_execution_memos_keep_instantiation_frames_distinct() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[0].execution.runs.push(conditional(Flow::Nothing, Some(0)));
        let question = QueryContext::default();
        let framed = Leaf {
            frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(NodeRef::here(1)), None),
            ..leaf(0)
        };
        let semantic = Semantic::new(&world, &tables, &question);
        for (entry, reachable) in [(leaf(0), true), (framed, false), (leaf(0), true)] {
            let graph = semantic.effects().execution(entry).known().expect("execution must be known");
            assert_eq!(graph.reachable(NodeRef::here(3), &question.budget), Known::Yes(reachable));
        }
    }

    #[test]
    fn active_conditional_shapes_report_a_defect_instead_of_choosing_one() {
        let world = TestWorld::new();
        let mut tables = tables();
        let first = conditional(Flow::Nothing, Some(0));
        let mut second = conditional(Flow::Children, Some(0));
        second.item_id = 1;
        tables.kinds[0].execution.runs.extend([first, second]);
        let question = QueryContext::default();
        let entry = Leaf {
            frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(NodeRef::here(1)), None),
            ..leaf(0)
        };
        let semantic = Semantic::new(&world, &tables, &question);
        assert!(matches!(semantic.effects().execution(entry), Known::Unknown));
        assert_eq!(semantic.defects().len(), 1);
    }

    #[test]
    fn declared_effects_include_dead_sites_but_exclude_separate_entries() {
        let mut world = TestWorld::new();
        world.tree = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 0, children: vec![
                Edge { label: 1, node: 1 }, Edge { label: 2, node: 2 }, Edge { label: 4, node: 4 },
            ] },
            Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 2, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 4, start: 0, end: 0, children: vec![Edge { label: 5, node: 5 }] },
            Node { matcher: 5, start: 0, end: 0, children: Vec::new() },
        ], 0);
        let mut tables = Names { kinds: vec![KindNames::default(); 6], ..Names::default() };
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["returned".into()], ..Default::default()
        });
        tables.controls = super::super::compile::controls(&tables.vocabulary);
        tables.kinds[4].execution.runs.push(conditional(Flow::Nothing, None));
        tables.kinds[4].execution.entries.push(conditional(vec![5], None));
        for kind in [1, 2, 5] {
            tables.kinds[kind].unwraps.push(crate::grammar::names::Unwrapping {
                effect: Some(0), ..Default::default()
            });
        }
        let question = QueryContext::default();
        let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(NodeRef::here(1)), None);
        let entry = Leaf { frame, ..leaf(0) };
        let at = |node| Leaf { frame, ..leaf(node) };
        let semantic = Semantic::new(&world, &tables, &question);
        let effects = semantic.effects();
        assert_eq!(effects.declared(entry, 0), Known::Yes(vec![
            DeclaredEffect { site: at(1), target: None, value: None },
            DeclaredEffect { site: at(2), target: None, value: None },
        ]));
        assert_eq!(effects.exits(entry), Known::Yes(vec![
            ExecutedEffect { effect: 0, site: at(1), target: None, value: None },
        ]));
        assert_eq!(effects.normal(entry), Known::Yes(false));
        assert_eq!(effects.declared(at(5), 0), Known::Yes(vec![
            DeclaredEffect { site: at(5), target: None, value: None },
        ]));
    }

    #[test]
    fn active_conditional_finalizers_report_a_defect() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[0].execution.runs.push(conditional(Flow::Field(vec![1]), None));
        let first = conditional(super::super::After { finalizer: vec![2], sources: vec![vec![1]] }, Some(0));
        let mut second = conditional(super::super::After { finalizer: vec![3], sources: vec![vec![1]] }, Some(0));
        second.item_id = 1;
        tables.kinds[0].execution.after.extend([first, second]);
        let question = QueryContext::default();
        let entry = Leaf {
            frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(NodeRef::here(1)), None),
            ..leaf(0)
        };
        let semantic = Semantic::new(&world, &tables, &question);
        assert!(matches!(semantic.effects().execution(entry), Known::Unknown));
        assert_eq!(semantic.defects().len(), 1);
    }

    #[test]
    fn nearest_effect_targets_use_field_limited_scope_membership() {
        let mut world = TestWorld::new();
        world.tree = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 1, node: 1 }] },
            Node { matcher: 1, start: 0, end: 0, children: vec![Edge { label: 2, node: 2 }, Edge { label: 3, node: 3 }] },
            Node { matcher: 2, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
        ], 0);
        let mut tables = tables();
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["returned".into()], scope_classes: vec!["function".into()], ..Default::default()
        });
        tables.controls = super::super::compile::controls(&tables.vocabulary);
        for (kind, within) in [(0, Vec::new()), (1, vec![2])] {
            tables.kinds[kind].scopes.push(crate::grammar::names::OpenScope { classes: 1, within, when: None });
        }
        tables.kinds[1].execution.runs.push(conditional(Flow::Field(vec![3]), None));
        tables.kinds[1].execution.entries.push(conditional(vec![2], None));
        for kind in [2, 3] {
            tables.kinds[kind].unwraps.push(crate::grammar::names::Unwrapping { effect: Some(0), nearest: 1, ..Default::default() });
        }
        world.names = tables.resolve(&world.tree, "", &std::borrow::Cow::Borrowed, &|_, _| true);
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.effects().declared(leaf(2), 0), Known::Yes(vec![
            DeclaredEffect { site: leaf(2), target: Some(NodeRef::here(1)), value: None },
        ]));
        assert_eq!(semantic.effects().declared(leaf(0), 0), Known::Yes(vec![
            DeclaredEffect { site: leaf(3), target: Some(NodeRef::here(0)), value: None },
        ]));
    }

    #[test]
    #[ignore = "RED until active items are selected before payloads"]
    fn conflicting_conditional_terminators_report_their_item() {
        for unavailable_payload in [false, true] {
            let world = TestWorld::new();
            let mut tables = tables();
            tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
                effects: vec!["returned".into(), "thrown".into()], ..Default::default()
            });
            tables.controls = super::super::compile::controls(&tables.vocabulary);
            for effect in [0, 1] {
                tables.kinds[0].unwraps.push(crate::grammar::names::Unwrapping {
                    effect: Some(effect), item: effect as u32, when: Some(0),
                    value: unavailable_payload.then(|| crate::grammar::names::Value::Path(vec![crate::grammar::names::ATTR_STEP])),
                    ..Default::default()
                });
            }
            let question = QueryContext::default();
            let entry = Leaf {
                frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(NodeRef::here(1)), None),
                ..leaf(0)
            };
            let semantic = Semantic::new(&world, &tables, &question);
            assert!(matches!(semantic.effects().execution(entry), Known::Unknown));
            assert_eq!(semantic.defects().len(), 1);
            assert_eq!(semantic.defects()[0].item, 1);
        }
    }
}

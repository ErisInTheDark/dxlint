use super::{Conditional, Flow};
use super::build::{Shape, Source};
use crate::fx_hash::FxHashSet;
use crate::grammar::names::question::{Known, Leaf, NodeRef, QueryContext, Target};
use crate::grammar::names::semantic::Semantic;

pub(super) struct Runtime<'a> {
    pub semantic: &'a Semantic<'a>,
}

impl Runtime<'_> {
    pub fn enabled<'a, T>(&self, at: Leaf, items: &'a [Conditional<T>]) -> Known<Vec<&'a Conditional<T>>> {
        let mut active = Vec::new();
        let mut seen = FxHashSet::default();
        for item in items {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let enabled = match item.when {
                Some(condition) => match self.semantic.world.condition(at, condition, self.semantic) {
                    Known::Yes(enabled) => enabled,
                    Known::Unknown => return Known::Unknown,
                },
                None => true,
            };
            if enabled && seen.insert(item.item_id) { active.push(item) }
        }
        Known::Yes(active)
    }
}

impl Source for Runtime<'_> {
    fn shape(&self, at: Leaf, _: &QueryContext) -> Known<Shape> {
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.semantic.tables.kinds.get(syntax.matcher as usize) else {
            return Known::Yes(Shape { flow: Flow::Children, effects: Vec::new(), catches: Vec::new(), after: Vec::new() });
        };
        if !kind.unwraps.is_empty() || !kind.effects.is_empty() { return Known::Unknown }
        let Some(runs) = self.enabled(at, &kind.execution.runs).known() else { return Known::Unknown };
        if runs.len() > 1 {
            self.semantic.defect(runs[1].item_id, at, "more than one active execution shape");
            return Known::Unknown;
        }
        let flow = runs.first().map_or(Flow::Children, |run| run.item.clone());
        let Some(catches) = self.enabled(at, &kind.execution.catches).known() else { return Known::Unknown };
        for (index, catch) in catches.iter().enumerate() {
            if catches[..index].iter().any(|previous| previous.item.source == catch.item.source
                && previous.item.effects.iter().any(|effect| catch.item.effects.contains(effect))) {
                self.semantic.defect(catch.item_id, at, "active handlers catch the same effect");
                return Known::Unknown;
            }
        }
        let Some(after) = self.enabled(at, &kind.execution.after).known() else { return Known::Unknown };
        if let Some(mut sources) = flow.sources() {
            sources.extend(catches.iter().map(|catch| catch.item.handler.clone()));
            for finalizer in &after {
                if sources.iter().any(|source| !finalizer.item.sources.contains(source)) {
                    self.semantic.defect(finalizer.item_id, at, "`runs after` must cover the whole active execution and its handlers");
                    return Known::Unknown;
                }
            }
        }
        Known::Yes(Shape {
            flow,
            effects: Vec::new(),
            catches: catches.into_iter().map(|item| item.item.clone()).collect(),
            after: after.into_iter().map(|item| item.item.clone()).collect(),
        })
    }

    fn children(&self, at: Leaf, _: &QueryContext) -> Known<Vec<Leaf>> {
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        Known::Yes(syntax.children.iter().map(|edge| Leaf {
            target: Target::Node(NodeRef { node: edge.node, ..node }), ..at
        }).collect())
    }

    fn path(&self, at: Leaf, path: &[u16], _: &QueryContext) -> Known<Vec<Leaf>> {
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        if tree.nodes.get(node.node as usize).is_none() { return Known::Unknown }
        Known::Yes(crate::grammar::names::follow(tree, node.node, path).into_iter().map(|id| Leaf {
            target: Target::Node(NodeRef { node: id, ..node }), ..at
        }).collect())
    }

    fn truth(&self, at: Leaf, attribute: u16, _: &QueryContext) -> Known<Option<bool>> {
        match self.semantic.world.attribute(at, attribute, self.semantic) {
            Known::Yes(crate::expr::Value::Bool(value)) => Known::Yes(Some(value)),
            Known::Yes(_) => Known::Yes(None),
            Known::Unknown => Known::Unknown,
        }
    }
}

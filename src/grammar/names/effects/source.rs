use super::{Conditional, DeclaredEffect, EffectId, EffectRole, Event, EventKind, Flow, Phase};
use super::build::{Control, Effect, Shape, Source};
use crate::fx_hash::FxHashSet;
use crate::grammar::names::question::{Known, Leaf, NodeRef, QueryContext, Target};
use crate::grammar::names::semantic::Semantic;

pub(super) struct Runtime<'a> {
    pub semantic: &'a Semantic<'a>,
}

pub(super) struct Emission {
    pub effect: EffectId,
    pub possible: bool,
    pub declared: DeclaredEffect,
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

    pub fn declarations(&self, at: Leaf, effect: Option<EffectId>) -> Known<Vec<Emission>> {
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.semantic.tables.kinds.get(syntax.matcher as usize) else { return Known::Yes(Vec::new()) };
        if kind.effects.iter().any(|item| self.role(item.effect) == Some(EffectRole::Event)
            && effect.is_none_or(|effect| item.effect == effect)) { return Known::Unknown }
        let mut active = Vec::new();
        let mut seen = FxHashSet::default();
        for unwrap in &kind.unwraps {
            let Some(id) = unwrap.effect else { continue };
            if self.role(id) == Some(EffectRole::Proof) { continue }
            if effect.is_some_and(|effect| id != effect) { continue }
            if !self.semantic.question.budget.step() { return Known::Unknown }
            if let Some(condition) = unwrap.when {
                match self.semantic.world.condition(at, condition, self.semantic) {
                    Known::Yes(false) => continue,
                    Known::Yes(true) => {}
                    Known::Unknown => return Known::Unknown,
                }
            }
            if !seen.insert(unwrap.item) { continue }
            active.push(unwrap);
        }
        let terminators: Vec<_> = active.iter().filter(|item| !item.possible
            && item.effect.and_then(|effect| self.control(effect)).is_some()).collect();
        if terminators.len() > 1 {
            self.semantic.defect(terminators[1].item, at, "more than one active terminating effect");
            return Known::Unknown;
        }
        let mut declared = Vec::new();
        for unwrap in active {
            let Some(id) = unwrap.effect else { continue };
            let Some(target) = self.nearest(at, unwrap.nearest).known() else { return Known::Unknown };
            let value = match &unwrap.value {
                Some(value) => {
                    let Some(_nested) = self.semantic.question.budget.nest() else { return Known::Unknown };
                    let Some(sites) = self.semantic.values().sites(at, value).known() else { return Known::Unknown };
                    if sites.len() > 1 { return Known::Unknown }
                    sites.first().copied()
                }
                None => None,
            };
            if value.is_some() && matches!(self.control(id), Some(Control::Broken | Control::Continued)) {
                return Known::Unknown;
            }
            declared.push(Emission {
                effect: id, possible: unwrap.possible, declared: DeclaredEffect { site: at, target, value },
            });
        }
        Known::Yes(declared)
    }

    fn control(&self, effect: EffectId) -> Option<Control> {
        self.role(effect).and_then(EffectRole::control)
    }

    fn role(&self, effect: EffectId) -> Option<EffectRole> {
        self.semantic.tables.effect_roles.get(effect as usize).copied()
    }

    fn nearest(&self, at: Leaf, classes: u64) -> Known<Option<NodeRef>> {
        if classes == 0 { return Known::Yes(None) }
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
        let mut scope = names.scope_of(node.node);
        while let Some(id) = scope {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let Some(info) = names.scopes.get(id as usize) else { return Known::Unknown };
            if info.classes & classes != 0 {
                return Known::Yes(Some(NodeRef { node: info.node, ..node }));
            }
            scope = info.parent;
        }
        Known::Yes(None)
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
        if kind.effects.iter().any(|item| self.role(item.effect) == Some(EffectRole::Event)) { return Known::Unknown }
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
        if after.len() > 1 {
            self.semantic.defect(after[1].item_id, at, "more than one active finalizer");
            return Known::Unknown;
        }
        if let Some(mut sources) = flow.sources() {
            sources.extend(catches.iter().map(|catch| catch.item.handler.clone()));
            for finalizer in &after {
                if sources.iter().any(|source| !finalizer.item.sources.contains(source)) {
                    self.semantic.defect(finalizer.item_id, at, "`runs after` must cover the whole active execution and its handlers");
                    return Known::Unknown;
                }
            }
        }
        let Some(declared) = self.declarations(at, None).known() else { return Known::Unknown };
        let mut effects = Vec::new();
        for emission in declared {
            let value = match emission.declared.value {
                Some(value) if value.frame == at.frame => match value.target {
                    Target::Node(value) => Some(value),
                    Target::Scope(_) => return Known::Unknown,
                },
                Some(_) => return Known::Unknown,
                None => None,
            };
            effects.push(Effect {
                event: Event { kind: EventKind::Effect(emission.effect), site: node, target: None, value, phase: Phase::Leave },
                control: self.control(emission.effect),
                target: emission.declared.target,
                possible: emission.possible,
            });
        }
        Known::Yes(Shape {
            flow,
            effects,
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

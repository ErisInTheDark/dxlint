use super::{Conditional, DeclaredEffect, EffectId, EffectRole, Event, EventKind, Flow, Phase};
use super::build::{Control, Effect, Failure, Shape, Source};
use crate::fx_hash::FxHashSet;
use crate::grammar::names::question::{Known, Leaf, NodeRef, QueryContext, Target};
use crate::grammar::names::semantic::Semantic;

pub(super) struct Runtime<'a> {
    pub semantic: &'a Semantic<'a>,
    failure: std::cell::Cell<Option<Failure>>,
    /// Whether shapes carry read events (only read-use questions need them;
    /// they are most of a body's events).
    reads: bool,
    /// Every condition the build decided, with its answer: what makes a
    /// body's graph differ between frames.
    decided: std::cell::RefCell<Vec<(Leaf, u32, bool)>>,
}

pub(super) struct Emission {
    pub effect: EffectId,
    pub item_id: Option<u32>,
    pub declared: DeclaredEffect,
}

impl Runtime<'_> {
    pub fn new<'a>(semantic: &'a Semantic<'a>) -> Runtime<'a> {
        Runtime { semantic, failure: std::cell::Cell::new(None), reads: true, decided: Default::default() }
    }

    /// The conditions decided so far, in the order asked.
    pub fn decided(&self) -> Vec<(Leaf, u32, bool)> {
        self.decided.take()
    }

    /// A runtime whose shapes leave read events out.
    pub fn without_reads<'a>(semantic: &'a Semantic<'a>) -> Runtime<'a> {
        Runtime { reads: false, ..Runtime::new(semantic) }
    }

    pub fn failure(&self) -> Failure {
        if self.semantic.question.budget.limited() { Failure::Budget }
        else { self.failure.get().unwrap_or(Failure::Shape) }
    }

    /// The build ran out of its own allowance (its asker's budget, checked
    /// by [`Runtime::failure`], is back as it was).
    pub fn ran_out(&self) {
        self.failure.set(Some(Failure::Budget));
    }

    fn checked<T>(&self, result: Known<T>, failure: Failure) -> Known<T> {
        if matches!(result, Known::Unknown) && self.failure.get().is_none() {
            self.failure.set(Some(failure));
        }
        result
    }

    fn condition(&self, at: Leaf, condition: u32) -> Known<bool> {
        let answer = self.checked(self.semantic.world.condition(at, condition, self.semantic), Failure::Condition);
        if let Known::Yes(holds) = answer { self.decided.borrow_mut().push((at, condition, holds)) }
        answer
    }

    pub(super) fn binding(&self, at: Leaf, node: NodeRef) -> Known<Option<NodeRef>> {
        let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
        let binding = names.declaration.get(&node.node).copied()
            .or_else(|| names.declared.contains_key(&node.node).then_some(node.node));
        let Some(mut binding) = binding else { return Known::Yes(None) };
        while let Some(&merged) = names.merged.get(&binding) {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            if merged == binding { break }
            binding = merged;
        }
        let Some(binding) = self.semantic.leaf(Leaf {
            target: Target::Node(NodeRef { node: binding, ..node }), ..at
        }).known() else { return Known::Unknown };
        let Target::Node(binding) = binding.target else { return Known::Unknown };
        Known::Yes(Some(binding))
    }

    pub fn read(&self, at: Leaf, emission: &Emission) -> Known<super::Target> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Some(value) = emission.declared.value else { return Known::Unknown };
        let Some(value) = self.semantic.leaf(value).known() else { return Known::Unknown };
        let (Target::Node(owner), Target::Node(node)) = (at.target, value.target) else { return Known::Unknown };
        if value.frame != at.frame || node.file != owner.file { return Known::Unknown }
        let Some(binding) = self.binding(at, node).known() else { return Known::Unknown };
        Known::Yes(super::Target { value: node, binding })
    }

    pub fn enabled<'a, T>(&self, at: Leaf, items: &'a [Conditional<T>]) -> Known<Vec<&'a Conditional<T>>> {
        // Most kinds have none of a given item: answer before any lookup.
        if items.is_empty() { return Known::Yes(Vec::new()) }
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let mut active = Vec::new();
        let mut seen = FxHashSet::default();
        for item in items {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let enabled = match item.when {
                Some(condition) => match self.condition(at, condition) {
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
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.semantic.tables.kinds.get(syntax.matcher as usize) else { return Known::Yes(Vec::new()) };
        if !kind.unwraps.iter().any(|unwrap| unwrap.effect.is_some()) { return Known::Yes(Vec::new()) }
        let mut active = Vec::new();
        let mut seen = FxHashSet::default();
        for unwrap in &kind.unwraps {
            let Some(id) = unwrap.effect else { continue };
            if matches!(self.role(id), Some(EffectRole::Proof | EffectRole::Event { writes: true })) { continue }
            if !self.reads && self.role(id) == Some(EffectRole::Read) { continue }
            if effect.is_some_and(|effect| id != effect) { continue }
            if !self.semantic.question.budget.step() { return Known::Unknown }
            if let Some(condition) = unwrap.when {
                match self.condition(at, condition) {
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
            self.failure.set(Some(Failure::Shape));
            self.semantic.defect(terminators[1].item, at, "more than one active terminating effect");
            return Known::Unknown;
        }
        let mut declared = Vec::new();
        for unwrap in active {
            let Some(id) = unwrap.effect else { continue };
            let value = match &unwrap.value {
                Some(value) => {
                    let Some(_nested) = self.semantic.question.budget.nest() else { return Known::Unknown };
                    let Some(sites) = self.semantic.values().sites(at, value).known() else { return Known::Unknown };
                    if sites.len() > 1 { return Known::Unknown }
                    sites.first().copied()
                }
                None => None,
            };
            let target = match (self.control(id), value) {
                (Some(control @ (Control::Broken | Control::Continued)), Some(label)) => {
                    let Some(target) = self.label_target(at, label, control, unwrap.nearest).known() else { return Known::Unknown };
                    target
                }
                _ => {
                    let Some(target) = self.nearest(at, unwrap.nearest).known() else { return Known::Unknown };
                    target
                }
            };
            declared.push(Emission {
                effect: id, item_id: Some(unwrap.item), declared: DeclaredEffect { site: at, target, value, possible: unwrap.possible },
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

    pub fn writes(&self, at: Leaf, effect: Option<EffectId>) -> Known<Vec<(EffectId, Option<u32>, Effect)>> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.semantic.tables.kinds.get(syntax.matcher as usize) else { return Known::Yes(Vec::new()) };
        if kind.effects.is_empty() { return Known::Yes(Vec::new()) }
        let mut writes = Vec::new();
        for item in &kind.effects {
            if self.role(item.effect) != Some(EffectRole::Event { writes: true })
                || effect.is_some_and(|effect| item.effect != effect) { continue }
            if !self.semantic.question.budget.step() { return Known::Unknown }
            if let Some(condition) = item.when {
                match self.condition(at, condition) {
                    Known::Yes(false) => continue,
                    Known::Yes(true) => {}
                    Known::Unknown => return Known::Unknown,
                }
            }
            let Some(projected) = self.semantic.follow(at, &item.path).known() else { return Known::Unknown };
            for target in projected {
                let Target::Node(target) = target.target else { return Known::Unknown };
                let Some(targets) = self.semantic.tables.targets_of(tree, target.node, &|target, condition| {
                    if !self.semantic.question.budget.step() { return Known::Unknown }
                    let Some(target) = self.semantic.leaf(Leaf {
                        target: Target::Node(NodeRef { node: target, ..node }), ..at
                    }).known() else { return Known::Unknown };
                    self.condition(target, condition)
                }).known() else { return Known::Unknown };
                for target in targets {
                    if !self.semantic.question.budget.step() { return Known::Unknown }
                    let Some(target) = self.semantic.leaf(Leaf { target: Target::Node(NodeRef { node: target, ..node }), ..at }).known() else { return Known::Unknown };
                    let Target::Node(value) = target.target else { return Known::Unknown };
                    let Some(binding) = self.binding(at, value).known() else { return Known::Unknown };
                    writes.push((item.effect, item.item_id, Effect {
                        event: Event {
                            kind: EventKind::Write, site: node,
                            target: Some(super::Target { value, binding }),
                            value: None,
                            written: item.value.as_ref().map(|recipe| super::Written { owner: at, recipe: recipe.clone() }),
                            phase: Phase::Leave,
                        },
                        control: None, target: None, possible: item.possible,
                    }));
                }
            }
        }
        Known::Yes(writes)
    }

    fn nearest(&self, at: Leaf, classes: u64) -> Known<Option<NodeRef>> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        if classes == 0 { return Known::Yes(None) }
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
        let mut scope = names.scope_of(node.node);
        while let Some(id) = scope {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let Some(info) = names.scopes.get(id as usize) else { return Known::Unknown };
            if info.classes & classes != 0 {
                let Some(owner) = self.semantic.leaf(Leaf { target: Target::Node(NodeRef { node: info.node, ..node }), ..at }).known() else { return Known::Unknown };
                let Target::Node(owner) = owner.target else { return Known::Unknown };
                return Known::Yes(Some(owner));
            }
            scope = info.parent;
        }
        Known::Yes(None)
    }

    fn label_target(&self, at: Leaf, label: Leaf, control: Control, classes: u64) -> Known<Option<NodeRef>> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Some(label) = self.semantic.leaf(label).known() else { return Known::Unknown };
        let (Target::Node(node), Target::Node(label_node)) = (at.target, label.target) else { return Known::Unknown };
        if label.frame != at.frame || label_node.file != node.file { return Known::Unknown }
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(text) = self.semantic.world.text(node.file) else { return Known::Unknown };
        let Some(label_syntax) = tree.nodes.get(label_node.node as usize) else { return Known::Unknown };
        let Some(label_text) = text.get(label_syntax.start as usize..label_syntax.end as usize) else { return Known::Unknown };
        let key = self.semantic.world.name_key(node.file, label_text);
        let mut owner = tree.parent(node.node);
        while let Some(id) = owner {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let site = Leaf { target: Target::Node(NodeRef { node: id, ..node }), ..at };
            let Some(site) = self.semantic.leaf(site).known() else { return Known::Unknown };
            if site.target != Target::Node(NodeRef { node: id, ..node }) { owner = tree.parent(id); continue }
            if let Some(kind) = self.semantic.tables.kinds.get(tree.node(id).matcher as usize) {
                let Some(labels) = self.enabled(site, &kind.execution.labels).known() else { return Known::Unknown };
                let mut matched = false;
                for label in labels {
                    let Some(targets) = self.semantic.follow(site, &label.item).known() else { return Known::Unknown };
                    for target in targets {
                        let Target::Node(target) = target.target else { return Known::Unknown };
                        let syntax = tree.node(target.node);
                        matched |= text.get(syntax.start as usize..syntax.end as usize)
                            .is_some_and(|label| self.semantic.world.name_key(node.file, label) == key);
                    }
                }
                if matched {
                    if control == Control::Broken { return Known::Yes(Some(NodeRef { node: id, ..node })) }
                    let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
                    let mut target = id;
                    loop {
                        if !self.semantic.question.budget.step() { return Known::Unknown }
                        let target_leaf = Leaf { target: Target::Node(NodeRef { node: target, ..node }), ..at };
                        let Some(target_leaf) = self.semantic.leaf(target_leaf).known() else { return Known::Unknown };
                        let Target::Node(target_node) = target_leaf.target else { return Known::Unknown };
                        target = target_node.node;
                        let Some(kind) = self.semantic.tables.kinds.get(tree.node(target).matcher as usize) else { return Known::Unknown };
                        let Some(labels) = self.enabled(target_leaf, &kind.execution.labels).known() else { return Known::Unknown };
                        if labels.is_empty() {
                            return if names.scopes.iter().any(|scope| scope.node == target && scope.classes & classes != 0) {
                                Known::Yes(Some(NodeRef { node: target, ..node }))
                            } else { Known::Unknown };
                        }
                        let Some(runs) = self.enabled(target_leaf, &kind.execution.runs).known() else { return Known::Unknown };
                        let [run] = runs.as_slice() else { return Known::Unknown };
                        let Flow::Field(path) = &run.item else { return Known::Unknown };
                        let Some(targets) = self.semantic.follow(target_leaf, path).known() else { return Known::Unknown };
                        let [next] = targets.as_slice() else { return Known::Unknown };
                        let Target::Node(next) = next.target else { return Known::Unknown };
                        target = next.node;
                    }
                }
            }
            owner = tree.parent(id);
        }
        Known::Unknown
    }
}

impl Runtime<'_> {
    fn shape(&self, at: Leaf) -> Known<Shape> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.semantic.tables.kinds.get(syntax.matcher as usize) else {
            return Known::Yes(Shape { flow: Flow::Children, effects: Vec::new(), catches: Vec::new(), after: Vec::new() });
        };
        // Profile-only: split builder steps by the part of a shape they decide.
        let site = |name| self.semantic.world.profile().map(|_| crate::engine::StepSite::enter(name));
        let flow_site = site("effects.build.flow");
        let Some(runs) = self.enabled(at, &kind.execution.runs).known() else { return Known::Unknown };
        if runs.len() > 1 {
            self.failure.set(Some(Failure::Shape));
            self.semantic.defect(runs[1].item_id, at, "more than one active execution shape");
            return Known::Unknown;
        }
        let flow = runs.first().map_or(Flow::Children, |run| run.item.clone());
        let Some(catches) = self.enabled(at, &kind.execution.catches).known() else { return Known::Unknown };
        for (index, catch) in catches.iter().enumerate() {
            if catches[..index].iter().any(|previous| previous.item.source == catch.item.source
                && previous.item.effects.iter().any(|effect| catch.item.effects.contains(effect))) {
                self.failure.set(Some(Failure::Shape));
                self.semantic.defect(catch.item_id, at, "active handlers catch the same effect");
                return Known::Unknown;
            }
        }
        let Some(after) = self.enabled(at, &kind.execution.after).known() else { return Known::Unknown };
        if after.len() > 1 {
            self.failure.set(Some(Failure::Shape));
            self.semantic.defect(after[1].item_id, at, "more than one active finalizer");
            return Known::Unknown;
        }
        if let Some(mut sources) = flow.sources() {
            sources.extend(catches.iter().map(|catch| catch.item.handler.clone()));
            for finalizer in &after {
                if sources.iter().any(|source| !finalizer.item.sources.contains(source)) {
                    self.failure.set(Some(Failure::Shape));
                    self.semantic.defect(finalizer.item_id, at, "`runs after` must cover the whole active execution and its handlers");
                    return Known::Unknown;
                }
            }
        }
        drop(flow_site);
        let declared_site = site("effects.build.declarations");
        let Some(declared) = self.declarations(at, None).known() else { return Known::Unknown };
        drop(declared_site);
        let writes_site = site("effects.build.writes");
        let Some(writes) = self.writes(at, None).known() else { return Known::Unknown };
        drop(writes_site);
        let mut effects: Vec<_> = writes.into_iter().map(|(_, item_id, effect)| (item_id, effect)).collect();
        for emission in declared {
            if self.role(emission.effect) == Some(EffectRole::Read) {
                let _reads_site = site("effects.build.reads");
                let Some(target) = self.read(at, &emission).known() else { return Known::Unknown };
                effects.push((emission.item_id, Effect {
                    event: Event { kind: EventKind::Read, site: node, target: Some(target), value: None, written: None, phase: Phase::Enter },
                    control: None, target: None, possible: emission.declared.possible,
                }));
                continue;
            }
            let value = match emission.declared.value {
                Some(value) if value.frame == at.frame => match value.target {
                    Target::Node(value) => Some(value),
                    _ => return Known::Unknown,
                },
                Some(_) => return Known::Unknown,
                None => None,
            };
            effects.push((emission.item_id, Effect {
                event: Event { kind: EventKind::Effect(emission.effect), site: node, target: None, value, written: None, phase: Phase::Leave },
                control: self.control(emission.effect),
                target: emission.declared.target,
                possible: emission.declared.possible,
            }));
        }
        effects.sort_by_key(|(item_id, _)| *item_id);
        Known::Yes(Shape {
            flow,
            effects: effects.into_iter().map(|(_, effect)| effect).collect(),
            catches: catches.into_iter().map(|item| item.item.clone()).collect(),
            after: after.into_iter().map(|item| item.item.clone()).collect(),
        })
    }
    fn children(&self, at: Leaf) -> Known<Vec<Leaf>> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        if tree.nodes.get(node.node as usize).is_none() { return Known::Unknown }
        tree.children(node.node).iter().map(|edge| self.semantic.leaf(Leaf {
            target: Target::Node(NodeRef { node: edge.node, ..node }), ..at
        }).known()).collect::<Option<Vec<_>>>().map_or(Known::Unknown, Known::Yes)
    }

}

impl Source for Runtime<'_> {
    fn leaf(&self, at: Leaf, _: &QueryContext) -> Known<Leaf> {
        self.checked(self.semantic.leaf(at), Failure::Dependency)
    }

    fn shape(&self, at: Leaf, _: &QueryContext) -> Known<Shape> {
        self.checked(self.shape(at), Failure::Dependency)
    }

    fn children(&self, at: Leaf, _: &QueryContext) -> Known<Vec<Leaf>> {
        self.checked(self.children(at), Failure::Dependency)
    }

    fn path(&self, at: Leaf, path: &[u16], _: &QueryContext) -> Known<Vec<Leaf>> {
        self.checked(self.semantic.follow(at, path), Failure::Dependency)
    }

    fn truth(&self, at: Leaf, attribute: u16, _: &QueryContext) -> Known<Option<bool>> {
        let Some(at) = self.checked(self.semantic.leaf(at), Failure::Dependency).known() else { return Known::Unknown };
        match self.checked(self.semantic.world.attribute(at, attribute, self.semantic), Failure::Dependency) {
            Known::Yes(crate::expr::Value::Bool(value)) => Known::Yes(Some(value)),
            Known::Yes(_) => Known::Yes(None),
            Known::Unknown => Known::Unknown,
        }
    }
}

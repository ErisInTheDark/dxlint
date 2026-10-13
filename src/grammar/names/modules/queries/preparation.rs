use super::{Queries, leaf, nodes};
use super::super::compile::{Action, Conditional};
use super::super::index::PublicationKind;
use crate::expr::Value;
use crate::grammar::names::question::{Known, NodeRef, ScopeRef};
use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ClassAnswers {
    pub known: u64,
    pub present: u64,
}

impl ClassAnswers {
    fn insert(&mut self, class: u64, answer: Known<bool>) {
        if let Known::Yes(present) = answer {
            self.known |= class;
            if present { self.present |= class; }
        }
    }
}

#[derive(Clone)]
pub(super) struct PreparedItem {
    pub(super) node: NodeRef,
    pub(super) item: Conditional,
    pub(super) target: Option<Value<'static>>,
}

struct BatchPlan {
    node: u32,
    item: Conditional,
    prepared: OnceCell<Option<PreparedItem>>,
}

pub(super) struct Batch {
    revision: u64,
    plans: RefCell<BTreeMap<(ScopeRef, Box<str>, PublicationKind, bool), Rc<Vec<BatchPlan>>>>,
    conditions: RefCell<BTreeMap<(NodeRef, u32), bool>>,
    operands: RefCell<BTreeMap<(NodeRef, u32), Value<'static>>>,
    globals: RefCell<BTreeMap<u32, Vec<ScopeRef>>>,
}

impl Batch {
    fn new(revision: u64) -> Self {
        Self {
            revision, plans: Default::default(), conditions: Default::default(),
            operands: Default::default(), globals: Default::default(),
        }
    }

    fn finish(&self, query: &Queries<'_>, answers: ClassAnswers) -> ClassAnswers {
        if query.semantic.world.revision() != self.revision || query.semantic.question.budget.limited() {
            ClassAnswers::default()
        } else { answers }
    }
}

impl Queries<'_> {
    pub fn binding_classes(&self, at: NodeRef, name: &str, classes: u64) -> ClassAnswers {
        let batch = Batch::new(self.sync_revision());
        if classes.is_power_of_two() {
            let mut answers = ClassAnswers::default();
            answers.insert(classes, self.binding_has_class(at, name, classes));
            return batch.finish(self, answers);
        }
        let query = Queries { semantic: self.semantic, batch: Some(&batch) };
        let collect = || -> Result<ClassAnswers, ()> {
            query.spend()?;
            let world = query.semantic.world;
            let names = world.names(at.file).ok_or(())?;
            let tree = world.tree(at.file).ok_or(())?;
            let source = world.text(at.file).ok_or(())?;
            if !names.failures.is_empty() { return Err(()) }
            let key = world.name_key(at.file, name);
            let mut remaining = classes;
            let mut answers = ClassAnswers::default();
            while let Some(declaration) = names.visible(tree, source, &|text| world.name_key(at.file, text), at.node, &key, remaining) {
                let (_, _, declared, _) = names.declared.get(&declaration).ok_or(())?;
                let matched = remaining & declared;
                if matched == 0 { return Err(()) }
                for class in (0..64).map(|bit| 1u64 << bit).filter(|class| matched & class != 0) {
                    let answer = match query.definition(NodeRef { file: at.file, node: declaration }, class) {
                        Known::Yes(Some(targets)) => Known::Yes(!targets.is_empty()),
                        Known::Yes(None) => Known::Yes(true),
                        Known::Unknown => Known::Unknown,
                    };
                    answers.insert(class, answer);
                }
                remaining &= !matched;
                if remaining == 0 { break }
            }
            for class in (0..64).map(|bit| 1u64 << bit).filter(|class| remaining & class != 0) {
                answers.insert(class, match query.global(at.file, &key, class) {
                    Known::Yes(targets) => Known::Yes(!targets.is_empty()),
                    Known::Unknown => Known::Unknown,
                });
            }
            Ok(answers)
        };
        batch.finish(&query, collect().unwrap_or_default())
    }

    pub fn export_classes(&self, scope: ScopeRef, name: &str, classes: u64) -> ClassAnswers {
        let batch = Batch::new(self.sync_revision());
        if classes.is_power_of_two() {
            let mut answers = ClassAnswers::default();
            answers.insert(classes, self.export_has_class(scope, name, classes));
            return batch.finish(self, answers);
        }
        let query = Queries { semantic: self.semantic, batch: Some(&batch) };
        let mut answers = ClassAnswers::default();
        for class in (0..64).map(|bit| 1u64 << bit).filter(|class| classes & class != 0) {
            answers.insert(class, query.export_has_class(scope, name, class));
        }
        batch.finish(&query, answers)
    }

    pub(super) fn spend(&self) -> Result<(), ()> {
        if self.batch.is_some_and(|batch| batch.revision != self.semantic.world.revision()) { return Err(()) }
        self.semantic.question.budget.step().then_some(()).ok_or(())
    }

    pub(super) fn holds(&self, node: NodeRef, condition: Option<u32>) -> Result<bool, ()> {
        if let (Some(batch), Some(condition)) = (self.batch, condition) {
            if batch.revision != self.semantic.world.revision() { return Err(()) }
            if let Some(&held) = batch.conditions.borrow().get(&(node, condition)) { return Ok(held) }
        }
        self.spend()?;
        let held = condition.map_or(Ok(true), |condition| self.profiled("modules.condition", || {
            self.semantic.world.condition(leaf(node), condition, self.semantic)
        }).known().ok_or(()))?;
        if let (Some(batch), Some(condition)) = (self.batch, condition) {
            if batch.revision != self.semantic.world.revision() || self.semantic.question.budget.limited() { return Err(()) }
            batch.conditions.borrow_mut().insert((node, condition), held);
        }
        Ok(held)
    }

    pub(super) fn operand(&self, node: NodeRef, operand: u32) -> Result<Value<'static>, ()> {
        if let Some(batch) = self.batch {
            if batch.revision != self.semantic.world.revision() { return Err(()) }
            if let Some(value) = batch.operands.borrow().get(&(node, operand)) { return Ok(value.clone()) }
        }
        self.spend()?;
        let value = self.profiled("modules.operand", || self.semantic.world.operand(leaf(node), operand, self.semantic)).known().ok_or(())?;
        if let Some(batch) = self.batch {
            if batch.revision != self.semantic.world.revision() || self.semantic.question.budget.limited() { return Err(()) }
            batch.operands.borrow_mut().insert((node, operand), value.clone());
        }
        Ok(value)
    }

    pub(super) fn global_scopes(&self, importer: u32) -> Result<Vec<ScopeRef>, ()> {
        if let Some(batch) = self.batch {
            if batch.revision != self.semantic.world.revision() { return Err(()) }
            if let Some(scopes) = batch.globals.borrow().get(&importer) { return Ok(scopes.clone()) }
        }
        let scopes = self.profiled("modules.roots", || self.semantic.world.global_scopes(importer, self.semantic)).known().ok_or(())?;
        if let Some(batch) = self.batch {
            if batch.revision != self.semantic.world.revision() || self.semantic.question.budget.limited() { return Err(()) }
            batch.globals.borrow_mut().insert(importer, scopes.clone());
        }
        Ok(scopes)
    }

    pub(super) fn target_value(&self, node: NodeRef, operand: u32, prepared: Option<Value<'static>>) -> Result<Value<'static>, ()> {
        prepared.map(Ok).unwrap_or_else(|| self.operand(node, operand))
    }

    pub(super) fn indexed_items(&self, scope: ScopeRef, name: Option<(&str, u64)>, publication: PublicationKind, first_globals: bool) -> Result<Vec<PreparedItem>, ()> {
        let plans = if let (Some(batch), Some((name, _))) = (self.batch, name) {
            if batch.revision != self.semantic.world.revision() { return Err(()) }
            let key = (scope, name.into(), publication, first_globals);
            let cached = batch.plans.borrow().get(&key).cloned();
            match cached {
                Some(plans) => plans,
                None => {
                    let plans = Rc::new(self.indexed_plans(scope, Some((name, u64::MAX)), publication, first_globals)?);
                    batch.plans.borrow_mut().insert(key, plans.clone());
                    plans
                }
            }
        } else {
            Rc::new(self.indexed_plans(scope, name, publication, first_globals)?)
        };
        let mut items = Vec::new();
        for plan in &*plans {
            let classes = match plan.item.action {
                Action::Exports { classes, .. } | Action::Globals { classes, .. } => classes,
                _ => continue,
            };
            if name.is_some_and(|(_, class)| classes != 0 && classes & class == 0) { continue }
            let prepared = match plan.prepared.get() {
                Some(prepared) => prepared.clone(),
                None => {
                    let prepared = self.profiled("modules.prepare", || self.prepare_item(scope, plan))?;
                    let _ = plan.prepared.set(prepared.clone());
                    prepared
                }
            };
            if let Some(prepared) = prepared { items.push(prepared); }
        }
        Ok(items)
    }

    fn indexed_plans(&self, scope: ScopeRef, name: Option<(&str, u64)>, publication: PublicationKind, first_globals: bool) -> Result<Vec<BatchPlan>, ()> {
        let world = self.semantic.world;
        let index = world.module_index(scope.file, self.semantic.tables).ok_or(())?;
        if name.is_some_and(|(name, class)| !index.mentions(name, class, publication)) { return Ok(Vec::new()) }
        let names = world.names(scope.file).ok_or(())?;
        let owner = names.scopes.get(scope.scope as usize).ok_or(())?.node;
        let plans = || index.plans(owner, name, publication).into_iter().filter(|plan| {
            !first_globals || matches!(plan.item.action, Action::Globals { first: true, .. })
        }).map(|plan| BatchPlan { node: plan.node, item: plan.item.clone(), prepared: OnceCell::new() }).collect::<Vec<_>>();
        Ok(match world.profile() {
            Some(profile) => profile.time_module_plans(plans),
            None => plans(),
        })
    }

    fn prepare_item(&self, scope: ScopeRef, plan: &BatchPlan) -> Result<Option<PreparedItem>, ()> {
        self.spend()?;
        let names = self.semantic.world.names(scope.file).ok_or(())?;
        let Some(mut containing) = names.scope_of(plan.node) else { return Ok(None) };
        let node = NodeRef { file: scope.file, node: plan.node };
        let mut item = plan.item.clone();
        let mut target = None;
        let local = match item.action {
            Action::Exports { target, module: None, .. } | Action::Globals { target, module: None, .. } => target,
            _ => None,
        };
        let source_scope = &names.scopes[containing as usize];
        if source_scope.node == plan.node && source_scope.parent.is_some()
            && self.is_publication(ScopeRef { file: scope.file, scope: containing })? {
            if let Some(operand) = local {
                if !self.holds(node, item.when)? { return Ok(None) }
                item.when = None;
                let value = self.operand(node, operand)?;
                let raw = nodes(&value);
                let targets = if raw.iter().all(|target| names.declared.contains_key(target)) { raw }
                    else { self.target_nodes(scope.file, &value)? };
                let declaring = targets.first().and_then(|target| names.declared.get(target))
                    .filter(|(_, owner, ..)| *owner == plan.node).map(|(scope, ..)| *scope);
                if let Some(declaring) = declaring.filter(|&declaring| targets.iter().all(|target| {
                    names.declared.get(target).is_some_and(|&(scope, owner, ..)| scope == declaring && owner == plan.node)
                })) {
                    containing = declaring;
                }
                target = Some(value);
            }
        }
        while !self.is_publication(ScopeRef { file: scope.file, scope: containing })? {
            let Some(parent) = names.scopes[containing as usize].parent else { break };
            containing = parent;
        }
        Ok(if containing == scope.scope && self.is_publication(scope)? {
            Some(PreparedItem { node, item, target })
        } else { None })
    }
}

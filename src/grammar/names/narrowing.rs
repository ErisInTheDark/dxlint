//! Narrowing: `propagates`, `truthy`, `excludes` / `includes` / `has`
//! (docs/design/type-layers.md "Narrowing"). Owner: Iris.

use super::effects::{Event, EventKind, Execution, Phase, Point, StepId};
use super::question::{Known, Leaf, NodeRef, NodeValue, QueryContext, Target};
use super::semantic::Semantic;
use super::values::Key;
use super::Value;

mod proof;

pub struct Narrowing<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
}

impl Narrowing<'_> {
    pub fn refine(&self, at: Leaf, base: NodeValue) -> NodeValue {
        let refined = self.refine_graph(at, base);
        if self.semantic.question.budget.exhausted() { NodeValue::Unknown } else { refined }
    }

    fn refine_graph(&self, at: Leaf, mut base: NodeValue) -> NodeValue {
        let Known::Yes(subject) = self.place(at) else { return base };
        let Target::Node(node) = at.target else { return base };
        let effects = self.semantic.effects();
        let Known::Yes(entry) = effects.entry_for(at) else { return base };
        let Known::Yes(execution) = effects.execution(entry) else { return base };
        let mut uses = Vec::new();
        for site in execution.sites(node, Phase::Enter) {
            match execution.reaches(execution.entry(), site, &[], &self.semantic.question.budget) {
                Known::Yes(true) => uses.push(site),
                Known::Yes(false) => {}
                Known::Unknown => return base,
            }
        }
        if uses.is_empty() { return base }
        let writes = |event: &Event| {
            if event.kind != EventKind::Write || event.phase != Phase::Leave { return false }
            let Some(target) = event.target else { return true };
            let at = Leaf { target: Target::Node(target.value), frame: entry.frame };
            match self.place(at) {
                Known::Yes(written) => written.invalidates(&subject),
                Known::Unknown => true,
            }
        };
        let mut owners = crate::fx_hash::FxHashSet::default();
        for (_, step) in execution.steps() {
            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
            if !matches!(step.point, Point::Enter) { continue }
            let Some(kind) = self.semantic.world.tree(step.node.file)
                .and_then(|tree| tree.nodes.get(step.node.node as usize))
                .and_then(|data| self.semantic.tables.kinds.get(data.matcher as usize)) else { continue };
            if kind.propagates.is_empty() || !owners.insert(step.node) { continue }
            let owner = Leaf { target: Target::Node(step.node), frame: entry.frame };
            for (path, proof, when) in &kind.propagates {
                if !self.semantic.question.budget.step() { return NodeValue::Unknown }
                if when.is_some_and(|condition| self.semantic.world.condition(owner, condition, self.semantic) != Known::Yes(true)) {
                    continue;
                }
                let entries = execution.field_entries(step.node, path);
                if entries.is_empty() { continue }
                if uses.iter().all(|&site| guarded(&execution, entries, site, writes, self.semantic.question) == Known::Yes(true)) {
                    base = self.apply(owner, proof, &subject, base);
                }
                if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
            }
        }
        base
    }

    fn place(&self, mut at: Leaf) -> Known<Place> {
        let values = self.semantic.values();
        let mut path = Vec::new();
        loop {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let Target::Node(node) = at.target else { return Known::Unknown };
            let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
            if let Some(&declaration) = names.merged.get(&node.node).or_else(|| names.declaration.get(&node.node)) {
                if declaration != node.node {
                    at.target = Target::Node(NodeRef { node: declaration, ..node });
                    continue;
                }
            }
            if names.declared.contains_key(&node.node) {
                path.reverse();
                return Known::Yes(Place { root: at, path });
            }
            let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
            let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
            let Some(kind) = self.semantic.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
            let mut recipe = None;
            for item in &kind.unwraps {
                if !self.semantic.question.budget.step() { return Known::Unknown }
                if item.effect.is_some() || item.value.is_none() { continue }
                match item.when.map_or(Known::Yes(true), |condition| self.semantic.world.condition(at, condition, self.semantic)) {
                    Known::Yes(false) => continue,
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(true) => {}
                }
                if recipe.is_some() { return Known::Unknown }
                recipe = item.value.as_ref();
            }
            let (source, key) = match recipe {
                Some(value @ Value::Path(_)) => (value, None),
                Some(Value::From(source, key, _)) => (source.as_ref(), Some(key.as_ref())),
                _ => return Known::Unknown,
            };
            let Known::Yes(sites) = values.sites(at, source) else { return Known::Unknown };
            let [next] = sites.as_slice() else { return Known::Unknown };
            if let Some(key) = key {
                let Known::Yes(key) = values.key(at, key) else { return Known::Unknown };
                path.push(key);
            }
            at = *next;
        }
    }
}

/// A narrowed place: a root binding and the member path below it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    root: Leaf,
    path: Vec<Key>,
}

impl Place {
    /// Whether writing `self` makes a guard on `guarded` stale.
    fn invalidates(&self, guarded: &Place) -> bool {
        self.root == guarded.root && guarded.path.starts_with(&self.path)
    }
}

/// Whether every path from the entry to `at` passes one of the guard
/// `entries` with no `writes` event after it (a fresh guard resets).
pub fn guarded(
    execution: &Execution,
    entries: &[StepId],
    at: StepId,
    writes: impl Fn(&Event) -> bool,
    question: &QueryContext,
) -> Known<bool> {
    let budget = &question.budget;
    match execution.reaches(execution.entry(), at, &[], budget) {
        Known::Yes(true) => {}
        other => return other,
    }
    match execution.reaches(execution.entry(), at, entries, budget) {
        Known::Yes(false) => {}
        Known::Yes(true) => return Known::Yes(false),
        Known::Unknown => return Known::Unknown,
    }
    for &entry in entries {
        match execution.reaches(execution.entry(), entry, &[], budget) {
            Known::Yes(true) => {}
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
        }
        match execution.reaches_through(entry, at, entries, &writes, budget) {
            Known::Yes(false) => {}
            Known::Yes(true) => return Known::Yes(false),
            Known::Unknown => return Known::Unknown,
        }
    }
    Known::Yes(true)
}

#[cfg(test)]
mod tests;

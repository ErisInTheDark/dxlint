//! Narrowing: `propagates`, `truthy`, `excludes` / `includes` / `has`
//! (docs/design/type-layers.md "Narrowing"). Owner: Iris.

use std::rc::Rc;

use super::effects::{Event, EventKind, Execution, Phase, Point, StepId};
use super::memo::{Epoch, Memo, Settled};
use super::question::{Known, Leaf, NodeRef, NodeValue, QueryContext, Target};
use super::semantic::Semantic;
use super::values::Key;
use super::Value;

mod proof;

/// Profile rows for `--stats` (registered with the engine profile).
pub const PROFILE_QUERIES: &[&str] = &["narrowing.refine", "narrowing.assigned", "narrowing.guards"];

/// Proof nesting followed when collecting a guard's tested roots; deeper
/// guards are kept for every read.
const TESTED_DEPTH: u32 = 32;

/// Answers every read in a function shares: the place each node names,
/// which bindings each guard can narrow, and closure bindings' unwritten
/// proofs. Complete answers only.
#[derive(Default)]
pub(crate) struct State {
    places: Memo<Leaf, Known<Place>>,
    holds: Memo<(Leaf, usize), Known<bool>>,
    tested: Memo<(Leaf, usize), Tested>,
    unwritten: Memo<Leaf, Known<bool>>,
}

impl State {
    pub(super) fn finish(&self, epoch: Epoch, failed: bool, cut: u64) -> u64 {
        self.places.finish(epoch, failed, cut, Settled::settled)
            + self.holds.finish(epoch, failed, cut, Settled::settled)
            + self.tested.finish(epoch, failed, cut, Settled::settled)
            + self.unwritten.finish(epoch, failed, cut, Settled::settled)
    }
}

/// The binding roots a guard's proof tests: a read rooted elsewhere is
/// left as it is by every atom of the proof, so the guard is skipped.
#[derive(Clone)]
enum Tested {
    Any,
    Roots(Rc<[Leaf]>),
}

impl Settled for Tested {
    fn settled(&self) -> bool { true }
}

impl Tested {
    fn admits(&self, root: Leaf) -> bool {
        match self {
            Tested::Any => true,
            Tested::Roots(roots) => roots.contains(&root),
        }
    }
}

pub struct Narrowing<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
}

impl Narrowing<'_> {
    pub fn refine(&self, at: Leaf, base: NodeValue) -> NodeValue {
        self.profile("narrowing.refine", || {
            let Known::Yes(at) = self.semantic.leaf(at) else { return NodeValue::Unknown };
            let refined = self.refine_graph(at, base);
            if self.semantic.question.budget.exhausted() { NodeValue::Unknown } else { refined }
        })
    }

    fn profile<T>(&self, name: &'static str, run: impl FnOnce() -> T) -> T {
        match self.semantic.world.profile() {
            Some(profile) => profile.time_semantic_query(name, run),
            None => run(),
        }
    }

    fn refine_graph(&self, at: Leaf, mut base: NodeValue) -> NodeValue {
        let Known::Yes(subject) = self.place(at) else { return base };
        let effects = self.semantic.effects();
        let Known::Yes(entry) = effects.entry_for(at) else { return base };
        let mut contexts = vec![(entry, at)];
        if subject.path.is_empty() {
            let mut enclosing = Vec::new();
            let mut cursor = entry;
            loop {
                if !self.semantic.question.budget.step() { return NodeValue::Unknown }
                let Known::Yes(Some(creation)) = effects.enclosing(cursor) else { break };
                enclosing.push((creation.entry, creation.site));
                cursor = creation.entry;
            }
            // Only a value read inside a closure can use its creators' guards,
            // and only then is the (whole-file) unwritten proof worth asking.
            if !enclosing.is_empty() && self.unwritten(subject.root) == Known::Yes(true) {
                contexts.extend(enclosing);
            }
        }
        for (context_entry, site) in contexts.into_iter().rev() {
            base = self.refine_in(&subject, context_entry, site, base, context_entry != entry);
            if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
        }
        base
    }

    fn refine_in(&self, subject: &Place, entry: Leaf, at: Leaf, mut base: NodeValue, captured: bool) -> NodeValue {
        let Known::Yes(at) = self.semantic.leaf(at) else { return base };
        let Target::Node(node) = at.target else { return base };
        let effects = self.semantic.effects();
        let Known::Yes(execution) = effects.execution(entry) else { return base };
        let mut uses = Vec::new();
        for site in execution.sites(node, Phase::Enter) {
            match execution.reached(site, &self.semantic.question.budget) {
                Known::Yes(true) => uses.push(site),
                Known::Yes(false) => {}
                Known::Unknown => return base,
            }
        }
        if uses.is_empty() { return base }
        if !captured {
            base = self.profile("narrowing.assigned", || self.assigned(subject, &execution, &uses, entry.frame, base));
        }
        self.profile("narrowing.guards", || self.guards(subject, &execution, &uses, entry.frame, captured, base))
    }

    /// Applies, in step order, every guard of `execution` that dominates
    /// `uses` and tests `subject`'s root.
    fn guards(&self, subject: &Place, execution: &Execution, uses: &[StepId], frame: super::question::FrameId, captured: bool, mut base: NodeValue) -> NodeValue {
        for &node in execution.guard_owners(|execution| self.guard_owners(execution)) {
            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
            let owner = Leaf { target: Target::Node(node), frame };
            let Known::Yes(owner) = self.semantic.leaf(owner) else { continue };
            let Some(kind) = self.kind(node) else { continue };
            for (item, (path, proof, when)) in kind.propagates.iter().enumerate() {
                if !self.semantic.question.budget.step() { return NodeValue::Unknown }
                let tested = self.semantic.narrowing.tested.get(&(owner, item));
                if tested.as_ref().is_some_and(|tested| !tested.admits(subject.root)) { continue }
                if !self.holds(owner, item, *when) { continue }
                let entries = execution.field_entries(node, path);
                if entries.is_empty() { continue }
                if execution.guarded(uses, entries, &self.semantic.question.budget) == Known::Yes(true) {
                    // Collected only where the proof would be applied anyway,
                    // so it asks nothing the proof itself wouldn't.
                    if tested.is_none() && !self.tested(owner, item, proof).admits(subject.root) { continue }
                    let region = Region { execution, entries, uses, frame, captured };
                    base = self.apply_in(owner, proof, subject, base, Some(&region));
                }
                if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
            }
        }
        base
    }

    /// Whether no write anywhere in the file replaces closure binding `root`.
    fn unwritten(&self, root: Leaf) -> Known<bool> {
        self.kept(&self.semantic.narrowing.unwritten, root, || {
            self.semantic.effects().unwritten(root, &|target| self.writes_binding(target, root))
        })
    }

    /// Whether guard `item` of `owner` applies (its `when` holds).
    fn holds(&self, owner: Leaf, item: usize, when: Option<u32>) -> bool {
        let Some(condition) = when else { return true };
        self.kept(&self.semantic.narrowing.holds, (owner, item), || {
            self.semantic.world.condition(owner, condition, self.semantic)
        }) == Known::Yes(true)
    }

    /// A narrowing memo entry, kept only when complete: settled, within
    /// budget, and computed without reading anything still undecided up the
    /// stack (such an answer may only reflect the unfinished cycle).
    fn kept<K: Clone + Eq + std::hash::Hash, V: Clone + Settled>(&self, memo: &Memo<K, V>, key: K, compute: impl FnOnce() -> V) -> V {
        if let Some(answer) = memo.get(&key) { return answer }
        let values = self.semantic.values();
        let mark = values.cycle_mark();
        let answer = compute();
        if answer.settled() && !values.cycled_since(mark) && !self.semantic.question.budget.exhausted() {
            memo.insert(self.semantic.epoch, key, answer.clone());
        }
        answer
    }

    /// Every node entered in `execution` whose kind propagates guards, once
    /// each in step order (as its semantic node).
    fn guard_owners(&self, execution: &Execution) -> Vec<NodeRef> {
        let mut seen = crate::fx_hash::FxHashSet::default();
        let mut owners = Vec::new();
        for (_, step) in execution.steps() {
            if !matches!(step.point, Point::Enter) { continue }
            let Some(tree) = self.semantic.world.tree(step.node.file) else { continue };
            let Some(canonical) = tree.semantic_node_with(step.node.node, &mut || true) else { continue };
            let node = NodeRef { node: canonical, ..step.node };
            if self.kind(node).is_some_and(|kind| !kind.propagates.is_empty()) && seen.insert(node) {
                owners.push(node);
            }
        }
        owners
    }

    fn kind(&self, node: NodeRef) -> Option<&super::KindNames> {
        self.semantic.world.tree(node.file)
            .and_then(|tree| tree.nodes.get(node.node as usize))
            .and_then(|data| self.semantic.tables.kinds.get(data.matcher as usize))
    }

    fn assigned(&self, subject: &Place, execution: &Execution, uses: &[StepId], frame: super::question::FrameId, base: NodeValue) -> NodeValue {
        let budget = &self.semantic.question.budget;
        let mut candidates = Vec::new();
        for &id in execution.write_leaves() {
            if !budget.step() { return NodeValue::Unknown }
            let Some(event) = execution.step(id).and_then(|step| step.event()) else { continue };
            if self.write_invalidates(event, subject, frame) { candidates.push(id) }
        }
        let Known::Yes(Some(last)) = execution.last_steps(uses, &candidates, budget) else { return base };
        let values = self.semantic.values();
        let mut alternatives = Vec::new();
        for id in last {
            if !budget.step() { return NodeValue::Unknown }
            let Some(event) = execution.step(id).and_then(|step| step.event()) else { return base };
            let Some(target) = event.target else { return base };
            let at = Leaf { target: Target::Node(target.value), frame };
            if self.place(at) != Known::Yes(subject.clone()) { return base }
            let Some(written) = &event.written else { return base };
            let written = values.evaluate(written.owner, &written.recipe);
            let NodeValue::Alternatives(rows) = values.reduce(&base, &written) else { return base };
            alternatives.extend(rows);
        }
        NodeValue::Alternatives(alternatives)
    }

    fn write_invalidates(&self, event: &Event, tested: &Place, frame: super::question::FrameId) -> bool {
        if event.kind != EventKind::Write || event.phase != Phase::Leave { return false }
        let Some(target) = event.target else { return true };
        let at = Leaf { target: Target::Node(target.value), frame };
        match self.place(at) {
            Known::Yes(written) => written.invalidates(tested),
            Known::Unknown => true,
        }
    }

    /// Whether a write to `target` replaces binding `root`. Lexical: a write
    /// replaces a binding only through a name that resolves to it, never as
    /// a property write and never through call arguments.
    fn writes_binding(&self, target: Leaf, root: Leaf) -> Known<bool> {
        // A subject rooted elsewhere (a parameter read through its call's
        // argument) is no lexical binding to compare against.
        let Target::Node(bound) = root.target else { return Known::Unknown };
        if !self.semantic.world.names(bound.file).is_some_and(|names| names.declared.contains_key(&bound.node)) { return Known::Unknown }
        let Known::Yes(at) = self.semantic.leaf(target) else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Yes(false) };
        let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
        let declaration = names.merged.get(&node.node).or_else(|| names.declaration.get(&node.node)).copied()
            .or_else(|| names.declared.contains_key(&node.node).then_some(node.node));
        match declaration {
            Some(declaration) => Known::Yes(root.target == Target::Node(NodeRef { node: declaration, ..node })),
            // A name nothing declares could still be this binding.
            None if names.keys.contains_key(&node.node) || names.asked.contains_key(&node.node) => Known::Unknown,
            None => Known::Yes(false),
        }
    }

    /// The binding and member path `at` names. Every read asks it of each
    /// write and guard operand in its function, so answers are kept.
    fn place(&self, at: Leaf) -> Known<Place> {
        self.kept(&self.semantic.narrowing.places, at, || self.find_place(at))
    }

    fn find_place(&self, mut at: Leaf) -> Known<Place> {
        let values = self.semantic.values();
        let mut path = Vec::new();
        loop {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let Known::Yes(normalized) = self.semantic.leaf(at) else { return Known::Unknown };
            at = normalized;
            let Target::Node(node) = at.target else { return Known::Unknown };
            let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
            if let Some(&declaration) = names.merged.get(&node.node).or_else(|| names.declaration.get(&node.node)) {
                if declaration != node.node {
                    at.target = Target::Node(NodeRef { node: declaration, ..node });
                    continue;
                }
            }
            if names.declared.contains_key(&node.node) {
                match self.semantic.calls().argument_sites(at) {
                    Known::Yes(Some(sites)) => {
                        let [argument] = sites.as_slice() else { return Known::Unknown };
                        at = *argument;
                        continue;
                    }
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(None) => {}
                }
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

struct Region<'a> {
    execution: &'a Execution,
    entries: &'a [StepId],
    uses: &'a [StepId],
    frame: super::question::FrameId,
    captured: bool,
}

impl Region<'_> {
    fn valid(&self, narrowing: &Narrowing<'_>, tested: &Place) -> Known<bool> {
        if self.captured && !tested.path.is_empty() { return Known::Yes(false) }
        let writes = |event: &Event| narrowing.write_invalidates(event, tested, self.frame);
        let budget = &narrowing.semantic.question.budget;
        for &entry in self.entries {
            match self.execution.reached(entry, budget) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            for &site in self.uses {
                match self.execution.reaches_through(entry, site, self.entries, &writes, budget) {
                    Known::Yes(true) => return Known::Yes(false),
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(false) => {}
                }
            }
        }
        Known::Yes(true)
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
        self.root == guarded.root && self.path.len() <= guarded.path.len()
            && self.path.iter().zip(&guarded.path).all(|(written, tested)| written.may_alias(tested))
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
    match execution.reached(at, budget) {
        Known::Yes(true) => {}
        other => return other,
    }
    match execution.guarded(&[at], entries, budget) {
        Known::Yes(true) => {}
        Known::Yes(false) => return Known::Yes(false),
        Known::Unknown => return Known::Unknown,
    }
    for &entry in entries {
        match execution.reached(entry, budget) {
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

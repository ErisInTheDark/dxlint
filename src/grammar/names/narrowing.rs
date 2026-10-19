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

/// The places writes name, by write target: every read refined in a body
/// asks about each of its writes, and a member target's place reads its
/// recipe conditions (semantic questions) to find.
#[derive(Default)]
pub(crate) struct State {
    written: super::memo::Memo<Leaf, Known<Place>>,
    /// Per execution entry and written place, the write steps that can
    /// replace it: every read of one variable in a body shares the scan.
    candidates: super::memo::Memo<(Leaf, Place), std::rc::Rc<[super::effects::StepId]>>,
    /// Per execution entry, its applicable guards ([`Narrowing::guards`]).
    guards: super::memo::Memo<Leaf, std::rc::Rc<[Guard]>>,
    /// Per execution entry, region entry steps and tested place: the steps
    /// a write to the place reaches inside the region ([`Region::valid`]).
    written_through: super::memo::Memo<(Leaf, Box<[super::effects::StepId]>, Place), std::rc::Rc<[u64]>>,
    /// Per file, the binding roots its guards' proofs can test
    /// ([`Narrowing::tested_roots`]); `None` when they can't be told.
    tested: super::memo::Memo<u32, Option<std::rc::Rc<crate::fx_hash::FxHashSet<NodeRef>>>>,
}

/// One `in <field> propagates <proof>` item that holds at its owner.
#[derive(Clone)]
struct Guard {
    owner: Leaf,
    node: NodeRef,
    /// Index into the owner kind's `propagates`.
    item: usize,
    path: Vec<u16>,
}

impl State {
    pub(super) fn finish(&self, epoch: super::memo::Epoch, failed: bool, cut: u64) -> u64 {
        self.written.finish(epoch, failed, cut, super::memo::Settled::settled)
            + self.candidates.finish(epoch, failed, cut, |_| true)
            + self.guards.finish(epoch, failed, cut, |_| true)
            + self.written_through.finish(epoch, failed, cut, |_| true)
            + self.tested.finish(epoch, failed, cut, |_| true)
    }
}

impl Narrowing<'_> {
    pub fn refine(&self, at: Leaf, base: NodeValue) -> NodeValue {
        let _site = self.semantic.world.profile().map(|_| crate::engine::StepSite::enter("narrowing.refine"));
        let Known::Yes(at) = self.semantic.leaf(at) else { return NodeValue::Unknown };
        let refined = self.refine_graph(at, base);
        if self.semantic.question.budget.exhausted() { NodeValue::Unknown } else { refined }
    }

    fn refine_graph(&self, at: Leaf, mut base: NodeValue) -> NodeValue {
        let Known::Yes((subject, written)) = self.find_place(at, true) else { return base };
        // A lone declared alternative no write changes, and no guard in the
        // file tests this binding: nothing can narrow it, so no flow graph.
        if !matches!(&base, NodeValue::Alternatives(rows) if rows.len() > 1) {
            if let (Target::Node(read), Target::Node(root)) = (at.target, written.root.target) {
                if self.tested_roots(read.file).is_some_and(|roots| !roots.contains(&root)) { return base }
            }
        }
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
            if !enclosing.is_empty()
                && effects.unwritten(subject.root, &|target| self.writes_binding(target, subject.root)) == Known::Yes(true) {
                contexts.extend(enclosing);
            }
        }
        for (context_entry, site) in contexts.into_iter().rev() {
            base = self.refine_in(&subject, &written, context_entry, site, base, context_entry != entry);
            if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
        }
        base
    }

    /// `subject` matches guards (through call arguments); `written` is the
    /// binding writes replace.
    fn refine_in(&self, subject: &Place, written: &Place, entry: Leaf, at: Leaf, mut base: NodeValue, captured: bool) -> NodeValue {
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
            base = self.assigned(written, &execution, entry, &uses, base);
        }
        let Some(guards) = self.guards(&execution, entry) else { return NodeValue::Unknown };
        let _site = self.semantic.world.profile().map(|_| crate::engine::StepSite::enter("narrowing.guards"));
        for guard in guards.iter() {
            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
            let entries = execution.field_entries(guard.node, &guard.path);
            if execution.guarded(&uses, entries, &self.semantic.question.budget) == Known::Yes(true) {
                let Some((_, proof, _)) = self.kind(guard.node).and_then(|kind| kind.propagates.get(guard.item)) else { continue };
                let region = Region { execution: &execution, entry, entries, uses: &uses, captured };
                base = self.apply_in(guard.owner, proof, subject, base, Some(&region));
            }
            if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
        }
        base
    }

    /// The guards whose conditions hold in `execution` (entered at
    /// `entry`) and that guard some steps, decided once per execution: every
    /// read there checks the same guards.
    fn guards(&self, execution: &Execution, entry: Leaf) -> Option<std::rc::Rc<[Guard]>> {
        let memo = &self.semantic.narrowing_places.guards;
        if let Some(guards) = memo.get(&entry) { return Some(guards) }
        let budget = &self.semantic.question.budget;
        let mut guards = Vec::new();
        for &node in execution.guard_owners(|execution| self.guard_owners(execution)) {
            if !budget.step() { return None }
            let owner = Leaf { target: Target::Node(node), frame: entry.frame };
            let Known::Yes(owner) = self.semantic.leaf(owner) else { continue };
            let Some(kind) = self.kind(node) else { continue };
            for (item, (path, _, when)) in kind.propagates.iter().enumerate() {
                if !budget.step() { return None }
                if when.is_some_and(|condition| self.semantic.world.condition(owner, condition, self.semantic) != Known::Yes(true)) {
                    continue;
                }
                if execution.field_entries(node, path).is_empty() { continue }
                guards.push(Guard { owner, node, item, path: path.clone() });
            }
        }
        if budget.exhausted() { return None }
        let guards: std::rc::Rc<[Guard]> = guards.into();
        memo.insert(self.semantic.epoch, entry, guards.clone());
        Some(guards)
    }

    /// The lexical binding roots any guard in `file` can test: the roots of
    /// every name inside the sites its `propagates` proofs read, found once
    /// per file from syntax and names. `None` when a proof can't be followed.
    fn tested_roots(&self, file: u32) -> Option<std::rc::Rc<crate::fx_hash::FxHashSet<NodeRef>>> {
        let memo = &self.semantic.narrowing_places.tested;
        if let Some(roots) = memo.get(&file) { return roots }
        let roots = self.collect_tested_roots(file).map(std::rc::Rc::new);
        if self.semantic.question.budget.exhausted() { return None }
        memo.insert(self.semantic.epoch, file, roots.clone());
        roots
    }

    fn collect_tested_roots(&self, file: u32) -> Option<crate::fx_hash::FxHashSet<NodeRef>> {
        let tree = self.semantic.world.tree(file)?;
        let names = self.semantic.world.names(file)?;
        let values = self.semantic.values();
        let mut roots = crate::fx_hash::FxHashSet::default();
        let mut paths = Vec::new();
        for (id, data) in tree.nodes.iter().enumerate() {
            let Some(kind) = self.semantic.tables.kinds.get(data.matcher as usize) else { continue };
            if kind.propagates.is_empty() { continue }
            let owner = Leaf { target: Target::Node(NodeRef { file, node: id as u32 }), frame: super::question::FrameId::ROOT };
            for (_, proof, _) in &kind.propagates {
                paths.clear();
                if !proof_paths(proof, &mut paths) { return None }
                for path in &paths {
                    let Known::Yes(sites) = values.follow(owner, path) else { return None };
                    for site in sites {
                        let Target::Node(site) = site.target else { return None };
                        if site.file != file { return None }
                        let mut pending = vec![site.node];
                        while let Some(node) = pending.pop() {
                            let named = names.merged.contains_key(&node) || names.declaration.contains_key(&node)
                                || names.declared.contains_key(&node);
                            if named {
                                let at = Leaf { target: Target::Node(NodeRef { file, node }), frame: super::question::FrameId::ROOT };
                                match self.written_place(at) {
                                    Known::Yes(place) => match place.root.target {
                                        Target::Node(root) => { roots.insert(root); }
                                        _ => return None,
                                    },
                                    // A name that names no place tests none.
                                    Known::Unknown if !self.semantic.question.budget.exhausted() => {}
                                    Known::Unknown => return None,
                                }
                            }
                            if let Some(data) = tree.nodes.get(node as usize) {
                                pending.extend(data.children.iter().map(|edge| edge.node));
                            }
                        }
                    }
                }
            }
        }
        Some(roots)
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

    /// The write steps of `execution` (entered at `entry`) that can replace
    /// `subject`, found once per execution and place.
    fn write_candidates(&self, subject: &Place, execution: &Execution, entry: Leaf) -> Option<std::rc::Rc<[StepId]>> {
        let memo = &self.semantic.narrowing_places.candidates;
        let key = (entry, subject.clone());
        if let Some(candidates) = memo.get(&key) { return Some(candidates) }
        let budget = &self.semantic.question.budget;
        let mut candidates = Vec::new();
        for &id in execution.write_leaves() {
            if !budget.step() { return None }
            let Some(event) = execution.step(id).and_then(|step| step.event()) else { continue };
            if self.write_invalidates(event, subject, entry.frame) { candidates.push(id) }
        }
        if budget.exhausted() { return None }
        let candidates: std::rc::Rc<[StepId]> = candidates.into();
        memo.insert(self.semantic.epoch, key, candidates.clone());
        Some(candidates)
    }

    fn assigned(&self, subject: &Place, execution: &Execution, entry: Leaf, uses: &[StepId], base: NodeValue) -> NodeValue {
        let _site = self.semantic.world.profile().map(|_| crate::engine::StepSite::enter("narrowing.assigned"));
        // Writes only pick among the declared alternatives (as TypeScript
        // narrows assignments to union declared types only): one alternative
        // stays whatever was written.
        if !matches!(&base, NodeValue::Alternatives(rows) if rows.len() > 1) { return base }
        let budget = &self.semantic.question.budget;
        let frame = entry.frame;
        let Some(candidates) = self.write_candidates(subject, execution, entry) else { return NodeValue::Unknown };
        // Unwritten in the body: the entry reaches every use without a write.
        if candidates.is_empty() { return base }
        let Known::Yes(Some(last)) = execution.last_steps(uses, &candidates, budget) else { return base };
        let values = self.semantic.values();
        let mut alternatives = Vec::new();
        for id in last {
            if !budget.step() { return NodeValue::Unknown }
            let Some(event) = execution.step(id).and_then(|step| step.event()) else { return base };
            let Some(target) = event.target else { return base };
            let at = Leaf { target: Target::Node(target.value), frame };
            if self.written_place(at) != Known::Yes(subject.clone()) { return base }
            let Some(written) = &event.written else { return base };
            let written = values.evaluate(written.owner, &written.recipe);
            let NodeValue::Alternatives(rows) = values.reduce(&base, &written) else { return base };
            alternatives.extend(rows);
        }
        NodeValue::Alternatives(alternatives)
    }

    /// Whether `event` replaces (part of) `tested`, a written place.
    fn write_invalidates(&self, event: &Event, tested: &Place, frame: super::question::FrameId) -> bool {
        if event.kind != EventKind::Write || event.phase != Phase::Leave { return false }
        let Some(target) = event.target else { return true };
        let at = Leaf { target: Target::Node(target.value), frame };
        match self.written_place(at) {
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

    /// The binding and member path `at` names; a parameter read through its
    /// call is followed to the argument it was bound to.
    fn place(&self, at: Leaf) -> Known<Place> {
        self.find_place(at, true).map(|(place, _)| place)
    }

    /// The binding a write replaces: lexical, never through a call's
    /// arguments (writing a parameter changes the parameter, not what the
    /// caller passed).
    fn written_place(&self, at: Leaf) -> Known<Place> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        self.semantic.values().memoized(&self.semantic.narrowing_places.written, at,
            || self.find_place(at, false).map(|(place, _)| place))
    }

    /// The place `at` names (through call arguments when asked) and the
    /// lexical place a write there replaces: one walk for both.
    fn find_place(&self, mut at: Leaf, through_arguments: bool) -> Known<(Place, Place)> {
        let values = self.semantic.values();
        let mut path = Vec::new();
        let mut lexical = None;
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
                if lexical.is_none() {
                    lexical = Some(Place { root: at, path: path.iter().rev().cloned().collect() });
                }
                if through_arguments {
                    match self.semantic.calls().argument_sites(at) {
                        Known::Yes(Some(sites)) => {
                            let [argument] = sites.as_slice() else { return Known::Unknown };
                            at = *argument;
                            continue;
                        }
                        Known::Unknown => return Known::Unknown,
                        Known::Yes(None) => {}
                    }
                }
                path.reverse();
                let place = Place { root: at, path };
                let lexical = lexical.unwrap_or_else(|| place.clone());
                return Known::Yes((place, lexical));
            }
            let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
            let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
            let Some(kind) = self.semantic.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
            // `refers` names the place from syntax; the `unwraps` recipe
            // only stands in where a kind declares none, as its conditions
            // can ask whole type questions.
            let fallback = kind.refers.is_empty();
            let recipes = kind.refers.iter().map(|(value, when)| (value, *when))
                .chain(kind.unwraps.iter()
                    .filter(|item| fallback && item.effect.is_none())
                    .filter_map(|item| Some((item.value.as_ref()?, item.when))));
            let mut recipe = None;
            for (value, when) in recipes {
                if !self.semantic.question.budget.step() { return Known::Unknown }
                match when.map_or(Known::Yes(true), |condition| self.semantic.world.condition(at, condition, self.semantic)) {
                    Known::Yes(false) => continue,
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(true) => {}
                }
                if recipe.is_some() { return Known::Unknown }
                recipe = Some(value);
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

/// The field paths a `propagates` proof reads its tested sites through;
/// false for a proof shape whose sites can't be told from paths.
fn proof_paths<'a>(proof: &'a Value, paths: &mut Vec<&'a [u16]>) -> bool {
    match proof {
        Value::Path(path) => { paths.push(path); true }
        Value::Not(inner) => proof_paths(inner, paths),
        Value::Union(parts) | Value::Intersection(parts) => parts.iter().all(|part| proof_paths(part, paths)),
        Value::Excludes { from: Some(from), .. } | Value::Includes { from: Some(from), .. } | Value::Has { from: Some(from), .. } =>
            proof_paths(from, paths),
        _ => false,
    }
}

struct Region<'a> {
    execution: &'a Execution,
    /// The execution's entry (its frame is the region's).
    entry: Leaf,
    entries: &'a [StepId],
    uses: &'a [StepId],
    captured: bool,
}

impl Region<'_> {
    fn valid(&self, narrowing: &Narrowing<'_>, tested: &Place) -> Known<bool> {
        let _site = narrowing.semantic.world.profile().map(|_| crate::engine::StepSite::enter("narrowing.valid"));
        if self.captured && !tested.path.is_empty() { return Known::Yes(false) }
        // Nothing in the body writes the tested place: no path can.
        let Some(candidates) = narrowing.write_candidates(tested, self.execution, self.entry) else { return Known::Unknown };
        let budget = &narrowing.semantic.question.budget;
        let mut starts = Vec::new();
        for &entry in self.entries {
            match self.execution.reached(entry, budget) {
                Known::Yes(false) => {}
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => starts.push(entry),
            }
        }
        if candidates.is_empty() || starts.is_empty() { return Known::Yes(true) }
        // The steps a write reaches inside the region, once per region and
        // tested place: every read there checks its own steps.
        let memo = &narrowing.semantic.narrowing_places.written_through;
        let key = (self.entry, Box::<[StepId]>::from(self.entries), tested.clone());
        let reached = match memo.get(&key) {
            Some(reached) => reached,
            None => {
                let writes = |event: &Event| narrowing.write_invalidates(event, tested, self.entry.frame);
                let Known::Yes(reached) = self.execution.reached_through(&starts, self.entries, writes, budget) else { return Known::Unknown };
                if budget.exhausted() { return Known::Unknown }
                let reached: std::rc::Rc<[u64]> = reached.into();
                memo.insert(narrowing.semantic.epoch, key, reached.clone());
                reached
            }
        };
        Known::Yes(!self.uses.iter().any(|&step| reached[step as usize / 64] & (1 << (step % 64)) != 0))
    }
}

/// A narrowed place: a root binding and the member path below it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

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
    /// The places guard sites test, by site: every read a guard covers
    /// projects the same site.
    tested_places: super::memo::Memo<Leaf, Known<Place>>,
    /// Per execution entry and written place, the write steps that can
    /// replace it: every read of one variable in a body shares the scan.
    candidates: super::memo::Memo<(Leaf, Place), std::rc::Rc<[super::effects::StepId]>>,
    /// Per execution entry, its writes by the binding root they write.
    writes: super::memo::Memo<Leaf, std::rc::Rc<WriteIndex>>,
    /// Per execution entry, its applicable guards ([`Narrowing::guards`]).
    guards: super::memo::Memo<Leaf, std::rc::Rc<[Guard]>>,
    /// Per file, the binding roots its guards' proofs can test, with those
    /// guards' owner nodes ([`Narrowing::tested_roots`]); `None` when they
    /// can't be told.
    tested: super::memo::Memo<u32, Option<std::rc::Rc<TestedRoots>>>,
}

/// Binding root -> the guard owners (node ids, in the root's file) whose
/// proofs may test it.
type TestedRoots = crate::fx_hash::FxHashMap<NodeRef, Vec<u32>>;

/// A body's write steps by the binding root each writes (its syntax shape,
/// found before any computed key is evaluated): only a place's own root's
/// writes can replace it.
#[derive(Default)]
struct WriteIndex {
    /// With each write's path length (its syntax shape).
    by_root: crate::fx_hash::FxHashMap<Leaf, Vec<(StepId, usize)>>,
    /// Writes whose root can't be told from syntax: every place checks them.
    open: Vec<StepId>,
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
            + self.tested_places.finish(epoch, failed, cut, super::memo::Settled::settled)
            + self.candidates.finish(epoch, failed, cut, |_| true)
            + self.writes.finish(epoch, failed, cut, |_| true)
            + self.guards.finish(epoch, failed, cut, |_| true)
            + self.tested.finish(epoch, failed, cut, |_| true)
    }
}

impl Narrowing<'_> {
    pub fn refine(&self, at: Leaf, base: NodeValue) -> NodeValue {
        if crate::levers::on(crate::levers::Lever::NoNarrowing) { return base }
        let _site = self.semantic.world.profile().map(|_| crate::engine::StepSite::enter("narrowing.refine"));
        let Known::Yes(at) = self.semantic.leaf(at) else { return NodeValue::Unknown };
        let refined = self.refine_graph(at, base);
        if self.semantic.question.budget.exhausted() { NodeValue::Unknown } else { refined }
    }

    fn refine_graph(&self, at: Leaf, mut base: NodeValue) -> NodeValue {
        if crate::levers::on(crate::levers::Lever::LoneSkip) && !matches!(&base, NodeValue::Alternatives(rows) if rows.len() > 1) {
            return base;
        }
        let Known::Yes((subject, written)) = self.find_place(at, true, true) else { return base };
        let effects = self.semantic.effects();
        let Known::Yes(entry) = effects.entry_for(at) else { return base };
        // No guard that can reach this read tests its binding, and no write
        // can change it (a lone declared alternative is what any write picks;
        // a binding the file never writes keeps every alternative): nothing
        // can narrow it, so no flow graph. A guard can reach a read inside
        // the body the guard runs in (its own statements, early exits, and
        // closures created there).
        if let (Target::Node(read), Target::Node(root), Target::Node(body)) = (at.target, written.root.target, entry.target) {
            if let Some(roots) = self.tested_roots(read.file) {
                let reaches = |&owner: &u32| {
                    if body.file != read.file || self.within(owner, body.node, read.file) { return true }
                    let guard = Leaf { target: Target::Node(NodeRef { file: read.file, node: owner }), frame: at.frame };
                    match effects.entry_for(guard) {
                        Known::Yes(Leaf { target: Target::Node(runs_in), .. }) => runs_in.file != read.file || self.within(read.node, runs_in.node, read.file),
                        _ => true,
                    }
                };
                if !roots.get(&root).is_some_and(|owners| owners.iter().any(reaches)) {
                    let lone = !matches!(&base, NodeValue::Alternatives(rows) if rows.len() > 1);
                    if lone || (subject.path.is_empty() && written.path.is_empty()
                        && self.unwritten(written.root) == Known::Yes(true)) {
                        return base;
                    }
                }
            }
        }
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
                && self.unwritten(subject.root) == Known::Yes(true) {
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
        let Known::Yes(execution) = effects.narrowing_execution(entry) else { return base };
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
        // Only guards whose proofs test the read's binding can narrow it
        // (projection rejects the others after proving them): skip the rest
        // before any proof work. Unknown roots keep every guard.
        let tested = self.tested_roots(node.file);
        let owners = |place: &Place| match (&tested, place.root.target) {
            (Some(roots), Target::Node(root)) if root.file == node.file => Some(roots.get(&root).map_or(&[][..], Vec::as_slice)),
            _ => None,
        };
        let filter = owners(subject).zip(owners(written));
        let _site = self.semantic.world.profile().map(|_| crate::engine::StepSite::enter("narrowing.guards"));
        for guard in guards.iter() {
            if let Some((by_subject, by_written)) = filter {
                if guard.node.file == node.file && !by_subject.contains(&guard.node.node) && !by_written.contains(&guard.node.node) { continue }
            }
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

    /// The lexical binding roots any guard in `file` can test, with the
    /// guards: the roots of every name inside the sites `propagates` proofs
    /// read, found once per file from syntax and names. `None` when a proof
    /// can't be followed.
    fn tested_roots(&self, file: u32) -> Option<std::rc::Rc<TestedRoots>> {
        let memo = &self.semantic.narrowing_places.tested;
        if let Some(roots) = memo.get(&file) { return roots }
        // A once-per-file scan on its own allowance: on the asking read's
        // budget a cut would redo it for every later read in the file.
        const STEPS_PER_NODE: u32 = 4;
        let nodes = self.semantic.world.tree(file).map_or(0, |tree| tree.nodes.len());
        let allowance = u32::try_from(nodes).unwrap_or(u32::MAX).saturating_mul(STEPS_PER_NODE)
            .max(super::question::Budget::DEFAULT);
        let budget = &self.semantic.question.budget;
        let roots = budget.detached(allowance, || self.collect_tested_roots(file).map(std::rc::Rc::new))?;
        if budget.exhausted() { return None }
        memo.insert(self.semantic.epoch, file, roots.clone());
        roots
    }

    /// Whether node `inner` lies within node `outer` (both in `file`).
    fn within(&self, inner: u32, outer: u32, file: u32) -> bool {
        let Some(tree) = self.semantic.world.tree(file) else { return true };
        let (Some(inner), Some(outer)) = (tree.nodes.get(inner as usize), tree.nodes.get(outer as usize)) else { return true };
        outer.start <= inner.start && inner.end <= outer.end
    }

    fn collect_tested_roots(&self, file: u32) -> Option<TestedRoots> {
        let tree = self.semantic.world.tree(file)?;
        let names = self.semantic.world.names(file)?;
        let values = self.semantic.values();
        let mut roots = TestedRoots::default();
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
                        // A whole test that narrows nothing in any frame
                        // (`x < limit`) tests none of its operands. Only the
                        // test itself: inside it, a parent's items reach
                        // through children (`typeof x === "string"`).
                        if let Some(data) = tree.nodes.get(site.node as usize) {
                            if self.proves_nothing(Leaf { target: Target::Node(site), frame: super::question::FrameId::ROOT }, data.matcher) { continue }
                        }
                        let mut pending = vec![site.node];
                        while let Some(node) = pending.pop() {
                            let named = names.merged.contains_key(&node) || names.declaration.contains_key(&node)
                                || names.declared.contains_key(&node);
                            if named {
                                let at = Leaf { target: Target::Node(NodeRef { file, node }), frame: super::question::FrameId::ROOT };
                                match self.written_place(at) {
                                    Known::Yes(place) => match place.root.target {
                                        Target::Node(root) => {
                                            let owners = roots.entry(root).or_default();
                                            if owners.last() != Some(&(id as u32)) { owners.push(id as u32) }
                                        }
                                        _ => return None,
                                    },
                                    // A name that names no place tests none.
                                    Known::Unknown if !self.semantic.question.budget.exhausted() => {}
                                    Known::Unknown => return None,
                                }
                            }
                            if tree.nodes.get(node as usize).is_some() {
                                pending.extend(tree.children(node).iter().map(|edge| edge.node));
                            }
                        }
                    }
                }
            }
        }
        Some(roots)
    }

    /// Whether a test at `at` (of kind `matcher`) narrows nothing in any
    /// frame: it calls nothing (no type predicates), and each of its truth
    /// items has a condition its syntax refutes.
    fn proves_nothing(&self, at: Leaf, matcher: u32) -> bool {
        let (Some(kind), Some(truthy)) = (self.semantic.tables.kinds.get(matcher as usize), self.semantic.tables.vocabulary.effect("truthy")) else { return false };
        if !kind.calls.invokes.is_empty() { return false }
        let mut items = kind.unwraps.iter().filter(|item| item.effect == Some(truthy)).peekable();
        items.peek().is_some() && items.all(|item| item.when.is_some_and(|condition| self.semantic.world.refuted_by_syntax(at, condition)))
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
        let index = self.write_index(execution, entry)?;
        let own = index.by_root.get(&subject.root).map_or(&[][..], Vec::as_slice);
        // An indexed write's root is the subject's: only a path longer than
        // the subject's rules it out before its keys are evaluated.
        let indexed = own.iter().filter(|&&(_, length)| length <= subject.path.len()).map(|&(id, _)| (id, true));
        for (id, shaped) in index.open.iter().map(|&id| (id, false)).chain(indexed) {
            if !budget.step() { return None }
            let Some(event) = execution.step(id).and_then(|step| step.event()) else { continue };
            let replaces = if shaped { self.write_replaces(event, subject, entry.frame) } else { self.write_invalidates(event, subject, entry.frame) };
            if replaces { candidates.push(id) }
        }
        candidates.sort_unstable();
        if budget.exhausted() { return None }
        let candidates: std::rc::Rc<[StepId]> = candidates.into();
        memo.insert(self.semantic.epoch, key, candidates.clone());
        Some(candidates)
    }

    /// `execution`'s writes by root, sorted once per entry: before, every
    /// place a body's reads tested scanned every write in the body.
    fn write_index(&self, execution: &Execution, entry: Leaf) -> Option<std::rc::Rc<WriteIndex>> {
        let memo = &self.semantic.narrowing_places.writes;
        if let Some(index) = memo.get(&entry) { return Some(index) }
        let budget = &self.semantic.question.budget;
        // Once per execution, kept for every later read in it: on the
        // graph's allowance, not the first asking read's budget.
        let index = budget.detached(execution.walk_allowance(), || {
            let mut index = WriteIndex::default();
            for &id in execution.write_leaves() {
                if !budget.step() { return None }
                let Some(event) = execution.step(id).and_then(|step| step.event()) else { continue };
                let Some(target) = event.target else { index.open.push(id); continue };
                let at = Leaf { target: Target::Node(target.value), frame: entry.frame };
                match self.find_place(at, false, false) {
                    Known::Yes((shape, _)) => index.by_root.entry(shape.root).or_default().push((id, shape.path.len())),
                    Known::Unknown => index.open.push(id),
                }
            }
            Some(index)
        });
        let Some(Some(index)) = index else { return None };
        if budget.exhausted() { return None }
        let index = std::rc::Rc::new(index);
        memo.insert(self.semantic.epoch, entry, index.clone());
        Some(index)
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
            let written = values.evaluate(execution.owner(written, frame), &written.recipe);
            let NodeValue::Alternatives(rows) = values.reduce(&base, &written) else { return base };
            alternatives.extend(rows);
        }
        NodeValue::rows(alternatives)
    }

    /// Whether `event` replaces (part of) `tested`, a written place.
    fn write_invalidates(&self, event: &Event, tested: &Place, frame: super::question::FrameId) -> bool {
        if event.kind != EventKind::Write || event.phase != Phase::Leave { return false }
        let Some(target) = event.target else { return true };
        let at = Leaf { target: Target::Node(target.value), frame };
        // Another binding, or a path longer than the tested one, can't
        // replace it whatever its keys are: decided before evaluating any
        // computed key (`input.cells[z * size + x] = ...` would type `z`,
        // whose reads narrow through these same writes).
        if let Known::Yes((shape, _)) = self.find_place(at, false, false) {
            if shape.root != tested.root || shape.path.len() > tested.path.len() { return false }
        }
        self.write_replaces(event, tested, frame)
    }

    /// [`Self::write_invalidates`] for a write whose shape already passed.
    fn write_replaces(&self, event: &Event, tested: &Place, frame: super::question::FrameId) -> bool {
        if event.kind != EventKind::Write || event.phase != Phase::Leave { return false }
        let Some(target) = event.target else { return true };
        let at = Leaf { target: Target::Node(target.value), frame };
        match self.written_place(at) {
            Known::Yes(written) => written.invalidates(tested),
            Known::Unknown => true,
        }
    }

    /// The binding a write to `target` replaces (`None`: none). Lexical: a
    /// write replaces a binding only through a name that resolves to it,
    /// never as a property write and never through call arguments.
    fn write_declaration(&self, target: Leaf) -> Known<Option<NodeRef>> {
        let Known::Yes(at) = self.semantic.leaf(target) else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Yes(None) };
        let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
        let declaration = names.merged.get(&node.node).or_else(|| names.declaration.get(&node.node)).copied()
            .or_else(|| names.declared.contains_key(&node.node).then_some(node.node));
        match declaration {
            Some(declaration) => Known::Yes(Some(NodeRef { node: declaration, ..node })),
            // A name nothing declares could still be any binding.
            None if names.keys.contains_key(&node.node) || names.asked.contains_key(&node.node) => Known::Unknown,
            None => Known::Yes(None),
        }
    }

    /// Whether no write in `root`'s file replaces it ([`super::effects::Effects::unwritten`]).
    fn unwritten(&self, root: Leaf) -> Known<bool> {
        self.semantic.effects().unwritten(root, &|target| self.write_declaration(target))
    }

    /// The binding and member path `at` names; a parameter read through its
    /// call is followed to the argument it was bound to.
    fn place(&self, at: Leaf) -> Known<Place> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        self.semantic.values().memoized(&self.semantic.narrowing_places.tested_places, at,
            || self.find_place(at, true, true).map(|(place, _)| place))
    }

    /// The binding a write replaces: lexical, never through a call's
    /// arguments (writing a parameter changes the parameter, not what the
    /// caller passed).
    fn written_place(&self, at: Leaf) -> Known<Place> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        self.semantic.values().memoized(&self.semantic.narrowing_places.written, at,
            || self.find_place(at, false, true).map(|(place, _)| place))
    }

    /// Whether the key expression `key` (of an element access at `at`) is a
    /// binding nothing in its file writes.
    fn constant_key(&self, at: Leaf, key: &Value) -> bool {
        let Known::Yes(sites) = self.semantic.values().sites(at, key) else { return false };
        let [site] = sites.as_slice() else { return false };
        let Known::Yes((place, _)) = self.find_place(*site, false, false) else { return false };
        place.path.is_empty() && self.unwritten(place.root) == Known::Yes(true)
    }

    /// The place `at` names (through call arguments when asked) and the
    /// lexical place a write there replaces: one walk for both. Without
    /// `keys`, computed keys are not evaluated: the path only has the
    /// right length (each key stands as any key).
    fn find_place(&self, mut at: Leaf, through_arguments: bool, keys: bool) -> Known<(Place, Place)> {
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
                if keys {
                    let Known::Yes(found) = values.key(at, key) else { return Known::Unknown };
                    // A read through a wide key names one place only while
                    // the key can't change (TypeScript narrows `obj[key]`
                    // for a constant `key` only): `text[i]` with a
                    // reassigned `i` is not narrowed.
                    if through_arguments && matches!(found, Key::Domain(_)) && !self.constant_key(at, key) { return Known::Unknown }
                    path.push(found);
                } else {
                    path.push(Key::Domain(crate::config::ast::values::KeyDomain::Text));
                }
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
        if candidates.is_empty() { return Known::Yes(true) }
        let budget = &narrowing.semantic.question.budget;
        let mut starts = Vec::new();
        for &entry in self.entries {
            match self.execution.reached(entry, budget) {
                Known::Yes(false) => {}
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => starts.push(entry),
            }
        }
        if starts.is_empty() { return Known::Yes(true) }
        // The candidates are exactly the steps whose write replaces the
        // tested place (sorted). The walk runs on an allowance for the
        // graph's size, not the asking read's.
        let written = budget.detached(self.execution.walk_allowance(), || self.execution.marked_before_steps(&starts, self.entries, self.uses, &candidates, budget));
        let Some(Known::Yes(written)) = written else { return Known::Unknown };
        Known::Yes(!written)
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

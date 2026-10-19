use super::question::{Budget, Known, NodeRef};
use crate::fx_hash::FxHashMap;
use std::cell::{OnceCell, RefCell};

pub mod compile;
pub mod build;
pub use build::Control;
mod controller;
mod source;
pub use controller::{Creation, DeclaredEffect, Effects, ExecutedEffect};
pub(crate) use controller::State;

pub type Path = Vec<u16>;
pub type StepId = u32;
pub type EffectId = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectRole {
    Control(Control),
    Proof,
    Read,
    Event { writes: bool },
}

impl EffectRole {
    fn control(self) -> Option<Control> {
        match self {
            Self::Control(control) => Some(control),
            Self::Proof | Self::Read | Self::Event { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Flow {
    Children,
    Nothing,
    Field(Path),
    Then(Vec<Flow>),
    Or(Vec<Flow>),
    Maybe(Box<Flow>),
    Repeat {
        body: Box<Flow>,
        test: Option<Path>,
        truth: Option<u16>,
        next: Option<Box<Flow>>,
        first: bool,
    },
    Select { entries: Path, test: Path, body: Path },
}

impl Flow {
    pub(crate) fn sources(&self) -> Option<crate::fx_hash::FxHashSet<Path>> {
        let mut sources = crate::fx_hash::FxHashSet::default();
        let mut pending = vec![self];
        while let Some(flow) = pending.pop() {
            match flow {
                Self::Children => return None,
                Self::Nothing => {}
                Self::Field(path) => { sources.insert(path.clone()); }
                Self::Then(parts) | Self::Or(parts) => pending.extend(parts),
                Self::Maybe(flow) => pending.push(flow),
                Self::Repeat { body, test, next, .. } => {
                    pending.push(body);
                    if let Some(test) = test { sources.insert(test.clone()); }
                    if let Some(next) = next { pending.push(next); }
                }
                Self::Select { entries, .. } => { sources.insert(entries.clone()); }
            }
        }
        Some(sources)
    }
}

#[derive(Clone, Debug)]
pub struct Conditional<T> {
    pub item: T,
    pub when: Option<u32>,
    pub item_id: u32,
    pub span: crate::config::source::Span,
}

#[derive(Clone, Debug)]
pub struct Catch {
    pub handler: Path,
    pub source: Path,
    pub effects: Vec<EffectId>,
}

#[derive(Clone, Debug)]
pub struct After {
    pub finalizer: Path,
    pub sources: Vec<Path>,
}

#[derive(Clone, Debug, Default)]
pub struct KindEffects {
    pub runs: Vec<Conditional<Flow>>,
    pub entries: Vec<Conditional<Path>>,
    pub catches: Vec<Conditional<Catch>>,
    pub after: Vec<Conditional<After>>,
    pub labels: Vec<Conditional<Path>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Enter,
    Leave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Read,
    Write,
    Effect(EffectId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub value: NodeRef,
    pub binding: Option<NodeRef>,
}

#[derive(Clone, Debug)]
pub struct Written {
    pub owner: super::question::Leaf,
    pub recipe: std::sync::Arc<super::Value>,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub kind: EventKind,
    pub site: NodeRef,
    pub target: Option<Target>,
    pub value: Option<NodeRef>,
    pub written: Option<Written>,
    pub phase: Phase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Exit {
    pub effect: EffectId,
    pub site: NodeRef,
    pub target: Option<NodeRef>,
    pub value: Option<NodeRef>,
}

/// What a step is. Boxed payloads keep a step at 32 bytes: graphs hold one
/// per node entry and exit, and only fields and events carry more.
#[derive(Clone, Debug)]
pub enum Point {
    Enter,
    Leave,
    Field(Box<[u16]>),
    Event(Box<Event>),
    Join,
}

#[derive(Clone, Debug)]
pub struct Step {
    pub node: NodeRef,
    pub point: Point,
}

impl Step {
    pub fn event(&self) -> Option<&Event> {
        match &self.point {
            Point::Event(event) => Some(&**event),
            _ => None,
        }
    }
}

pub struct Execution {
    entry: StepId,
    steps: Vec<Step>,
    /// Each step's successors.
    edges: Adjacency,
    /// `(node, step)` of every enter and leave step, sorted.
    sites: Vec<(NodeRef, StepId)>,
    /// Field steps, sorted by node, then path, then step.
    fields: Vec<StepId>,
    writes: FxHashMap<NodeRef, Vec<StepId>>,
    binding_writes: FxHashMap<NodeRef, crate::fx_hash::FxHashSet<StepId>>,
    binding_reads: FxHashMap<NodeRef, ReadSet>,
    /// Write events at their leave point, in step order: the writes that
    /// can replace a narrowed value.
    write_leaves: Vec<StepId>,
    reachable: OnceCell<Vec<bool>>,
    /// The nodes whose guards may narrow reads here ([`Self::guard_owners`]).
    guards: OnceCell<Vec<NodeRef>>,
    /// Steps reachable from the entry around each guard's entry set, as
    /// bitsets ([`Self::guarded`]).
    reachable_avoiding: RefCell<FxHashMap<Box<[StepId]>, std::sync::Arc<[u64]>>>,
    /// Each step's predecessors, built on first use; `None` when an edge
    /// leaves the graph.
    predecessors: OnceCell<Option<Adjacency>>,
    /// Built on the first single-entry guard query ([`Self::guarded`]).
    dominators: OnceCell<Dominators>,
    completion: Option<Completion>,
}

/// The dominator tree of an execution's reachable steps, as pre/post
/// numbers: `a` dominates `b` when `b`'s interval nests in `a`'s.
struct Dominators {
    /// `u32::MAX` for steps the entry never reaches.
    pre: Vec<u32>,
    post: Vec<u32>,
}

impl Dominators {
    /// Graph steps one budget unit pays for while building: each step is
    /// visited a few times, a bit test each, nowhere near a semantic step.
    const STEPS_PER_UNIT: u32 = 32;

    fn new(entry: StepId, successors: &Adjacency, predecessors: &Adjacency, budget: &Budget) -> Option<Dominators> {
        let count = successors.start.len() - 1;
        let mut visits = 0u32;
        let mut spend = || { visits += 1; visits % Self::STEPS_PER_UNIT != 1 || budget.step() };
        // Reverse postorder of the reachable steps.
        const UNSEEN: u32 = u32::MAX;
        let mut order_of = vec![UNSEEN; count];
        let mut postorder = Vec::new();
        let mut stack: Vec<(StepId, usize)> = vec![(entry, 0)];
        let mut entered = vec![false; count];
        entered[entry as usize] = true;
        while let Some(&mut (step, ref mut next)) = stack.last_mut() {
            if !spend() { return None }
            let edges = successors.of(step);
            if let Some(&child) = edges.get(*next) {
                *next += 1;
                if !std::mem::replace(entered.get_mut(child as usize)?, true) { stack.push((child, 0)) }
            } else {
                postorder.push(step);
                stack.pop();
            }
        }
        let rpo: Vec<StepId> = postorder.iter().rev().copied().collect();
        for (index, &step) in rpo.iter().enumerate() { order_of[step as usize] = index as u32 }
        // Cooper, Harvey and Kennedy: iterate immediate dominators to a fixpoint.
        let mut idom = vec![UNSEEN; count];
        idom[entry as usize] = entry;
        let mut changed = true;
        while changed {
            changed = false;
            for &step in rpo.iter().skip(1) {
                let mut new = UNSEEN;
                for &previous in predecessors.of(step) {
                    if !spend() { return None }
                    if idom[previous as usize] == UNSEEN { continue }
                    new = if new == UNSEEN { previous } else {
                        let (mut a, mut b) = (previous, new);
                        while a != b {
                            while order_of[a as usize] > order_of[b as usize] { a = idom[a as usize] }
                            while order_of[b as usize] > order_of[a as usize] { b = idom[b as usize] }
                        }
                        a
                    };
                }
                if new != UNSEEN && idom[step as usize] != new {
                    idom[step as usize] = new;
                    changed = true;
                }
            }
        }
        // Number the tree depth first.
        let mut children = vec![Vec::new(); count];
        for &step in rpo.iter().skip(1) {
            let parent = idom[step as usize];
            if parent == UNSEEN { return None }
            children[parent as usize].push(step);
        }
        let mut pre = vec![UNSEEN; count];
        let mut post = vec![UNSEEN; count];
        let mut clock = 0u32;
        let mut stack: Vec<(StepId, usize)> = vec![(entry, 0)];
        pre[entry as usize] = 0;
        while let Some(&mut (step, ref mut next)) = stack.last_mut() {
            if let Some(&child) = children[step as usize].get(*next) {
                *next += 1;
                clock += 1;
                pre[child as usize] = clock;
                stack.push((child, 0));
            } else {
                clock += 1;
                post[step as usize] = clock;
                stack.pop();
            }
        }
        Some(Dominators { pre, post })
    }

    /// Whether every path from the entry to `step` passes `guard`: `step`
    /// is unreachable, or `guard` dominates it.
    fn guards(&self, guard: StepId, step: StepId) -> bool {
        let (step, guard) = (step as usize, guard as usize);
        if self.pre[step] == u32::MAX { return true }
        self.pre[guard] != u32::MAX && self.pre[guard] <= self.pre[step] && self.post[step] <= self.post[guard]
    }
}

/// Steps' neighbours, packed: step `s` has `steps[start[s]..start[s + 1]]`.
struct Adjacency {
    start: Vec<u32>,
    steps: Vec<StepId>,
}

impl Adjacency {
    fn successors(edges: &[Vec<StepId>]) -> Adjacency {
        let mut start = Vec::with_capacity(edges.len() + 1);
        start.push(0);
        let mut steps = Vec::with_capacity(edges.iter().map(Vec::len).sum());
        for next in edges {
            steps.extend_from_slice(next);
            start.push(steps.len() as u32);
        }
        Adjacency { start, steps }
    }

    /// The reverse of `successors`; `None` when an edge leaves the graph.
    fn predecessors(successors: &Adjacency) -> Option<Adjacency> {
        let count = successors.start.len() - 1;
        let mut start = vec![0u32; count + 1];
        for &next in &successors.steps {
            *start.get_mut(next as usize + 1)? += 1;
        }
        for index in 1..start.len() { start[index] += start[index - 1] }
        let mut fill = start.clone();
        let mut steps = vec![0; successors.steps.len()];
        for from in 0..count {
            for &next in successors.of(from as StepId) {
                steps[fill[next as usize] as usize] = from as StepId;
                fill[next as usize] += 1;
            }
        }
        Some(Adjacency { start, steps })
    }

    fn get(&self, step: StepId) -> Option<&[StepId]> {
        let from = *self.start.get(step as usize)? as usize;
        let to = *self.start.get(step as usize + 1)? as usize;
        Some(&self.steps[from..to])
    }

    fn of(&self, step: StepId) -> &[StepId] {
        self.get(step).unwrap_or_default()
    }
}

#[derive(Default)]
struct ReadSet {
    sites: crate::fx_hash::FxHashSet<(NodeRef, NodeRef)>,
    steps: crate::fx_hash::FxHashSet<StepId>,
    external: OnceCell<bool>,
    used: RefCell<FxHashMap<StepId, bool>>,
}

struct Completion {
    normal: Vec<StepId>,
    exits: Vec<(StepId, Exit)>,
}

enum Walk {
    Found,
    Seen(Vec<bool>),
}

#[derive(Clone, Copy, Default)]
struct ThroughVisit {
    without_event: bool,
    with_event: bool,
    barrier: bool,
}

impl Execution {
    pub(crate) fn new(entry: StepId, steps: Vec<Step>, edges: Vec<Vec<StepId>>) -> Execution {
        let mut sites: Vec<(NodeRef, StepId)> = Vec::new();
        let mut fields: Vec<StepId> = Vec::new();
        let mut writes: FxHashMap<NodeRef, Vec<StepId>> = FxHashMap::default();
        let mut binding_writes: FxHashMap<NodeRef, crate::fx_hash::FxHashSet<StepId>> = FxHashMap::default();
        let mut binding_reads: FxHashMap<NodeRef, ReadSet> = FxHashMap::default();
        let mut write_leaves = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            if let Point::Event(event) = &step.point {
                if event.kind == EventKind::Write && event.phase == Phase::Leave { write_leaves.push(index as StepId) }
            }
            match &step.point {
                Point::Enter | Point::Leave => sites.push((step.node, index as StepId)),
                Point::Field(_) => fields.push(index as StepId),
                Point::Event(event) if event.kind == EventKind::Write => {
                    if let Some(target) = event.target {
                        writes.entry(target.value).or_default().push(index as StepId);
                        if let Some(binding) = target.binding {
                            binding_writes.entry(binding).or_default().insert(index as StepId);
                            binding_reads.entry(binding).or_default();
                        }
                    }
                }
                Point::Event(event) if event.kind == EventKind::Read => {
                    if let Some(target) = event.target {
                        if let Some(binding) = target.binding {
                            let reads = binding_reads.entry(binding).or_default();
                            reads.sites.insert((event.site, target.value));
                            reads.steps.insert(index as StepId);
                        }
                    }
                }
                _ => {}
            }
        }
        sites.sort_unstable();
        fields.sort_unstable_by(|&a, &b| Self::field_key(&steps, a).cmp(&Self::field_key(&steps, b)).then(a.cmp(&b)));
        let edges = Adjacency::successors(&edges);
        Execution { entry, steps, edges, sites, fields, writes, binding_writes, binding_reads, write_leaves,
            reachable: OnceCell::new(), guards: OnceCell::new(), reachable_avoiding: RefCell::default(),
            predecessors: OnceCell::new(), dominators: OnceCell::new(), completion: None }
    }

    pub fn write_leaves(&self) -> &[StepId] {
        &self.write_leaves
    }

    /// The nodes whose guards may narrow reads in this execution, built once
    /// by `build` (which knows the language) and shared by every read.
    pub fn guard_owners(&self, build: impl FnOnce(&Execution) -> Vec<NodeRef>) -> &[NodeRef] {
        self.guards.get_or_init(|| build(self))
    }

    /// Whether the entry reaches `step`.
    pub fn reached(&self, step: StepId, budget: &Budget) -> Known<bool> {
        let Known::Yes(seen) = self.reachable_steps(budget) else { return Known::Unknown };
        seen.get(step as usize).copied().map_or(Known::Unknown, Known::Yes)
    }

    /// Whether every path from the entry to each of `steps` passes one of
    /// `avoiding` (as [`Self::reaches`] from the entry being false for all).
    /// The steps reachable around each entry set are walked once and kept:
    /// every read in the execution asks about the same few guards.
    pub fn guarded(&self, steps: &[StepId], avoiding: &[StepId], budget: &Budget) -> Known<bool> {
        if steps.iter().any(|&step| step as usize >= self.steps.len()) { return Known::Unknown }
        // One entry step guards exactly what it dominates.
        if let [guard] = avoiding {
            if *guard as usize >= self.steps.len() { return Known::Unknown }
            let Some(dominators) = self.dominators(budget) else { return Known::Unknown };
            return Known::Yes(steps.iter().all(|&step| dominators.guards(*guard, step)));
        }
        let cached = self.reachable_avoiding.borrow().get(avoiding).cloned();
        let seen = match cached {
            Some(seen) => seen,
            None => {
                let Known::Yes(Walk::Seen(seen)) = self.walk(self.entry, avoiding, |_| false, budget) else { return Known::Unknown };
                let mut bits = vec![0u64; seen.len().div_ceil(64)];
                for (step, &reached) in seen.iter().enumerate() {
                    if reached { bits[step / 64] |= 1 << (step % 64) }
                }
                let bits: std::sync::Arc<[u64]> = bits.into();
                self.reachable_avoiding.borrow_mut().insert(avoiding.into(), bits.clone());
                bits
            }
        };
        Known::Yes(steps.iter().all(|&step| seen[step as usize / 64] & (1 << (step % 64)) == 0))
    }

    /// The entry, steps and successor lists `new` was given.
    #[cfg(test)]
    fn into_parts(self) -> (StepId, Vec<Step>, Vec<Vec<StepId>>) {
        let edges = (0..self.steps.len() as StepId).map(|step| self.edges.of(step).to_vec()).collect();
        (self.entry, self.steps, edges)
    }

    fn with_completion(mut self, normal: Vec<StepId>, exits: Vec<(StepId, Exit)>) -> Self {
        self.completion = Some(Completion { normal, exits });
        self
    }

    pub fn entry(&self) -> StepId {
        self.entry
    }

    pub fn step(&self, id: StepId) -> Option<&Step> {
        self.steps.get(id as usize)
    }

    pub fn steps(&self) -> impl Iterator<Item = (StepId, &Step)> {
        self.steps.iter().enumerate().map(|(id, step)| (id as StepId, step))
    }

    pub fn successors(&self, id: StepId) -> Option<&[StepId]> {
        self.edges.get(id)
    }

    /// The enter and leave steps of `node`, in step order.
    fn node_steps(&self, node: NodeRef) -> impl Iterator<Item = StepId> + '_ {
        let from = self.sites.partition_point(|&(site, _)| site < node);
        let to = from + self.sites[from..].partition_point(|&(site, _)| site == node);
        self.sites[from..to].iter().map(|&(_, id)| id)
    }

    pub fn sites(&self, node: NodeRef, phase: Phase) -> impl Iterator<Item = StepId> + '_ {
        self.node_steps(node).filter(move |&id| {
            matches!((&self.steps[id as usize].point, phase), (Point::Enter, Phase::Enter) | (Point::Leave, Phase::Leave))
        })
    }

    /// The node and path of field step `id` (empty path for other steps).
    fn field_key(steps: &[Step], id: StepId) -> (NodeRef, &[u16]) {
        let step = &steps[id as usize];
        let path: &[u16] = match &step.point {
            Point::Field(path) => path,
            _ => &[],
        };
        (step.node, path)
    }

    pub fn field_entries(&self, node: NodeRef, path: &[u16]) -> &[StepId] {
        let key = (node, path);
        let from = self.fields.partition_point(|&id| Self::field_key(&self.steps, id) < key);
        let to = from + self.fields[from..].partition_point(|&id| Self::field_key(&self.steps, id) == key);
        &self.fields[from..to]
    }

    pub fn write_steps(&self, target: NodeRef) -> &[StepId] {
        self.writes.get(&target).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn normal(&self, budget: &Budget) -> Known<bool> {
        let Some(completion) = &self.completion else { return Known::Unknown };
        let Some(seen) = self.reachable_steps(budget).known() else { return Known::Unknown };
        for &at in &completion.normal {
            if !budget.step() { return Known::Unknown }
            if seen[at as usize] { return Known::Yes(true) }
        }
        Known::Yes(false)
    }

    pub(crate) fn exits(&self, budget: &Budget) -> Known<Vec<Exit>> {
        let Some(completion) = &self.completion else { return Known::Unknown };
        let Some(seen) = self.reachable_steps(budget).known() else { return Known::Unknown };
        let mut exits = Vec::new();
        let mut unique = crate::fx_hash::FxHashSet::default();
        for &(at, exit) in &completion.exits {
            if !budget.step() { return Known::Unknown }
            if seen[at as usize] && unique.insert(exit) { exits.push(exit) }
        }
        Known::Yes(exits)
    }

    pub fn completes_avoiding(&self, avoiding: &[StepId], effects: &[EffectId], budget: &Budget) -> Known<bool> {
        let Some(completion) = &self.completion else { return Known::Unknown };
        let Known::Yes(Walk::Seen(seen)) = self.walk(self.entry, avoiding, |_| false, budget) else { return Known::Unknown };
        for &step in &completion.normal {
            if !budget.step() { return Known::Unknown }
            if seen[step as usize] { return Known::Yes(true) }
        }
        for &(step, exit) in &completion.exits {
            if !budget.step() { return Known::Unknown }
            if effects.contains(&exit.effect) && seen[step as usize] { return Known::Yes(true) }
        }
        Known::Yes(false)
    }

    pub fn completes_through(&self, effect: EffectId, effects: &[EffectId], budget: &Budget) -> Known<bool> {
        let Some(completion) = &self.completion else { return Known::Unknown };
        let targets: Vec<StepId> = completion.normal.iter().copied()
            .chain(completion.exits.iter().filter(|(_, exit)| effects.contains(&exit.effect)).map(|(step, _)| *step))
            .collect();
        if targets.is_empty() { return Known::Yes(false) }
        self.reaches_any_through(self.entry, &targets, &[], |event| event.kind == EventKind::Effect(effect), budget)
    }

    fn reachable_steps(&self, budget: &Budget) -> Known<&[bool]> {
        if !budget.step() { return Known::Unknown }
        let seen = match self.reachable.get() {
            Some(seen) => seen,
            None => match self.walk(self.entry, &[], |_| false, budget) {
                Known::Yes(Walk::Seen(seen)) => self.reachable.get_or_init(|| seen),
                _ => return Known::Unknown,
            },
        };
        Known::Yes(seen)
    }

    pub fn reachable(&self, node: NodeRef, budget: &Budget) -> Known<bool> {
        let Some(seen) = self.reachable_steps(budget).known() else { return Known::Unknown };
        Known::Yes(self.node_steps(node).any(|step| seen[step as usize]))
    }

    pub fn reaches(&self, from: StepId, to: StepId, avoiding: &[StepId], budget: &Budget) -> Known<bool> {
        if to as usize >= self.steps.len() { return Known::Unknown }
        match self.walk(from, avoiding, |step| step == to, budget) {
            Known::Yes(Walk::Found) => Known::Yes(true),
            Known::Yes(Walk::Seen(_)) => Known::Yes(false),
            Known::Unknown => Known::Unknown,
        }
    }

    pub(crate) fn value_used(&self, target: NodeRef, catalogue: &controller::Reads, budget: &Budget) -> Known<bool> {
        if !budget.step() { return Known::Unknown }
        let Some(writes) = self.writes.get(&target) else { return Known::Unknown };
        let mut binding = None;
        for &write in writes {
            if !budget.step() { return Known::Unknown }
            let Some(current) = self.step(write).and_then(Step::event).and_then(|event| event.target).and_then(|target| target.binding) else {
                return Known::Unknown;
            };
            if binding.is_some_and(|binding| binding != current) { return Known::Unknown }
            binding = Some(current);
        }
        let Some(binding) = binding else { return Known::Unknown };
        let Some(reads) = self.binding_reads.get(&binding) else { return Known::Unknown };
        let external = match reads.external.get() {
            Some(external) => *external,
            None => {
                let mut external = false;
                for site in catalogue.get(&binding).into_iter().flatten() {
                    if !budget.step() { return Known::Unknown }
                    if !reads.sites.contains(site) { external = true; break }
                }
                *reads.external.get_or_init(|| external)
            }
        };
        let Some(reachable) = self.reachable_steps(budget).known() else { return Known::Unknown };
        let Some(overwrites) = self.binding_writes.get(&binding) else { return Known::Unknown };
        let mut has_reachable_write = false;
        for &write in writes {
            if !budget.step() { return Known::Unknown }
            if !reachable[write as usize] { continue }
            has_reachable_write = true;
            if external { return Known::Yes(true) }
            let cached = reads.used.borrow().get(&write).copied();
            let used = match cached {
                Some(used) => used,
                None => {
                    let Some(used) = self.read_before_overwrite(write, &reads.steps, overwrites, budget).known() else { return Known::Unknown };
                    reads.used.borrow_mut().insert(write, used);
                    used
                }
            };
            if used { return Known::Yes(true) }
        }
        if has_reachable_write { Known::Yes(false) } else { Known::Unknown }
    }

    fn read_before_overwrite(&self, write: StepId, reads: &crate::fx_hash::FxHashSet<StepId>,
        overwrites: &crate::fx_hash::FxHashSet<StepId>, budget: &Budget) -> Known<bool> {
        let Some(next) = self.successors(write) else { return Known::Unknown };
        let mut pending = next.to_vec();
        let mut seen = vec![false; self.steps.len()];
        while let Some(step) = pending.pop() {
            if !budget.step() || step as usize >= seen.len() { return Known::Unknown }
            if seen[step as usize] || overwrites.contains(&step) { continue }
            seen[step as usize] = true;
            if reads.contains(&step) { return Known::Yes(true) }
            let Some(next) = self.successors(step) else { return Known::Unknown };
            for &next in next {
                if !budget.step() || next as usize >= seen.len() { return Known::Unknown }
                pending.push(next);
            }
        }
        Known::Yes(false)
    }

    pub fn crosses(&self, starts: &[StepId], observations: &[StepId], effects: &[EffectId],
        barriers: &[StepId], budget: &Budget) -> Known<bool> {
        let mut seen = vec![ThroughVisit::default(); self.steps.len()];
        let mut targets = crate::fx_hash::FxHashSet::default();
        for &step in starts.iter().chain(observations).chain(barriers) {
            if !budget.step() || self.step(step).is_none() { return Known::Unknown }
        }
        targets.extend(observations.iter().copied());
        for &step in barriers { seen[step as usize].barrier = true; }
        if starts.is_empty() || targets.is_empty() || effects.is_empty() { return Known::Yes(false) }
        let Some(reachable) = self.reachable_steps(budget).known() else { return Known::Unknown };
        let mut pending: Vec<_> = starts.iter().copied().filter(|&step| reachable[step as usize])
            .map(|step| (step, false)).collect();
        while let Some((step, crossed)) = pending.pop() {
            if !budget.step() { return Known::Unknown }
            if crossed && seen[step as usize].barrier { continue }
            let crossed = crossed || self.steps[step as usize].event().is_some_and(|event| {
                matches!(event.kind, EventKind::Effect(effect) if effects.contains(&effect))
            });
            let visit = if crossed { &mut seen[step as usize].with_event } else { &mut seen[step as usize].without_event };
            if *visit { continue }
            *visit = true;
            if crossed && targets.contains(&step) { return Known::Yes(true) }
            let Some(next) = self.successors(step) else { return Known::Unknown };
            for &next in next {
                if !budget.step() || next as usize >= self.steps.len() { return Known::Unknown }
                pending.push((next, crossed));
            }
        }
        Known::Yes(false)
    }

    pub fn prior_event(&self, node: NodeRef, effect: EffectId, budget: &Budget) -> Known<bool> {
        let Some(seen) = self.reachable_steps(budget).known() else { return Known::Unknown };
        let mut events = Vec::new();
        let mut targets = Vec::new();
        for (step, point) in self.steps() {
            if !budget.step() { return Known::Unknown }
            if let Some(event) = point.event().filter(|event| event.kind == EventKind::Effect(effect)) {
                if seen[step as usize] { events.push(step) }
                if event.site == node { targets.push(step) }
            }
        }
        for event in events {
            let Some(next) = self.successors(event) else { return Known::Unknown };
            for &next in next {
                for &target in &targets {
                    match self.reaches(next, target, &[], budget) {
                        Known::Yes(true) => return Known::Yes(true),
                        Known::Yes(false) => {}
                        Known::Unknown => return Known::Unknown,
                    }
                }
            }
        }
        Known::Yes(false)
    }

    /// The reachable `candidates` some path takes last before reaching one of
    /// `uses` (no other candidate after it); `None` when a reachable use can
    /// be reached from the entry passing no candidate. One backward walk from
    /// the uses, stopping at candidates.
    pub fn last_steps(&self, uses: &[StepId], candidates: &[StepId], budget: &Budget) -> Known<Option<Vec<StepId>>> {
        for &step in uses.iter().chain(candidates) {
            if !budget.step() || self.step(step).is_none() { return Known::Unknown }
        }
        let Some(seen) = self.reachable_steps(budget).known() else { return Known::Unknown };
        let uses: Vec<_> = uses.iter().copied().filter(|&step| seen[step as usize]).collect();
        if uses.is_empty() { return Known::Yes(None) }
        let Some(predecessors) = self.predecessors() else { return Known::Unknown };
        let mut candidate = vec![false; self.steps.len()];
        for &step in candidates { candidate[step as usize] = true }
        let mut visited = vec![false; self.steps.len()];
        let mut last_before = vec![false; self.steps.len()];
        let mut pending: Vec<StepId> = uses.iter().copied().filter(|&step| !candidate[step as usize]).collect();
        while let Some(step) = pending.pop() {
            if !budget.step() { return Known::Unknown }
            if std::mem::replace(&mut visited[step as usize], true) { continue }
            if step == self.entry { return Known::Yes(None) }
            for &previous in predecessors.of(step) {
                if !budget.step() { return Known::Unknown }
                if candidate[previous as usize] {
                    last_before[previous as usize] = true;
                } else if !visited[previous as usize] {
                    pending.push(previous);
                }
            }
        }
        let mut last = Vec::new();
        for &step in candidates {
            if seen[step as usize] && last_before[step as usize] && !last.contains(&step) { last.push(step) }
        }
        Known::Yes(Some(last))
    }

    /// Each step's predecessors, built once; `None` when an edge leaves the
    /// graph.
    fn predecessors(&self) -> Option<&Adjacency> {
        self.predecessors.get_or_init(|| Adjacency::predecessors(&self.edges)).as_ref()
    }

    /// The dominator tree, kept once a build finishes within `budget`.
    fn dominators(&self, budget: &Budget) -> Option<&Dominators> {
        if let Some(dominators) = self.dominators.get() { return Some(dominators) }
        let dominators = Dominators::new(self.entry, &self.edges, self.predecessors()?, budget)?;
        Some(self.dominators.get_or_init(|| dominators))
    }

    /// The steps some path from one of `starts` reaches after passing an
    /// `event` step without crossing a barrier (a start may be one), as a
    /// bitset: [`Self::reaches_any_through`] from every start, for every
    /// target at once.
    pub fn reached_through(&self, starts: &[StepId], barriers: &[StepId], event: impl Fn(&Event) -> bool, budget: &Budget) -> Known<Vec<u64>> {
        if starts.iter().chain(barriers).any(|&step| step as usize >= self.steps.len()) { return Known::Unknown }
        let mut seen = vec![ThroughVisit::default(); self.steps.len()];
        for &barrier in barriers {
            if !budget.step() { return Known::Unknown }
            seen[barrier as usize].barrier = true;
        }
        let mut reached = vec![0u64; self.steps.len().div_ceil(64)];
        let mut pending: Vec<(StepId, bool, bool)> = starts.iter().map(|&step| (step, false, true)).collect();
        while let Some((step, matched, start)) = pending.pop() {
            if !budget.step() { return Known::Unknown }
            if !start && seen[step as usize].barrier { continue }
            let matched = matched || self.steps[step as usize].event().is_some_and(&event);
            let visit = if matched { &mut seen[step as usize].with_event } else { &mut seen[step as usize].without_event };
            if *visit { continue }
            *visit = true;
            if matched { reached[step as usize / 64] |= 1 << (step % 64) }
            let Some(edges) = self.edges.get(step) else { return Known::Unknown };
            for &next in edges {
                if !budget.step() || next as usize >= self.steps.len() { return Known::Unknown }
                pending.push((next, matched, false));
            }
        }
        Known::Yes(reached)
    }

    pub fn reaches_through(&self, from: StepId, to: StepId, barriers: &[StepId], event: impl Fn(&Event) -> bool, budget: &Budget) -> Known<bool> {
        self.reaches_any_through(from, &[to], barriers, event, budget)
    }

    /// Whether some path from `from` passes an `event` step and then reaches
    /// one of `targets` without crossing a barrier (`from` itself may be
    /// one): [`Self::reaches_through`] for many targets in one walk.
    pub fn reaches_any_through(&self, from: StepId, targets: &[StepId], barriers: &[StepId], event: impl Fn(&Event) -> bool, budget: &Budget) -> Known<bool> {
        if from as usize >= self.steps.len() || targets.iter().any(|&to| to as usize >= self.steps.len()) { return Known::Unknown }
        let mut target = vec![false; self.steps.len()];
        for &to in targets { target[to as usize] = true }
        let mut seen = vec![ThroughVisit::default(); self.steps.len()];
        for &barrier in barriers {
            if !budget.step() || barrier as usize >= self.steps.len() { return Known::Unknown }
            seen[barrier as usize].barrier = true;
        }
        let mut pending = vec![(from, false)];
        let mut initial = true;
        while let Some((step, matched)) = pending.pop() {
            if !budget.step() { return Known::Unknown }
            let starts_here = std::mem::take(&mut initial);
            if !starts_here && seen[step as usize].barrier { continue }
            let matched = matched || self.steps[step as usize].event().is_some_and(&event);
            let visit = if matched { &mut seen[step as usize].with_event } else { &mut seen[step as usize].without_event };
            if *visit { continue }
            *visit = true;
            if matched && target[step as usize] { return Known::Yes(true) }
            let Some(edges) = self.edges.get(step) else { return Known::Unknown };
            for &next in edges {
                if !budget.step() || next as usize >= self.steps.len() { return Known::Unknown }
                pending.push((next, matched));
            }
        }
        Known::Yes(false)
    }

    fn walk(&self, from: StepId, avoiding: &[StepId], found: impl Fn(StepId) -> bool, budget: &Budget) -> Known<Walk> {
        if from as usize >= self.steps.len() { return Known::Unknown }
        let mut blocked = vec![false; self.steps.len()];
        for &step in avoiding {
            if !budget.step() || step as usize >= self.steps.len() { return Known::Unknown }
            blocked[step as usize] = true;
        }
        let mut seen = vec![false; self.steps.len()];
        let mut pending = vec![from];
        while let Some(step) = pending.pop() {
            if !budget.step() { return Known::Unknown }
            if blocked[step as usize] || seen[step as usize] { continue }
            seen[step as usize] = true;
            if found(step) { return Known::Yes(Walk::Found) }
            let Some(edges) = self.edges.get(step) else { return Known::Unknown };
            for &next in edges {
                if !budget.step() || next as usize >= self.steps.len() { return Known::Unknown }
                if !seen[next as usize] { pending.push(next) }
            }
        }
        Known::Yes(Walk::Seen(seen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[&[StepId]]) -> Execution {
        Execution::new(0,
            (0..edges.len()).map(|node| Step { node: NodeRef::here(node as u32), point: Point::Enter }).collect(),
            edges.iter().map(|edges| edges.to_vec()).collect(),
        )
    }

    #[test]
    fn reachability_keeps_branch_alternatives_and_dead_sites_separate() {
        let graph = graph(&[&[1, 2], &[3], &[3], &[], &[]]);
        for node in [0, 1, 2, 3] {
            assert_eq!(graph.reachable(NodeRef::here(node), &Budget::default()), Known::Yes(true));
        }
        assert_eq!(graph.reachable(NodeRef::here(4), &Budget::default()), Known::Yes(false));
        assert_eq!(graph.reaches(1, 2, &[], &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn avoiding_events_blocks_only_the_paths_that_run_them() {
        let graph = graph(&[&[1, 2], &[3], &[3], &[]]);
        assert_eq!(graph.reaches(0, 3, &[1], &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reaches(0, 3, &[1, 2], &Budget::default()), Known::Yes(false));
        assert_eq!(graph.reaches(0, 3, &[3], &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn guard_reachability_matches_reaches_for_every_entry_set() {
        let graph = graph(&[&[1, 2], &[3], &[3], &[]]);
        assert_eq!(graph.guarded(&[3], &[1], &Budget::new(0)), Known::Unknown);
        assert_eq!(graph.guarded(&[3], &[1], &Budget::default()), Known::Yes(false));
        assert_eq!(graph.guarded(&[3], &[1, 2], &Budget::default()), Known::Yes(true));
        assert_eq!(graph.guarded(&[2, 3], &[1], &Budget::default()), Known::Yes(false));
        assert_eq!(graph.guarded(&[3], &[1, 2], &Budget::default()), Known::Yes(true));
        assert_eq!(graph.guarded(&[9], &[1, 2], &Budget::default()), Known::Unknown);
    }

    #[test]
    fn single_guards_agree_with_walks_around_them() {
        let graphs: [&[&[StepId]]; 4] = [
            &[&[1, 2], &[3], &[3], &[4], &[]],
            &[&[1], &[2, 4], &[3], &[1], &[5], &[]],
            &[&[1, 2], &[3], &[3, 4], &[5], &[5], &[], &[6]],
            &[&[1], &[2, 3], &[1], &[4], &[1, 5], &[]],
        ];
        for edges in graphs {
            let graph = graph(edges);
            for guard in 0..edges.len() as StepId {
                for step in 0..edges.len() as StepId {
                    let walked = match graph.reaches(0, step, &[guard], &Budget::default()) {
                        Known::Yes(reached) => !reached,
                        Known::Unknown => panic!("small walk finishes"),
                    };
                    assert_eq!(graph.guarded(&[step], &[guard], &Budget::default()), Known::Yes(walked), "guard {guard} step {step} in {edges:?}");
                }
            }
        }
    }

    #[test]
    fn back_edges_do_not_hide_exits_or_turn_missing_paths_into_unknown() {
        let graph = graph(&[&[1], &[0, 2], &[], &[]]);
        assert_eq!(graph.reaches(0, 2, &[], &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reaches(0, 3, &[], &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn exhausted_work_cannot_be_used_as_a_negative_proof() {
        let graph = graph(&[&[1], &[2], &[]]);
        assert_eq!(graph.reaches(0, 2, &[], &Budget::new(0)), Known::Unknown);
        assert_eq!(graph.reaches(0, 2, &[], &Budget::new(1)), Known::Unknown);
        assert_eq!(graph.reaches(0, 2, &[], &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reachable(NodeRef::here(2), &Budget::new(1)), Known::Unknown);
        assert_eq!(graph.reachable(NodeRef::here(2), &Budget::default()), Known::Yes(true));
    }

    /// `graph` with step `step` replaced by `event`.
    fn with_point(graph: Execution, step: StepId, event: Event) -> Execution {
        let (entry, mut steps, edges) = graph.into_parts();
        steps[step as usize].point = Point::Event(Box::new(event));
        Execution::new(entry, steps, edges)
    }

    fn with_write(graph: Execution, step: StepId) -> Execution {
        with_point(graph, step, Event {
            kind: EventKind::Write,
            site: NodeRef::here(step),
            target: Some(Target { value: NodeRef::here(99), binding: Some(NodeRef::here(99)) }),
            value: None,
            written: None,
            phase: Phase::Leave,
        })
    }

    fn with_writes(mut graph: Execution, writes: &[(StepId, u32)]) -> Execution {
        for &(step, target) in writes {
            graph = with_point(graph, step, Event {
                kind: EventKind::Write, site: NodeRef::here(step),
                target: Some(Target { value: NodeRef::here(target), binding: Some(NodeRef::here(99)) }),
                value: None, written: None, phase: Phase::Leave,
            });
        }
        graph
    }

    fn with_read(graph: Execution, step: StepId) -> Execution {
        with_point(graph, step, Event {
            kind: EventKind::Read, site: NodeRef::here(step),
            target: Some(Target { value: NodeRef::here(step), binding: Some(NodeRef::here(99)) }),
            value: None, written: None, phase: Phase::Enter,
        })
    }

    fn read_catalogue(nodes: &[u32]) -> controller::Reads {
        [(NodeRef::here(99), nodes.iter().copied().map(|node| (NodeRef::here(node), NodeRef::here(node))).collect())].into_iter().collect()
    }

    #[test]
    fn value_use_stops_at_overwrites_but_keeps_branch_reads() {
        let reads = read_catalogue(&[3]);
        let sequence = with_read(with_writes(graph(&[&[1], &[2], &[3], &[]]), &[(1, 10), (2, 20)]), 3);
        assert_eq!(sequence.value_used(NodeRef::here(10), &reads, &Budget::default()), Known::Yes(false));
        assert_eq!(sequence.value_used(NodeRef::here(20), &reads, &Budget::default()), Known::Yes(true));
        let branch = with_read(with_writes(graph(&[&[1], &[2, 3], &[4], &[4], &[]]), &[(1, 10), (2, 20)]), 3);
        assert_eq!(branch.value_used(NodeRef::here(10), &reads, &Budget::default()), Known::Yes(true));
    }

    #[test]
    fn value_use_cache_never_keeps_incomplete_queries() {
        let reads = read_catalogue(&[3]);
        let execution = with_read(with_writes(graph(&[&[1], &[2], &[3], &[], &[]]), &[(1, 10)]), 3);
        assert_eq!(execution.reachable(NodeRef::here(2), &Budget::default()), Known::Yes(true));
        assert_eq!(execution.value_used(NodeRef::here(10), &reads, &Budget::new(7)), Known::Unknown);
        assert_eq!(execution.value_used(NodeRef::here(10), &reads, &Budget::default()), Known::Yes(true));
        assert_eq!(execution.value_used(NodeRef::here(10), &reads, &Budget::default()), Known::Yes(true));
        assert_eq!(execution.value_used(NodeRef::here(50), &reads, &Budget::default()), Known::Unknown);
    }

    #[test]
    fn external_reads_are_conservative_but_dead_writes_are_not_proofs() {
        let reads = read_catalogue(&[200]);
        let execution = with_writes(graph(&[&[1], &[]]), &[(1, 10)]);
        assert_eq!(execution.value_used(NodeRef::here(10), &reads, &Budget::default()), Known::Yes(true));
        let dead = with_writes(graph(&[&[], &[]]), &[(1, 10)]);
        assert_eq!(dead.value_used(NodeRef::here(10), &reads, &Budget::default()), Known::Unknown);
    }

    fn with_event(graph: Execution, step: StepId, effect: EffectId) -> Execution {
        with_point(graph, step, Event {
            kind: EventKind::Effect(effect), site: NodeRef::here(step),
            target: None, value: None, written: None, phase: Phase::Leave,
        })
    }

    #[test]
    fn crossing_keeps_start_event_and_observation_on_one_route() {
        let split = with_event(graph(&[&[1, 2], &[3], &[3], &[]]), 2, 9);
        assert_eq!(split.crosses(&[1], &[3], &[9], &[], &Budget::default()), Known::Yes(false));
        let joined = with_event(graph(&[&[1, 2], &[3], &[3], &[4], &[]]), 3, 9);
        assert_eq!(joined.crosses(&[1, 2], &[4], &[9], &[], &Budget::default()), Known::Yes(true));
        let dead = with_event(graph(&[&[3], &[2], &[3], &[]]), 2, 9);
        assert_eq!(dead.crosses(&[1], &[3], &[9], &[], &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn crossing_refresh_stops_only_post_event_paths_and_can_start_again() {
        let sequence = with_event(graph(&[&[1], &[2], &[3], &[4], &[]]), 2, 9);
        assert_eq!(sequence.crosses(&[0], &[4], &[9], &[1], &Budget::default()), Known::Yes(true));
        assert_eq!(sequence.crosses(&[0], &[4], &[9], &[3], &Budget::default()), Known::Yes(false));
        let looped = with_event(with_event(graph(&[&[1], &[2], &[3], &[1, 4], &[5], &[]]), 2, 9), 4, 9);
        assert_eq!(looped.crosses(&[0], &[5], &[9], &[3], &Budget::default()), Known::Yes(false));
        assert_eq!(looped.crosses(&[0, 3], &[5], &[9], &[3], &Budget::default()), Known::Yes(true));
        assert_eq!(looped.crosses(&[0], &[5], &[9], &[], &Budget::new(0)), Known::Unknown);
        assert_eq!(looped.crosses(&[99], &[5], &[9], &[], &Budget::default()), Known::Unknown);
    }

    #[test]
    fn a_write_on_an_unrelated_arm_is_not_between_guard_and_use() {
        let graph = with_write(graph(&[&[1, 2], &[3], &[4], &[], &[]]), 2);
        assert_eq!(graph.reaches_through(0, 3, &[], |event| event.kind == EventKind::Write, &Budget::default()), Known::Yes(false));
        assert_eq!(graph.reaches_through(0, 4, &[], |event| event.kind == EventKind::Write, &Budget::default()), Known::Yes(true));
    }

    #[test]
    fn a_later_round_write_can_reach_a_use_already_seen_without_it() {
        let graph = with_write(graph(&[&[1], &[2, 3], &[1], &[]]), 2);
        assert_eq!(graph.reaches_through(0, 3, &[], |event| event.kind == EventKind::Write, &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reaches_through(0, 3, &[], |event| event.kind == EventKind::Read, &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn an_exhausted_event_path_search_is_not_a_proof_of_no_write() {
        let graph = with_write(graph(&[&[1], &[]]), 1);
        assert_eq!(graph.reaches_through(0, 1, &[], |_| true, &Budget::new(0)), Known::Unknown);
        assert_eq!(graph.reaches_through(0, 1, &[], |_| true, &Budget::default()), Known::Yes(true));
    }

    #[test]
    fn fresh_guard_stops_writes_from_an_earlier_loop_round() {
        let graph = with_write(graph(&[&[1], &[2], &[0]]), 2);
        assert_eq!(graph.reaches_through(0, 1, &[0], |event| event.kind == EventKind::Write, &Budget::default()), Known::Yes(false));
        assert_eq!(graph.reaches_through(0, 1, &[], |event| event.kind == EventKind::Write, &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reaches_through(0, 2, &[0], |event| event.kind == EventKind::Write, &Budget::default()), Known::Yes(true));
    }

    #[test]
    fn completion_queries_keep_event_paths_and_selected_exits_separate() {
        let graph = with_event(graph(&[&[1, 2], &[3], &[4], &[], &[]]), 1, 9);
        let graph = graph.with_completion(vec![3], vec![(4, Exit {
            effect: 7, site: NodeRef::here(4), target: None, value: None,
        })]);
        assert_eq!(graph.completes_avoiding(&[1], &[], &Budget::default()), Known::Yes(false));
        assert_eq!(graph.completes_avoiding(&[1], &[7], &Budget::default()), Known::Yes(true));
        assert_eq!(graph.completes_through(9, &[], &Budget::default()), Known::Yes(true));
        assert_eq!(graph.completes_through(8, &[7], &Budget::default()), Known::Yes(false));
        assert_eq!(graph.completes_avoiding(&[], &[7], &Budget::new(0)), Known::Unknown);
        assert_eq!(graph.completes_through(9, &[7], &Budget::new(0)), Known::Unknown);
    }

    #[test]
    fn prior_events_include_children_and_previous_rounds_but_not_the_current_event() {
        for (end, repeated) in [(&[][..], false), (&[0][..], true)] {
            let graph = with_event(graph(&[&[1], &[2], &[3], &[4], end]), 1, 9);
            let graph = with_point(graph, 3, Event {
                kind: EventKind::Effect(9), site: NodeRef::here(0),
                target: None, value: None, written: None, phase: Phase::Leave,
            });
            assert_eq!(graph.prior_event(NodeRef::here(0), 9, &Budget::default()), Known::Yes(true));
            assert_eq!(graph.prior_event(NodeRef::here(1), 9, &Budget::default()), Known::Yes(repeated));
            assert_eq!(graph.prior_event(NodeRef::here(0), 9, &Budget::new(0)), Known::Unknown);
        }
    }

    #[test]
    fn last_steps_distinguish_overwrites_branch_writes_and_unwritten_routes() {
        let sequence = graph(&[&[1], &[2], &[3], &[], &[]]);
        assert_eq!(sequence.last_steps(&[3], &[1, 2], &Budget::default()), Known::Yes(Some(vec![2])));
        assert_eq!(sequence.last_steps(&[4], &[1, 2], &Budget::default()), Known::Yes(None));
        let branches = graph(&[&[1, 2], &[3], &[3], &[]]);
        assert_eq!(branches.last_steps(&[3], &[1, 2], &Budget::default()), Known::Yes(Some(vec![1, 2])));
        assert_eq!(branches.last_steps(&[3], &[1], &Budget::default()), Known::Yes(None));
        let looped = graph(&[&[1], &[2], &[1, 3], &[]]);
        assert_eq!(looped.last_steps(&[3], &[1, 2], &Budget::default()), Known::Yes(Some(vec![2])));
        assert_eq!(branches.last_steps(&[1, 3], &[1, 2], &Budget::new(0)), Known::Unknown);
        assert_eq!(branches.last_steps(&[99], &[1], &Budget::default()), Known::Unknown);
    }
}

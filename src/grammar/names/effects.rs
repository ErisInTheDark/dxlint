use super::question::{Budget, Known, NodeRef};
use crate::fx_hash::FxHashMap;
use std::cell::OnceCell;

pub mod compile;
pub mod build;
pub use build::Control;
mod controller;
mod source;
pub use controller::{DeclaredEffect, Effects, ExecutedEffect};
pub(crate) use controller::State;

pub type Path = Vec<u16>;
pub type StepId = u32;
pub type EffectId = u16;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: EventKind,
    pub site: NodeRef,
    pub target: Option<Target>,
    pub value: Option<NodeRef>,
    pub phase: Phase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Exit {
    pub effect: EffectId,
    pub site: NodeRef,
    pub target: Option<NodeRef>,
    pub value: Option<NodeRef>,
}

#[derive(Clone, Debug)]
pub enum Point {
    Enter,
    Leave,
    Field(Path),
    Event(Event),
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
            Point::Event(event) => Some(event),
            _ => None,
        }
    }
}

pub struct Execution {
    entry: StepId,
    steps: Vec<Step>,
    edges: Vec<Vec<StepId>>,
    sites: FxHashMap<NodeRef, Vec<StepId>>,
    fields: FxHashMap<NodeRef, FxHashMap<Path, Vec<StepId>>>,
    reachable: OnceCell<Vec<bool>>,
    completion: Option<Completion>,
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
        let mut sites: FxHashMap<NodeRef, Vec<StepId>> = FxHashMap::default();
        let mut fields: FxHashMap<NodeRef, FxHashMap<Path, Vec<StepId>>> = FxHashMap::default();
        for (index, step) in steps.iter().enumerate() {
            match &step.point {
                Point::Enter | Point::Leave => sites.entry(step.node).or_default().push(index as u32),
                Point::Field(path) => fields.entry(step.node).or_default().entry(path.clone()).or_default().push(index as u32),
                _ => {}
            }
        }
        Execution { entry, steps, edges, sites, fields, reachable: OnceCell::new(), completion: None }
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
        self.edges.get(id as usize).map(Vec::as_slice)
    }

    pub fn sites(&self, node: NodeRef, phase: Phase) -> impl Iterator<Item = StepId> + '_ {
        self.sites.get(&node).into_iter().flatten().copied().filter(move |&id| {
            matches!((&self.steps[id as usize].point, phase), (Point::Enter, Phase::Enter) | (Point::Leave, Phase::Leave))
        })
    }

    pub fn field_entries(&self, node: NodeRef, path: &[u16]) -> &[StepId] {
        self.fields.get(&node).and_then(|fields| fields.get(path)).map_or(&[], Vec::as_slice)
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
        Known::Yes(self.sites.get(&node).is_some_and(|sites| sites.iter().any(|&step| seen[step as usize])))
    }

    pub fn reaches(&self, from: StepId, to: StepId, avoiding: &[StepId], budget: &Budget) -> Known<bool> {
        if to as usize >= self.steps.len() { return Known::Unknown }
        match self.walk(from, avoiding, |step| step == to, budget) {
            Known::Yes(Walk::Found) => Known::Yes(true),
            Known::Yes(Walk::Seen(_)) => Known::Yes(false),
            Known::Unknown => Known::Unknown,
        }
    }

    pub fn reaches_through(&self, from: StepId, to: StepId, barriers: &[StepId], event: impl Fn(&Event) -> bool, budget: &Budget) -> Known<bool> {
        if from as usize >= self.steps.len() || to as usize >= self.steps.len() { return Known::Unknown }
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
            if step == to && matched { return Known::Yes(true) }
            let Some(edges) = self.edges.get(step as usize) else { return Known::Unknown };
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
            let Some(edges) = self.edges.get(step as usize) else { return Known::Unknown };
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

    fn with_write(graph: Execution, step: StepId) -> Execution {
        let mut steps = graph.steps;
        steps[step as usize].point = Point::Event(Event {
            kind: EventKind::Write,
            site: NodeRef::here(step),
            target: Some(Target { value: NodeRef::here(99), binding: Some(NodeRef::here(99)) }),
            value: None,
            phase: Phase::Leave,
        });
        Execution::new(graph.entry, steps, graph.edges)
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
}

use super::question::{Budget, Known, NodeRef};
use crate::fx_hash::FxHashMap;
use std::cell::OnceCell;

pub mod compile;

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
        next: Option<Box<Flow>>,
        first: bool,
    },
    Select { entries: Path, test: Path, body: Path },
}

#[derive(Clone, Debug)]
pub struct Conditional<T> {
    pub item: T,
    pub when: Option<u32>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit {
    pub effect: EffectId,
    pub site: NodeRef,
    pub target: Option<NodeRef>,
    pub value: Option<NodeRef>,
}

#[derive(Clone, Debug)]
pub struct Step {
    pub node: NodeRef,
    pub phase: Phase,
    pub event: Option<Event>,
}

pub struct Execution {
    entry: StepId,
    steps: Vec<Step>,
    edges: Vec<Vec<StepId>>,
    sites: FxHashMap<NodeRef, Vec<StepId>>,
    reachable: OnceCell<Vec<bool>>,
}

enum Walk {
    Found,
    Seen(Vec<bool>),
}

impl Execution {
    pub fn new(entry: StepId, steps: Vec<Step>, edges: Vec<Vec<StepId>>) -> Execution {
        let mut sites: FxHashMap<NodeRef, Vec<StepId>> = FxHashMap::default();
        for (index, step) in steps.iter().enumerate() {
            sites.entry(step.node).or_default().push(index as u32);
        }
        Execution { entry, steps, edges, sites, reachable: OnceCell::new() }
    }

    pub fn reachable(&self, node: NodeRef, budget: &Budget) -> Known<bool> {
        if !budget.step() { return Known::Unknown }
        let seen = match self.reachable.get() {
            Some(seen) => seen,
            None => match self.walk(self.entry, &[], |_| false, budget) {
                Known::Yes(Walk::Seen(seen)) => self.reachable.get_or_init(|| seen),
                _ => return Known::Unknown,
            },
        };
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
            (0..edges.len()).map(|node| Step { node: NodeRef::here(node as u32), phase: Phase::Enter, event: None }).collect(),
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
}

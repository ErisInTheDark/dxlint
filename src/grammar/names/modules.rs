use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use super::question::{Known, QueryContext, ScopeRef, Target};

pub mod compile;
pub mod resolution;
pub mod queries;
pub mod index;
mod metadata;
mod configuration;
mod profiles;
mod context;
mod inputs;

/// A module question whose answer depends only on the program's files and
/// settings, so every world of a lint run may share it
/// ([`super::question::World::shared_modules`]).
#[derive(Clone, Copy)]
pub enum SharedQuestion<'a> {
    /// What `scope` publishes as `name` in `class`.
    Publication { kind: index::PublicationKind, scope: ScopeRef, name: &'a str, class: u64 },
    /// What the program's global scopes declare as `name` in `class`.
    Global { name: &'a str, class: u64 },
}

#[derive(Clone, Debug)]
pub enum Entry {
    Local { name: Box<str>, classes: u64, targets: Vec<Target> },
    Reexport { name: Box<str>, classes: u64, asks: u64, source: Known<ScopeRef>, imported: Box<str> },
    Namespace { name: Box<str>, classes: u64, source: Known<ScopeRef> },
    Star { classes: u64, asks: u64, source: Known<ScopeRef> },
}

pub trait Source {
    fn entries(&self, scope: ScopeRef, name: &str, class: u64, question: &QueryContext) -> Known<Vec<Entry>>;
    fn star_excludes(&self, scope: ScopeRef, name: &str, question: &QueryContext) -> Known<bool>;
    fn publication_kind(&self) -> index::PublicationKind { index::PublicationKind::Exports }
    fn revision(&self) -> u64 { 0 }
}

pub struct Modules<'a, S> {
    pub source: &'a S,
}

#[derive(Default)]
pub struct Memo {
    state: RefCell<MemoState>,
}

/// Frames and targets are kept with what the publication walk that found
/// them had read of the editor's file so far ([`crate::footprint`]).
#[derive(Default)]
struct MemoState {
    revision: Option<u64>,
    frames: BTreeMap<(index::PublicationKind, Key), (Frame, crate::footprint::Footprint)>,
    targets: BTreeMap<(index::PublicationKind, Key), (Vec<Target>, crate::footprint::Footprint)>,
    pending: BTreeSet<(index::PublicationKind, Key)>,
}

impl Memo {
    pub(crate) fn sync_revision(&self, revision: u64) -> bool {
        let mut state = self.state.borrow_mut();
        if state.revision == Some(revision) { return false }
        *state = MemoState { revision: Some(revision), ..Default::default() };
        true
    }

    fn enter(&self, kind: index::PublicationKind, key: &Key, revision: u64) -> Option<Pending<'_>> {
        let mut state = self.state.borrow_mut();
        let key = (kind, key.clone());
        if state.revision != Some(revision) || !state.pending.insert(key.clone()) { return None }
        Some(Pending { memo: self, key, revision })
    }
}

struct Pending<'a> {
    memo: &'a Memo,
    key: (index::PublicationKind, Key),
    revision: u64,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let mut state = self.memo.state.borrow_mut();
        if state.revision == Some(self.revision) { state.pending.remove(&self.key); }
    }
}

impl<S: Source> Modules<'_, S> {
    pub fn publication(&self, scope: ScopeRef, name: &str, class: u64, question: &QueryContext, retained: &Memo) -> Known<Vec<Target>> {
        if !class.is_power_of_two() {
            return Known::Unknown;
        }
        let outer = crate::footprint::begin();
        let answer = self.publication_walk(scope, name, class, question, retained);
        crate::footprint::resume(outer);
        answer
    }

    fn publication_walk(&self, scope: ScopeRef, name: &str, class: u64, question: &QueryContext, retained: &Memo) -> Known<Vec<Target>> {
        let revision = self.source.revision();
        let kind = self.source.publication_kind();
        retained.sync_revision(revision);
        let root = Key { scope, name: name.into(), class };
        if let Some((targets, read)) = retained.state.borrow().targets.get(&(kind, root.clone())) {
            crate::footprint::enter(read);
            return if self.source.revision() == revision { Known::Yes(targets.clone()) } else { Known::Unknown };
        }
        let Some(_pending) = retained.enter(kind, &root, revision) else { return Known::Unknown };
        let mut memo = BTreeMap::new();
        let mut stack = vec![self.frame(root.clone(), question, retained, kind, revision)];
        memo.insert(root.clone(), Answer::Active);
        while let Some(frame) = stack.last_mut() {
            if self.source.revision() != revision || question.budget.limited() || !question.budget.step() {
                return Known::Unknown;
            }
            if frame.unknown {
                let key = frame.key.clone();
                stack.pop();
                memo.insert(key, Answer::Done(Known::Unknown));
                continue;
            }
            if let Some(branch) = frame.links.last_mut() {
                if let Some(key) = branch.keys.last() {
                    match memo.get(key) {
                        Some(Answer::Active) | Some(Answer::Done(Known::Unknown)) => frame.unknown = true,
                        Some(Answer::Done(Known::Yes(targets))) => {
                            branch.targets.extend_from_slice(targets);
                            branch.keys.pop();
                        }
                        None => {
                            let key = key.clone();
                            let known = retained.state.borrow().targets.get(&(kind, key.clone())).cloned();
                            if let Some((targets, read)) = known {
                                crate::footprint::enter(&read);
                                memo.insert(key, Answer::Done(Known::Yes(targets)));
                                continue;
                            }
                            let Some(_preparing) = retained.enter(kind, &key, revision) else {
                                frame.unknown = true;
                                continue;
                            };
                            memo.insert(key.clone(), Answer::Active);
                            stack.push(self.frame(key, question, retained, kind, revision));
                        }
                    }
                    continue;
                }
                let mut branch = frame.links.pop().expect("last branch");
                branch.targets.sort_unstable();
                branch.targets.dedup();
                if frame.stars && !branch.targets.is_empty() && !frame.targets.is_empty() && frame.targets != branch.targets {
                    frame.unknown = true;
                } else {
                    frame.targets.extend(branch.targets);
                    frame.targets.sort_unstable();
                    frame.targets.dedup();
                }
                continue;
            }
            let frame = stack.pop().expect("last frame");
            if self.source.revision() != revision || question.budget.limited() { return Known::Unknown }
            retained.state.borrow_mut().targets.insert((kind, frame.key.clone()), (frame.targets.clone(), crate::footprint::current()));
            memo.insert(frame.key, Answer::Done(Known::Yes(frame.targets)));
        }
        match memo.remove(&root) {
            Some(Answer::Done(answer)) => answer,
            _ => Known::Unknown,
        }
    }

    fn frame(&self, key: Key, question: &QueryContext, retained: &Memo, kind: index::PublicationKind, revision: u64) -> Frame {
        if let Some((frame, read)) = retained.state.borrow().frames.get(&(kind, key.clone())) {
            crate::footprint::enter(read);
            return frame.clone();
        }
        let class = key.class;
        let mut frame = Frame { key, links: Vec::new(), targets: Vec::new(), stars: false, unknown: false };
        if question.budget.limited() || !question.budget.step() {
            frame.unknown = true;
            return frame;
        }
        let Known::Yes(entries) = self.source.entries(frame.key.scope, &frame.key.name, class, question) else {
            frame.unknown = true;
            return frame;
        };
        let direct = entries.iter().any(|entry| match entry {
            Entry::Local { name, classes, .. } | Entry::Reexport { name, classes, .. } | Entry::Namespace { name, classes, .. } => **name == *frame.key.name && classes & class != 0,
            Entry::Star { .. } => false,
        });
        frame.stars = !direct;
        let excluded = if !direct && entries.iter().any(|entry| matches!(entry, Entry::Star { classes, .. } if classes & class != 0)) {
            let Known::Yes(excluded) = self.source.star_excludes(frame.key.scope, &frame.key.name, question) else {
                frame.unknown = true;
                return frame;
            };
            excluded
        } else { false };
        for entry in entries {
            if !question.budget.step() {
                frame.unknown = true;
                break;
            }
            match entry {
                Entry::Local { name, classes, targets } if direct && name == frame.key.name && classes & class != 0 => frame.targets.extend(targets),
                Entry::Reexport { name, classes, asks, source, imported } if direct && name == frame.key.name && classes & class != 0 => match source {
                    Known::Yes(scope) => frame.links.push(Branch::new(scope, imported, if asks == 0 { class } else { asks })),
                    Known::Unknown => frame.unknown = true,
                },
                Entry::Namespace { name, classes, source } if direct && name == frame.key.name && classes & class != 0 => match source {
                    Known::Yes(scope) => frame.targets.push(Target::Scope(scope)),
                    Known::Unknown => frame.unknown = true,
                },
                Entry::Star { classes, asks, source } if !direct && classes & class != 0 && !excluded => match source {
                    Known::Yes(scope) => frame.links.push(Branch::new(scope, frame.key.name.clone(), if asks == 0 { class } else { asks })),
                    Known::Unknown => frame.unknown = true,
                },
                _ => {}
            }
        }
        frame.targets.sort_unstable();
        frame.targets.dedup();
        if self.source.revision() != revision || question.budget.limited() {
            frame.unknown = true;
        } else if !frame.unknown {
            retained.state.borrow_mut().frames.insert((kind, frame.key.clone()), (frame.clone(), crate::footprint::current()));
        }
        frame
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    scope: ScopeRef,
    name: Box<str>,
    class: u64,
}

#[derive(Clone)]
struct Branch {
    keys: Vec<Key>,
    targets: Vec<Target>,
}

impl Branch {
    fn new(scope: ScopeRef, name: Box<str>, asks: u64) -> Self {
        Self {
            keys: (0..64).map(|bit| 1 << bit).filter(|bit| asks & bit != 0).map(|class| Key { scope, name: name.clone(), class }).collect(),
            targets: Vec::new(),
        }
    }
}

enum Answer {
    Active,
    Done(Known<Vec<Target>>),
}

#[derive(Clone)]
struct Frame {
    key: Key,
    links: Vec<Branch>,
    targets: Vec<Target>,
    stars: bool,
    unknown: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::question::NodeRef;
    use std::collections::BTreeMap;

    const VALUE: u64 = 1;
    const TYPE: u64 = 2;

    #[derive(Default)]
    struct Graph(BTreeMap<ScopeRef, Vec<Entry>>);

    impl Source for Graph {
        fn entries(&self, scope: ScopeRef, _: &str, _: u64, _: &QueryContext) -> Known<Vec<Entry>> {
            self.0.get(&scope).cloned().map_or(Known::Unknown, Known::Yes)
        }
        fn star_excludes(&self, _: ScopeRef, name: &str, _: &QueryContext) -> Known<bool> {
            Known::Yes(matches!(name, "default" | "="))
        }
    }

    fn scope(file: u32) -> ScopeRef {
        ScopeRef { file, scope: 0 }
    }

    fn declaration(file: u32) -> Target {
        Target::Node(NodeRef { file, node: 1 })
    }

    fn local(file: u32, classes: u64) -> Entry {
        Entry::Local { name: "T".into(), classes, targets: vec![declaration(file)] }
    }

    fn star(file: u32) -> Entry {
        Entry::Star { classes: VALUE | TYPE, asks: 0, source: Known::Yes(scope(file)) }
    }

    #[test]
    fn type_only_hops_do_not_restore_values() {
        let graph = Graph(BTreeMap::from([
            (scope(0), vec![Entry::Reexport { name: "T".into(), classes: TYPE, asks: 0, source: Known::Yes(scope(1)), imported: "T".into() }]),
            (scope(1), vec![local(1, VALUE | TYPE)]),
        ]));
        let modules = Modules { source: &graph };
        assert_eq!(modules.publication(scope(0), "T", TYPE, &QueryContext::default(), &Memo::default()), Known::Yes(vec![declaration(1)]));
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &Memo::default()), Known::Yes(vec![]));
    }

    #[test]
    fn direct_publications_shadow_stars_without_traversing_cycles() {
        let graph = Graph(BTreeMap::from([
            (scope(0), vec![local(0, VALUE), star(1)]),
            (scope(1), vec![star(0)]),
        ]));
        let modules = Modules { source: &graph };
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &Memo::default()), Known::Yes(vec![declaration(0)]));
        assert_eq!(modules.publication(scope(0), "Missing", VALUE, &QueryContext::default(), &Memo::default()), Known::Unknown);
    }

    #[test]
    fn diamonds_dedupe_but_distinct_star_targets_are_ambiguous() {
        let mut graph = Graph(BTreeMap::from([
            (scope(0), vec![star(1), star(2)]),
            (scope(1), vec![star(3)]),
            (scope(2), vec![star(3)]),
            (scope(3), vec![local(3, VALUE)]),
        ]));
        assert_eq!(Modules { source: &graph }.publication(scope(0), "T", VALUE, &QueryContext::default(), &Memo::default()), Known::Yes(vec![declaration(3)]));
        graph.0.insert(scope(2), vec![local(2, VALUE)]);
        assert_eq!(Modules { source: &graph }.publication(scope(0), "T", VALUE, &QueryContext::default(), &Memo::default()), Known::Unknown);
    }

    #[test]
    fn an_unknown_explicit_export_blocks_star_fallback() {
        let graph = Graph(BTreeMap::from([
            (scope(0), vec![Entry::Reexport { name: "T".into(), classes: VALUE, asks: 0, source: Known::Unknown, imported: "T".into() }, star(1)]),
            (scope(1), vec![local(1, VALUE)]),
        ]));
        assert_eq!(Modules { source: &graph }.publication(scope(0), "T", VALUE, &QueryContext::default(), &Memo::default()), Known::Unknown);
    }

    #[test]
    fn type_only_aliases_can_ask_for_the_value_definition_without_exposing_values() {
        let graph = Graph(BTreeMap::from([
            (scope(0), vec![Entry::Reexport { name: "T".into(), classes: TYPE, asks: VALUE | TYPE, source: Known::Yes(scope(1)), imported: "T".into() }]),
            (scope(1), vec![local(1, VALUE)]),
        ]));
        let modules = Modules { source: &graph };
        assert_eq!(modules.publication(scope(0), "T", TYPE, &QueryContext::default(), &Memo::default()), Known::Yes(vec![declaration(1)]));
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &Memo::default()), Known::Yes(vec![]));
    }

    #[test]
    fn unknown_star_filters_do_not_prove_members_but_cannot_hide_direct_publications() {
        struct Filtered<'a> { graph: &'a Graph, excluded: Known<bool> }
        impl Source for Filtered<'_> {
            fn entries(&self, scope: ScopeRef, name: &str, class: u64, question: &QueryContext) -> Known<Vec<Entry>> {
                self.graph.entries(scope, name, class, question)
            }
            fn star_excludes(&self, _: ScopeRef, _: &str, _: &QueryContext) -> Known<bool> { self.excluded }
        }
        let mut graph = Graph(BTreeMap::from([(scope(0), vec![star(1)]), (scope(1), vec![local(1, VALUE)])]));
        for (excluded, expected) in [
            (Known::Yes(true), Known::Yes(Vec::new())),
            (Known::Yes(false), Known::Yes(vec![declaration(1)])),
            (Known::Unknown, Known::Unknown),
        ] {
            let source = Filtered { graph: &graph, excluded };
            assert_eq!(Modules { source: &source }.publication(scope(0), "T", VALUE, &QueryContext::default(), &Memo::default()), expected);
        }
        graph.0.get_mut(&scope(0)).unwrap().push(local(0, VALUE));
        let source = Filtered { graph: &graph, excluded: Known::Unknown };
        assert_eq!(Modules { source: &source }.publication(scope(0), "T", VALUE, &QueryContext::default(), &Memo::default()), Known::Yes(vec![declaration(0)]));
    }

    #[test]
    fn publication_memo_reuses_shared_subtrees_and_empty_answers_without_caching_unknowns() {
        struct Counted {
            graph: Graph,
            calls: std::cell::RefCell<BTreeMap<(ScopeRef, String, u64), usize>>,
            unreadable: std::cell::Cell<bool>,
        }
        impl Source for Counted {
            fn entries(&self, scope: ScopeRef, name: &str, class: u64, question: &QueryContext) -> Known<Vec<Entry>> {
                *self.calls.borrow_mut().entry((scope, name.into(), class)).or_default() += 1;
                if scope.file == 4 && self.unreadable.get() { return Known::Unknown }
                self.graph.entries(scope, name, class, question)
            }
            fn star_excludes(&self, scope: ScopeRef, name: &str, question: &QueryContext) -> Known<bool> {
                self.graph.star_excludes(scope, name, question)
            }
        }
        let source = Counted {
            graph: Graph(BTreeMap::from([
                (scope(0), vec![star(2)]), (scope(1), vec![star(2)]),
                (scope(2), vec![local(2, VALUE)]), (scope(3), vec![star(3)]),
                (scope(4), vec![local(4, VALUE)]),
            ])),
            calls: Default::default(), unreadable: std::cell::Cell::new(true),
        };
        let memo = Memo::default();
        let modules = Modules { source: &source };
        for root in [0, 1] {
            for (name, class, expected) in [
                ("T", VALUE, vec![declaration(2)]), ("Missing", VALUE, vec![]), ("T", TYPE, vec![]),
            ] {
                assert_eq!(modules.publication(scope(root), name, class, &QueryContext::default(), &memo), Known::Yes(expected));
            }
        }
        for _ in 0..2 {
            assert_eq!(modules.publication(scope(3), "T", VALUE, &QueryContext::default(), &memo), Known::Unknown);
        }
        assert_eq!(modules.publication(scope(4), "T", VALUE, &QueryContext::default(), &memo), Known::Unknown);
        source.unreadable.set(false);
        assert_eq!(modules.publication(scope(4), "T", VALUE, &QueryContext::default(), &memo), Known::Yes(vec![declaration(4)]));
        let calls = source.calls.borrow();
        for (name, class) in [("T", VALUE), ("Missing", VALUE), ("T", TYPE)] {
            assert_eq!(calls.get(&(scope(2), name.into(), class)), Some(&1));
        }
        assert_eq!(calls.get(&(scope(4), "T".into(), VALUE)), Some(&2));
    }

    #[test]
    fn completed_subtrees_survive_a_later_budget_cut() {
        struct Cut {
            shared: std::cell::Cell<usize>,
        }
        impl Source for Cut {
            fn entries(&self, at: ScopeRef, _: &str, _: u64, question: &QueryContext) -> Known<Vec<Entry>> {
                match at.file {
                    0 => Known::Yes(vec![star(1), star(2)]),
                    1 => {
                        while question.budget.step() {}
                        Known::Yes(Vec::new())
                    },
                    2 => {
                        self.shared.set(self.shared.get() + 1);
                        Known::Yes(vec![local(2, VALUE)])
                    },
                    _ => Known::Yes(vec![star(2)]),
                }
            }
            fn star_excludes(&self, _: ScopeRef, _: &str, _: &QueryContext) -> Known<bool> { Known::Yes(false) }
        }
        let source = Cut { shared: Default::default() };
        let modules = Modules { source: &source };
        let memo = Memo::default();
        let mut question = QueryContext::default();
        question.budget = super::super::question::Budget::new(64);
        assert_eq!(modules.publication(scope(0), "T", VALUE, &question, &memo), Known::Unknown);
        assert!(question.budget.limited());
        assert_eq!(modules.publication(scope(3), "T", VALUE, &QueryContext::default(), &memo), Known::Yes(vec![declaration(2)]));
        assert_eq!(source.shared.get(), 1);
        assert_eq!(modules.publication(scope(1), "T", VALUE, &QueryContext::default(), &memo), Known::Unknown);
    }

    #[test]
    fn publication_memos_separate_modes_and_revisions_and_reject_mid_work_changes() {
        struct Changing {
            revision: std::cell::Cell<u64>,
            mode: std::cell::Cell<index::PublicationKind>,
            drift: std::cell::Cell<bool>,
        }
        impl Source for Changing {
            fn entries(&self, _: ScopeRef, _: &str, _: u64, _: &QueryContext) -> Known<Vec<Entry>> {
                let file = self.revision.get() as u32 + if self.mode.get() == index::PublicationKind::Globals { 10 } else { 0 };
                if self.drift.replace(false) { self.revision.set(self.revision.get() + 1); }
                Known::Yes(vec![local(file, VALUE)])
            }
            fn star_excludes(&self, _: ScopeRef, _: &str, _: &QueryContext) -> Known<bool> { Known::Yes(false) }
            fn publication_kind(&self) -> index::PublicationKind { self.mode.get() }
            fn revision(&self) -> u64 { self.revision.get() }
        }
        let source = Changing { revision: Default::default(), mode: std::cell::Cell::new(index::PublicationKind::Exports), drift: Default::default() };
        let modules = Modules { source: &source };
        let memo = Memo::default();
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &memo), Known::Yes(vec![declaration(0)]));
        source.mode.set(index::PublicationKind::Globals);
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &memo), Known::Yes(vec![declaration(10)]));
        source.revision.set(1);
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &memo), Known::Yes(vec![declaration(11)]));
        source.revision.set(2);
        source.drift.set(true);
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &memo), Known::Unknown);
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &memo), Known::Yes(vec![declaration(13)]));
    }

    #[test]
    fn recursive_source_preparation_does_not_publish_an_unfinished_root() {
        struct Recursive {
            memo: Memo,
            recurse: std::cell::Cell<bool>,
        }
        impl Source for Recursive {
            fn entries(&self, at: ScopeRef, name: &str, class: u64, question: &QueryContext) -> Known<Vec<Entry>> {
                if self.recurse.replace(false) {
                    let answer = Modules { source: self }.publication(at, name, class, question, &self.memo);
                    if answer == Known::Unknown { return Known::Unknown }
                }
                Known::Yes(vec![local(0, VALUE)])
            }
            fn star_excludes(&self, _: ScopeRef, _: &str, _: &QueryContext) -> Known<bool> { Known::Yes(false) }
        }
        let source = Recursive { memo: Default::default(), recurse: std::cell::Cell::new(true) };
        let modules = Modules { source: &source };
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &source.memo), Known::Unknown);
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default(), &source.memo), Known::Yes(vec![declaration(0)]));
    }
}

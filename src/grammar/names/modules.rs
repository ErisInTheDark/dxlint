use std::collections::BTreeMap;

use super::question::{Known, QueryContext, ScopeRef, Target};

pub mod compile;

#[derive(Clone, Debug)]
pub enum Entry {
    Local { name: Box<str>, classes: u64, targets: Vec<Target> },
    Reexport { name: Box<str>, classes: u64, source: Known<ScopeRef>, imported: Box<str> },
    Namespace { name: Box<str>, classes: u64, source: Known<ScopeRef> },
    Star { classes: u64, source: Known<ScopeRef> },
}

pub trait Source {
    fn entries(&self, scope: ScopeRef, question: &QueryContext) -> Known<Vec<Entry>>;
    fn star_excludes(&self, scope: ScopeRef, name: &str) -> bool;
}

pub struct Modules<'a, S> {
    pub source: &'a S,
}

impl<S: Source> Modules<'_, S> {
    pub fn publication(&self, scope: ScopeRef, name: &str, class: u64, question: &QueryContext) -> Known<Vec<Target>> {
        if !class.is_power_of_two() {
            return Known::Unknown;
        }
        let root = Key { scope, name: name.into() };
        let mut memo = BTreeMap::new();
        let mut stack = vec![self.frame(root.clone(), class, question)];
        memo.insert(root.clone(), Answer::Active);
        while let Some(frame) = stack.last_mut() {
            if !question.budget.step() {
                return Known::Unknown;
            }
            if frame.unknown {
                let key = frame.key.clone();
                stack.pop();
                memo.insert(key, Answer::Done(Known::Unknown));
                continue;
            }
            if let Some(key) = frame.links.last() {
                match memo.get(key) {
                    Some(Answer::Active) | Some(Answer::Done(Known::Unknown)) => frame.unknown = true,
                    Some(Answer::Done(Known::Yes(targets))) => {
                        if frame.stars && !targets.is_empty() && !frame.targets.is_empty() && frame.targets != *targets {
                            frame.unknown = true;
                        } else {
                            frame.targets.extend_from_slice(targets);
                            frame.targets.sort_unstable();
                            frame.targets.dedup();
                        }
                        frame.links.pop();
                    }
                    None => {
                        let key = key.clone();
                        memo.insert(key.clone(), Answer::Active);
                        stack.push(self.frame(key, class, question));
                    }
                }
                continue;
            }
            let frame = stack.pop().expect("last frame");
            memo.insert(frame.key, Answer::Done(Known::Yes(frame.targets)));
        }
        match memo.remove(&root) {
            Some(Answer::Done(answer)) => answer,
            _ => Known::Unknown,
        }
    }

    fn frame(&self, key: Key, class: u64, question: &QueryContext) -> Frame {
        let mut frame = Frame { key, links: Vec::new(), targets: Vec::new(), stars: false, unknown: false };
        if !question.budget.step() {
            frame.unknown = true;
            return frame;
        }
        let Known::Yes(entries) = self.source.entries(frame.key.scope, question) else {
            frame.unknown = true;
            return frame;
        };
        let direct = entries.iter().any(|entry| match entry {
            Entry::Local { name, classes, .. } | Entry::Reexport { name, classes, .. } | Entry::Namespace { name, classes, .. } => **name == *frame.key.name && classes & class != 0,
            Entry::Star { .. } => false,
        });
        frame.stars = !direct;
        for entry in entries {
            if !question.budget.step() {
                frame.unknown = true;
                break;
            }
            match entry {
                Entry::Local { name, classes, targets } if direct && name == frame.key.name && classes & class != 0 => frame.targets.extend(targets),
                Entry::Reexport { name, classes, source, imported } if direct && name == frame.key.name && classes & class != 0 => match source {
                    Known::Yes(scope) => frame.links.push(Key { scope, name: imported }),
                    Known::Unknown => frame.unknown = true,
                },
                Entry::Namespace { name, classes, source } if direct && name == frame.key.name && classes & class != 0 => match source {
                    Known::Yes(scope) => frame.targets.push(Target::Scope(scope)),
                    Known::Unknown => frame.unknown = true,
                },
                Entry::Star { classes, source } if !direct && classes & class != 0 && !self.source.star_excludes(frame.key.scope, &frame.key.name) => match source {
                    Known::Yes(scope) => frame.links.push(Key { scope, name: frame.key.name.clone() }),
                    Known::Unknown => frame.unknown = true,
                },
                _ => {}
            }
        }
        frame.targets.sort_unstable();
        frame.targets.dedup();
        frame
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    scope: ScopeRef,
    name: Box<str>,
}

enum Answer {
    Active,
    Done(Known<Vec<Target>>),
}

struct Frame {
    key: Key,
    links: Vec<Key>,
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
        fn entries(&self, scope: ScopeRef, _: &QueryContext) -> Known<Vec<Entry>> {
            self.0.get(&scope).cloned().map_or(Known::Unknown, Known::Yes)
        }
        fn star_excludes(&self, _: ScopeRef, name: &str) -> bool {
            matches!(name, "default" | "=")
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
        Entry::Star { classes: VALUE | TYPE, source: Known::Yes(scope(file)) }
    }

    #[test]
    fn type_only_hops_do_not_restore_values() {
        let graph = Graph(BTreeMap::from([
            (scope(0), vec![Entry::Reexport { name: "T".into(), classes: TYPE, source: Known::Yes(scope(1)), imported: "T".into() }]),
            (scope(1), vec![local(1, VALUE | TYPE)]),
        ]));
        let modules = Modules { source: &graph };
        assert_eq!(modules.publication(scope(0), "T", TYPE, &QueryContext::default()), Known::Yes(vec![declaration(1)]));
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default()), Known::Yes(vec![]));
    }

    #[test]
    fn direct_publications_shadow_stars_without_traversing_cycles() {
        let graph = Graph(BTreeMap::from([
            (scope(0), vec![local(0, VALUE), star(1)]),
            (scope(1), vec![star(0)]),
        ]));
        let modules = Modules { source: &graph };
        assert_eq!(modules.publication(scope(0), "T", VALUE, &QueryContext::default()), Known::Yes(vec![declaration(0)]));
        assert_eq!(modules.publication(scope(0), "Missing", VALUE, &QueryContext::default()), Known::Unknown);
    }

    #[test]
    fn diamonds_dedupe_but_distinct_star_targets_are_ambiguous() {
        let mut graph = Graph(BTreeMap::from([
            (scope(0), vec![star(1), star(2)]),
            (scope(1), vec![star(3)]),
            (scope(2), vec![star(3)]),
            (scope(3), vec![local(3, VALUE)]),
        ]));
        assert_eq!(Modules { source: &graph }.publication(scope(0), "T", VALUE, &QueryContext::default()), Known::Yes(vec![declaration(3)]));
        graph.0.insert(scope(2), vec![local(2, VALUE)]);
        assert_eq!(Modules { source: &graph }.publication(scope(0), "T", VALUE, &QueryContext::default()), Known::Unknown);
    }

    #[test]
    fn an_unknown_explicit_export_blocks_star_fallback() {
        let graph = Graph(BTreeMap::from([
            (scope(0), vec![Entry::Reexport { name: "T".into(), classes: VALUE, source: Known::Unknown, imported: "T".into() }, star(1)]),
            (scope(1), vec![local(1, VALUE)]),
        ]));
        assert_eq!(Modules { source: &graph }.publication(scope(0), "T", VALUE, &QueryContext::default()), Known::Unknown);
    }
}

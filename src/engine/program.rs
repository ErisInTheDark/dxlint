use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;

use crate::grammar::names::question::Known;
use crate::fx_hash::FxHashMap;

use super::reads::{self, Cached, Dep};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Context {
    pub directory: PathBuf,
    pub settings: usize,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct CatalogKey {
    pub context: Context,
    pub outside: Option<PathBuf>,
    pub classes: u64,
}

pub(super) struct Declaration {
    pub path: PathBuf,
    pub scope: u32,
    pub node: u32,
    pub classes: u64,
    pub name: String,
    pub source: Option<String>,
}

pub(super) struct Catalog {
    declarations: Vec<Declaration>,
    names: FxHashMap<String, Vec<usize>>,
    wildcards: Vec<usize>,
    sources: Vec<usize>,
}

impl Catalog {
    pub fn new(declarations: Vec<Declaration>) -> Self {
        let mut names = FxHashMap::<String, Vec<usize>>::default();
        let mut wildcards = Vec::new();
        let mut sources = Vec::new();
        for (index, declaration) in declarations.iter().enumerate() {
            names.entry(declaration.name.clone()).or_default().push(index);
            if declaration.source.is_some() {
                sources.push(index);
            } else if declaration.name.contains('*') {
                wildcards.push(index);
            }
        }
        Self { declarations, names, wildcards, sources }
    }

    pub fn named<'a>(&'a self, name: &str, classes: u64) -> impl Iterator<Item = &'a Declaration> {
        self.names.get(name).into_iter().flatten().map(|&index| &self.declarations[index])
            .filter(move |declaration| declaration.classes & classes != 0)
    }

    pub fn ambient(&self, name: &str, classes: u64) -> Option<&Declaration> {
        self.named(name, classes).find(|declaration| declaration.source.is_none()).or_else(|| {
            self.wildcards.iter().map(|&index| &self.declarations[index])
                .find(|declaration| declaration.classes & classes != 0 && crate::path_globs::wildcard(&declaration.name, name))
        })
    }

    pub fn candidates(&self, name: Option<&str>, classes: u64) -> impl Iterator<Item = &Declaration> {
        let named = name.and_then(|name| self.names.get(name));
        let augment = name.is_none();
        let augmented = self.sources.iter().filter(move |_| augment);
        named.into_iter().flatten().chain(augmented).map(|&index| &self.declarations[index])
            .filter(move |declaration| declaration.classes & classes != 0)
    }
}

enum Entry<V> {
    Building(ThreadId),
    Ready(Cached<Arc<V>>),
}

struct Memo<K, V> {
    entries: Mutex<FxHashMap<K, Entry<V>>>,
    changed: Condvar,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self { entries: Mutex::default(), changed: Condvar::new() }
    }
}

impl<K: Clone + Eq + Hash, V> Memo<K, V> {
    fn get(&self, key: K, holds: impl Fn(&Dep) -> bool, build: impl FnOnce() -> Known<V>) -> Known<Arc<V>> {
        let owner = std::thread::current().id();
        let mut entries = self.entries.lock().expect("program cache");
        loop {
            match entries.get(&key) {
                Some(Entry::Ready(answer)) if !answer.reads.any(&mut |dep| !holds(dep)) => {
                    return Known::Yes(answer.get());
                }
                Some(Entry::Building(thread)) if *thread == owner => return Known::Unknown,
                Some(Entry::Building(_)) => {
                    if entries.values().any(|entry| matches!(entry, Entry::Building(thread) if *thread == owner)) {
                        return Known::Unknown;
                    }
                    entries = self.changed.wait(entries).expect("program cache");
                }
                _ => {
                    entries.insert(key.clone(), Entry::Building(owner));
                    break;
                }
            }
        }
        drop(entries);
        let lease = Build { memo: self, key };
        let answer = Cached::compute(|| match build() {
            Known::Yes(value) => Known::Yes(Arc::new(value)),
            Known::Unknown => Known::Unknown,
        });
        reads::note_group(&answer.reads);
        match answer.value {
            Known::Yes(value) => {
                if !answer.reads.any(&mut |dep| !holds(dep)) {
                    lease.publish(Cached { value: value.clone(), reads: answer.reads });
                }
                Known::Yes(value)
            }
            _ => Known::Unknown,
        }
    }

    fn clear(&mut self) {
        self.entries.get_mut().expect("program cache").clear();
    }
}

struct Build<'a, K: Eq + Hash, V> {
    memo: &'a Memo<K, V>,
    key: K,
}

impl<K: Clone + Eq + Hash, V> Build<'_, K, V> {
    fn publish(self, answer: Cached<Arc<V>>) {
        self.memo.entries.lock().expect("program cache").insert(self.key.clone(), Entry::Ready(answer));
    }
}

impl<K: Eq + Hash, V> Drop for Build<'_, K, V> {
    fn drop(&mut self) {
        let mut entries = self.memo.entries.lock().expect("program cache");
        if matches!(entries.get(&self.key), Some(Entry::Building(thread)) if *thread == std::thread::current().id()) {
            entries.remove(&self.key);
        }
        self.memo.changed.notify_all();
    }
}

#[derive(Default)]
pub(super) struct Programs {
    paths: Memo<Context, Vec<PathBuf>>,
    catalogs: Memo<CatalogKey, Catalog>,
}

impl Programs {
    pub fn paths(&self, context: Context, holds: impl Fn(&Dep) -> bool, build: impl FnOnce() -> Known<Vec<PathBuf>>) -> Known<Arc<Vec<PathBuf>>> {
        self.paths.get(context, holds, build)
    }

    pub fn catalog(&self, key: CatalogKey, holds: impl Fn(&Dep) -> bool, build: impl FnOnce() -> Known<Catalog>) -> Known<Arc<Catalog>> {
        self.catalogs.get(key, holds, build)
    }

    pub fn clear(&mut self) {
        self.paths.clear();
        self.catalogs.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use super::*;

    #[test]
    fn ambient_matches_prefer_exact_names_and_respect_classes_and_augmentations() {
        let declaration = |scope, name: &str, classes, source: Option<&str>| Declaration {
            path: PathBuf::from("/catalog/library"), scope, node: scope, classes,
            name: name.into(), source: source.map(str::to_owned),
        };
        let catalog = Catalog::new(vec![
            declaration(1, "lib-*", 1, None),
            declaration(2, "lib-one", 2, None),
            declaration(3, "lib-one", 1, Some("./target")),
            declaration(4, "lib-one", 1, None),
        ]);
        assert_eq!(catalog.ambient("lib-one", 1).map(|declaration| declaration.scope), Some(4));
        assert_eq!(catalog.ambient("lib-other", 1).map(|declaration| declaration.scope), Some(1));
        assert_eq!(catalog.ambient("lib-one", 2).map(|declaration| declaration.scope), Some(2));
        assert_eq!(catalog.ambient("lib-other", 2).map(|declaration| declaration.scope), None);
        assert_eq!(catalog.candidates(Some("lib-one"), 1).map(|declaration| declaration.scope).collect::<Vec<_>>(), [3, 4]);
        assert_eq!(catalog.candidates(None, 1).map(|declaration| declaration.scope).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn completed_answers_replay_reads_and_rebuild_when_inputs_change() {
        let path = Path::new("/program-cache-counted-input");
        reads::set_buffer(path, "first");
        let memo = Memo::<(), String>::default();
        let builds = Cell::new(0);
        let load = || memo.get((), |dep| dep.holds(|_| reads::MISSING), || {
            builds.set(builds.get() + 1);
            Known::Yes(reads::read(path).unwrap().to_string())
        });
        for _ in 0..20 {
            let (answer, dependencies) = reads::track(load);
            assert_eq!(answer.known().as_deref().map(String::as_str), Some("first"));
            assert!(!dependencies.is_empty());
        }
        assert_eq!(builds.get(), 1);
        reads::set_buffer(path, "second");
        assert_eq!(load().known().as_deref().map(String::as_str), Some("second"));
        assert_eq!(builds.get(), 2);
        reads::drop_buffer(path);
    }

    #[test]
    fn unknown_and_recursive_builds_do_not_become_completed_answers() {
        let memo = Memo::<(), u32>::default();
        let builds = Cell::new(0);
        let answer = memo.get((), |_| true, || {
            builds.set(builds.get() + 1);
            assert_eq!(memo.get((), |_| true, || Known::Yes(99)), Known::Unknown);
            Known::Unknown
        });
        assert_eq!(answer, Known::Unknown);
        let answer = memo.get((), |_| true, || {
            builds.set(builds.get() + 1);
            Known::Yes(7)
        });
        assert_eq!(answer.known().map(|value| *value), Some(7));
        assert_eq!(builds.get(), 2);
    }
}

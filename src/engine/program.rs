use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::ThreadId;

use crate::grammar::names::question::Known;
use crate::fx_hash::FxHashMap;

use super::{Profile, reads::{self, Cached, Dep}};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Context {
    pub directory: PathBuf,
    pub settings: usize,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct CatalogKey {
    pub settings: usize,
    /// The program's identity: a hash of its input paths.
    pub program: u64,
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
    texts: FxHashMap<PathBuf, u64>,
}

impl Catalog {
    pub fn new(declarations: Vec<Declaration>, texts: FxHashMap<PathBuf, u64>) -> Self {
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
        Self { declarations, names, wildcards, sources, texts }
    }

    fn includes_text(&self, path: &std::path::Path, hash: u64) -> bool {
        self.texts.get(path).is_some_and(|&stored| stored == hash)
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
    Ready(Cached<V>),
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

impl<K: Clone + Eq + Hash, V: Clone> Memo<K, V> {
    fn get(&self, key: K, profile: Option<&Profile>, accepts: impl Fn(&Cached<V>) -> bool, build: impl FnOnce() -> Known<V>) -> Known<V> {
        let owner = std::thread::current().id();
        let mut entries = self.entries.lock().expect("program cache");
        loop {
            match entries.get(&key) {
                Some(Entry::Ready(answer)) => {
                    if let Some(profile) = profile { Profile::add(&profile.catalogue_read_validations, 1); }
                    // Validation can read every source: never under the lock.
                    let answer = answer.clone();
                    drop(entries);
                    if accepts(&answer) {
                        return Known::Yes(answer.get());
                    }
                    entries = self.entries.lock().expect("program cache");
                    match entries.get(&key) {
                        Some(Entry::Ready(current)) if Arc::ptr_eq(&current.reads, &answer.reads) => {
                            entries.insert(key.clone(), Entry::Building(owner));
                            break;
                        }
                        _ => continue,
                    }
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
        if let Some(profile) = profile { Profile::add(&profile.program_catalogue_builds, 1); }
        let answer = Cached::compute(build);
        reads::note_group(&answer.reads);
        match answer.value {
            Known::Yes(value) => {
                lease.publish(Cached { value: value.clone(), reads: answer.reads });
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
    fn publish(self, answer: Cached<V>) {
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
    paths: Memo<Context, Arc<Vec<PathBuf>>>,
    /// Input lists by content: files in different directories of one
    /// program get the same list, and its address is the program's identity
    /// for run-shared answers ([`super::world`]'s `program_identity`).
    inputs: Mutex<FxHashMap<u64, Vec<std::sync::Weak<Vec<PathBuf>>>>>,
    catalogs: Memo<CatalogKey, Arc<Catalog>>,
}

impl Programs {
    pub fn paths(&self, context: Context, holds: impl Fn(&Dep) -> bool, build: impl FnOnce() -> Known<Vec<PathBuf>>) -> Known<Arc<Vec<PathBuf>>> {
        self.paths.get(context, None, |answer| !answer.reads.any(&mut |dep| !holds(dep)), || match build() {
            Known::Yes(paths) => Known::Yes(self.intern(paths)),
            Known::Unknown => Known::Unknown,
        })
    }

    fn intern(&self, paths: Vec<PathBuf>) -> Arc<Vec<PathBuf>> {
        let key = std::hash::BuildHasher::hash_one(&crate::fx_hash::FxBuild::default(), &paths);
        let mut inputs = self.inputs.lock().expect("program inputs");
        let lists = inputs.entry(key).or_default();
        lists.retain(|list| list.strong_count() > 0);
        if let Some(same) = lists.iter().filter_map(std::sync::Weak::upgrade).find(|list| **list == paths) { return same }
        let list = Arc::new(paths);
        lists.push(Arc::downgrade(&list));
        list
    }

    pub fn catalog(&self, key: CatalogKey, profile: Option<&Profile>, holds: impl Fn(&Dep) -> bool, build: impl FnOnce() -> Known<Catalog>) -> Known<Arc<Catalog>> {
        self.catalogs.get(key, profile, |answer| !answer.reads.any(&mut |dep| !holds(dep)), || match build() {
            Known::Yes(catalog) => Known::Yes(Arc::new(catalog)),
            Known::Unknown => Known::Unknown,
        })
    }

    pub fn clear(&mut self) {
        self.paths.clear();
        self.catalogs.clear();
    }
}

/// Run-wide answers computed once per key, however many threads ask at once.
type Slots<K, V> = Mutex<FxHashMap<K, Arc<std::sync::OnceLock<Cached<V>>>>>;

/// `key`'s answer in `slots`: the first asker computes it, the rest wait.
fn once<K: Eq + Hash, V: Clone>(slots: &Slots<K, V>, key: K, compute: impl FnOnce() -> V) -> Cached<V> {
    let slot = slots.lock().expect("run slots").entry(key).or_default().clone();
    slot.get_or_init(|| Cached::compute(compute)).clone()
}

#[derive(Default)]
pub(super) struct Snapshot {
    paths: Memo<(u64, Context), Arc<Vec<PathBuf>>>,
    catalogs: Memo<(u64, CatalogKey), Arc<Catalog>>,
    // Read-mostly maps every lint thread consults: read locks on hits.
    units: RwLock<FxHashMap<PathBuf, (u64, Arc<UnitSlot>)>>,
    modules: RwLock<FxHashMap<ModuleKey, Cached<Arc<[StableTarget]>>>>,
    /// Per program and scope classes, the augmentations each module scope
    /// (by path and scope) receives.
    augmentations: RwLock<FxHashMap<ModuleKey, Cached<Arc<Augmentations>>>>,
    /// Program inputs (by input list hash) whose units were loaded in parallel.
    prefetched: Mutex<FxHashMap<u64, Arc<std::sync::OnceLock<()>>>>,
    /// Module resolution's file probes and reads, once per path and revision.
    probes: RwLock<FxHashMap<PathBuf, (u64, bool)>>,
    /// Directory listings behind those probes (`None`: unreadable).
    listings: RwLock<FxHashMap<PathBuf, (u64, Option<Arc<reads::Listing>>)>>,
    texts: RwLock<FxHashMap<PathBuf, (u64, Option<Arc<str>>, u64)>>,
    /// Per program (settings, input hash) and global name: the inputs whose
    /// text may publish it ([`crate::grammar::names::modules::resolution::Globals::may_publish`]).
    candidates: Slots<(usize, u64, Box<str>), Arc<[PathBuf]>>,
    /// Per program (settings, input hash): every input's text, read once.
    surveys: Slots<(usize, u64), Survey>,
    /// Program configurations by revision, settings and program directory.
    configurations: RwLock<FxHashMap<(u64, usize, PathBuf), Cached<Arc<crate::grammar::names::modules::resolution::Configuration>>>>,
    /// Metadata parsed from those texts, by text hash (`None`: malformed).
    metadata: RwLock<FxHashMap<PathBuf, (u64, Option<Arc<crate::grammar::names::modules::resolution::Json>>)>>,
}

/// A shared module question ([`crate::grammar::names::modules::SharedQuestion`])
/// with files by path, under one program and revision.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct ModuleKey {
    pub revision: u64,
    pub settings: usize,
    /// The program's identity: a hash of its input paths.
    pub program: u64,
    /// The linted file when it is outside the program's inputs.
    pub outside: Option<PathBuf>,
    pub question: StableQuestion,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum StableQuestion {
    Publication { kind: crate::grammar::names::modules::index::PublicationKind, path: reads::DepPath, scope: u32, name: Box<str>, class: u64 },
    Global { name: Box<str>, class: u64 },
    /// Every scope merged into module scope `scope` of `path`.
    Scopes { path: reads::DepPath, scope: u32 },
    /// The module scope `specifier` names, imported from a file in `directory`.
    Resolve { directory: reads::DepPath, specifier: Box<str> },
    /// Every augmentation of the program's module scopes in `classes`.
    Augmentations { classes: u64 },
}

/// A program's inputs with their texts (`None`: unreadable) and whether the
/// text holds a name-independent publication hint (so it may publish any
/// global name).
pub(super) type Survey = Arc<[(PathBuf, Option<Arc<str>>, bool)]>;

/// Module scope (path, scope) -> the scopes that augment it.
pub(super) type Augmentations = FxHashMap<(reads::DepPath, u32), Vec<(reads::DepPath, u32)>>;

/// A module answer's target with its file by path.
#[derive(Clone)]
pub(super) enum StableTarget {
    Node(reads::DepPath, u32),
    Scope(reads::DepPath, u32),
}

/// One file's run unit: its text hash and the unit, when it parses.
type UnitSlot = std::sync::OnceLock<(u64, Option<Arc<super::units::Unit>>)>;

impl Snapshot {
    /// The run's unit for `path` at `revision`, loaded once even when
    /// several lint threads ask at the same time (the others wait).
    pub(super) fn unit(&self, revision: u64, path: &std::path::Path,
        load: impl FnOnce() -> (u64, Option<Arc<super::units::Unit>>)) -> (u64, Option<Arc<super::units::Unit>>) {
        let known = self.units.read().expect("run units").get(path)
            .filter(|(at, _)| *at == revision).map(|(_, slot)| slot.clone());
        let slot = known.unwrap_or_else(|| {
            let mut units = self.units.write().expect("run units");
            match units.get(path) {
                Some((at, slot)) if *at == revision => slot.clone(),
                _ => {
                    let slot = Arc::new(UnitSlot::new());
                    units.insert(path.to_owned(), (revision, slot.clone()));
                    slot
                }
            }
        });
        slot.get_or_init(load).clone()
    }

    /// Whether `path` is a file, probed once per revision; the caller
    /// records the read.
    pub(super) fn is_file(&self, revision: u64, path: &std::path::Path) -> bool {
        if let Some(&(at, found)) = self.probes.read().expect("run probes").get(path) {
            if at == revision { return found }
        }
        // Resolution probes many candidate names per directory: list each
        // directory once instead of asking the file system per name.
        let listing = path.parent().and_then(|dir| {
            if let Some((at, listing)) = self.listings.read().expect("run listings").get(dir) {
                if *at == revision { return listing.clone() }
            }
            let listing = reads::list_unrecorded(dir).map(Arc::new);
            self.listings.write().expect("run listings").insert(dir.to_owned(), (revision, listing.clone()));
            listing
        });
        let found = match &listing {
            Some(listing) => reads::is_file_listed(path, listing),
            None => reads::is_file_unrecorded(path),
        };
        self.probes.write().expect("run probes").insert(path.to_owned(), (revision, found));
        found
    }

    /// `path`'s text and hash, read once per revision; the caller records
    /// the read.
    pub(super) fn text(&self, revision: u64, path: &std::path::Path) -> (Option<Arc<str>>, u64) {
        if let Some((at, text, hash)) = self.texts.read().expect("run texts").get(path) {
            if *at == revision { return (text.clone(), *hash) }
        }
        let (text, hash) = reads::read_unrecorded(path);
        self.texts.write().expect("run texts").insert(path.to_owned(), (revision, text.clone(), hash));
        (text, hash)
    }

    /// A program configuration, loaded once per run (`load` records its
    /// reads; failures are not kept).
    pub(super) fn configuration(&self, key: (u64, usize, PathBuf),
        load: impl FnOnce() -> Result<Arc<crate::grammar::names::modules::resolution::Configuration>, ()>)
        -> Result<Cached<Arc<crate::grammar::names::modules::resolution::Configuration>>, ()> {
        if let Some(found) = self.configurations.read().expect("run configurations").get(&key) { return Ok(found.clone()) }
        let answer = Cached::compute(load);
        let Ok(value) = answer.value else { reads::note_group(&answer.reads); return Err(()) };
        let found = Cached { value, reads: answer.reads };
        self.configurations.write().expect("run configurations").insert(key, found.clone());
        Ok(found)
    }

    /// A program's input texts, read once per run (`read` records them);
    /// concurrent callers wait for the one reading.
    pub(super) fn survey(&self, key: (usize, u64), read: impl FnOnce() -> Survey) -> Cached<Survey> {
        once(&self.surveys, key, read)
    }

    /// The inputs that may publish a global name, computed once per run
    /// (`compute` records the text reads it makes).
    pub(super) fn candidates(&self, key: (usize, u64, Box<str>), compute: impl FnOnce() -> Arc<[PathBuf]>) -> Cached<Arc<[PathBuf]>> {
        once(&self.candidates, key, compute)
    }

    /// `path`'s metadata parsed from the text with `hash`, parsed once per
    /// text.
    pub(super) fn metadata(&self, path: &std::path::Path, hash: u64, text: &str)
        -> Option<Arc<crate::grammar::names::modules::resolution::Json>> {
        if let Some((at, parsed)) = self.metadata.read().expect("run metadata").get(path) {
            if *at == hash { return parsed.clone() }
        }
        let parsed = crate::grammar::names::modules::resolution::parse_metadata(Some(text)).ok().flatten();
        self.metadata.write().expect("run metadata").insert(path.to_owned(), (hash, parsed.clone()));
        parsed
    }

    /// Runs `load` once per program for this run; concurrent callers wait
    /// for it instead of loading the same files in the same order.
    pub(super) fn prefetch(&self, program: u64, load: impl FnOnce()) {
        let slot = self.prefetched.lock().expect("run prefetch").entry(program).or_default().clone();
        slot.get_or_init(load);
    }

    pub(super) fn augmentations(&self, key: &ModuleKey) -> Option<Cached<Arc<Augmentations>>> {
        self.augmentations.read().expect("run augmentations").get(key).cloned()
    }

    pub(super) fn keep_augmentations(&self, key: ModuleKey, answer: Cached<Arc<Augmentations>>) {
        self.augmentations.write().expect("run augmentations").insert(key, answer);
    }

    pub(super) fn module_answer(&self, key: &ModuleKey) -> Option<Cached<Arc<[StableTarget]>>> {
        self.modules.read().expect("run modules").get(key).cloned()
    }

    pub(super) fn keep_module_answer(&self, key: ModuleKey, answer: Cached<Arc<[StableTarget]>>) {
        self.modules.write().expect("run modules").insert(key, answer);
    }

    pub fn paths(&self, revision: u64, context: Context, build: impl FnOnce() -> Known<Arc<Vec<PathBuf>>>) -> Known<Arc<Vec<PathBuf>>> {
        self.paths.get((revision, context), None, |_| true, build)
    }

    pub fn catalog(&self, revision: u64, key: CatalogKey, path: &std::path::Path, hash: u64, build: impl FnOnce() -> Known<Arc<Catalog>>) -> Known<Arc<Catalog>> {
        self.catalogs.get((revision, key), None, |answer| answer.value.includes_text(path, hash), build)
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
        ], Default::default());
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
        let validations = Cell::new(0);
        let load = || memo.get((), None, |answer| !answer.reads.any(&mut |dep| {
            validations.set(validations.get() + 1);
            !dep.holds(|_| reads::MISSING)
        }), || {
            builds.set(builds.get() + 1);
            Known::Yes(reads::read(path).unwrap().to_string())
        });
        for lookup in 0..20 {
            let (answer, dependencies) = reads::track(load);
            assert_eq!(answer.known().as_deref(), Some("first"));
            assert!(!dependencies.is_empty());
            assert_eq!(validations.get(), lookup, "newly read input needs no second read before publication");
        }
        assert_eq!(builds.get(), 1);
        reads::set_buffer(path, "second");
        assert_eq!(load().known().as_deref(), Some("second"));
        assert_eq!(builds.get(), 2);
        reads::drop_buffer(path);
    }

    #[test]
    fn run_snapshots_share_validation_and_refresh_after_invalidation_or_a_new_run() {
        let main = Path::new("/program-snapshot/main");
        let library = Path::new("/program-snapshot/library");
        reads::set_buffer(main, "main");
        reads::set_buffer(library, "one");
        let programs = Programs::default();
        let profile = Profile::new(0, 0);
        let key = CatalogKey { settings: 0, program: 0, outside: None, classes: 1 };
        let snapshot = Snapshot::default();
        let hash = reads::hash_bytes(b"main");
        let load = |snapshot: &Snapshot, revision| snapshot.catalog(revision, key.clone(), main, hash, || {
            programs.catalog(key.clone(), Some(&profile), |dep| dep.holds(|_| reads::MISSING), || {
                let text = reads::read(library).unwrap();
                let mut texts = FxHashMap::default();
                texts.insert(main.into(), reads::hash_bytes(reads::read(main).unwrap().as_bytes()));
                texts.insert(library.into(), reads::hash_bytes(text.as_bytes()));
                Known::Yes(Catalog::new(vec![Declaration {
                    path: library.into(), scope: 1, node: 1, classes: 1,
                    name: text.to_string(), source: None,
                }], texts))
            })
        });
        for _ in 0..20 {
            let (answer, dependencies) = reads::track(|| load(&snapshot, 0));
            assert!(answer.known().unwrap().ambient("one", 1).is_some());
            assert!(!dependencies.is_empty());
        }
        assert_eq!(Profile::get(&profile.program_catalogue_builds), 1);
        assert_eq!(Profile::get(&profile.catalogue_read_validations), 0);
        let next = Snapshot::default();
        for _ in 0..20 { assert!(load(&next, 0).known().unwrap().ambient("one", 1).is_some()); }
        assert_eq!(Profile::get(&profile.catalogue_read_validations), 1);
        reads::set_buffer(library, "two");
        assert!(load(&snapshot, 1).known().unwrap().ambient("two", 1).is_some());
        assert_eq!(Profile::get(&profile.program_catalogue_builds), 2);
        reads::set_buffer(library, "three");
        assert!(load(&Snapshot::default(), 1).known().unwrap().ambient("three", 1).is_some());
        assert_eq!(Profile::get(&profile.program_catalogue_builds), 3);
        for path in [main, library] { reads::drop_buffer(path); }
    }

    #[test]
    fn unknown_and_recursive_builds_do_not_become_completed_answers() {
        let memo = Memo::<(), u32>::default();
        let builds = Cell::new(0);
        let answer = memo.get((), None, |_| true, || {
            builds.set(builds.get() + 1);
            assert_eq!(memo.get((), None, |_| true, || Known::Yes(99)), Known::Unknown);
            Known::Unknown
        });
        assert_eq!(answer, Known::Unknown);
        let answer = memo.get((), None, |_| true, || {
            builds.set(builds.get() + 1);
            Known::Yes(7)
        });
        assert_eq!(answer.known(), Some(7));
        assert_eq!(builds.get(), 2);
    }
}

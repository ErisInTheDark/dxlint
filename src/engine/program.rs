use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::ThreadId;

use crate::names::question::Known;
use crate::fx_hash::FxHashMap;

use super::{Profile, reads::{self, Cached, Reads}};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Context {
    /// The program's configuration file, else its directory
    /// (`program_anchor`): files of one project share their input list.
    pub anchor: PathBuf,
    pub settings: usize,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct CatalogKey {
    pub settings: usize,
    /// The program's identity (`program_identity` in [`super::world`]).
    pub program: u64,
    pub classes: u64,
}

pub(super) struct Declaration {
    /// Shared with every other catalog naming the file (a run keeps many
    /// catalogs of the same program).
    pub path: reads::DepPath,
    pub scope: u32,
    pub node: u32,
    pub classes: u64,
    pub name: String,
    pub source: Option<String>,
}

/// A program's ambient declarations. A linted file outside the program's
/// inputs sees the program's shared catalog as its `base` plus its own
/// declarations after it, as if listed last.
pub(super) struct Catalog {
    base: Option<Arc<Catalog>>,
    declarations: Vec<Declaration>,
    names: FxHashMap<String, Vec<usize>>,
    wildcards: Vec<usize>,
    sources: Vec<usize>,
    texts: FxHashMap<reads::DepPath, u64>,
}

impl Catalog {
    pub fn new(declarations: Vec<Declaration>, texts: FxHashMap<reads::DepPath, u64>) -> Self {
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
        Self { base: None, declarations, names, wildcards, sources, texts }
    }

    /// `base` with `declarations` (of files outside its program) after its own.
    pub fn over(base: Arc<Catalog>, declarations: Vec<Declaration>, texts: FxHashMap<reads::DepPath, u64>) -> Self {
        Self { base: Some(base), ..Self::new(declarations, texts) }
    }

    fn includes_text(&self, path: &std::path::Path, hash: u64) -> bool {
        self.texts.get(&super::reads::dep_path(path)).map_or_else(|| self.base.as_ref().is_some_and(|base| base.includes_text(path, hash)), |&stored| stored == hash)
    }

    /// The catalogs in declaration order: the base's first.
    fn layers(&self) -> impl Iterator<Item = &Catalog> {
        self.base.as_deref().into_iter().chain(std::iter::once(self))
    }

    fn own_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a Declaration> + use<'a> {
        self.names.get(name).into_iter().flatten().map(|&index| &self.declarations[index])
    }

    pub fn named<'a, 'n>(&'a self, name: &'n str, classes: u64) -> impl Iterator<Item = &'a Declaration> + use<'a, 'n> {
        self.layers().flat_map(move |layer| layer.own_named(name))
            .filter(move |declaration| declaration.classes & classes != 0)
    }

    pub fn ambient<'a>(&'a self, name: &str, classes: u64) -> Option<&'a Declaration> {
        self.named(name, classes).find(|declaration| declaration.source.is_none()).or_else(|| {
            self.layers().flat_map(|layer| layer.wildcards.iter().map(|&index| &layer.declarations[index]))
                .find(|declaration| declaration.classes & classes != 0 && crate::path_globs::wildcard(&declaration.name, name))
        })
    }

    pub fn candidates<'a>(&'a self, name: Option<&'a str>, classes: u64) -> impl Iterator<Item = &'a Declaration> {
        let named = name.into_iter().flat_map(move |name| self.layers().flat_map(move |layer| layer.own_named(name)));
        let augment = name.is_none();
        let augmented = self.layers().filter(move |_| augment)
            .flat_map(|layer| layer.sources.iter().map(|&index| &layer.declarations[index]));
        named.chain(augmented).filter(move |declaration| declaration.classes & classes != 0)
    }
}

enum Entry<V> {
    Building(ThreadId),
    Ready(Cached<V>),
}

thread_local! {
    /// Unknown answers this thread got because the answer was still being
    /// built (by itself, or by a thread waiting on it): a cycle, so callers
    /// must not keep unknowns computed meanwhile ([`reentries`]).
    static REENTRIES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// This thread's count of answers cut short by a build in progress.
pub(super) fn reentries() -> u64 {
    REENTRIES.with(std::cell::Cell::get)
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
                Some(Entry::Building(thread)) if *thread == owner => {
                    REENTRIES.with(|count| count.set(count.get() + 1));
                    return Known::Unknown;
                }
                Some(Entry::Building(_)) => {
                    if entries.values().any(|entry| matches!(entry, Entry::Building(thread) if *thread == owner)) {
                        REENTRIES.with(|count| count.set(count.get() + 1));
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

impl<K: Eq + Hash, V> Memo<(u64, K), V> {
    /// The finished answers `keep` accepts, moved to revision `to`.
    fn carry(self, to: u64, keep: &mut Keep<'_>) -> Self {
        let entries = self.entries.into_inner().expect("program cache").into_iter().filter_map(|((revision, key), entry)| match entry {
            Entry::Ready(answer) if keep(revision, Some(&answer.reads), None) => Some(((to, key), Entry::Ready(answer))),
            _ => None,
        }).collect();
        Memo { entries: Mutex::new(entries), changed: Condvar::new() }
    }
}

/// Whether an answer kept at a revision still holds: its reads, or the
/// file it is about.
pub(super) type Keep<'k> = dyn FnMut(u64, Option<&Reads>, Option<&std::path::Path>) -> bool + 'k;

/// The entries of a run-wide map `keep` accepts, moved to revision `to`.
fn carried<K: Eq + Hash, V>(map: RwLock<FxHashMap<K, V>>, mut judge: impl FnMut(K, V) -> Option<(K, V)>) -> RwLock<FxHashMap<K, V>> {
    RwLock::new(map.into_inner().expect("run map").into_iter().filter_map(|(key, value)| judge(key, value)).collect())
}

/// The answers of a map holding each with its revision that `keep`
/// accepts, moved to revision `to` where they lie.
fn carried_in_place<K, V>(map: RwLock<FxHashMap<K, (u64, Cached<V>)>>, to: u64, keep: &mut Keep<'_>) -> RwLock<FxHashMap<K, (u64, Cached<V>)>> {
    let mut map = map.into_inner().expect("run map");
    map.retain(|_, (at, answer)| {
        let kept = keep(*at, Some(&answer.reads), None);
        *at = to;
        kept
    });
    RwLock::new(map)
}

type SurveySlots = FxHashMap<(usize, u64), Arc<std::sync::OnceLock<Cached<Surveyed>>>>;

/// The candidates and sketches a carried run keeps of those built from
/// `surveys`: those of surveys gone are dropped, and those of surveys whose
/// `patched` inputs were read again match those inputs' texts now.
fn patch_surveyed(surveys: &SurveySlots, patched: &FxHashMap<(usize, u64), Vec<usize>>,
    candidates: Slots<(usize, u64, Box<str>), Arc<[u32]>>, words: Slots<(usize, u64), Words>)
    -> (Slots<(usize, u64, Box<str>), Arc<[u32]>>, Slots<(usize, u64), Words>) {
    let survey = |key: &(usize, u64)| surveys.get(key).and_then(|slot| slot.get()).map(|surveyed| &surveyed.value);
    fn ready<V>(cached: Cached<V>) -> Arc<std::sync::OnceLock<Cached<V>>> { Arc::new(std::sync::OnceLock::from(cached)) }
    // Each input read again, sketched alone: names are tested against it,
    // not against every input.
    let alone: FxHashMap<(usize, u64), Vec<(usize, (Survey, Sketches))>> = patched.iter().filter_map(|(key, changed)| {
        let surveyed = survey(key)?;
        Some((*key, changed.iter().map(|&at| {
            let one: Survey = Arc::from([surveyed.sources[at].clone()]);
            let sketches = Sketches::of(&one);
            (at, (one, sketches))
        }).collect()))
    }).collect();
    let words = words.into_inner().expect("run slots").into_iter().filter(|(key, _)| surveys.contains_key(key)).map(|(key, slot)| {
        let (Some(changed), Some(surveyed), Some(built)) = (patched.get(&key), survey(&key), slot.get()) else { return (key, slot) };
        let mut sketches = (*built.value).clone();
        for &at in changed { sketches.add(at, surveyed.sources[at].1.as_deref()) }
        let now = ready(Cached { value: Arc::new(sketches), reads: built.reads.clone() });
        (key, now)
    }).collect();
    let candidates = candidates.into_inner().expect("run slots").into_iter().filter_map(|((settings, program, name), slot)| {
        let key = (settings, program);
        if !surveys.contains_key(&key) { return None }
        let (Some(alone), Some(surveyed), Some(found)) = (alone.get(&key), survey(&key), slot.get()) else { return Some(((settings, program, name), slot)) };
        let flips: Vec<(u32, bool)> = alone.iter()
            .map(|(at, text)| (u32::try_from(*at).expect("inputs fit u32"), surveyed.may_publish(*at, &name, text)))
            .filter(|&(at, member)| found.value.binary_search(&at).is_ok() != member).collect();
        if flips.is_empty() { return Some(((settings, program, name), slot)) }
        let mut list: Vec<u32> = found.value.iter().copied().filter(|at| !flips.iter().any(|(flipped, _)| flipped == at)).collect();
        list.extend(flips.iter().filter(|(_, member)| *member).map(|(at, _)| *at));
        list.sort_unstable();
        let now = ready(Cached { value: list.into(), reads: found.reads.clone() });
        Some(((settings, program, name), now))
    }).collect();
    (Mutex::new(candidates), Mutex::new(words))
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

/// A program's input list; `None` when it cannot be known (a default
/// library missing, say). Unknown is kept like an answer: every lookup
/// asking again would redo the whole collection, per file and per name.
type Inputs = Option<Arc<Vec<PathBuf>>>;

fn inputs(answer: Known<Inputs>) -> Known<Arc<Vec<PathBuf>>> {
    match answer {
        Known::Yes(Some(paths)) => Known::Yes(paths),
        _ => Known::Unknown,
    }
}

#[derive(Default)]
pub(super) struct Programs {
    paths: Memo<Context, Inputs>,
    /// Per program: the revision and overlay count its kept list was last
    /// found holding at. Until files are invalidated or overlaid nothing it
    /// read can change, so it is checked once per such state, not per
    /// asking file.
    checked: Mutex<FxHashMap<Context, (u64, u64)>>,
    /// Input lists by content: files in different directories of one
    /// program share one list.
    inputs: Mutex<FxHashMap<u64, Vec<std::sync::Weak<Vec<PathBuf>>>>>,
    catalogs: Memo<CatalogKey, Arc<Catalog>>,
}

impl Programs {
    /// `holds`: whether an answer's reads still hold, asked once per
    /// `revision` of the files and change of their overlays.
    pub fn paths(&self, context: Context, revision: u64, holds: impl Fn(&Reads) -> bool, build: impl FnOnce() -> Known<Vec<PathBuf>>) -> Known<Arc<Vec<PathBuf>>> {
        let state = (revision, reads::overlays_changed());
        let checked = self.checked.lock().expect("program checks").get(&context) == Some(&state);
        let answer = inputs(self.paths.get(context.clone(), None, |answer| checked || holds(&answer.reads), || Known::Yes(build().known().map(|paths| self.intern(paths)))));
        self.checked.lock().expect("program checks").insert(context, state);
        answer
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

    pub fn catalog(&self, key: CatalogKey, profile: Option<&Profile>, holds: impl Fn(&Reads) -> bool, build: impl FnOnce() -> Known<Catalog>) -> Known<Arc<Catalog>> {
        self.catalogs.get(key, profile, |answer| holds(&answer.reads), || match build() {
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
    /// The linter's revision when the run began: answers kept without one
    /// ([`Slots`]) are at least this fresh.
    revision: u64,
    paths: Memo<(u64, Context), Inputs>,
    catalogs: Memo<(u64, CatalogKey), Arc<Catalog>>,
    // Read-mostly maps every lint thread consults: read locks on hits.
    units: RwLock<FxHashMap<PathBuf, (u64, Arc<UnitSlot>)>>,
    /// Shared module answers, with the revision each holds at.
    modules: RwLock<FxHashMap<ModuleKey, (u64, Cached<Arc<[StableTarget]>>)>>,
    /// Per program and scope classes, the augmentations each module scope
    /// (by path and scope) receives.
    augmentations: RwLock<FxHashMap<ModuleKey, (u64, Cached<Arc<Augmentations>>)>>,
    /// Program inputs (by input list hash) whose units were loaded in parallel.
    prefetched: Mutex<FxHashMap<u64, Arc<std::sync::OnceLock<()>>>>,
    /// Each listed program's inputs as a set: every world asks whether its
    /// linted file is one.
    input_sets: RwLock<FxHashMap<(u64, u64), Arc<crate::fx_hash::FxHashSet<PathBuf>>>>,
    /// Which needles each text (by hash) spells.
    mentions: RwLock<FxHashMap<u64, Vec<(Box<str>, bool)>>>,
    pub(super) text_holds: TextHolds,
    /// [`Snapshot::listed_inputs`] by (revision, program).
    listed: Mutex<FxHashMap<(u64, u64), (Arc<Vec<PathBuf>>, Arc<Reads>, Arc<[reads::Dep]>)>>,
    /// Module resolution's file probes and reads, once per path and revision.
    probes: RwLock<FxHashMap<reads::DepPath, (u64, bool)>>,
    /// Directory listings behind those probes (`None`: unreadable).
    listings: RwLock<FxHashMap<PathBuf, (u64, Option<Arc<reads::Listing>>)>>,
    texts: RwLock<FxHashMap<PathBuf, (u64, Option<Arc<str>>, u64)>>,
    /// Per program (settings, identity) and global name: the inputs whose
    /// text may publish it, as positions in the program's survey.
    candidates: Slots<(usize, u64, Box<str>), Arc<[u32]>>,
    /// Per program: every input's text, read once. Its reads (inputs' texts
    /// and listing) are its value's, not the answer's: users record them as
    /// a scan. A carried run keeps a survey only while its listing holds,
    /// and the candidates and sketches built from it with it.
    surveys: Slots<(usize, u64), Surveyed>,
    /// Per surveyed program, the inputs each word occurs in.
    words: Slots<(usize, u64), Words>,
    /// Program configurations by revision, settings and program directory.
    configurations: RwLock<FxHashMap<(u64, usize, PathBuf), Cached<Arc<crate::names::modules::resolution::Configuration>>>>,
    /// Metadata parsed from those texts, by text hash (`None`: malformed).
    metadata: RwLock<FxHashMap<PathBuf, (u64, Option<Arc<crate::names::modules::resolution::Json>>)>>,
    /// What each package directory offers, by root, subpath and conditions:
    /// importers in every directory below it share the probes (and their
    /// reads, kept once instead of in every importer's answer).
    packages: RwLock<FxHashMap<PackageKey, Cached<PackageProbe>>>,
    /// Values of other files' nodes, by stable leaf under one program: every
    /// linted file reading a declaration in a shared module computes the
    /// same value ([`super::world`]'s `shared_value`). `None`: unknown, and
    /// unknown for every linted file alike.
    answers: RwLock<FxHashMap<ProgramKey, Arc<ProgramAnswers>>>,
    /// Other files' tested roots: (root's file, root, owners in the file).
    tested: RwLock<FxHashMap<TestedKey, Cached<Arc<[TestedRoot]>>>>,
    /// Frames those values are read in, interned run-wide.
    frames: RwLock<(FxHashMap<Arc<StableBinding>, u32>, Vec<Arc<StableBinding>>)>,
    /// An editor's run: each file's last lint, for its next one.
    pub(super) editor: Option<super::editing::Memories>,
    /// Lints finished so far.
    lints: std::sync::atomic::AtomicU64,
    /// When set ([`Snapshot::retire_idle`]), each program's last use, in
    /// [`Self::lints`].
    uses: Mutex<Option<FxHashMap<(usize, u64), u64>>>,
}

/// Lints a program or unit may sit unused before it goes. Too few costs
/// computing it again, never a different answer.
const PROGRAM_IDLE_LINTS: u64 = 32;
const UNIT_IDLE_LINTS: u64 = 512;
/// Lints between sweeps of the run's units.
const UNIT_SWEEP_LINTS: u64 = 16;

/// A leaf by file path: the same node in another world's file ids.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum StableLeaf {
    Node(reads::DepPath, u32, StableFrame),
    Scope(reads::DepPath, u32, StableFrame),
    Traits(u64, StableFrame),
}

/// A frame every world builds alike: none, or one built only from other
/// files' nodes and run-wide values ([`Snapshot::stable_frame`] ids).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum StableFrame {
    Root,
    Bound(u32),
}

/// A frame by stable parts: instantiating `owner` through a stable leaf or
/// with arguments every world evaluates alike.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct StableBinding {
    pub parent: StableFrame,
    pub owner: (reads::DepPath, u32),
    pub through: Option<StableLeaf>,
    /// (overlay, slots).
    pub args: Option<(bool, Box<[Option<StableValue>]>)>,
    pub classes: u64,
}

/// A value's rows by stable leaf, and how far they are normalized.
pub(super) type StableValue = (Arc<[Box<[StableLeaf]>]>, crate::names::question::Normal);

/// (settings, program, outside).
type ProgramKey = (usize, u64, Option<PathBuf>);

/// What every linted file of one program derives alike. A table per program
/// keeps its keys short, and goes whole when the program is done.
#[derive(Default)]
pub(super) struct ProgramAnswers {
    values: RwLock<FxHashMap<ValueKey, Cached<Option<StableValue>>>>,
    /// Facts derived from those values, the same way.
    facts: RwLock<FxHashMap<RunFactKey, Cached<StableFact>>>,
}

impl ProgramAnswers {
    pub(super) fn value_answer(&self, key: &ValueKey) -> Option<Cached<Option<StableValue>>> {
        self.values.read().expect("run values").get(key).cloned()
    }

    pub(super) fn keep_value(&self, key: ValueKey, answer: Cached<Option<StableValue>>) {
        self.values.write().expect("run values").insert(key, answer);
    }

    pub(super) fn fact_answer(&self, key: &RunFactKey) -> Option<Cached<StableFact>> {
        self.facts.read().expect("run facts").get(key).cloned()
    }

    pub(super) fn keep_fact(&self, key: RunFactKey, answer: Cached<StableFact>) {
        self.facts.write().expect("run facts").insert(key, answer);
    }
}

/// (revision, leaf).
pub(super) type ValueKey = (u64, StableLeaf);

/// A [`crate::names::question::FactKey`] by path.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum StableFactKey {
    Trait(StableLeaf, u16),
    Optional(StableLeaf),
    Closed(StableLeaf),
    MemberKey(StableLeaf, StableKey),
    Members(StableLeaf),
    Accepts(StableValue, StableValue),
}

/// (revision, fact).
pub(super) type RunFactKey = (u64, StableFactKey);

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum StableKey {
    Text(Box<str>),
    Ordinal(u32),
    Symbol(reads::DepPath, u32),
    Domain(crate::config::ast::values::KeyDomain),
}

#[derive(Clone)]
pub(super) struct StableSource {
    pub site: StableLeaf,
    pub value: Option<crate::names::Value>,
    pub origin: crate::names::values::MemberOrigin,
}

#[derive(Clone)]
pub(super) struct StableMember {
    pub key: StableKey,
    pub sources: Box<[StableSource]>,
    pub required: Known<bool>,
}

#[derive(Clone)]
pub(super) enum StableFact {
    Flag(bool),
    Member(Option<StableMember>),
    Members(Box<[StableMember]>),
}

/// (revision, settings, scanned file).
pub(super) type TestedKey = (u64, usize, reads::DepPath);
pub(super) type TestedRoot = (reads::DepPath, u32, Vec<u32>);

type PackageProbe = crate::names::modules::resolution::Probe<std::path::PathBuf>;

/// (revision, settings, package root, subpath, conditions).
pub(super) type PackageKey = (u64, usize, std::path::PathBuf, Box<str>, Box<[Box<str>]>);

/// A shared module question ([`crate::semantic::modules::SharedQuestion`])
/// with files by path, under one program.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct ModuleKey {
    pub settings: usize,
    /// The program's identity: a hash of its input paths (0 for questions
    /// no program changes).
    pub program: u64,
    /// The linted file when it is outside the program's inputs (and the
    /// question depends on the program).
    pub outside: Option<PathBuf>,
    pub question: StableQuestion,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum StableQuestion {
    Publication { kind: crate::names::modules::index::PublicationKind, path: reads::DepPath, scope: u32, name: Box<str>, class: u64 },
    Global { name: Box<str>, class: u64 },
    /// Every scope merged into module scope `scope` of `path`.
    Scopes { path: reads::DepPath, scope: u32 },
    /// The files `scope` of `path` pulls in through its global links.
    GlobalReach { path: reads::DepPath, scope: u32, class: u64 },
    /// The module scope `specifier` names, imported from a file in `directory`.
    Resolve { directory: reads::DepPath, specifier: Box<str> },
    /// Every augmentation of the program's module scopes in `classes`.
    Augmentations { classes: u64 },
}

/// A program's inputs with their texts (`None`: unreadable) and whether the
/// text holds a name-independent publication hint (so it may publish any
/// global name).
pub(super) type Survey = Arc<[(PathBuf, Option<Arc<str>>, bool)]>;

/// A program's input list (when known), its reads other than which files
/// exist, and those listing reads ([`reads::track_listing`]).
pub(super) type ListedInputs = (Known<Arc<Vec<PathBuf>>>, Arc<Reads>, Arc<[reads::Dep]>);

/// A survey, with what it read for the scans of lookups over it: the
/// inputs' [`reads::Dep::Text`] reads, in survey order, then the listing's.
#[derive(Clone)]
pub(super) struct Surveyed {
    pub sources: Survey,
    pub reads: Arc<Reads>,
    /// The name-independent publication hints each text was checked for.
    pub hints: Arc<[Box<str>]>,
}

/// Whether text (`None`: unreadable) may publish any global name.
pub(super) fn holds_hint(text: Option<&str>, hints: &[Box<str>]) -> bool {
    text.is_none_or(|text| hints.iter().any(|hint| text.contains(&**hint)))
}

impl Surveyed {
    /// The inputs whose text changed, by the editor's `keep`.
    fn changed(&self, revision: u64, keep: &mut Keep<'_>) -> Vec<usize> {
        self.sources.iter().enumerate().filter(|(_, (path, ..))| !keep(revision, None, Some(path))).map(|(at, _)| at).collect()
    }

    /// This survey with inputs `changed` read again.
    fn patched(&self, changed: &[usize]) -> Surveyed {
        let mut sources = self.sources.to_vec();
        let mut deps = self.reads.deps.clone();
        for &at in changed {
            let (path, text, hinted) = &mut sources[at];
            let (now, hash) = reads::read_unrecorded(path);
            *hinted = holds_hint(now.as_deref(), &self.hints);
            *text = now;
            if let Some(reads::Dep::Text(_, read)) = deps.get_mut(at) { *read = hash }
        }
        Surveyed { sources: sources.into(), reads: Arc::new(Reads { deps, ..Default::default() }), hints: self.hints.clone() }
    }

    /// Whether input `at`'s text may publish global `name` (`text`: its
    /// sketch alone).
    fn may_publish(&self, at: usize, name: &str, text: &(Survey, Sketches)) -> bool {
        let mut holds = [false];
        text.1.mark(name, &text.0, &mut holds, &TextHolds::default());
        self.sources[at].2 || holds[0]
    }
}

/// The byte trigrams each surveyed input's text holds, hashed into a fixed
/// set of bits (2 KiB per input): a name can only occur in a text holding
/// every trigram of it, so only those texts are searched. Kept per bit (the
/// inputs holding it), so a name's candidates are a few rows ANDed together
/// instead of a test of every input.
#[derive(Clone)]
pub(super) struct Sketches {
    /// Words per row: one bit per input.
    width: usize,
    /// Row `bit` is `rows[bit * width..][..width]`.
    rows: Box<[u64]>,
    /// The inputs with a readable text.
    readable: Box<[u64]>,
}

const SKETCH_BITS: usize = 256 * 64;

pub(super) type Words = Arc<Sketches>;

/// Which texts spell which global names: programs of one run mostly share
/// their libraries, so each pair is searched once. By the text's address,
/// which a run's survey keeps alive.
#[derive(Default)]
pub(super) struct TextHolds(RwLock<FxHashMap<Box<str>, FxHashMap<usize, bool>>>);

impl TextHolds {
    fn holds(&self, name: &str, text: &Arc<str>, search: impl FnOnce() -> bool) -> bool {
        let at = Arc::as_ptr(text).cast::<u8>() as usize;
        if let Some(found) = self.0.read().expect("run text holds").get(name).and_then(|texts| texts.get(&at)) { return *found }
        let found = search();
        self.0.write().expect("run text holds").entry(name.into()).or_default().insert(at, found);
        found
    }
}

fn trigram_bit(gram: &[u8]) -> usize {
    let packed = u32::from(gram[0]) | u32::from(gram[1]) << 8 | u32::from(gram[2]) << 16;
    (packed.wrapping_mul(0x9E37_79B1) >> 15) as usize % SKETCH_BITS
}

impl Sketches {
    /// Marks in `holding` (one flag per input of `survey`, which these
    /// sketches were built from) exactly the inputs whose text contains
    /// `name`, as a substring search of every text would.
    pub(super) fn mark(&self, name: &str, survey: &Survey, holding: &mut [bool], known: &TextHolds) {
        let finder = memchr::memmem::Finder::new(name.as_bytes());
        let mut candidates = self.readable.to_vec();
        for bit in name.as_bytes().windows(3).map(trigram_bit) {
            for (word, row) in candidates.iter_mut().zip(&self.rows[bit * self.width..][..self.width]) { *word &= row }
        }
        for (index, mut word) in candidates.into_iter().enumerate() {
            while word != 0 {
                let input = index * 64 + word.trailing_zeros() as usize;
                word &= word - 1;
                let (Some((_, Some(text), _)), Some(holds)) = (survey.get(input), holding.get_mut(input)) else { continue };
                if known.holds(name, text, || finder.find(text.as_bytes()).is_some()) { *holds = true }
            }
        }
    }

    fn of(survey: &Survey) -> Sketches {
        let width = survey.len().div_ceil(64);
        let mut sketches = Sketches { width, rows: vec![0u64; SKETCH_BITS * width].into(), readable: vec![0u64; width].into() };
        for (input, (_, text, _)) in survey.iter().enumerate() { sketches.add(input, text.as_deref()) }
        sketches
    }

    /// Adds input `input`'s text: its trigrams join those it held (a superset
    /// still marks exactly, see [`Sketches::mark`]).
    fn add(&mut self, input: usize, text: Option<&str>) {
        let Some(text) = text else { return };
        let (word, mask) = (input / 64, 1u64 << (input % 64));
        self.readable[word] |= mask;
        for gram in text.as_bytes().windows(3) { self.rows[trigram_bit(gram) * self.width + word] |= mask }
    }
}

/// The trigram sketches of `survey`'s texts.
pub(super) fn sketch_texts(survey: &Survey) -> Words {
    Arc::new(Sketches::of(survey))
}

/// Module scope (path, scope) -> the scopes that augment it.
pub(super) type Augmentations = FxHashMap<(reads::DepPath, u32), Vec<(reads::DepPath, u32)>>;

/// A module answer's target with its file by path.
#[derive(Clone)]
pub(super) enum StableTarget {
    Node(reads::DepPath, u32),
    Scope(reads::DepPath, u32),
}

/// One file's run unit: its text hash and the unit, when it parses.
#[derive(Default)]
struct UnitSlot {
    loaded: std::sync::OnceLock<(u64, Option<Arc<super::units::Unit>>)>,
    /// [`Snapshot::lints`] when a lint last asked for this unit.
    last_asked: std::sync::atomic::AtomicU64,
}

impl Snapshot {
    pub(super) fn retire_idle(&self) {
        *self.uses.lock().expect("program uses") = Some(Default::default());
    }

    /// A lint is using program (`settings`, `program`).
    pub(super) fn using_program(&self, settings: usize, program: u64) {
        if let Some(uses) = &mut *self.uses.lock().expect("program uses") {
            uses.insert((settings, program), self.lints.load(std::sync::atomic::Ordering::Relaxed));
        }
    }

    /// A lint ended: programs and units idle since long are dropped, here
    /// and from `kept`, the units kept between runs.
    pub(super) fn lint_ended(&self, kept: &Mutex<FxHashMap<PathBuf, Arc<super::units::Unit>>>) {
        let now = self.lints.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let retired: Vec<(usize, u64)> = {
            let mut guard = self.uses.lock().expect("program uses");
            let Some(uses) = &mut *guard else { return };
            let idle: Vec<_> = uses.iter().filter(|(_, &last)| last + PROGRAM_IDLE_LINTS < now).map(|(&key, _)| key).collect();
            for key in &idle { uses.remove(key); }
            idle
        };
        if !retired.is_empty() {
            for (settings, program) in retired { self.forget_program(settings, program) }
            // A program's worth of pages went free: the system can have them.
            crate::alloc::trim();
            crate::alloc::release();
        }
        if now % UNIT_SWEEP_LINTS == 0 {
            let mut forgotten = Vec::new();
            self.units.write().expect("run units").retain(|path, (_, slot)| {
                let keep = slot.last_asked.load(std::sync::atomic::Ordering::Relaxed) + UNIT_IDLE_LINTS >= now;
                if !keep { forgotten.push(path.clone()) }
                keep
            });
            let mut kept = kept.lock().expect("unit cache");
            for path in forgotten { kept.remove(&path); }
        }
    }

    /// Drops what the run keeps for one program alone.
    fn forget_program(&self, settings: usize, program: u64) {
        let other = |s: usize, p: u64| s != settings || p != program;
        self.answers.write().expect("run answers").retain(|key, _| other(key.0, key.1));
        self.modules.write().expect("run modules").retain(|key, _| other(key.settings, key.program));
        self.augmentations.write().expect("run augmentations").retain(|key, _| other(key.settings, key.program));
        self.candidates.lock().expect("run slots").retain(|key, _| other(key.0, key.1));
        self.surveys.lock().expect("run slots").retain(|key, _| other(key.0, key.1));
        self.words.lock().expect("run slots").retain(|key, _| other(key.0, key.1));
    }

    pub(super) fn new(revision: u64) -> Snapshot {
        Snapshot { revision, ..Default::default() }
    }

    /// A run at revision `to` starting with this run's answers that still
    /// hold: an editor's next lint after a keystroke redoes only what read
    /// the edited files.
    pub(super) fn carry(self, to: u64, keep: &mut Keep<'_>) -> Snapshot {
        let Snapshot { revision, paths, catalogs, units, modules, augmentations, prefetched: _, input_sets: _, mentions: _, text_holds: _, listed: _, probes, listings, texts,
            candidates, surveys, words, configurations, metadata, packages, answers, tested: _, frames, editor, lints: _, uses: _ } = self;
        // A survey holds while its listing does; inputs whose text changed
        // are read again, and what was built from them patched to match.
        let mut patched: FxHashMap<(usize, u64), Vec<usize>> = FxHashMap::default();
        let surveys: FxHashMap<_, _> = surveys.into_inner().expect("run slots").into_iter().filter_map(|(key, slot)| {
            let survey = slot.get()?;
            let listing = Reads { deps: survey.value.reads.deps.iter().filter(|dep| !dep.reads_text()).cloned().collect(), ..Default::default() };
            if !keep(revision, Some(&listing), None) { return None }
            let changed = survey.value.changed(revision, keep);
            if changed.is_empty() { return Some((key, slot)) }
            let now = Cached { value: survey.value.patched(&changed), reads: survey.reads.clone() };
            patched.insert(key, changed);
            Some((key, Arc::new(std::sync::OnceLock::from(now))))
        }).collect();
        let (candidates, words) = patch_surveyed(&surveys, &patched, candidates, words);
        Snapshot {
            revision: to,
            paths: paths.carry(to, keep),
            catalogs: catalogs.carry(to, keep),
            units: carried(units, |path, (at, slot)| {
                let names = slot.loaded.get().and_then(|(_, unit)| unit.as_ref()?.names.get()).map(|names| &*names.reads);
                keep(at, names, Some(&*path)).then(|| (path, (to, slot.clone())))
            }),
            modules: carried_in_place(modules, to, keep),
            augmentations: carried_in_place(augmentations, to, keep),
            prefetched: Default::default(),
            input_sets: Default::default(),
            mentions: Default::default(),
            text_holds: Default::default(),
            listed: Default::default(),
            probes: carried(probes, |path, (at, found)| keep(at, None, Some(&*path)).then_some((path, (to, found)))),
            listings: carried(listings, |dir, (at, listing)| keep(at, None, Some(&*dir)).then_some((dir, (to, listing)))),
            texts: carried(texts, |path, (at, text, hash)| keep(at, None, Some(&*path)).then_some((path, (to, text, hash)))),
            candidates,
            words,
            surveys: Mutex::new(surveys),
            configurations: carried(configurations, |(at, settings, dir), answer| keep(at, Some(&answer.reads), None).then(|| ((to, settings, dir), answer))),
            metadata,
            packages: carried(packages, |(at, settings, root, subpath, conditions), answer|
                keep(at, Some(&answer.reads), None).then(|| ((to, settings, root, subpath, conditions), answer))),
            answers: RwLock::new(answers.into_inner().expect("run answers").into_iter().map(|(program, table)| {
                let values = table.values.read().expect("run values").iter()
                    .filter(|((at, _), answer)| keep(*at, Some(&answer.reads), None))
                    .map(|((_, leaf), answer)| ((to, leaf.clone()), answer.clone())).collect();
                (program, Arc::new(ProgramAnswers { values: RwLock::new(values), facts: Default::default() }))
            }).collect()),
            tested: Default::default(),
            frames,
            editor,
            lints: Default::default(),
            uses: Default::default(),
        }
    }

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
                    let slot = Arc::new(UnitSlot::default());
                    units.insert(path.to_owned(), (revision, slot.clone()));
                    slot
                }
            }
        });
        slot.last_asked.store(self.lints.load(std::sync::atomic::Ordering::Relaxed), std::sync::atomic::Ordering::Relaxed);
        slot.loaded.get_or_init(load).clone()
    }

    /// Whether `path` is a file, probed once per revision; the caller
    /// records the read.
    pub(super) fn is_file(&self, revision: u64, path: &std::path::Path) -> bool {
        if let Some(&(at, found)) = self.probes.read().expect("run probes").get(&super::reads::dep_path(path)) {
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
        self.probes.write().expect("run probes").insert(reads::dep_path(path), (revision, found));
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
        load: impl FnOnce() -> Result<Arc<crate::names::modules::resolution::Configuration>, ()>)
        -> Result<Cached<Arc<crate::names::modules::resolution::Configuration>>, ()> {
        if let Some(found) = self.configurations.read().expect("run configurations").get(&key) { return Ok(found.clone()) }
        let answer = Cached::compute(load);
        let Ok(value) = answer.value else { reads::note_group(&answer.reads); return Err(()) };
        let found = Cached { value, reads: answer.reads };
        self.configurations.write().expect("run configurations").insert(key, found.clone());
        Ok(found)
    }

    /// A program's input texts, read once per run; concurrent callers wait
    /// for the one reading.
    pub(super) fn survey(&self, key: (usize, u64), read: impl FnOnce() -> Surveyed) -> Cached<Surveyed> {
        once(&self.surveys, key, read)
    }

    /// A survey's trigram sketches, built once per run.
    pub(super) fn words(&self, key: (usize, u64), build: impl FnOnce() -> Words) -> Cached<Words> {
        once(&self.words, key, build)
    }

    /// The inputs that may publish a global name, computed once per run.
    pub(super) fn candidates(&self, key: (usize, u64, Box<str>), compute: impl FnOnce() -> Arc<[u32]>) -> Cached<Arc<[u32]>> {
        once(&self.candidates, key, compute)
    }

    /// `path`'s metadata parsed from the text with `hash`, parsed once per
    /// text.
    pub(super) fn metadata(&self, path: &std::path::Path, hash: u64, text: &str)
        -> Option<Arc<crate::names::modules::resolution::Json>> {
        if let Some((at, parsed)) = self.metadata.read().expect("run metadata").get(path) {
            if *at == hash { return parsed.clone() }
        }
        let parsed = crate::names::modules::resolution::parse_metadata(Some(text)).ok().flatten();
        self.metadata.write().expect("run metadata").insert(path.to_owned(), (hash, parsed.clone()));
        parsed
    }

    /// Runs `load` once per program for this run; concurrent callers wait
    /// for it instead of loading the same files in the same order.
    /// Whether the text hashing to `hash` spells `needle`, searched once.
    pub(super) fn text_mentions(&self, hash: u64, needle: &str, find: impl FnOnce() -> bool) -> bool {
        if let Some(needles) = self.mentions.read().expect("run mentions").get(&hash) {
            if let Some((_, found)) = needles.iter().find(|(known, _)| &**known == needle) { return *found }
        }
        let found = find();
        self.mentions.write().expect("run mentions").entry(hash).or_default().push((needle.into(), found));
        found
    }

    /// Whether `path` is one of the inputs listed under `key`.
    pub(super) fn is_input(&self, key: (u64, u64), inputs: &[PathBuf], path: &std::path::Path) -> bool {
        if let Some(set) = self.input_sets.read().expect("run input sets").get(&key) { return set.contains(path) }
        let set = Arc::new(inputs.iter().cloned().collect::<crate::fx_hash::FxHashSet<_>>());
        let holds = set.contains(path);
        self.input_sets.write().expect("run input sets").insert(key, set);
        holds
    }

    pub(super) fn prefetch(&self, program: u64, load: impl FnOnce()) {
        let slot = self.prefetched.lock().expect("run prefetch").entry(program).or_default().clone();
        slot.get_or_init(load);
    }

    pub(super) fn augmentations(&self, revision: u64, key: &ModuleKey) -> Option<Cached<Arc<Augmentations>>> {
        self.augmentations.read().expect("run augmentations").get(key).filter(|(at, _)| *at == revision).map(|(_, answer)| answer.clone())
    }

    pub(super) fn keep_augmentations(&self, revision: u64, key: ModuleKey, answer: Cached<Arc<Augmentations>>) {
        self.augmentations.write().expect("run augmentations").insert(key, (revision, answer));
    }

    pub(super) fn module_answer(&self, revision: u64, key: &ModuleKey) -> Option<Cached<Arc<[StableTarget]>>> {
        self.modules.read().expect("run modules").get(key).filter(|(at, _)| *at == revision).map(|(_, answer)| answer.clone())
    }

    pub(super) fn keep_module_answer(&self, revision: u64, key: ModuleKey, answer: Cached<Arc<[StableTarget]>>) {
        self.modules.write().expect("run modules").insert(key, (revision, answer));
    }

    /// What this program's linted files share.
    pub(super) fn answers(&self, settings: usize, program: u64, outside: &Option<PathBuf>) -> Arc<ProgramAnswers> {
        let key = (settings, program, outside.clone());
        if let Some(found) = self.answers.read().expect("run answers").get(&key) { return found.clone() }
        self.answers.write().expect("run answers").entry(key).or_default().clone()
    }

    pub(super) fn tested_answer(&self, key: &TestedKey) -> Option<Cached<Arc<[TestedRoot]>>> {
        self.tested.read().expect("run tested roots").get(key).cloned()
    }

    pub(super) fn keep_tested(&self, key: TestedKey, answer: Cached<Arc<[TestedRoot]>>) {
        self.tested.write().expect("run tested roots").insert(key, answer);
    }

    ///`binding`'s run-wide frame id.
    pub(super) fn stable_frame(&self, binding: StableBinding) -> StableFrame {
        if let Some(&id) = self.frames.read().expect("run frames").0.get(&binding) { return StableFrame::Bound(id) }
        let mut frames = self.frames.write().expect("run frames");
        let (ids, bindings) = &mut *frames;
        let binding = Arc::new(binding);
        let next = u32::try_from(bindings.len()).expect("run frames fit u32");
        let id = *ids.entry(binding.clone()).or_insert_with(|| { bindings.push(binding); next });
        StableFrame::Bound(id)
    }

    pub(super) fn stable_binding(&self, id: u32) -> Option<Arc<StableBinding>> {
        self.frames.read().expect("run frames").1.get(id as usize).cloned()
    }

    /// `key`'s package probe, run once per run (failures are not kept); the
    /// caller records its reads as one group.
    pub(super) fn package(&self, key: PackageKey, probe: impl FnOnce() -> Result<PackageProbe, ()>) -> Result<PackageProbe, ()> {
        if let Some(found) = self.packages.read().expect("run packages").get(&key) {
            reads::note_group(&found.reads);
            return Ok(found.value.clone());
        }
        let answer = Cached::compute(probe);
        reads::note_group(&answer.reads);
        let value = answer.value?;
        self.packages.write().expect("run packages").insert(key, Cached { value: value.clone(), reads: answer.reads });
        Ok(value)
    }

    pub fn paths(&self, revision: u64, context: Context, build: impl FnOnce() -> Known<Arc<Vec<PathBuf>>>) -> Known<Arc<Vec<PathBuf>>> {
        inputs(self.paths.get((revision, context), None, |_| true, || Known::Yes(build().known())))
    }

    /// A program's input list with its reads split ([`reads::track_listing`]),
    /// by revision and program: every file of the program splits the same.
    pub(super) fn listed_inputs(&self, key: (u64, u64), list: impl FnOnce() -> ListedInputs) -> ListedInputs {
        if let Some((paths, kept, listing)) = self.listed.lock().expect("run listed").get(&key) {
            return (Known::Yes(paths.clone()), kept.clone(), listing.clone());
        }
        let (paths, kept, listing) = list();
        if let Known::Yes(paths) = &paths {
            self.listed.lock().expect("run listed").insert(key, (paths.clone(), kept.clone(), listing.clone()));
        }
        (paths, kept, listing)
    }

    /// `linted`: the linted file's path and text hash when the catalog lists
    /// it (an input), so a catalog built from another text of it is redone.
    pub fn catalog(&self, revision: u64, key: CatalogKey, linted: Option<(&std::path::Path, u64)>, build: impl FnOnce() -> Known<Arc<Catalog>>) -> Known<Arc<Catalog>> {
        self.catalogs.get((revision, key), None, |answer| linted.is_none_or(|(path, hash)| answer.value.includes_text(path, hash)), build)
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
            path: reads::dep_path(Path::new("/catalog/library")), scope, node: scope, classes,
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
    fn sketches_mark_exactly_the_texts_a_substring_search_finds() {
        let texts = ["declare var Promise: PromiseConstructor;", "", "Map Set WeakMap", "no names here", "promise", "PromiseLike<T>"];
        let mut entries: Vec<(PathBuf, Option<Arc<str>>, bool)> = texts.iter().enumerate()
            .map(|(index, text)| (PathBuf::from(format!("/sketch/{index}")), Some(Arc::from(*text)), false)).collect();
        entries.push((PathBuf::from("/sketch/unreadable"), None, false));
        // Past one word of inputs, so rows span several words.
        entries.extend((0..70).map(|index| (PathBuf::from(format!("/sketch/fill{index}")), Some(Arc::from(format!("filler {index} Set"))), false)));
        let survey: Survey = entries.into();
        let sketches = sketch_texts(&survey);
        for name in ["Promise", "Map", "Set", "Pr", "x", "WeakMap", "absent", "filler 69"] {
            let mut holding = vec![false; survey.len()];
            sketches.mark(name, &survey, &mut holding, &TextHolds::default());
            let expected: Vec<bool> = survey.iter().map(|(_, text, _)| text.as_deref().is_some_and(|text| text.contains(name))).collect();
            assert_eq!(holding, expected, "{name}");
        }
    }

    #[test]
    fn a_catalog_over_a_base_answers_like_one_listing_both() {
        let declaration = |scope, name: &str, classes, source: Option<&str>| Declaration {
            path: reads::dep_path(Path::new("/catalog/layers")), scope, node: scope, classes,
            name: name.into(), source: source.map(str::to_owned),
        };
        let base = || vec![declaration(1, "lib-*", 1, None), declaration(2, "lib-one", 1, Some("./a")), declaration(3, "other", 3, None)];
        let own = || vec![declaration(4, "lib-one", 1, None), declaration(5, "lib-one", 1, Some("./b")), declaration(6, "own-*", 2, None)];
        let flat = Catalog::new(base().into_iter().chain(own()).collect(), Default::default());
        let layered = Catalog::over(Arc::new(Catalog::new(base(), Default::default())), own(), Default::default());
        let scopes = |declarations: Vec<&Declaration>| declarations.into_iter().map(|declaration| declaration.scope).collect::<Vec<_>>();
        for classes in [1, 2, 3] {
            for name in ["lib-one", "lib-two", "other", "own-x"] {
                assert_eq!(layered.ambient(name, classes).map(|declaration| declaration.scope), flat.ambient(name, classes).map(|declaration| declaration.scope));
                assert_eq!(scopes(layered.named(name, classes).collect()), scopes(flat.named(name, classes).collect()));
                assert_eq!(scopes(layered.candidates(Some(name), classes).collect()), scopes(flat.candidates(Some(name), classes).collect()));
            }
            assert_eq!(scopes(layered.candidates(None, classes).collect()), scopes(flat.candidates(None, classes).collect()));
        }
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
            !dep.holds()
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
        let profile = crate::engine::new_profile(0, 0);
        let key = CatalogKey { settings: 0, program: 0, classes: 1 };
        let snapshot = Snapshot::default();
        let hash = reads::hash_bytes(b"main");
        let load = |snapshot: &Snapshot, revision| snapshot.catalog(revision, key.clone(), Some((main, hash)), || {
            programs.catalog(key.clone(), Some(&profile), |reads| !reads.any(&mut |dep| !dep.holds()), || {
                let text = reads::read(library).unwrap();
                let mut texts = FxHashMap::default();
                texts.insert(reads::dep_path(main), reads::hash_bytes(reads::read(main).unwrap().as_bytes()));
                texts.insert(reads::dep_path(library), reads::hash_bytes(text.as_bytes()));
                Known::Yes(Catalog::new(vec![Declaration {
                    path: reads::dep_path(library), scope: 1, node: 1, classes: 1,
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
    fn unknown_program_inputs_are_collected_once_until_their_reads_change() {
        let config = Path::new("/program-inputs/tsconfig.json");
        reads::set_buffer(config, "{}");
        let programs = Programs::default();
        let context = Context { anchor: config.to_path_buf(), settings: 0 };
        let (builds, validations) = (Cell::new(0), Cell::new(0));
        let ask = |snapshot: &Snapshot, revision: u64| snapshot.paths(revision, context.clone(), || {
            programs.paths(context.clone(), revision, |reads| {
                validations.set(validations.get() + 1);
                !reads.any(&mut |dep| !dep.holds())
            }, || {
                builds.set(builds.get() + 1);
                reads::read(config);
                Known::Unknown
            })
        });
        let run = Snapshot::default();
        for _ in 0..20 { assert_eq!(ask(&run, 0), Known::Unknown) }
        assert_eq!((builds.get(), validations.get()), (1, 0), "one run collects an unknown input list once");
        // Other tests set buffers concurrently: each change may cost one check.
        let overlays = reads::overlays_changed();
        for _ in 0..20 { assert_eq!(ask(&Snapshot::default(), 0), Known::Unknown) }
        assert_eq!(builds.get(), 1);
        assert!(validations.get() as u64 <= reads::overlays_changed() - overlays, "nothing changed before the next revision");
        assert_eq!(ask(&Snapshot::default(), 1), Known::Unknown);
        assert_eq!(builds.get(), 1, "a new revision keeps it while its reads hold");
        assert!(validations.get() >= 1);
        reads::set_buffer(config, "{\"files\":[]}");
        assert_eq!(ask(&Snapshot::default(), 1), Known::Unknown);
        assert_eq!(builds.get(), 2, "a changed buffer is checked within the revision");
        reads::drop_buffer(config);
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

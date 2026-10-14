//! The only door from native code to the filesystem. Inside [`track`],
//! every read is recorded as a [`Dep`], so a lint result can be reused for
//! exactly as long as everything it read still reads the same (see
//! `engine::cache`). Open editor buffers ([`set_buffer`]) are read in place
//! of the disk.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use crate::grammar::names::modules::resolution::DiscoveryRequest;

/// One thing a lint read, with what it read.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dep {
    /// A file's content hash ([`MISSING`] when unreadable).
    Text(PathBuf, u64),
    IsFile(PathBuf, bool),
    IsDir(PathBuf, bool),
    /// A directory's entry names, hashed.
    List(PathBuf, u64),
    /// Another file's summary: its content hash and summary fingerprint
    /// ([`MISSING`] for both when there is none). Still holds when the
    /// content changed but the fingerprint did not.
    Summary(PathBuf, u64, u64),
    /// The files below a directory a [`Discovery`] (by name) finds.
    Discovered(&'static str, PathBuf, u64),
    /// A policy-filtered discovery, retaining its request for cache replay.
    Requested(&'static str, DiscoveryRequest, u64),
}

impl Dep {
    pub fn path(&self) -> &Path {
        match self {
            Dep::Text(p, _) | Dep::IsFile(p, _) | Dep::IsDir(p, _) | Dep::List(p, _) | Dep::Summary(p, ..) | Dep::Discovered(_, p, _) => p,
            Dep::Requested(_, request, _) => &request.root,
        }
    }

    /// Could a change to the file `changed` (edited, created or deleted)
    /// change this dep's answer?
    pub fn touches(&self, changed: &Path) -> bool {
        match self {
            Dep::List(dir, _) | Dep::IsDir(dir, _) | Dep::Discovered(_, dir, _) => changed.starts_with(dir),
            Dep::Requested(_, request, _) => changed.starts_with(&request.root),
            dep => dep.path() == changed,
        }
    }

    /// Whether the dep still holds. Summaries need the summariser, so the
    /// caller answers those: `fingerprint(path)` is the current summary's,
    /// asked only when the content changed.
    pub fn holds(&self, fingerprint: impl Fn(&Path) -> u64) -> bool {
        match self {
            Dep::Text(p, h) => content(p).1 == *h,
            Dep::IsFile(p, b) => file_exists(p) == *b,
            Dep::IsDir(p, b) => directory_exists(p) == *b,
            Dep::List(p, h) => listing(p).1 == *h,
            Dep::Summary(p, content_hash, f) => content(p).1 == *content_hash || fingerprint(p) == *f,
            Dep::Discovered(name, p, h) => discovery(name).is_some_and(|d| walk(d, p, 1).1 == *h),
            Dep::Requested(name, request, hash) => *name == PROGRAM_FILES.name && request_walk(request).map_or(MISSING, |(_, current)| current) == *hash,
        }
    }
}

/// The hash of something that is not there.
pub const MISSING: u64 = 0;

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish().max(1)
}

// ---- recording ---------------------------------------------------------------

/// What some work read: its own deps, plus the reads of the shared answers
/// it used (kept whole: many lints share them).
#[derive(Debug, Default)]
pub struct Reads {
    pub deps: Vec<Dep>,
    pub groups: Vec<Arc<Reads>>,
}

/// An answer and its dependencies; every consumer records those reads.
#[derive(Clone)]
pub(crate) struct Cached<T> {
    pub(crate) value: T,
    pub(crate) reads: Arc<Reads>,
}

impl<T: Clone> Cached<T> {
    pub(crate) fn compute(f: impl FnOnce() -> T) -> Self {
        let (value, reads) = track(f);
        Cached { value, reads: Arc::new(reads) }
    }

    pub(crate) fn get(&self) -> T {
        self.as_ref().clone()
    }

    pub(crate) fn as_ref(&self) -> &T {
        note_group(&self.reads);
        &self.value
    }
}

impl Reads {
    pub fn is_empty(&self) -> bool {
        self.deps.is_empty() && self.groups.is_empty()
    }

    /// Whether `f` is true for any dep, looking into each group once.
    pub fn any(&self, f: &mut impl FnMut(&Dep) -> bool) -> bool {
        self.any_in(f, &mut HashSet::new())
    }

    fn any_in(&self, f: &mut impl FnMut(&Dep) -> bool, seen: &mut HashSet<*const Reads>) -> bool {
        self.deps.iter().any(&mut *f) || self.groups.iter().any(|g| seen.insert(Arc::as_ptr(g)) && g.any_in(f, seen))
    }
}

#[derive(Default)]
struct Frame {
    deps: HashSet<Dep>,
    groups: Vec<Arc<Reads>>,
    /// `groups` by address: shared answers are used many times per frame.
    group_set: crate::fx_hash::FxHashSet<*const Reads>,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f`, returning what it read. An enclosing `track` does not see
/// these reads, unless they are passed on with [`note_group`].
pub fn track<R>(f: impl FnOnce() -> R) -> (R, Reads) {
    /// Pops the frame if `f` panics.
    struct Unwinding(bool);
    impl Drop for Unwinding {
        fn drop(&mut self) {
            if self.0 {
                FRAMES.with(|f| f.borrow_mut().pop());
            }
        }
    }
    FRAMES.with(|f| f.borrow_mut().push(Frame::default()));
    let mut unwinding = Unwinding(true);
    let r = f();
    unwinding.0 = false;
    let frame = FRAMES.with(|f| f.borrow_mut().pop()).unwrap_or_default();
    let mut deps: Vec<Dep> = frame.deps.into_iter().collect();
    deps.sort_unstable();
    (r, Reads { deps, groups: frame.groups })
}

/// Records a read for the innermost `track`, if any.
pub fn note(dep: Dep) {
    FRAMES.with(|f| {
        if let Some(frame) = f.borrow_mut().last_mut() {
            frame.deps.insert(dep);
        }
    });
}

/// Records the reads a shared answer was computed from, when work uses it.
pub fn note_group(group: &Arc<Reads>) {
    if group.is_empty() {
        return;
    }
    FRAMES.with(|f| {
        if let Some(frame) = f.borrow_mut().last_mut() {
            if frame.group_set.insert(Arc::as_ptr(group)) {
                frame.groups.push(group.clone());
            }
        }
    });
}

// ---- editor buffers ----------------------------------------------------------------

#[derive(Default)]
struct Buffers {
    texts: HashMap<PathBuf, Arc<str>>,
    /// Child reference counts include descendants: inserting `a/b.ts`
    /// supplies `a` in its parent without scanning all open files.
    directories: HashMap<PathBuf, HashMap<OsString, usize>>,
}

impl Buffers {
    fn index(&mut self, path: &Path, add: bool) {
        let mut child = path;
        while let (Some(parent), Some(name)) = (child.parent(), child.file_name()) {
            if add {
                *self.directories.entry(parent.to_path_buf()).or_default().entry(name.to_os_string()).or_default() += 1;
            } else if let Some(children) = self.directories.get_mut(parent) {
                if let Some(count) = children.get_mut(name) {
                    *count -= 1;
                    if *count == 0 {
                        children.remove(name);
                    }
                }
                if children.is_empty() {
                    self.directories.remove(parent);
                }
            }
            child = parent;
        }
    }

    fn files_under(&self, dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut dirs = vec![dir.to_path_buf()];
        while let Some(dir) = dirs.pop() {
            if let Some(children) = self.directories.get(&dir) {
                for name in children.keys() {
                    let path = dir.join(name);
                    if self.texts.contains_key(&path) {
                        files.push(path.clone());
                    }
                    if self.directories.contains_key(&path) {
                        dirs.push(path);
                    }
                }
            }
        }
        files.sort_unstable();
        files
    }
}

fn buffers() -> &'static RwLock<Buffers> {
    static BUFFERS: OnceLock<RwLock<Buffers>> = OnceLock::new();
    BUFFERS.get_or_init(Default::default)
}

/// From now on, `path` reads as `text` (an unsaved editor buffer).
pub fn set_buffer(path: &Path, text: &str) {
    let mut buffers = buffers().write().expect("buffers lock");
    if buffers.texts.insert(path.to_path_buf(), text.into()).is_none() {
        buffers.index(path, true);
    }
}

/// `path` reads from disk again.
pub fn drop_buffer(path: &Path) {
    let mut buffers = buffers().write().expect("buffers lock");
    if buffers.texts.remove(path).is_some() {
        buffers.index(path, false);
    }
}

pub(crate) fn buffer(path: &Path) -> Option<Arc<str>> {
    buffers().read().ok()?.texts.get(path).cloned()
}

// ---- reading -----------------------------------------------------------------------

fn text(path: &Path) -> Option<Arc<str>> {
    buffer(path).or_else(|| std::fs::read_to_string(path).ok().map(Into::into))
}

/// A file's text and its hash ([`MISSING`] when unreadable).
fn content(path: &Path) -> (Option<Arc<str>>, u64) {
    let text = text(path);
    let hash = text.as_deref().map_or(MISSING, |t| hash_bytes(t.as_bytes()));
    (text, hash)
}

fn file_exists(path: &Path) -> bool {
    buffer(path).is_some() || path.is_file()
}

/// A file's text, recorded.
pub fn read(path: &Path) -> Option<Arc<str>> {
    let (text, hash) = content(path);
    note(Dep::Text(path.to_path_buf(), hash));
    text
}

/// A file's text and hash, unrecorded: for callers that record something
/// finer (a summary records its fingerprint).
pub fn read_unrecorded(path: &Path) -> (Option<Arc<str>>, u64) {
    content(path)
}

/// Whether `path` is a file, unrecorded: for callers that record the
/// [`Dep::IsFile`] themselves (a run's shared probe).
pub fn is_file_unrecorded(path: &Path) -> bool {
    file_exists(path)
}

/// One directory's entries by name, read once: plain files, and names
/// whose answer needs a real probe (links, entries of unknown type).
#[derive(Default)]
pub struct Listing {
    files: std::collections::HashSet<std::ffi::OsString>,
    unsure: std::collections::HashSet<std::ffi::OsString>,
    /// Lowercased names, for case-insensitive file systems.
    folded: std::collections::HashSet<String>,
}

/// `dir`'s listing, unrecorded (empty when there is no such directory);
/// `None` when it cannot be read.
pub fn list_unrecorded(dir: &Path) -> Option<Listing> {
    let mut listing = Listing::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(listing),
        Err(_) => return None,
    };
    for entry in entries {
        let entry = entry.ok()?;
        let name = entry.file_name();
        listing.folded.insert(name.to_string_lossy().to_lowercase());
        match entry.file_type() {
            Ok(kind) if kind.is_file() => { listing.files.insert(name); }
            Ok(kind) if kind.is_dir() => {}
            _ => { listing.unsure.insert(name); }
        }
    }
    Some(listing)
}

/// Whether `path` is a file, unrecorded, answered from its directory's
/// `listing` where that is certain (an exact plain-file name, or no entry
/// of that name even ignoring case); a real probe otherwise.
pub fn is_file_listed(path: &Path, listing: &Listing) -> bool {
    if buffer(path).is_some() { return true }
    let Some(name) = path.file_name() else { return file_exists(path) };
    if listing.files.contains(name) { return true }
    if !listing.unsure.contains(name) && !listing.folded.contains(&name.to_string_lossy().to_lowercase()) { return false }
    file_exists(path)
}

pub fn is_file(path: &Path) -> bool {
    let found = file_exists(path);
    note(Dep::IsFile(path.to_path_buf(), found));
    found
}

pub fn is_dir(path: &Path) -> bool {
    let found = directory_exists(path);
    note(Dep::IsDir(path.to_path_buf(), found));
    found
}

fn directory_exists(path: &Path) -> bool {
    buffers().read().expect("buffers lock").directories.contains_key(path) || path.is_dir()
}

fn listing(dir: &Path) -> (Vec<String>, u64) {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    if let Some(children) = buffers().read().expect("buffers lock").directories.get(dir) {
        names.extend(children.keys().map(|name| name.to_string_lossy().into_owned()));
    }
    names.sort_unstable();
    names.dedup();
    let hash = if names.is_empty() && !directory_exists(dir) { MISSING } else { hash_bytes(names.join("\n").as_bytes()) };
    (names, hash)
}

/// A directory's entry names, sorted.
pub fn list(dir: &Path) -> Vec<String> {
    let (names, hash) = listing(dir);
    note(Dep::List(dir.to_path_buf(), hash));
    names
}

// ---- discovery ---------------------------------------------------------------------

/// A front end's search for the files below a directory that matter to
/// every file there (TypeScript's global declaration files). Registered by
/// name ([`super::frontend::Frontend::discoveries`]), so a recorded
/// [`Dep::Discovered`] can be checked again later.
pub struct Discovery {
    pub name: &'static str,
    /// A cheap test on the path, before the file is read.
    pub wanted: fn(&Path) -> bool,
    pub matches: fn(&Path, &str) -> bool,
}

fn discovery(name: &str) -> Option<&'static Discovery> {
    std::iter::once(&PROGRAM_FILES).chain(super::frontend::all().iter().flat_map(|front| front.discoveries().iter().copied())).find(|d| d.name == name)
}

static PROGRAM_FILES: Discovery = Discovery { name: "engine:program-inputs", wanted: |_| true, matches: |_, _| true };

pub(crate) fn requested(request: &DiscoveryRequest) -> Option<Vec<PathBuf>> {
    let found = request_walk(request);
    let hash = found.as_ref().map_or(MISSING, |(_, hash)| *hash);
    note(Dep::Requested(PROGRAM_FILES.name, request.clone(), hash));
    found.map(|(paths, _)| paths)
}

fn request_walk(request: &DiscoveryRequest) -> Option<(Vec<PathBuf>, u64)> {
    let include = crate::path_globs::PathGlobs::new(request.include.iter().map(|glob| &**glob)).ok()?;
    let exclude = crate::path_globs::PathGlobs::new(request.exclude.iter().flat_map(|glob| [Some(&**glob), glob.strip_suffix("/**")].into_iter().flatten())).ok()?;
    let wanted = |path: &Path| {
        let Some(rel) = path.strip_prefix(&request.root).ok() else { return false };
        let extension = request.extensions.is_empty() || request.extensions.iter().any(|ext| path.to_string_lossy().ends_with(&**ext));
        extension && (include.is_empty() || include.matches(rel)) && !exclude.matches(rel)
    };
    let root = request.root.clone();
    let dirs_excluded = exclude.clone();
    let mut walker = ignore::WalkBuilder::new(&request.root);
    let mut overrides = ignore::overrides::OverrideBuilder::new(&request.root);
    for glob in &request.include {
        let mut prefix = String::new();
        for part in glob.split('/').take_while(|part| !part.contains(['*', '?', '[', '{'])) {
            if !prefix.is_empty() { prefix.push('/'); }
            prefix.push_str(part);
            overrides.add(&prefix).ok()?;
        }
        if !prefix.is_empty() {
            overrides.add(&format!("{prefix}/**")).ok()?;
        }
    }
    walker.overrides(overrides.build().ok()?);
    walker.filter_entry(move |entry| entry.path().strip_prefix(&root).is_ok_and(|rel| !dirs_excluded.matches(rel)));
    let mut found = Vec::new();
    match std::fs::metadata(&request.root) {
        Ok(metadata) if metadata.is_dir() => {
            for entry in walker.build() {
                let entry = entry.ok()?;
                if entry.file_type().is_some_and(|kind| kind.is_file()) && wanted(entry.path()) { found.push(entry.into_path()); }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return None,
    }
    let buffers = buffers().read().ok()?;
    found.extend(buffers.files_under(&request.root).into_iter().filter(|path| wanted(path)));
    found.sort_unstable();
    found.dedup();
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    found.hash(&mut hash);
    Some((found, hash.finish().max(1)))
}

/// A registered discovery's name with its `'static` lifetime (decoding a
/// cached [`Dep::Discovered`]).
pub(crate) fn discovery_name(name: &str) -> Option<&'static str> {
    discovery(name).map(|d| d.name)
}

/// The latest [`discover`] answer per discovery and directory, to tell
/// whether an edit changed it without walking again ([`discovery_changed`]).
type Walks = HashMap<(&'static str, PathBuf), Arc<HashSet<PathBuf>>>;

fn walks() -> &'static RwLock<Walks> {
    static WALKS: OnceLock<RwLock<Walks>> = OnceLock::new();
    WALKS.get_or_init(Default::default)
}

fn walk(d: &Discovery, dir: &Path, jobs: usize) -> (Vec<PathBuf>, u64) {
    let found_at = |path: &Path| (d.wanted)(path) && text(path).is_some_and(|text| (d.matches)(path, &text));
    let mut found = if jobs <= 1 {
        ignore::WalkBuilder::new(dir).build().flatten()
            .filter(|entry| found_at(entry.path())).map(|entry| entry.into_path()).collect::<Vec<_>>()
    } else {
        let found = std::sync::Mutex::new(Vec::new());
        ignore::WalkBuilder::new(dir).threads(jobs).build_parallel().run(|| {
            Box::new(|entry| {
                if let Ok(entry) = entry {
                    if found_at(entry.path()) {
                        found.lock().expect("discovered files").push(entry.into_path());
                    }
                }
                ignore::WalkState::Continue
            })
        });
        found.into_inner().expect("discovered files")
    };
    let buffers = buffers().read().expect("buffers lock");
    let mut seen: HashSet<PathBuf> = found.iter().cloned().collect();
    for path in buffers.files_under(dir) {
        if (d.wanted)(&path) && buffers.texts.get(&path).is_some_and(|text| (d.matches)(&path, text)) && seen.insert(path.clone()) {
            found.push(path);
        }
    }
    found.sort_unstable();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    found.hash(&mut h);
    let hash = h.finish();
    (found, hash)
}

/// The files below `dir` (gitignore respected) that `d` finds, in lexical
/// path order, including unsaved editor buffers; recorded.
pub fn discover(d: &'static Discovery, dir: &Path, jobs: usize) -> Vec<PathBuf> {
    let (found, hash) = walk(d, dir, jobs);
    walks().write().expect("walks lock").insert((d.name, dir.to_path_buf()), Arc::new(found.iter().cloned().collect()));
    note(Dep::Discovered(d.name, dir.to_path_buf(), hash));
    found
}

/// The walked directories whose [`discover`] answer for `d` the `changed`
/// files change: a file that starts or stops matching (or is created or
/// deleted as one).
pub fn discovery_changed(d: &Discovery, changed: &[PathBuf]) -> Vec<PathBuf> {
    let walks = walks().read().map(|w| w.clone()).unwrap_or_default();
    let matching: Vec<(&PathBuf, bool)> = changed.iter().map(|p| (p, (d.wanted)(p) && text(p).is_some_and(|t| (d.matches)(p, &t)))).collect();
    walks
        .into_iter()
        .filter(|((name, dir), known)| *name == d.name && matching.iter().any(|(p, now)| p.starts_with(dir) && *now != known.contains(*p)))
        .map(|((_, dir), _)| dir)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_discovery_replays_its_filters_and_tracks_membership() {
        let root = PathBuf::from("/root/engine-requested-inputs");
        let included = root.join("src/first.ts");
        let excluded = root.join("src/generated/skip.ts");
        let other = root.join("src/skip.json");
        let outside = root.join("outside/skip.ts");
        for path in [&included, &excluded, &other, &outside] { set_buffer(path, ""); }
        let request = DiscoveryRequest { root, include: vec!["src/**".into()], exclude: vec!["src/generated/**".into()], extensions: vec![".ts".into()] };
        let (paths, reads) = track(|| requested(&request).unwrap());
        assert_eq!(paths, vec![included.clone()]);
        assert!(reads.deps.iter().all(|dep| dep.holds(|_| 0)));
        set_buffer(&included, "changed text, same membership");
        assert!(reads.deps.iter().all(|dep| dep.holds(|_| 0)));
        let added = request.root.join("src/second.ts");
        set_buffer(&added, "");
        assert!(reads.deps.iter().any(|dep| !dep.holds(|_| 0)));
        for path in [&included, &excluded, &other, &outside, &added] { drop_buffer(path); }
    }

    #[test]
    fn exact_requested_files_override_ignores_without_widening_membership() {
        let dir = std::env::temp_dir().join(format!("zlint-requested-files-{}", std::process::id()));
        let library = dir.join("node_modules/typescript/lib");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(dir.join(".ignore"), "node_modules/\n").unwrap();
        std::fs::write(library.join(".ignore"), "*.d.ts\n").unwrap();
        let included = library.join("lib.es2023.d.ts");
        let other = library.join("lib.dom.d.ts");
        std::fs::write(&included, "").unwrap();
        std::fs::write(&other, "").unwrap();
        let requests = [
            DiscoveryRequest { root: dir.clone(), include: vec!["node_modules/typescript/lib/lib.es2023.d.ts".into()], exclude: vec![], extensions: vec![] },
            DiscoveryRequest { root: library.clone(), include: vec!["lib.es2023.d.ts".into()], exclude: vec![], extensions: vec![] },
        ];
        let (_, reads) = track(|| {
            for request in &requests { assert_eq!(requested(request), Some(vec![included.clone()])); }
            let subtree = DiscoveryRequest {
                root: dir.clone(), include: vec!["node_modules/typescript/lib/**".into()],
                exclude: vec!["node_modules/typescript/lib/lib.dom.d.ts".into()], extensions: vec![".d.ts".into()],
            };
            assert_eq!(requested(&subtree), Some(vec![included.clone()]));
        });
        assert!(reads.deps.iter().all(|dep| dep.holds(|_| 0)));
        std::fs::write(&included, "changed text").unwrap();
        assert!(reads.deps.iter().all(|dep| dep.holds(|_| 0)));
        std::fs::remove_file(&included).unwrap();
        assert!(reads.deps.iter().all(|dep| !dep.holds(|_| 0)));
        std::fs::remove_file(&other).unwrap();
        std::fs::remove_file(library.join(".ignore")).unwrap();
        std::fs::remove_file(dir.join(".ignore")).unwrap();
        std::fs::remove_dir(&library).unwrap();
        std::fs::remove_dir(library.parent().unwrap()).unwrap();
        std::fs::remove_dir(dir.join("node_modules")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn buffered_directories_merge_with_disk_and_invalidate_reads() {
        let dir = std::env::temp_dir().join(format!("zlint-buffer-directories-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("disk.txt"), "").unwrap();
        let file = dir.join("virtual/nested/globals.d.ts");
        let other = dir.join("virtual/other.ts");
        set_buffer(&file, "declare const PackedGlobal: number;");
        set_buffer(&other, "export {};");
        set_buffer(&other, "export const replacement = 1;");
        set_buffer(&dir.join("disk.txt"), "buffer wins");
        let (_, reads) = track(|| {
            assert!(is_dir(&dir.join("virtual")));
            assert!(is_dir(&dir.join("virtual/nested")));
            assert_eq!(list(&dir), ["disk.txt", "virtual"]);
            assert_eq!(list(&dir.join("virtual")), ["nested", "other.ts"]);
        });
        assert!(reads.deps.iter().all(|dep| dep.holds(|_| 0)));
        assert!(reads.deps.iter().filter(|dep| matches!(dep, Dep::IsDir(..) | Dep::List(..))).all(|dep| dep.touches(&file)));
        drop_buffer(&file);
        assert!(!is_dir(&dir.join("virtual/nested")));
        assert!(reads.deps.iter().any(|dep| !dep.holds(|_| 0)));
        assert_eq!(list(&dir.join("virtual")), ["other.ts"]);
        drop_buffer(&other);
        assert!(!is_dir(&dir.join("virtual")));
        assert_eq!(list(&dir), ["disk.txt"]);
        drop_buffer(&dir.join("disk.txt"));
        std::fs::remove_file(dir.join("disk.txt")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn shared_reads_reach_their_users_as_groups() {
        let dir = std::env::temp_dir().join(format!("zlint-reads-{}", std::process::id()));
        let file = dir.join("a.ts");
        let (_, shared) = track(|| is_file(&file));
        let shared = Arc::new(shared);
        let ((), reads) = track(|| {
            note_group(&shared);
            note_group(&shared);
            is_dir(&dir);
        });
        assert_eq!(reads.deps, vec![Dep::IsDir(dir, false)]);
        assert_eq!(reads.groups.len(), 1);
        assert!(reads.any(&mut |d| *d == Dep::IsFile(file.clone(), false)));
        // Outside any scope nothing is recorded (and nothing breaks).
        note(Dep::IsDir(PathBuf::from("x"), true));
    }

    #[test]
    fn buffers_read_instead_of_the_disk() {
        let path = std::env::temp_dir().join(format!("zlint-buffer-{}.ts", std::process::id()));
        assert!(!Dep::IsFile(path.clone(), true).holds(|_| 0));
        set_buffer(&path, "declare global {}");
        let (text, reads) = track(|| read(&path));
        assert_eq!(text.as_deref(), Some("declare global {}"));
        assert!(reads.deps[0].holds(|_| 0) && Dep::IsFile(path.clone(), true).holds(|_| 0));
        drop_buffer(&path);
        assert!(!reads.deps[0].holds(|_| 0));
    }
}

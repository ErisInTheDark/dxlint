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
    /// The files below a directory that declare globals ([`global_files`]).
    Globals(PathBuf, u64),
}

impl Dep {
    pub fn path(&self) -> &Path {
        match self {
            Dep::Text(p, _) | Dep::IsFile(p, _) | Dep::IsDir(p, _) | Dep::List(p, _) | Dep::Summary(p, ..) | Dep::Globals(p, _) => p,
        }
    }

    /// Could a change to the file `changed` (edited, created or deleted)
    /// change this dep's answer?
    pub fn touches(&self, changed: &Path) -> bool {
        match self {
            Dep::List(dir, _) | Dep::IsDir(dir, _) => changed.starts_with(dir),
            Dep::Globals(dir, _) => changed.starts_with(dir),
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
            Dep::Globals(p, h) => walk_globals(p).1 == *h,
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
        note_group(&self.reads);
        self.value.clone()
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
            if !frame.groups.iter().any(|g| Arc::ptr_eq(g, group)) {
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

/// A file's text and its hash ([`MISSING`] when unreadable).
fn content(path: &Path) -> (Option<Arc<str>>, u64) {
    let text = buffer(path).or_else(|| std::fs::read_to_string(path).ok().map(Into::into));
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

// ---- global declarations -------------------------------------------------------------

/// The latest [`global_files`] answer per directory, to tell whether an
/// edit changed it without walking again (see [`globals_changed`]).
fn walks() -> &'static RwLock<HashMap<PathBuf, Arc<HashSet<PathBuf>>>> {
    static WALKS: OnceLock<RwLock<HashMap<PathBuf, Arc<HashSet<PathBuf>>>>> = OnceLock::new();
    WALKS.get_or_init(Default::default)
}

/// Does a TypeScript file add to the global scope (a declaration script,
/// or a `declare global`)?
fn declares_globals(path: &Path, text: &str) -> bool {
    let name = path.to_string_lossy();
    let script = name.ends_with(".d.ts") && !text.contains("import ") && !text.contains("export ");
    script || text.contains("declare global")
}

fn walk_globals(dir: &Path) -> (Vec<PathBuf>, u64) {
    let mut found = Vec::new();
    for entry in ignore::WalkBuilder::new(dir).build().flatten() {
        let path = entry.path();
        let name = path.to_string_lossy();
        if !(name.ends_with(".ts") || name.ends_with(".tsx")) {
            continue;
        }
        let Some(text) = content(path).0 else { continue };
        if declares_globals(path, &text) {
            found.push(path.to_path_buf());
        }
    }
    let buffers = buffers().read().expect("buffers lock");
    let mut seen: HashSet<PathBuf> = found.iter().cloned().collect();
    for path in buffers.files_under(dir) {
        let name = path.to_string_lossy();
        if (name.ends_with(".ts") || name.ends_with(".tsx"))
            && buffers.texts.get(&path).is_some_and(|text| declares_globals(&path, text))
            && seen.insert(path.clone())
        {
            found.push(path);
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    found.hash(&mut h);
    let hash = h.finish();
    (found, hash)
}

/// The TypeScript files below `dir` (gitignore respected) that add to the
/// global scope, in disk walk order followed by sorted buffer-only paths.
pub fn global_files(dir: &Path) -> Vec<PathBuf> {
    let (found, hash) = walk_globals(dir);
    walks().write().expect("walks lock").insert(dir.to_path_buf(), Arc::new(found.iter().cloned().collect()));
    note(Dep::Globals(dir.to_path_buf(), hash));
    found
}

/// The walked directories whose [`global_files`] the `changed` files change:
/// a file that starts or stops declaring globals (or is created or deleted
/// as one).
pub fn globals_changed(changed: &[PathBuf]) -> Vec<PathBuf> {
    let walks = walks().read().map(|w| w.clone()).unwrap_or_default();
    let declaring: Vec<(&PathBuf, bool)> = changed
        .iter()
        .map(|p| {
            let name = p.to_string_lossy();
            (p, (name.ends_with(".ts") || name.ends_with(".tsx")) && content(p).0.is_some_and(|t| declares_globals(p, &t)))
        })
        .collect();
    walks
        .into_iter()
        .filter(|(dir, known)| declaring.iter().any(|(p, now)| p.starts_with(dir) && *now != known.contains(*p)))
        .map(|(dir, _)| dir)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(global_files(&dir).as_slice(), std::slice::from_ref(&file));
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

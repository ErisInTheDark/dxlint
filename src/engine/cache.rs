//! `.dxcache`: lint results kept between runs, beside the config. A
//! file's result is reused when its text and everything its lint read
//! (other files' texts, module resolution probes, tsconfigs: see
//! `engine::reads`) still match. Results are kept for clean files and
//! for files with problems alike, so a warm run only lints what changed.
//!
//! The whole cache belongs to one rule state: the config's tokens (imports
//! and macros expanded), the project root and the dxlint executable. Any
//! change there starts afresh.
//!
//! Reads shared by many files (a tsconfig's globals, resolved member names)
//! are stored once as groups, and every dep and group is checked at most
//! once per run.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::{Diagnostic, Edit, Fix, Linter, Suggestion};
use crate::config::ast::Severity;
use super::reads::{hash_bytes, Dep, Reads, Scan, ScanTest};

pub const FILE_NAME: &str = ".dxcache";
/// Bumped whenever the encoding changes.
const MAGIC: &[u8; 4] = b"ZLC8";

/// Deps, groups and scans, by index into the tables.
#[derive(Default, Clone, PartialEq, Eq, Hash)]
struct Refs {
    deps: Vec<u32>,
    groups: Vec<u32>,
    scans: Vec<u32>,
}

/// A saved [`super::reads::Scan`]: its files are a group of text reads.
#[derive(Clone, PartialEq, Eq, Hash)]
struct SavedScan {
    files: u32,
    test: ScanTest,
}

struct Entry {
    content: u64,
    refs: Refs,
    diags: Vec<Diagnostic>,
}

/// A result this run produced or confirmed.
enum Fresh {
    /// The saved entry still holds.
    Kept,
    New { content: u64, reads: Arc<Reads>, diags: Vec<Diagnostic> },
}

/// What a saved cache holds.
#[derive(Default)]
struct Saved {
    deps: Vec<Dep>,
    /// A group's refs only point at earlier groups.
    groups: Vec<Refs>,
    scans: Vec<SavedScan>,
    entries: HashMap<String, Entry>,
}

pub struct Cache {
    path: PathBuf,
    root: PathBuf,
    key: u64,
    saved: Saved,
    /// Whether each saved dep and group still holds, checked once per run.
    dep_holds: Vec<OnceLock<bool>>,
    group_holds: Vec<OnceLock<bool>>,
    scan_holds: Vec<OnceLock<bool>>,
    /// Saved groups and scans as reads, for results taken from the cache.
    group_reads: Vec<OnceLock<Arc<Reads>>>,
    scan_reads: Vec<OnceLock<Arc<Scan>>>,
    /// Directory listings answering saved file probes ([`Cache::listed_file`]).
    listings: std::sync::RwLock<crate::fx_hash::FxHashMap<PathBuf, Option<Arc<super::reads::Listing>>>>,
    /// Content hashes of the files this run looked up: other files' saved
    /// text reads of them check against these instead of reading again.
    texts: std::sync::RwLock<crate::fx_hash::FxHashMap<PathBuf, u64>>,
    /// Texts of changed files that scans test ([`Cache::current_text`]).
    current: std::sync::RwLock<crate::fx_hash::FxHashMap<PathBuf, Option<Arc<str>>>>,
    fresh: Mutex<HashMap<String, Fresh>>,
    /// Something new to write.
    changed: AtomicBool,
    hits: AtomicUsize,
}

impl Cache {
    /// The cache for `linter`'s rules: what was saved, if it was saved for
    /// exactly these rules and this dxlint, else an empty one.
    pub fn open(linter: &Linter) -> Cache {
        let path = linter.root.join(FILE_NAME);
        let key = key(linter.fingerprint);
        let saved = std::fs::read(&path).ok().and_then(|bytes| decode(&bytes, key, &linter.root)).unwrap_or_default();
        Cache {
            path,
            root: linter.root.clone(),
            key,
            dep_holds: saved.deps.iter().map(|_| OnceLock::new()).collect(),
            group_holds: saved.groups.iter().map(|_| OnceLock::new()).collect(),
            group_reads: saved.groups.iter().map(|_| OnceLock::new()).collect(),
            scan_holds: saved.scans.iter().map(|_| OnceLock::new()).collect(),
            scan_reads: saved.scans.iter().map(|_| OnceLock::new()).collect(),
            listings: Default::default(),
            texts: Default::default(),
            current: Default::default(),
            saved,
            fresh: Mutex::default(),
            changed: AtomicBool::new(false),
            hits: AtomicUsize::new(0),
        }
    }

    /// Files whose results came from the cache this run.
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    /// The saved result for `rel` (a path relative to the config, `/`
    /// separated), if `text` and everything its lint read are unchanged.
    pub fn lookup(&self, linter: &Linter, rel: &str, text: &str) -> Option<Vec<Diagnostic>> {
        let entry = self.saved.entries.get(rel)?;
        let content = hash_bytes(text.as_bytes());
        self.texts.write().expect("cache texts").insert(self.root.join(rel), content);
        if entry.content != content || !self.holds(linter, &entry.refs) {
            return None;
        }
        self.hits.fetch_add(1, Ordering::Relaxed);
        self.fresh.lock().expect("cache lock").insert(rel.to_string(), Fresh::Kept);
        Some(entry.diags.clone())
    }

    /// What the saved lint of `rel` read (for callers that track reads on a
    /// hit, like the language server; a lint run doesn't need them).
    pub fn saved_reads(&self, rel: &str) -> Option<Reads> {
        Some(self.reads(&self.saved.entries.get(rel)?.refs))
    }

    fn holds(&self, linter: &Linter, refs: &Refs) -> bool {
        refs.deps.iter().all(|&i| self.dep_holds(linter, i))
            && refs.scans.iter().all(|&i| *self.scan_holds[i as usize].get_or_init(|| {
                let scan = &self.saved.scans[i as usize];
                // Only files read otherwise than saved are tested again.
                self.saved.groups[scan.files as usize].deps.iter().all(|&dep| {
                    self.dep_holds(linter, dep) || {
                        let path = self.saved.deps[dep as usize].path();
                        linter.passes(path, self.current_text(path).as_deref(), &scan.test)
                    }
                })
            }))
            && refs.groups.iter().all(|&i| *self.group_holds[i as usize].get_or_init(|| self.holds(linter, &self.saved.groups[i as usize])))
    }

    /// A changed file's text, read once however many scans test it.
    fn current_text(&self, path: &Path) -> Option<Arc<str>> {
        if let Some(text) = self.current.read().expect("cache current texts").get(path) { return text.clone() }
        let text = super::reads::read_unrecorded(path).0;
        self.current.write().expect("cache current texts").insert(path.to_owned(), text.clone());
        text
    }

    fn dep_holds(&self, linter: &Linter, i: u32) -> bool {
        *self.dep_holds[i as usize].get_or_init(|| match &self.saved.deps[i as usize] {
            Dep::IsFile(path, found) => self.listed_file(path) == *found,
            // A text read that found nothing still finds nothing while no
            // file is there: its directory's listing answers that.
            Dep::Text(path, super::reads::MISSING) => !self.listed_file(path),
            dep @ Dep::Text(path, hash) => {
                let seen = self.texts.read().expect("cache texts").get(&**path).copied();
                seen.map_or_else(|| linter.holds(dep), |seen| seen == *hash)
            }
            dep => linter.holds(dep),
        })
    }

    /// Whether `path` is a file, from its directory's listing (read once per
    /// run): saved probes are mostly module resolution candidates, many per
    /// directory, and one listing answers them all.
    fn listed_file(&self, path: &Path) -> bool {
        let Some(dir) = path.parent() else { return super::reads::is_file_unrecorded(path) };
        if let Some(listing) = self.listings.read().expect("cache listings").get(dir) {
            return listing.as_ref().map_or_else(|| super::reads::is_file_unrecorded(path), |listing| super::reads::is_file_listed(path, listing));
        }
        let listing = super::reads::list_unrecorded(dir).map(Arc::new);
        let held = listing.as_ref().map_or_else(|| super::reads::is_file_unrecorded(path), |listing| super::reads::is_file_listed(path, listing));
        self.listings.write().expect("cache listings").insert(dir.to_path_buf(), listing);
        held
    }

    /// Saved refs as reads, each group and scan built once.
    fn reads(&self, refs: &Refs) -> Reads {
        Reads {
            deps: refs.deps.iter().map(|&i| self.saved.deps[i as usize].clone()).collect(),
            groups: refs.groups.iter().map(|&i| self.group_reads(i)).collect(),
            scans: refs.scans.iter().map(|&i| self.scan_reads[i as usize].get_or_init(|| {
                let scan = &self.saved.scans[i as usize];
                Arc::new(Scan { files: self.group_reads(scan.files), test: scan.test.clone() })
            }).clone()).collect(),
        }
    }

    fn group_reads(&self, i: u32) -> Arc<Reads> {
        self.group_reads[i as usize].get_or_init(|| Arc::new(self.reads(&self.saved.groups[i as usize]))).clone()
    }

    /// Remembers this run's result for `rel`.
    pub fn store(&self, rel: &str, text: &str, diags: &[Diagnostic], reads: Arc<Reads>) {
        let fresh = Fresh::New { content: hash_bytes(text.as_bytes()), reads, diags: diags.to_vec() };
        self.fresh.lock().expect("cache lock").insert(rel.to_string(), fresh);
        self.changed.store(true, Ordering::Relaxed);
    }

    /// Writes the cache: this run's results, plus (unless `prune`, for a
    /// run that saw every file) older results for files not seen this run.
    /// Nothing is written when nothing changed.
    pub fn save(self, prune: bool) -> io::Result<()> {
        let fresh = self.fresh.into_inner().unwrap_or_default();
        let pruned = prune && self.saved.entries.keys().any(|k| !fresh.contains_key(k));
        if !self.changed.load(Ordering::Relaxed) && !pruned {
            return Ok(());
        }
        let mut tables = Tables::new(&self.saved);
        let mut entries: Vec<(&str, u64, Refs, &[Diagnostic])> = Vec::new();
        for (rel, f) in &fresh {
            entries.push(match f {
                Fresh::New { content, reads, diags } => (rel, *content, tables.reads(reads), diags),
                Fresh::Kept => {
                    let e = &self.saved.entries[rel];
                    (rel, e.content, tables.saved_refs(&e.refs), &e.diags)
                }
            });
        }
        if !prune {
            for (rel, e) in self.saved.entries.iter().filter(|(k, _)| !fresh.contains_key(*k)) {
                entries.push((rel, e.content, tables.saved_refs(&e.refs), &e.diags));
            }
        }
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));

        let mut w = Writer { out: Vec::new(), root: &self.root, strings: Vec::new(), string_ids: HashMap::new() };
        w.var(tables.deps.len() as u64);
        for d in &tables.deps {
            w.dep(d);
        }
        // Groups name scans and scans name groups: both counts come first.
        w.var(tables.scans.len() as u64);
        w.var(tables.groups.len() as u64);
        for g in &tables.groups {
            w.refs(g);
        }
        for s in &tables.scans {
            w.scan(s);
        }
        w.var(entries.len() as u64);
        for (rel, content, refs, diags) in entries {
            w.str(rel);
            w.out.extend_from_slice(&content.to_le_bytes());
            w.refs(&refs);
            w.var(diags.len() as u64);
            for d in diags {
                w.diag(d);
            }
        }
        // Written aside and renamed over, so a reader never sees half a file.
        let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, w.finish(self.key))?;
        std::fs::rename(&tmp, &self.path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}

/// The rule state the cache belongs to.
fn key(fingerprint: u64) -> u64 {
    static EXE: OnceLock<u64> = OnceLock::new();
    // A dxlint that cannot read itself never reuses anything.
    let exe = *EXE.get_or_init(|| {
        std::env::current_exe().and_then(std::fs::read).map_or_else(|_| std::process::id().into(), |bytes| hash_bytes(&bytes))
    });
    hash_bytes(&[MAGIC.as_slice(), &fingerprint.to_le_bytes(), &exe.to_le_bytes()].concat())
}

/// The tables being written: deps, groups and scans interned, groups and
/// scans by content.
struct Tables<'a> {
    saved: &'a Saved,
    deps: Vec<Dep>,
    dep_ids: HashMap<Dep, u32>,
    groups: Vec<Refs>,
    group_ids: HashMap<Refs, u32>,
    /// Groups already interned, by address (alive as long as the run's
    /// results) or by saved index.
    by_address: HashMap<usize, u32>,
    by_saved: HashMap<u32, u32>,
    scans: Vec<SavedScan>,
    scan_ids: HashMap<SavedScan, u32>,
    /// Scans already interned, as for groups.
    scan_by_address: HashMap<usize, u32>,
    scan_by_saved: HashMap<u32, u32>,
}

impl<'a> Tables<'a> {
    fn new(saved: &'a Saved) -> Self {
        Tables {
            saved,
            deps: Vec::new(),
            dep_ids: HashMap::new(),
            groups: Vec::new(),
            group_ids: HashMap::new(),
            by_address: HashMap::new(),
            by_saved: HashMap::new(),
            scans: Vec::new(),
            scan_ids: HashMap::new(),
            scan_by_address: HashMap::new(),
            scan_by_saved: HashMap::new(),
        }
    }

    fn scan(&mut self, scan: SavedScan) -> u32 {
        if let Some(&id) = self.scan_ids.get(&scan) {
            return id;
        }
        let id = self.scans.len() as u32;
        self.scans.push(scan.clone());
        self.scan_ids.insert(scan, id);
        id
    }

    fn reads_scan(&mut self, s: &Arc<Scan>) -> u32 {
        let address = Arc::as_ptr(s) as usize;
        if let Some(&id) = self.scan_by_address.get(&address) {
            return id;
        }
        let files = self.reads_group(&s.files);
        let id = self.scan(SavedScan { files, test: s.test.clone() });
        self.scan_by_address.insert(address, id);
        id
    }

    fn saved_scan(&mut self, i: u32) -> u32 {
        if let Some(&id) = self.scan_by_saved.get(&i) {
            return id;
        }
        let saved = &self.saved.scans[i as usize];
        let files = self.saved_group(saved.files);
        let id = self.scan(SavedScan { files, test: saved.test.clone() });
        self.scan_by_saved.insert(i, id);
        id
    }

    fn dep(&mut self, dep: &Dep) -> u32 {
        if let Some(&id) = self.dep_ids.get(dep) {
            return id;
        }
        let id = self.deps.len() as u32;
        self.deps.push(dep.clone());
        self.dep_ids.insert(dep.clone(), id);
        id
    }

    fn group(&mut self, refs: Refs) -> u32 {
        if let Some(&id) = self.group_ids.get(&refs) {
            return id;
        }
        let id = self.groups.len() as u32;
        self.groups.push(refs.clone());
        self.group_ids.insert(refs, id);
        id
    }

    fn reads(&mut self, reads: &Reads) -> Refs {
        let deps = reads.deps.iter().map(|d| self.dep(d)).collect();
        let groups = reads.groups.iter().map(|g| self.reads_group(g)).collect();
        let scans = reads.scans.iter().map(|s| self.reads_scan(s)).collect();
        sorted(Refs { deps, groups, scans })
    }

    fn reads_group(&mut self, g: &Arc<Reads>) -> u32 {
        let address = Arc::as_ptr(g) as usize;
        if let Some(&id) = self.by_address.get(&address) {
            return id;
        }
        let refs = self.reads(g);
        let id = self.group(refs);
        self.by_address.insert(address, id);
        id
    }

    fn saved_refs(&mut self, refs: &Refs) -> Refs {
        let saved = self.saved;
        let deps = refs.deps.iter().map(|&i| self.dep(&saved.deps[i as usize])).collect();
        let groups = refs.groups.iter().map(|&i| self.saved_group(i)).collect();
        let scans = refs.scans.iter().map(|&i| self.saved_scan(i)).collect();
        sorted(Refs { deps, groups, scans })
    }

    fn saved_group(&mut self, i: u32) -> u32 {
        if let Some(&id) = self.by_saved.get(&i) {
            return id;
        }
        let refs = self.saved_refs(&self.saved.groups[i as usize].clone());
        let id = self.group(refs);
        self.by_saved.insert(i, id);
        id
    }
}

fn sorted(mut refs: Refs) -> Refs {
    refs.deps.sort_unstable();
    refs.deps.dedup();
    refs.groups.sort_unstable();
    refs.groups.dedup();
    refs.scans.sort_unstable();
    refs.scans.dedup();
    refs
}

// ---- encoding ------------------------------------------------------------------

const SEVERITIES: [Severity; 5] = [Severity::Off, Severity::Hint, Severity::Info, Severity::Warn, Severity::Error];

/// Encodes the body, then [`Writer::finish`] puts the header and string
/// table in front: every string (rule, message, directory, file name) is
/// stored once.
struct Writer<'a> {
    out: Vec<u8>,
    root: &'a Path,
    strings: Vec<String>,
    string_ids: HashMap<String, u32>,
}

fn write_var(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

impl Writer<'_> {
    fn finish(self, key: u64) -> Vec<u8> {
        let mut file = Vec::with_capacity(self.out.len() + 64 * self.strings.len());
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&key.to_le_bytes());
        write_var(&mut file, self.strings.len() as u64);
        for s in &self.strings {
            write_var(&mut file, s.len() as u64);
            file.extend_from_slice(s.as_bytes());
        }
        file.extend_from_slice(&self.out);
        file
    }

    fn var(&mut self, v: u64) {
        write_var(&mut self.out, v);
    }

    fn str(&mut self, s: &str) {
        let id = match self.string_ids.get(s) {
            Some(&id) => id,
            None => {
                let id = self.strings.len() as u32;
                self.strings.push(s.to_string());
                self.string_ids.insert(s.to_string(), id);
                id
            }
        };
        self.var(u64::from(id));
    }

    /// A directory and a name; paths below the root are kept relative.
    fn path(&mut self, p: &Path) {
        let (flag, p) = match p.strip_prefix(self.root) {
            Ok(rel) => (0, rel),
            Err(_) => (1, p),
        };
        self.out.push(flag);
        match (p.parent(), p.file_name()) {
            (Some(dir), Some(name)) => {
                self.str(&dir.to_string_lossy());
                self.str(&name.to_string_lossy());
            }
            _ => {
                self.str(&p.to_string_lossy());
                self.str("");
            }
        }
    }

    /// Sorted ids, as gaps (mostly one byte each).
    fn ids(&mut self, ids: &[u32]) {
        self.var(ids.len() as u64);
        let mut last = 0;
        for &id in ids {
            self.var(u64::from(id - last));
            last = id;
        }
    }

    fn refs(&mut self, refs: &Refs) {
        self.ids(&refs.deps);
        self.ids(&refs.groups);
        self.ids(&refs.scans);
    }

    fn scan(&mut self, s: &SavedScan) {
        self.var(u64::from(s.files));
        let words = match &s.test {
            ScanTest::Ambient { hints, classes } => {
                self.out.push(0);
                self.var(*classes);
                hints
            }
            ScanTest::Global { name, hints } => {
                self.out.push(1);
                self.str(name);
                hints
            }
        };
        self.var(words.len() as u64);
        for word in words {
            self.str(word);
        }
    }

    fn dep(&mut self, d: &Dep) {
        let (tag, a) = match d {
            Dep::Text(_, h) => (0, *h),
            Dep::Outside(_, bodies) => (7, bodies.hash),
            Dep::IsFile(_, found) => (1, u64::from(*found)),
            Dep::IsDir(_, found) => (2, u64::from(*found)),
            Dep::List(_, h) => (3, *h),
            Dep::Discovered(_, _, h) => (5, *h),
            Dep::Requested(_, _, hash) => (6, *hash),
        };
        self.out.push(tag);
        self.path(d.path());
        self.out.extend_from_slice(&a.to_le_bytes());
        if let Dep::Discovered(name, ..) = d {
            self.str(name);
        }
        if let Dep::Requested(name, request, _) = d {
            self.str(name);
            for list in [&request.include, &request.exclude, &request.extensions] {
                self.var(list.len() as u64);
                for value in list { self.str(value); }
            }
        }
        if let Dep::Outside(_, bodies) = d {
            let (len, ranges) = bodies.parts();
            self.var(len as u64);
            self.var(ranges.len() as u64);
            for &(start, end, before, after) in ranges {
                self.var(u64::from(start));
                self.var(u64::from(end));
                self.out.extend_from_slice(&before.to_le_bytes());
                self.out.extend_from_slice(&after.to_le_bytes());
            }
        }
    }

    fn diag(&mut self, d: &Diagnostic) {
        self.str(&d.rule);
        self.out.push(SEVERITIES.iter().position(|s| *s == d.severity).unwrap_or(0) as u8);
        self.str(&d.message);
        self.var(d.message_index.map_or(0, |index| index as u64 + 1));
        self.var(d.start as u64);
        self.var(d.end as u64);
        match &d.fix {
            None => self.var(0),
            Some(fix) => {
                self.var(1);
                self.fix(fix);
            }
        }
        self.var(d.suggestions.len() as u64);
        for suggestion in &d.suggestions {
            self.str(&suggestion.title);
            self.fix(&suggestion.fix);
        }
    }

    fn fix(&mut self, fix: &Fix) {
        self.var(fix.range.start as u64);
        self.var(fix.range.end as u64);
        self.var(fix.edits.len() as u64);
        for e in &fix.edits {
            self.var(e.start as u64);
            self.var(e.end as u64);
            self.str(&e.text);
        }
        self.var(fix.placeholders.len() as u64);
        for range in &fix.placeholders {
            self.var(range.start as u64);
            self.var(range.end as u64);
        }
    }
}

fn decode(bytes: &[u8], key: u64, root: &Path) -> Option<Saved> {
    let mut r = Reader { bytes, at: 0, root, strings: Vec::new() };
    if r.take(4)? != MAGIC || r.u64()? != key {
        return None;
    }
    for _ in 0..r.var()? {
        let n = usize::try_from(r.var()?).ok()?;
        let s = String::from_utf8(r.take(n)?.to_vec()).ok()?;
        r.strings.push(s);
    }
    let deps = (0..r.var()?).map(|_| r.dep()).collect::<Option<Vec<_>>>()?;
    let scan_count = usize::try_from(r.var()?).ok()?;
    let mut groups = Vec::new();
    for _ in 0..r.var()? {
        let refs = r.refs(deps.len(), groups.len(), scan_count)?;
        groups.push(refs);
    }
    let scans = (0..scan_count).map(|_| r.scan(&groups)).collect::<Option<Vec<_>>>()?;
    let mut entries = HashMap::new();
    for _ in 0..r.var()? {
        let rel = r.str()?;
        let content = r.u64()?;
        let refs = r.refs(deps.len(), groups.len(), scans.len())?;
        let diags = (0..r.var()?).map(|_| r.diag()).collect::<Option<Vec<_>>>()?;
        entries.insert(rel, Entry { content, refs, diags });
    }
    Some(Saved { deps, groups, scans, entries })
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    root: &'a Path,
    strings: Vec<String>,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(out)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn var(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            v |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    fn str(&mut self) -> Option<String> {
        let id = usize::try_from(self.var()?).ok()?;
        self.strings.get(id).cloned()
    }

    /// Sorted ids below `limit`, written as gaps.
    fn ids(&mut self, limit: usize) -> Option<Vec<u32>> {
        let mut last = 0u64;
        let n = self.var()?;
        let mut out = Vec::new();
        for _ in 0..n {
            last = last.checked_add(self.var()?)?;
            if last >= limit as u64 {
                return None;
            }
            out.push(last as u32);
        }
        Some(out)
    }

    fn refs(&mut self, deps: usize, groups: usize, scans: usize) -> Option<Refs> {
        Some(Refs { deps: self.ids(deps)?, groups: self.ids(groups)?, scans: self.ids(scans)? })
    }

    /// A scan, whose files group holds only deps.
    fn scan(&mut self, groups: &[Refs]) -> Option<SavedScan> {
        let files = u32::try_from(self.var()?).ok()?;
        let only_deps = groups.get(files as usize)?;
        if !only_deps.groups.is_empty() || !only_deps.scans.is_empty() { return None }
        enum Head { Ambient(u64), Global(Box<str>) }
        let head = match self.u8()? {
            0 => Head::Ambient(self.var()?),
            1 => Head::Global(self.str()?.into()),
            _ => return None,
        };
        let hints: Box<[Box<str>]> = (0..self.var()?).map(|_| self.str().map(Box::from)).collect::<Option<_>>()?;
        let test = match head {
            Head::Ambient(classes) => ScanTest::Ambient { hints, classes },
            Head::Global(name) => ScanTest::Global { name, hints },
        };
        Some(SavedScan { files, test })
    }

    fn path(&mut self) -> Option<PathBuf> {
        let flag = self.u8()?;
        let (dir, name) = (self.str()?, self.str()?);
        let mut p = match flag {
            0 => self.root.join(dir),
            1 => PathBuf::from(dir),
            _ => return None,
        };
        if !name.is_empty() {
            p.push(name);
        }
        Some(p)
    }

    fn dep(&mut self) -> Option<Dep> {
        let tag = self.u8()?;
        let raw = self.path()?;
        let path = super::reads::dep_path(&raw);
        let a = self.u64()?;
        Some(match tag {
            0 => Dep::Text(path, a),
            1 => Dep::IsFile(path, a != 0),
            2 => Dep::IsDir(path, a != 0),
            3 => Dep::List(path, a),
            // An unknown discovery (another build's front end) drops the entry.
            5 => Dep::Discovered(super::reads::discovery_name(&self.str()?)?, path, a),
            6 => {
                let name = super::reads::discovery_name(&self.str()?)?;
                let request = crate::grammar::names::modules::resolution::DiscoveryRequest { root: raw, include: self.string_list()?, exclude: self.string_list()?, extensions: self.string_list()? };
                Dep::Requested(name, std::sync::Arc::new(request), a)
            }
            7 => {
                let len = usize::try_from(self.var()?).ok()?;
                let count = usize::try_from(self.var()?).ok()?;
                if count > self.bytes.len().saturating_sub(self.at) { return None; }
                let ranges = (0..count).map(|_| Some((u32::try_from(self.var()?).ok()?, u32::try_from(self.var()?).ok()?, self.u64()?, self.u64()?)))
                    .collect::<Option<Vec<_>>>()?;
                Dep::Outside(path, std::sync::Arc::new(super::reads::Bodies::from_parts(a, len, ranges)))
            }
            _ => return None,
        })
    }

    fn string_list(&mut self) -> Option<Vec<Box<str>>> {
        let count = usize::try_from(self.var()?).ok()?;
        if count > self.bytes.len().saturating_sub(self.at) { return None; }
        (0..count).map(|_| self.str().map(String::into_boxed_str)).collect()
    }

    fn diag(&mut self) -> Option<Diagnostic> {
        let rule = self.str()?;
        let severity = *SEVERITIES.get(self.u8()? as usize)?;
        let message = self.str()?;
        let message_index = self.var()?.checked_sub(1).map(|index| index as usize);
        let start = self.var()? as usize;
        let end = self.var()? as usize;
        let fix = match self.var()? {
            0 => None,
            _ => Some(self.fix()?),
        };
        let count = usize::try_from(self.var()?).ok()?;
        if count > self.bytes.len().saturating_sub(self.at) { return None; }
        let suggestions = (0..count).map(|_| Some(Suggestion { title: self.str()?, fix: self.fix()? })).collect::<Option<Vec<_>>>()?;
        Some(Diagnostic { rule, severity, message, message_index, start, end, fix, suggestions })
    }

    fn fix(&mut self) -> Option<Fix> {
        let range = self.var()? as usize..self.var()? as usize;
        let count = usize::try_from(self.var()?).ok()?;
        if count > self.bytes.len().saturating_sub(self.at) { return None; }
        let edits = (0..count)
            .map(|_| Some(Edit { start: self.var()? as usize, end: self.var()? as usize, text: self.str()? }))
            .collect::<Option<Vec<_>>>()?;
        let count = usize::try_from(self.var()?).ok()?;
        if count > self.bytes.len().saturating_sub(self.at) { return None; }
        let placeholders = (0..count).map(|_| Some(self.var()? as usize..self.var()? as usize)).collect::<Option<Vec<_>>>()?;
        Some(Fix { range, edits, placeholders })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_fixes_keep_declared_range_priority() {
        let root = Path::new("/root");
        let linter = crate::engine::tests::linter(r#"
            in .t match outer /a/ inner:/b/ /c/ {
                rule outer warn "outer" fix as "abX"
            }
            in outer.inner rule inner warn "inner" fix as "B"
        "#);
        let diagnostics = linter.lint(&root.join("file.t"), "abc");
        let mut writer = Writer { out: Vec::new(), root, strings: Vec::new(), string_ids: HashMap::new() };
        for diagnostic in &diagnostics {
            writer.diag(diagnostic);
        }
        let mut reader = Reader { bytes: &writer.out, at: 0, root, strings: writer.strings };
        let cached: Vec<_> = (0..diagnostics.len()).map(|_| reader.diag().expect("cached diagnostic")).collect();
        assert_eq!(crate::engine::apply_fixes("abc", &cached), ("abX".into(), 1));
    }
    #[test]
    fn requested_discovery_keeps_its_replay_policy_when_saved() {
        let root = Path::new("/root/cached-engine-inputs");
        let first = root.join("src/first.ts");
        super::super::reads::set_buffer(&first, "");
        let request = crate::grammar::names::modules::resolution::DiscoveryRequest { root: root.to_owned(), include: vec!["src/**".into()], exclude: vec!["generated/**".into()], extensions: vec![".ts".into()] };
        let (_, reads) = super::super::reads::track(|| super::super::reads::requested(&request));
        let mut writer = Writer { out: Vec::new(), root, strings: Vec::new(), string_ids: HashMap::new() };
        writer.dep(&reads.deps[0]);
        let mut reader = Reader { bytes: &writer.out, at: 0, root, strings: writer.strings };
        let dep = reader.dep().expect("cached discovery");
        assert!(dep.holds());
        let added = root.join("src/second.ts");
        super::super::reads::set_buffer(&added, "");
        assert!(!dep.holds());
        super::super::reads::drop_buffer(&first);
        super::super::reads::drop_buffer(&added);
    }
}

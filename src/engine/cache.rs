//! `.dxcache`: lint results kept between runs, beside the config. A
//! file's result is reused when its text and everything its lint read
//! (other files' texts, module resolution probes, tsconfigs: see
//! `engine::reads`) still match. Results are kept for clean files and
//! for files with problems alike, so a warm run only lints what changed.
//!
//! The whole cache belongs to one rule state: the config's tokens (imports
//! and macros expanded) and the dxlint executable. Any change there starts
//! afresh. Paths below the project root are saved relative to it, so a cache
//! restored into another checkout (CI) is reused there.
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
const MAGIC: &[u8; 4] = b"ZLCF";

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
    /// Its rules' states, in [`Saved::rule_states`].
    state: u32,
}

/// Each rule's name and [`Linter::rule_states`] hash, sorted.
type RuleStates = Vec<(String, u64)>;

/// What a saved entry offers for a file's current text.
enum Found {
    Whole(Vec<Diagnostic>),
    /// Rules changed since: what the others reported, and which to run.
    Partly { kept: Vec<Diagnostic>, rerun: Vec<bool> },
}

/// A result this run produced or confirmed.
enum Fresh {
    /// The saved entry still holds.
    Kept,
    New { content: u64, reads: Arc<Reads>, diags: Vec<Diagnostic> },
    /// Its reads in [`Cache::tables`] already ([`Cache::compacting`]).
    Interned { content: u64, refs: Refs, diags: Vec<Diagnostic> },
}

/// What a saved cache holds.
#[derive(Default)]
struct Saved {
    deps: Vec<Dep>,
    /// A group's refs only point at earlier groups.
    groups: Vec<Refs>,
    scans: Vec<SavedScan>,
    rule_states: Vec<RuleStates>,
    entries: HashMap<String, Entry>,
}

pub struct Cache {
    /// This run's rules, as entries record them.
    rules_now: RuleStates,
    /// The file's hash when read, to tell whether another run wrote it since.
    opened: Option<u64>,
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
    kinds: std::sync::RwLock<crate::fx_hash::FxHashMap<PathBuf, super::reads::PathKind>>,
    /// Texts of changed files that scans test ([`Cache::current_text`]).
    current: std::sync::RwLock<crate::fx_hash::FxHashMap<PathBuf, Option<Arc<str>>>>,
    fresh: Mutex<HashMap<String, Fresh>>,
    /// The tables the next save writes; with [`Self::compact`], this run's
    /// reads go in as they come, and need not stay alive to the end.
    tables: Mutex<Tables>,
    compact: bool,
    /// Something new to write.
    changed: AtomicBool,
    hits: AtomicUsize,
    /// Files whose saved result held for unchanged rules, and how many rules
    /// they ran again.
    partly: AtomicUsize,
    rules_rerun: AtomicUsize,
    /// Whose finished runs the file keeps, whatever else invalidates it.
    commands: Arc<super::commands::Commands>,
}

impl Cache {
    /// The cache for `linter`'s rules: what was saved, if it was saved for
    /// exactly these rules and this dxlint, else an empty one.
    pub fn open(linter: &Linter) -> Cache {
        let path = linter.root.join(FILE_NAME);
        let key = key(linter.fingerprint);
        let bytes = std::fs::read(&path).ok();
        let opened = bytes.as_deref().map(hash_bytes);
        if let Some(runs) = bytes.as_deref().and_then(decode_runs) { linter.commands.restore(runs) }
        let saved = bytes.and_then(|bytes| decode(&bytes, key, &linter.root)).unwrap_or_default();
        let mut rules_now: RuleStates = linter.rules.iter().map(|rule| rule.name.clone()).zip(linter.rule_states.iter().copied()).collect();
        rules_now.sort_unstable();
        rules_now.dedup();
        Cache {
            commands: linter.commands.clone(),
            rules_now,
            opened,
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
            kinds: Default::default(),
            current: Default::default(),
            saved,
            fresh: Mutex::default(),
            tables: Mutex::new(Tables { writer: Writer { root: linter.root.clone(), ..Writer::default() }, ..Tables::default() }),
            compact: false,
            changed: AtomicBool::new(false),
            hits: AtomicUsize::new(0),
            partly: AtomicUsize::new(0),
            rules_rerun: AtomicUsize::new(0),
        }
    }

    /// For a whole run that never asks what a result read ([`Self::reads_of`]):
    /// results keep their reads as table entries, far smaller than the reads.
    pub fn compacting(mut self) -> Cache {
        self.compact = true;
        self
    }

    /// Whether a save would have something new to write.
    pub fn has_new(&self) -> bool {
        self.changed.load(Ordering::Relaxed)
    }

    /// Files whose results came from the cache this run.
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    /// Files whose saved result held for unchanged rules this run, and the
    /// rules they ran again.
    pub fn partly(&self) -> (usize, usize) {
        (self.partly.load(Ordering::Relaxed), self.rules_rerun.load(Ordering::Relaxed))
    }

    /// The saved result for `rel` (a path relative to the config, `/`
    /// separated), if `text` and everything its lint read are unchanged.
    pub fn lookup(&self, linter: &Linter, rel: &str, text: &str) -> Option<Vec<Diagnostic>> {
        match self.find(linter, rel, text)? {
            Found::Whole(diags) => Some(diags),
            Found::Partly { .. } => None,
        }
    }

    /// `path`'s result: saved, or for rules changed since it was saved,
    /// running only those.
    pub fn result(&self, run: &super::LintRun<'_>, linter: &Linter, rel: &str, path: &Path, text: &str) -> Option<Vec<Diagnostic>> {
        match self.find(linter, rel, text)? {
            Found::Whole(diags) => Some(diags),
            Found::Partly { kept, rerun } => {
                self.partly.fetch_add(1, Ordering::Relaxed);
                self.rules_rerun.fetch_add(rerun.iter().filter(|&&again| again).count(), Ordering::Relaxed);
                let (mut diags, reads) = run.lint_rules_tracked(path, text, &rerun);
                diags.extend(kept);
                Diagnostic::dedup(&mut diags);
                let earlier = self.saved_reads(rel)?;
                let both = Reads { groups: vec![Arc::new(earlier), Arc::new(reads)], ..Default::default() };
                self.store(rel, text, &diags, Arc::new(both));
                Some(diags)
            }
        }
    }

    fn find(&self, linter: &Linter, rel: &str, text: &str) -> Option<Found> {
        let entry = self.saved.entries.get(rel)?;
        let content = hash_bytes(text.as_bytes());
        self.texts.write().expect("cache texts").insert(self.root.join(rel), content);
        if entry.content != content {
            return None;
        }
        if !self.holds(linter, &entry.refs) {
            if std::env::var_os("DXLINT_RELINT_WHY").is_some() {
                eprintln!("relint {rel}: {}", self.broken(linter, &entry.refs).unwrap_or_else(|| "?".into()));
            }
            return None;
        }
        let then = self.saved.rule_states.get(entry.state as usize)?;
        if *then == self.rules_now {
            self.hits.fetch_add(1, Ordering::Relaxed);
            self.fresh.lock().expect("cache lock").insert(rel.to_string(), Fresh::Kept);
            return Some(Found::Whole(entry.diags.clone()));
        }
        let was = |name: &str| then.binary_search_by(|(rule, _)| rule.as_str().cmp(name)).ok().map(|at| then[at].1);
        let now = |name: &str| self.rules_now.binary_search_by(|(rule, _)| rule.as_str().cmp(name)).ok().map(|at| self.rules_now[at].1);
        let kept = entry.diags.iter().filter(|diag| {
            let (old, new) = (was(&diag.rule), now(&diag.rule));
            // Reports of no rule are the file's own: they don't change.
            old == new
        }).cloned().collect();
        let rerun = linter.rules.iter().zip(&linter.rule_states).map(|(rule, state)| !rule.transform && was(&rule.name) != Some(*state)).collect();
        Some(Found::Partly { kept, rerun })
    }

    /// What `rel`'s current result read: this run's, else as saved.
    pub fn reads_of(&self, rel: &str) -> Option<Reads> {
        if let Some(Fresh::New { reads, .. }) = self.fresh.lock().expect("cache lock").get(rel) {
            return Some(Reads { groups: vec![reads.clone()], ..Default::default() });
        }
        self.saved_reads(rel)
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
                // Only files read otherwise than saved are tested again; a
                // changed listing could have added any file.
                self.saved.groups[scan.files as usize].deps.iter().all(|&dep| {
                    self.dep_holds(linter, dep) || {
                        let saved = &self.saved.deps[dep as usize];
                        saved.reads_text() && linter.passes(saved.path(), self.current_text(saved.path()).as_deref(), &scan.test)
                    }
                })
            }))
            && refs.groups.iter().all(|&i| *self.group_holds[i as usize].get_or_init(|| self.holds(linter, &self.saved.groups[i as usize])))
    }

    /// The first read of `refs` that no longer holds, described (env
    /// `DXLINT_RELINT_WHY`).
    fn broken(&self, linter: &Linter, refs: &Refs) -> Option<String> {
        for &i in &refs.deps {
            if self.dep_holds(linter, i) { continue }
            let dep = &self.saved.deps[i as usize];
            return Some(match dep {
                Dep::Part(parts, read) => {
                    let failing: Vec<String> = read.iter()
                        .filter(|&one| !linter.holds(&Dep::Part(parts.clone(), Arc::new(crate::parts::PartReads::new([one])))))
                        .map(|one| match one {
                            crate::parts::Read::Statement(at) => parts.statements.get(at as usize)
                                .map_or_else(|| format!("statement {at}"), |part| format!("statement {at} at byte {} ({} members)", part.start, part.members.len())),
                            one => format!("{one:?}"),
                        }).take(4).collect();
                    let now = linter.parts_now(&parts.path, self.text_hash(&parts.path));
                    let named = |parts: Option<&crate::parts::FileParts>| parts.map_or("unparsed", |parts| if parts.failed { "failed" } else if parts.precise { "named" } else { "unnamed" });
                    format!("parts of {} ({}; was {}, now {})", parts.path.display(), failing.join(", "), named(Some(parts)), named(now.as_deref()))
                }
                dep => format!("{} of {}", match dep {
                    Dep::Text(..) => "text", Dep::IsFile(..) => "file probe", Dep::IsDir(..) => "dir probe", Dep::List(..) => "listing",
                    Dep::Discovered(..) => "discovery", Dep::Requested(..) => "requested files", Dep::Outside(..) => "outside bodies",
                    Dep::Module(..) => "module answer", Dep::Part(..) => "parts",
                }, dep.path().display()),
            });
        }
        for &i in &refs.scans {
            if *self.scan_holds[i as usize].get().unwrap_or(&true) { continue }
            let scan = &self.saved.scans[i as usize];
            let file = self.saved.groups[scan.files as usize].deps.iter().filter(|&&dep| !self.dep_holds(linter, dep)).map(|&dep| &self.saved.deps[dep as usize])
                .find(|saved| !saved.reads_text() || !linter.passes(saved.path(), self.current_text(saved.path()).as_deref(), &scan.test));
            let kind = match &scan.test { ScanTest::Global { name, .. } => format!("global {name}"), ScanTest::Ambient { .. } => "ambient".into(), ScanTest::Pulls { .. } => "program pulls".into() };
            return Some(format!("scan for {kind}: {}", file.map_or_else(|| "?".into(), |dep| dep.path().display().to_string())));
        }
        refs.groups.iter().find_map(|&i| self.broken(linter, &self.saved.groups[i as usize]))
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
            Dep::Text(path, hash) => self.text_hash(path) == *hash,
            // A part or the outside of an unchanged file holds; a changed one
            // needs a look.
            dep @ Dep::Part(parts, _) => self.text_hash(&parts.path) == parts.hash || linter.holds(dep),
            dep @ Dep::Outside(path, bodies) => self.text_hash(path) == bodies.hash || linter.holds(dep),
            Dep::Requested(request, hash) => super::reads::request_holds(request, *hash, &|path| self.path_kind(path)),
            dep => linter.holds(dep),
        })
    }

    /// What is at `path`, probed once however many saved requests ask: every
    /// program lists the same library files.
    fn path_kind(&self, path: &Path) -> super::reads::PathKind {
        if let Some(&kind) = self.kinds.read().expect("cache kinds").get(path) { return kind }
        let kind = super::reads::path_kind(path);
        self.kinds.write().expect("cache kinds").insert(path.to_owned(), kind);
        kind
    }

    /// `path`'s content hash, read once however many saved reads ask.
    fn text_hash(&self, path: &Path) -> u64 {
        if let Some(&seen) = self.texts.read().expect("cache texts").get(path) { return seen }
        let (_, hash) = super::reads::read_unrecorded(path);
        self.texts.write().expect("cache texts").insert(path.to_owned(), hash);
        hash
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
            ..Default::default()
        }
    }

    fn group_reads(&self, i: u32) -> Arc<Reads> {
        self.group_reads[i as usize].get_or_init(|| Arc::new(self.reads(&self.saved.groups[i as usize]))).clone()
    }

    /// Remembers this run's result for `rel`.
    pub fn store(&self, rel: &str, text: &str, diags: &[Diagnostic], reads: Arc<Reads>) {
        let content = hash_bytes(text.as_bytes());
        let fresh = if self.compact {
            Fresh::Interned { content, refs: self.tables.lock().expect("cache tables").result(&reads), diags: diags.to_vec() }
        } else {
            Fresh::New { content, reads, diags: diags.to_vec() }
        };
        self.fresh.lock().expect("cache lock").insert(rel.to_string(), fresh);
        self.changed.store(true, Ordering::Relaxed);
    }

    /// Writes the cache: this run's results, plus (unless `prune`, for a
    /// run that saw every file) older results for files not seen this run,
    /// the newest any run saved for these rules: others write it meanwhile.
    /// Nothing is written when nothing changed.
    pub fn save(self, prune: bool) -> io::Result<()> {
        let fresh = self.fresh.into_inner().unwrap_or_default();
        let pruned = prune && self.saved.entries.keys().any(|k| !fresh.contains_key(k));
        if !self.changed.load(Ordering::Relaxed) && !pruned {
            return Ok(());
        }
        let newer = if prune { None } else {
            std::fs::read(&self.path).ok().filter(|bytes| Some(hash_bytes(bytes)) != self.opened)
                .and_then(|bytes| decode(&bytes, self.key, &self.root))
        };
        let mut tables = self.tables.into_inner().unwrap_or_default();
        tables.writer.root = self.root.clone();
        let state_id = |states: &RuleStates, known: &mut Vec<RuleStates>| -> u32 {
            match known.iter().position(|seen| seen == states) {
                Some(at) => at as u32,
                None => { known.push(states.clone()); (known.len() - 1) as u32 }
            }
        };
        let mut known: Vec<RuleStates> = vec![self.rules_now.clone()];
        let mut entries: Vec<(&str, u64, Refs, &[Diagnostic], u32)> = Vec::new();
        for (rel, f) in &fresh {
            entries.push(match f {
                Fresh::New { content, reads, diags } => (rel, *content, tables.reads(reads), diags, 0),
                Fresh::Interned { content, refs, diags } => (rel, *content, refs.clone(), diags, 0),
                Fresh::Kept => {
                    let e = &self.saved.entries[rel];
                    (rel, e.content, tables.saved_refs(&self.saved, &e.refs), &e.diags, 0)
                }
            });
        }
        if !prune {
            let others = newer.as_ref().unwrap_or(&self.saved);
            tables.reading_another_save();
            for (rel, e) in others.entries.iter().filter(|(k, _)| !fresh.contains_key(*k)) {
                let state = others.rule_states.get(e.state as usize).map_or(0, |states| state_id(states, &mut known));
                entries.push((rel, e.content, tables.saved_refs(others, &e.refs), &e.diags, state));
            }
        }
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));

        let mut w = std::mem::take(&mut tables.writer);
        w.var(known.len() as u64);
        for states in &known {
            w.var(states.len() as u64);
            for (rule, state) in states {
                w.str(rule);
                w.out.extend_from_slice(&state.to_le_bytes());
            }
        }
        w.var(tables.parts.len() as u64);
        for parts in &tables.parts {
            w.parts(parts);
        }
        w.var(u64::from(tables.dep_count));
        w.out.append(&mut tables.dep_bytes);
        // Groups name scans and scans name groups: both counts come first.
        w.var(tables.scans.len() as u64);
        w.var(u64::from(tables.group_count));
        w.out.append(&mut tables.group_bytes);
        for s in &tables.scans {
            w.scan(s);
        }
        w.var(entries.len() as u64);
        for (rel, content, refs, diags, state) in entries {
            w.str(rel);
            w.out.extend_from_slice(&content.to_le_bytes());
            w.var(u64::from(state));
            w.refs(&refs);
            w.var(diags.len() as u64);
            for d in diags {
                w.diag(d);
            }
        }
        // Written aside and renamed over, so a reader never sees half a file.
        let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
        w.write_to(&tmp, self.key, &encode_runs(&self.commands.kept_runs()))?;
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
#[derive(Default)]
struct Tables {
    /// Deps and groups go to bytes as they come, kept by hash: a whole run's
    /// reads are the biggest thing it holds otherwise.
    writer: Writer,
    dep_bytes: Vec<u8>,
    dep_count: u32,
    dep_ids: HashMap<u64, u32>,
    group_bytes: Vec<u8>,
    group_count: u32,
    group_ids: HashMap<u64, u32>,
    /// Results interned since dead addresses were last dropped.
    interned: u32,
    /// Groups already interned, by address or by saved index. A weak
    /// reference holds the address: no later group can take it, though what
    /// the group held is gone once its readers are.
    by_address: HashMap<usize, (std::sync::Weak<Reads>, u32)>,
    by_saved: HashMap<u32, u32>,
    scans: Vec<SavedScan>,
    scan_ids: HashMap<SavedScan, u32>,
    /// Scans already interned, as for groups.
    scan_by_address: HashMap<usize, (std::sync::Weak<Scan>, u32)>,
    scan_by_saved: HashMap<u32, u32>,
    /// File versions part reads name, by address.
    parts: Vec<Arc<crate::parts::FileParts>>,
    part_ids: HashMap<usize, u32>,
}

impl Tables {
    /// `reads` of one result, as table entries.
    fn result(&mut self, reads: &Reads) -> Refs {
        // A dead group can't be met again, and its address can go.
        const DROP_DEAD_EVERY: u32 = 64;
        self.interned += 1;
        if self.interned % DROP_DEAD_EVERY == 0 {
            self.by_address.retain(|_, (group, _)| group.strong_count() > 0);
            self.scan_by_address.retain(|_, (scan, _)| scan.strong_count() > 0);
        }
        self.reads(reads)
    }

    /// Saved indexes from now on are another save's.
    fn reading_another_save(&mut self) {
        self.by_saved.clear();
        self.scan_by_saved.clear();
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
        if let Some((_, id)) = self.scan_by_address.get(&address) {
            return *id;
        }
        let files = self.reads_group(&s.files);
        let id = self.scan(SavedScan { files, test: s.test.clone() });
        self.scan_by_address.insert(address, (Arc::downgrade(s), id));
        id
    }

    fn saved_scan(&mut self, from: &Saved, i: u32) -> u32 {
        if let Some(&id) = self.scan_by_saved.get(&i) {
            return id;
        }
        let saved = &from.scans[i as usize];
        let files = self.saved_group(from, saved.files);
        let id = self.scan(SavedScan { files, test: saved.test.clone() });
        self.scan_by_saved.insert(i, id);
        id
    }

    fn dep(&mut self, dep: &Dep) -> u32 {
        let hash = content_hash(dep);
        if let Some(&id) = self.dep_ids.get(&hash) {
            return id;
        }
        if let Dep::Part(parts, _) = dep {
            let address = Arc::as_ptr(parts) as usize;
            if !self.part_ids.contains_key(&address) {
                self.part_ids.insert(address, self.parts.len() as u32);
                self.parts.push(parts.clone());
            }
        }
        let id = self.dep_count;
        self.dep_count += 1;
        let Tables { writer, dep_bytes, part_ids, .. } = self;
        writer.into_section(dep_bytes, |w| w.dep(dep, part_ids));
        self.dep_ids.insert(hash, id);
        id
    }

    fn group(&mut self, refs: Refs) -> u32 {
        let hash = content_hash(&refs);
        if let Some(&id) = self.group_ids.get(&hash) {
            return id;
        }
        let id = self.group_count;
        self.group_count += 1;
        let Tables { writer, group_bytes, .. } = self;
        writer.into_section(group_bytes, |w| w.refs(&refs));
        self.group_ids.insert(hash, id);
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
        if let Some((_, id)) = self.by_address.get(&address) {
            return *id;
        }
        let refs = self.reads(g);
        let id = self.group(refs);
        self.by_address.insert(address, (Arc::downgrade(g), id));
        id
    }

    fn saved_refs(&mut self, saved: &Saved, refs: &Refs) -> Refs {
        let deps = refs.deps.iter().map(|&i| self.dep(&saved.deps[i as usize])).collect();
        let groups = refs.groups.iter().map(|&i| self.saved_group(saved, i)).collect();
        let scans = refs.scans.iter().map(|&i| self.saved_scan(saved, i)).collect();
        sorted(Refs { deps, groups, scans })
    }

    fn saved_group(&mut self, saved: &Saved, i: u32) -> u32 {
        if let Some(&id) = self.by_saved.get(&i) {
            return id;
        }
        let refs = self.saved_refs(saved, &saved.groups[i as usize]);
        let id = self.group(refs);
        self.by_saved.insert(i, id);
        id
    }
}

fn content_hash(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
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
#[derive(Default)]
struct Writer {
    out: Vec<u8>,
    root: PathBuf,
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

impl Writer {
    /// The file: its header and strings in front of the body.
    fn write_to(self, path: &Path, key: u64, runs: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let mut header = Vec::with_capacity(64 * self.strings.len() + runs.len());
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&(runs.len() as u64).to_le_bytes());
        header.extend_from_slice(runs);
        header.extend_from_slice(&key.to_le_bytes());
        write_var(&mut header, self.strings.len() as u64);
        for s in &self.strings {
            write_var(&mut header, s.len() as u64);
            header.extend_from_slice(s.as_bytes());
        }
        let mut file = std::fs::File::create(path)?;
        file.write_all(&header)?;
        file.write_all(&self.out)
    }

    /// What `write` writes, into `section` instead of the body.
    fn into_section(&mut self, section: &mut Vec<u8>, write: impl FnOnce(&mut Self)) {
        std::mem::swap(&mut self.out, section);
        write(self);
        std::mem::swap(&mut self.out, section);
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
        let root = std::mem::take(&mut self.root);
        let (flag, p) = match p.strip_prefix(&root) {
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
        self.root = root;
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
            ScanTest::Pulls { settings, found } => {
                self.out.push(2);
                self.var(u64::from(*settings));
                self.var(found.len() as u64);
                for (path, hash) in found.iter() {
                    self.path(path);
                    self.out.extend_from_slice(&hash.to_le_bytes());
                }
                return;
            }
        };
        self.var(words.len() as u64);
        for word in words {
            self.str(word);
        }
    }

    fn bodies(&mut self, bodies: &super::reads::Bodies) {
        let (len, ranges) = bodies.parts();
        self.out.extend_from_slice(&bodies.hash.to_le_bytes());
        self.var(len as u64);
        self.var(ranges.len() as u64);
        for &(start, end, before, after) in ranges {
            self.var(u64::from(start));
            self.var(u64::from(end));
            self.out.extend_from_slice(&before.to_le_bytes());
            self.out.extend_from_slice(&after.to_le_bytes());
        }
    }

    fn names(&mut self, names: &[crate::parts::Name]) {
        self.var(names.len() as u64);
        for name in names { self.out.extend_from_slice(&name.to_le_bytes()) }
    }

    /// One file version part reads name ([`crate::parts::FileParts`]).
    fn parts(&mut self, parts: &crate::parts::FileParts) {
        self.path(&parts.path);
        self.out.extend_from_slice(&parts.hash.to_le_bytes());
        self.out.extend_from_slice(&parts.outside.to_le_bytes());
        self.out.push(u8::from(parts.precise) | u8::from(parts.failed) << 1 | u8::from(parts.coarse.is_some()) << 2);
        if let Some(bodies) = &parts.coarse { self.bodies(bodies) }
        for list in [&parts.statements, &parts.bodies] {
            self.var(list.len() as u64);
            for part in list.iter() {
                self.var(u64::from(part.start));
                self.var(u64::from(part.end));
                self.out.extend_from_slice(&part.hash.to_le_bytes());
                self.out.push(u8::from(part.computed));
                self.names(&part.declares);
                self.names(&part.mentions);
                self.var(part.refers.len() as u64);
                for &index in part.refers.iter() { self.var(u64::from(index)) }
                self.out.extend_from_slice(&part.shell.to_le_bytes());
                self.var(part.members.len() as u64);
                for member in part.members.iter() {
                    self.var(u64::from(member.start));
                    self.var(u64::from(member.end));
                    self.out.extend_from_slice(&member.hash.to_le_bytes());
                    self.names(&member.declares);
                    self.names(&member.mentions);
                }
            }
        }
        let mut declared: Vec<(&u32, &crate::parts::Name)> = parts.declared.iter().collect();
        declared.sort_unstable();
        self.var(declared.len() as u64);
        for (&node, &name) in declared {
            self.var(u64::from(node));
            self.out.extend_from_slice(&name.to_le_bytes());
        }
    }

    fn dep(&mut self, d: &Dep, parts: &HashMap<usize, u32>) {
        if let Dep::Part(version, read) = d {
            self.out.push(8);
            self.path(&version.path);
            self.var(u64::from(parts[&(Arc::as_ptr(version) as usize)]));
            for indices in [&read.statements, &read.bodies, &read.uses_of, &read.shells] {
                self.var(indices.len() as u64);
                for &index in indices.iter() { self.var(u64::from(index)) }
            }
            self.var(read.members.len() as u64);
            for &(index, member) in read.members.iter() {
                self.var(u64::from(index));
                self.var(u64::from(member));
            }
            self.names(&read.names);
            self.names(&read.uses);
            self.out.push(u8::from(read.whole));
            return;
        }
        let (tag, a) = match d {
            Dep::Text(_, h) => (0, *h),
            Dep::Outside(_, bodies) => (7, bodies.hash),
            Dep::IsFile(_, found) => (1, u64::from(*found)),
            Dep::IsDir(_, found) => (2, u64::from(*found)),
            Dep::List(_, h) => (3, *h),
            Dep::Discovered(found) => (5, found.hash),
            Dep::Requested(_, hash) => (6, *hash),
            Dep::Module(_, hash) => (9, *hash),
            Dep::Part(..) => unreachable!("written above"),
        };
        self.out.push(tag);
        self.path(d.path());
        self.out.extend_from_slice(&a.to_le_bytes());
        if let Dep::Discovered(found) = d {
            self.str(found.name);
        }
        if let Dep::Requested(request, _) = d {
            for list in [&request.include, &request.exclude, &request.extensions] {
                self.var(list.len() as u64);
                for value in list { self.str(value); }
            }
        }
        if let Dep::Module(ask, _) = d {
            self.var(u64::from(ask.settings));
            self.out.push(u8::from(ask.exports));
            self.str(&ask.module);
            for list in [&ask.stop, &ask.ignore] {
                self.var(list.len() as u64);
                for value in list.iter() { self.str(value); }
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

/// The file's command runs: they sit before its key, as they outlive it.
fn encode_runs(runs: &[super::commands::KeptRun]) -> Vec<u8> {
    fn text(out: &mut Vec<u8>, text: &str) {
        write_var(out, text.len() as u64);
        out.extend_from_slice(text.as_bytes());
    }
    fn texts(out: &mut Vec<u8>, list: &[String]) {
        write_var(out, list.len() as u64);
        for item in list { text(out, item) }
    }
    let mut out = Vec::new();
    write_var(&mut out, runs.len() as u64);
    for run in runs {
        text(&mut out, &run.name);
        texts(&mut out, &run.argv);
        texts(&mut out, &run.args);
        text(&mut out, &run.payload);
        write_var(&mut out, run.inputs.len() as u64);
        for (path, hash) in &run.inputs {
            text(&mut out, path);
            out.extend_from_slice(&hash.to_le_bytes());
        }
    }
    out
}

fn decode_runs(bytes: &[u8]) -> Option<Vec<super::commands::KeptRun>> {
    let mut r = Reader { bytes, at: 0, root: Path::new(""), strings: Vec::new() };
    if r.take(4)? != MAGIC { return None }
    let len = usize::try_from(r.u64()?).ok()?;
    let mut r = Reader { bytes: r.take(len)?, at: 0, root: Path::new(""), strings: Vec::new() };
    fn text(r: &mut Reader<'_>) -> Option<String> {
        let len = usize::try_from(r.var()?).ok()?;
        String::from_utf8(r.take(len)?.to_vec()).ok()
    }
    fn texts(r: &mut Reader<'_>) -> Option<Vec<String>> {
        (0..r.var()?).map(|_| text(r)).collect()
    }
    (0..r.var()?).map(|_| Some(super::commands::KeptRun {
        name: text(&mut r)?,
        argv: texts(&mut r)?,
        args: texts(&mut r)?,
        payload: text(&mut r)?.into(),
        inputs: (0..r.var()?).map(|_| Some((text(&mut r)?, r.u64()?))).collect::<Option<Vec<_>>>()?,
    })).collect()
}

fn decode(bytes: &[u8], key: u64, root: &Path) -> Option<Saved> {
    let mut r = Reader { bytes, at: 0, root, strings: Vec::new() };
    if r.take(4)? != MAGIC {
        return None;
    }
    let runs = usize::try_from(r.u64()?).ok()?;
    r.take(runs)?;
    if r.u64()? != key {
        return None;
    }
    for _ in 0..r.var()? {
        let n = usize::try_from(r.var()?).ok()?;
        let s = String::from_utf8(r.take(n)?.to_vec()).ok()?;
        r.strings.push(s);
    }
    let rule_states = (0..r.var()?).map(|_| (0..r.var()?).map(|_| Some((r.str()?, r.u64()?))).collect::<Option<RuleStates>>()).collect::<Option<Vec<_>>>()?;
    let parts = (0..r.var()?).map(|_| r.parts().map(|parts| Arc::new(parts.lasting()))).collect::<Option<Vec<_>>>()?;
    let deps = (0..r.var()?).map(|_| r.dep(&parts)).collect::<Option<Vec<_>>>()?;
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
        let state = u32::try_from(r.var()?).ok()?;
        let refs = r.refs(deps.len(), groups.len(), scans.len())?;
        let diags = (0..r.var()?).map(|_| r.diag()).collect::<Option<Vec<_>>>()?;
        entries.insert(rel, Entry { content, refs, diags, state });
    }
    Some(Saved { deps, groups, scans, rule_states, entries })
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
            2 => {
                let settings = u32::try_from(self.var()?).ok()?;
                let found = (0..self.count()?).map(|_| Some((super::reads::dep_path(&self.path()?), self.u64()?))).collect::<Option<_>>()?;
                return Some(SavedScan { files, test: ScanTest::Pulls { settings, found } });
            }
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

    fn count(&mut self) -> Option<usize> {
        let count = usize::try_from(self.var()?).ok()?;
        (count <= self.bytes.len().saturating_sub(self.at)).then_some(count)
    }

    fn bodies(&mut self) -> Option<super::reads::Bodies> {
        let hash = self.u64()?;
        let len = usize::try_from(self.var()?).ok()?;
        let count = self.count()?;
        let ranges = (0..count).map(|_| Some((u32::try_from(self.var()?).ok()?, u32::try_from(self.var()?).ok()?, self.u64()?, self.u64()?)))
            .collect::<Option<Vec<_>>>()?;
        Some(super::reads::Bodies::from_parts(hash, len, ranges))
    }

    fn name(&mut self) -> Option<crate::parts::Name> {
        Some(crate::parts::Name::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn names(&mut self) -> Option<Box<[crate::parts::Name]>> {
        let count = self.count()?;
        (0..count).map(|_| self.name()).collect()
    }

    fn parts(&mut self) -> Option<crate::parts::FileParts> {
        let path = super::reads::dep_path(&self.path()?);
        let (hash, outside) = (self.u64()?, self.u64()?);
        let flags = self.u8()?;
        let coarse = if flags & 4 != 0 { Some(Arc::new(self.bodies()?)) } else { None };
        let mut lists = Vec::new();
        for _ in 0..2 {
            let count = self.count()?;
            let list = (0..count).map(|_| {
                let (start, end) = (u32::try_from(self.var()?).ok()?, u32::try_from(self.var()?).ok()?);
                let hash = self.u64()?;
                let computed = self.u8()? != 0;
                let (declares, mentions) = (self.names()?, self.names()?);
                let refers_count = self.count()?;
                let refers = (0..refers_count).map(|_| u32::try_from(self.var()?).ok()).collect::<Option<Box<[u32]>>>()?;
                let shell = self.u64()?;
                let member_count = self.count()?;
                let members = (0..member_count).map(|_| {
                    let (start, end) = (u32::try_from(self.var()?).ok()?, u32::try_from(self.var()?).ok()?);
                    let hash = self.u64()?;
                    let (declares, mentions) = (self.names()?, self.names()?);
                    Some(crate::parts::Member { start, end, hash, declares, mentions })
                }).collect::<Option<Box<[_]>>>()?;
                Some(crate::parts::Part { start, end, hash, declares, computed, mentions, refers, shell, members })
            }).collect::<Option<Vec<_>>>()?;
            lists.push(list);
        }
        let bodies = lists.pop()?;
        let statements = lists.pop()?;
        let count = self.count()?;
        let declared = (0..count).map(|_| Some((u32::try_from(self.var()?).ok()?, self.name()?))).collect::<Option<crate::fx_hash::FxHashMap<u32, crate::parts::Name>>>()?;
        Some(crate::parts::FileParts::new((path, hash), outside, statements, bodies, (flags & 1 != 0, flags & 2 != 0), declared, coarse))
    }

    fn dep(&mut self, parts: &[Arc<crate::parts::FileParts>]) -> Option<Dep> {
        let tag = self.u8()?;
        let raw = self.path()?;
        let path = super::reads::dep_path(&raw);
        if tag == 8 {
            let version = parts.get(usize::try_from(self.var()?).ok()?)?.clone();
            let mut indices = Vec::new();
            for _ in 0..4 {
                let count = self.count()?;
                indices.push((0..count).map(|_| u32::try_from(self.var()?).ok()).collect::<Option<Box<[u32]>>>()?);
            }
            let count = self.count()?;
            let members = (0..count).map(|_| Some((u32::try_from(self.var()?).ok()?, u32::try_from(self.var()?).ok()?))).collect::<Option<Box<[_]>>>()?;
            let (names, uses) = (self.names()?, self.names()?);
            let whole = self.u8()? != 0;
            let shells = indices.pop()?;
            let uses_of = indices.pop()?;
            let bodies = indices.pop()?;
            let statements = indices.pop()?;
            let read = crate::parts::PartReads { statements, shells, members, bodies, names, uses, uses_of, whole };
            return Some(Dep::Part(version, Arc::new(read)));
        }
        let a = self.u64()?;
        Some(match tag {
            0 => Dep::Text(path, a),
            1 => Dep::IsFile(path, a != 0),
            2 => Dep::IsDir(path, a != 0),
            3 => Dep::List(path, a),
            // An unknown discovery (another build's front end) drops the entry.
            5 => Dep::Discovered(Arc::new(super::reads::Discovered { name: super::reads::discovery_name(&self.str()?)?, dir: path, hash: a })),
            6 => {
                let request = crate::names::modules::resolution::DiscoveryRequest { root: raw, include: self.string_list()?, exclude: self.string_list()?, extensions: self.string_list()? };
                Dep::Requested(std::sync::Arc::new(request), a)
            }
            7 => {
                let len = usize::try_from(self.var()?).ok()?;
                let count = usize::try_from(self.var()?).ok()?;
                if count > self.bytes.len().saturating_sub(self.at) { return None; }
                let ranges = (0..count).map(|_| Some((u32::try_from(self.var()?).ok()?, u32::try_from(self.var()?).ok()?, self.u64()?, self.u64()?)))
                    .collect::<Option<Vec<_>>>()?;
                Dep::Outside(path, std::sync::Arc::new(super::reads::Bodies::from_parts(a, len, ranges)))
            }
            9 => {
                let settings = u32::try_from(self.var()?).ok()?;
                let exports = *self.bytes.get(self.at)? != 0;
                self.at += 1;
                let module = self.str()?.into();
                let ask = super::reads::ModuleAsk { root: path, settings, module, stop: self.string_list()?.into(), ignore: self.string_list()?.into(), exports };
                Dep::Module(std::sync::Arc::new(ask), a)
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

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join("dxlint-cache-tests").join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("tmp dir");
        root
    }

    fn store(cache: &Cache, rel: &str) {
        cache.store(rel, rel, &[], Arc::default());
    }

    #[test]
    fn a_save_keeps_what_another_writer_saved_since_it_opened() {
        let root = project("merge");
        let linter = crate::engine::tests::linter_at(&root, "");
        let (first, second) = (Cache::open(&linter), Cache::open(&linter));
        store(&first, "first");
        store(&second, "second");
        first.save(false).expect("save");
        second.save(false).expect("save");
        let saved = Cache::open(&linter);
        assert!(saved.lookup(&linter, "first", "first").is_some());
        assert!(saved.lookup(&linter, "second", "second").is_some());
    }

    #[test]
    fn command_outputs_outlive_a_change_of_rules_until_their_input_changes() {
        let root = project("command-runs");
        std::fs::write(root.join("input.txt"), "one").expect("input");
        // Each run appends to `runs`: how many there were shows through its length.
        let command = r#"command counted "node -e \"const fs=require('fs');fs.appendFileSync('runs','x');console.log('dxlint-input input.txt');console.log('payload')\"""#;
        let runs = || std::fs::read_to_string(root.join("runs")).map_or(0, |text| text.len());
        let answer = |linter: &Linter| linter.commands.answer("counted", &[]);
        let linter = crate::engine::tests::linter_at(&root, command);
        let first = Cache::open(&linter);
        assert_eq!(answer(&linter), super::super::commands::Answer::Ready("payload\n".into()));
        store(&first, "a");
        first.save(false).expect("save");
        assert_eq!(runs(), 1);
        // Other rules: results are gone, the output is not.
        let other = crate::engine::tests::linter_at(&root, &format!("{command}\nlang t \"*.t\""));
        let second = Cache::open(&other);
        assert!(second.lookup(&other, "a", "a").is_none());
        assert_eq!(answer(&other), super::super::commands::Answer::Ready("payload\n".into()));
        assert_eq!(runs(), 1);
        std::fs::write(root.join("input.txt"), "two").expect("input");
        let changed = crate::engine::tests::linter_at(&root, command);
        let _opened = Cache::open(&changed);
        answer(&changed);
        assert_eq!(runs(), 2);
    }

    #[test]
    fn results_interned_as_they_come_save_what_they_read() {
        let root = project("interned");
        let linter = crate::engine::tests::linter_at(&root, "");
        let compact = Cache::open(&linter).compacting();
        let read = |path: &str| Dep::Text(super::super::reads::dep_path(&root.join(path)), 1);
        // Groups die between results, and their addresses may come back.
        for rel in ["a", "b"] {
            let group = Arc::new(Reads { deps: vec![read(rel)], ..Default::default() });
            compact.store(rel, rel, &[], Arc::new(Reads { groups: vec![group], ..Default::default() }));
        }
        compact.save(true).expect("save");
        let saved = Cache::open(&linter);
        for rel in ["a", "b"] {
            let reads = saved.saved_reads(rel).expect("saved");
            assert_eq!(reads.groups.len(), 1, "{rel}");
            assert_eq!(reads.groups[0].deps, vec![read(rel)], "{rel}");
        }
    }

    #[test]
    fn a_whole_run_forgets_files_it_did_not_see_whoever_saved_them() {
        let root = project("prune");
        let linter = crate::engine::tests::linter_at(&root, "");
        let whole = Cache::open(&linter);
        let other = Cache::open(&linter);
        store(&other, "gone");
        other.save(false).expect("save");
        store(&whole, "seen");
        whole.save(true).expect("save");
        let saved = Cache::open(&linter);
        assert!(saved.lookup(&linter, "seen", "seen").is_some());
        assert!(saved.lookup(&linter, "gone", "gone").is_none());
    }

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
        let mut writer = Writer { out: Vec::new(), root: root.to_path_buf(), strings: Vec::new(), string_ids: HashMap::new() };
        for diagnostic in &diagnostics {
            writer.diag(diagnostic);
        }
        let mut reader = Reader { bytes: &writer.out, at: 0, root, strings: writer.strings };
        let cached: Vec<_> = (0..diagnostics.len()).map(|_| reader.diag().expect("cached diagnostic")).collect();
        assert_eq!(crate::engine::apply_fixes("abc", &cached), ("abX".into(), 1));
    }
    #[test]
    fn part_reads_keep_what_they_read_and_of_which_version_when_saved() {
        use crate::parts::{name_hash, FileParts, Member, Part, PartReads, Read};
        let root = Path::new("/root/cached-parts");
        let member = Member { start: 2, end: 8, hash: 4, declares: [name_hash("m")].into(), mentions: [name_hash("b")].into() };
        let statement = Part { start: 0, end: 9, hash: 1, declares: [name_hash("a")].into(), mentions: [name_hash("a"), name_hash("b")].into(), refers: [1].into(), computed: false,
            shell: 5, members: [member].into() };
        let file = super::super::reads::dep_path(&root.join("f.ts"));
        let parts = Arc::new(FileParts::new((file.clone(), 7), 8, vec![statement, Part { start: 9, end: 20, hash: 2, ..Part::default() }], Vec::new(), (true, false),
            [(3, name_hash("a"))].into_iter().collect(), None));
        let read = Arc::new(PartReads::new([Read::Statement(1), Read::Shell(0), Read::Member(0, 0), Read::Name(name_hash("a")), Read::UsesOf(3)]));
        let dep = Dep::Part(parts.clone(), read.clone());
        let mut writer = Writer { out: Vec::new(), root: root.to_path_buf(), strings: Vec::new(), string_ids: HashMap::new() };
        writer.parts(&parts);
        writer.dep(&dep, &[(Arc::as_ptr(&parts) as usize, 0)].into_iter().collect());
        let mut reader = Reader { bytes: &writer.out, at: 0, root, strings: writer.strings };
        let version = Arc::new(reader.parts().expect("cached parts"));
        let Some(Dep::Part(back, reads)) = reader.dep(std::slice::from_ref(&version)) else { panic!("a part read") };
        assert_eq!(*reads, *read);
        assert!(back.precise && back.hash == 7 && back.statements.len() == 2 && back.path == file);
        assert_eq!((back.statements[0].shell, &back.statements[0].members), (5, &parts.statements[0].members));
        // The version judges an unrelated new statement as before.
        let added = FileParts::new((file.clone(), 9), 10, vec![Part { hash: 1, ..back.statements[0].clone() }, Part { hash: 2, ..Part::default() }, Part { hash: 3, declares: [name_hash("c")].into(), ..Part::default() }],
            Vec::new(), (false, false), Default::default(), None);
        assert!(back.holds_all(&back.diff(&added), &reads, false));
    }

    #[test]
    fn requested_discovery_keeps_its_replay_policy_when_saved() {
        let root = Path::new("/root/cached-engine-inputs");
        let first = root.join("src/first.ts");
        super::super::reads::set_buffer(&first, "");
        let request = crate::names::modules::resolution::DiscoveryRequest { root: root.to_owned(), include: vec!["src/**".into()], exclude: vec!["generated/**".into()], extensions: vec![".ts".into()] };
        let (_, reads) = super::super::reads::track(|| super::super::reads::requested(&request));
        let mut writer = Writer { out: Vec::new(), root: root.to_path_buf(), strings: Vec::new(), string_ids: HashMap::new() };
        writer.dep(&reads.deps[0], &HashMap::new());
        let mut reader = Reader { bytes: &writer.out, at: 0, root, strings: writer.strings };
        let dep = reader.dep(&[]).expect("cached discovery");
        assert!(dep.holds());
        let added = root.join("src/second.ts");
        super::super::reads::set_buffer(&added, "");
        assert!(!dep.holds());
        super::super::reads::drop_buffer(&first);
        super::super::reads::drop_buffer(&added);
    }
}

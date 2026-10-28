//! Background workspace scan: lints every file on disk and streams each
//! file's diagnostics to the client as soon as it is done, with progress.
//! Tabs the user has open go first, then the rest breadth-first. A newer scan
//! (config change) cancels older ones via the generation. Scans reuse and
//! update each config's `.dxcache`.
//!
//! Every lint's reads are kept, so an edit re-lints just the files it
//! affects ([`relint`]). Files the editor owns lint on their own worker
//! ([`edit`]), so the request loop never waits for a lint.
//!
//! Each batch (a scan, a relint queue, a burst of editor lints) shares one
//! [`LintRun`] per linter, like a CLI run: program-wide answers are checked
//! once per batch, not once per file. Editor batches go further and start
//! from the previous editor batch's answers the edits since left standing.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ignore::{DirEntry, WalkBuilder, WalkState};
use serde_json::json;

use super::projects::Linters;
use super::transport::Outbox;
use super::{convert, uri};
use crate::engine::cache::Cache;
use crate::engine::{Carried, LintRun, Linter};
use crate::engine::reads::Reads;

/// State shared between the request loop and scans.
#[derive(Default)]
pub struct Shared {
    /// Files the request loop lints itself (open buffers, gitignored tabs);
    /// scans leave them and their diagnostics alone.
    pub editor: Mutex<HashSet<PathBuf>>,
    /// URIs that currently show at least one diagnostic.
    pub published: Mutex<HashSet<String>>,
    pub generation: AtomicU64,
    /// Files the current scan lints next, ahead of the walk order.
    priority: Mutex<Priority>,
    /// What each file's latest lint read, to find the files an edit affects.
    pub reads: Mutex<HashMap<PathBuf, Arc<Reads>>>,
    relint: Mutex<Relint>,
    editing: Mutex<Editing>,
    /// The server is exiting: the editor worker stops.
    pub closing: AtomicBool,
}

/// A file the editor owns, to lint: an open buffer's text, or a gitignored
/// tab read from disk (`text` and `version` are `None`).
pub struct EditorLint {
    pub path: PathBuf,
    pub uri: String,
    pub version: Option<i64>,
    pub text: Option<String>,
    pub linter: Arc<Linter>,
}

/// Editor files waiting for [`edit`], latest text each, and whether its
/// worker runs.
#[derive(Default)]
struct Editing {
    queue: Vec<EditorLint>,
    running: bool,
    /// Each linter's last editor run, which the next one starts from.
    carried: Vec<(Arc<Linter>, Carried)>,
}

/// Files waiting for [`relint`], the linters to use, and whether its
/// worker runs.
#[derive(Default)]
struct Relint {
    queue: VecDeque<PathBuf>,
    linters: Linters,
    running: bool,
}

#[derive(Default)]
struct Priority {
    /// The scan this queue belongs to; older scans' workers never take from it.
    generation: u64,
    queue: VecDeque<PathBuf>,
}

impl Shared {
    /// Publishes, skipping the message when a clean file was already clean.
    pub fn publish(&self, out: &Outbox, uri: &str, version: Option<i64>, diagnostics: serde_json::Value) {
        let empty = diagnostics.as_array().is_some_and(Vec::is_empty);
        {
            let mut published = self.published.lock().expect("lock");
            if empty && !published.remove(uri) {
                return;
            }
            if !empty {
                published.insert(uri.to_string());
            }
        }
        let mut params = json!({ "uri": uri, "diagnostics": diagnostics });
        if let Some(v) = version {
            params["version"] = json!(v);
        }
        out.notify("textDocument/publishDiagnostics", params);
    }

    /// Moves files to the front of the running scan.
    pub fn prioritize(&self, files: impl IntoIterator<Item = PathBuf>) {
        self.priority.lock().expect("lock").queue.extend(files);
    }

    fn next_priority(&self, generation: u64) -> Option<PathBuf> {
        let mut p = self.priority.lock().expect("lock");
        if p.generation != generation {
            return None;
        }
        p.queue.pop_front()
    }

    /// Lints `path` with `text` in `run`, keeping what the lint read.
    fn lint(&self, run: &LintRun<'_>, path: &Path, text: &str) -> Vec<crate::engine::Diagnostic> {
        let (diags, reads) = run.lint_tracked(path, text);
        self.reads.lock().expect("lock").insert(path.to_path_buf(), Arc::new(reads));
        diags
    }
}

/// One batch's [`LintRun`] per linter.
struct Runs<'a>(Vec<(&'a Linter, LintRun<'a>)>);

impl<'a> Runs<'a> {
    /// Editor lints start from the last editor run's answers that the edits
    /// since left standing, and may split a big file over threads: a
    /// keystroke redoes only what the edited text could change.
    fn editor(linters: impl IntoIterator<Item = &'a Linter>, carried: &mut Vec<(Arc<Linter>, Carried)>) -> Self {
        Self::new(linters, |linter| {
            let previous = carried.iter().position(|(l, _)| std::ptr::eq(&**l, linter)).map(|index| carried.swap_remove(index).1);
            linter.run_after(previous)
        })
    }

    /// Background lints run each file on one thread: nobody waits on one
    /// file, and split rule groups would each hold its facts.
    fn background(linters: impl IntoIterator<Item = &'a Linter>) -> Self {
        Self::new(linters, |linter| linter.run().serial())
    }

    fn new(linters: impl IntoIterator<Item = &'a Linter>, mut run: impl FnMut(&'a Linter) -> LintRun<'a>) -> Self {
        let mut runs: Vec<(&Linter, LintRun<'_>)> = Vec::new();
        for linter in linters {
            if !runs.iter().any(|(l, _)| std::ptr::eq(*l, linter)) {
                runs.push((linter, run(linter)));
            }
        }
        Runs(runs)
    }

    fn of(&self, linter: &Linter) -> Option<&LintRun<'a>> {
        self.0.iter().find(|(l, _)| std::ptr::eq(*l, linter)).map(|(_, run)| run)
    }

    /// Ends the batch: the linters let go of the files it parsed, and the
    /// memory goes back to the system.
    fn finish(self) {
        for (linter, run) in self.0 {
            drop(run);
            linter.forget_units();
        }
        crate::alloc::trim();
        crate::alloc::release();
    }
}

/// Re-lints files on disk (whose reads an edit broke) on one background
/// worker, until a new scan takes over.
pub fn relint(linters: Linters, files: Vec<PathBuf>, shared: Arc<Shared>, out: Outbox) {
    let generation = shared.generation.load(Ordering::SeqCst);
    {
        let mut relint = shared.relint.lock().expect("lock");
        relint.linters = linters;
        for f in files {
            if !relint.queue.contains(&f) {
                relint.queue.push_back(f);
            }
        }
        if std::mem::replace(&mut relint.running, true) {
            return;
        }
    }
    lint_thread(move || {
        let current = || shared.generation.load(Ordering::SeqCst) == generation;
        // A config change that matters rescans, which ends this batch.
        let linters = shared.relint.lock().expect("lock").linters.clone();
        let runs = Runs::background(linters.all().map(|l| &**l));
        loop {
            let path = {
                let mut relint = shared.relint.lock().expect("lock");
                match relint.queue.pop_front() {
                    // A newer scan lints everything anyway.
                    Some(path) if current() => path,
                    _ => {
                        relint.queue.clear();
                        relint.running = false;
                        break;
                    }
                }
            };
            let Some(linter) = linters.for_path(&path).filter(|l| l.wants(&path)) else { continue };
            let Some(run) = runs.of(linter) else { continue };
            publish_disk(linter, run, None, &shared, &out, &path, current);
            crate::alloc::trim();
        }
        runs.finish();
    });
}

/// Lints a file the editor owns on the editor worker, replacing a queued
/// lint of the same file. Only the latest text's result is published.
pub fn edit(shared: &Arc<Shared>, out: &Outbox, lint: EditorLint) {
    {
        let mut editing = shared.editing.lock().expect("lock");
        match editing.queue.iter_mut().find(|queued| queued.path == lint.path) {
            Some(queued) => *queued = lint,
            None => editing.queue.push(lint),
        }
        if std::mem::replace(&mut editing.running, true) {
            return;
        }
    }
    let (shared, out) = (shared.clone(), out.clone());
    lint_thread(move || loop {
        let (batch, mut carried) = {
            let mut editing = shared.editing.lock().expect("lock");
            if editing.queue.is_empty() || shared.closing.load(Ordering::SeqCst) {
                editing.queue.clear();
                editing.running = false;
                return;
            }
            (std::mem::take(&mut editing.queue), std::mem::take(&mut editing.carried))
        };
        let runs = Runs::editor(batch.iter().map(|lint| &*lint.linter), &mut carried);
        for lint in &batch {
            if shared.closing.load(Ordering::SeqCst) {
                break;
            }
            let Some(run) = runs.of(&lint.linter) else { continue };
            let disk;
            let text = match &lint.text {
                Some(text) => Some(text.as_str()),
                None if lint.linter.wants(&lint.path) => {
                    disk = std::fs::read_to_string(&lint.path).ok();
                    disk.as_deref()
                }
                None => None,
            };
            let diags = match text {
                Some(text) => convert::diagnostics(text, &shared.lint(run, &lint.path, text), &lint.linter),
                None => json!([]),
            };
            // A newer text is queued, or the editor let go of the file.
            let newer = shared.editing.lock().expect("lock").queue.iter().any(|queued| queued.path == lint.path);
            if !newer && shared.editor.lock().expect("lock").contains(&lint.path) {
                shared.publish(&out, &lint.uri, lint.version, diags);
            }
            crate::alloc::trim();
        }
        // Runs of linters a reload replaced have nobody left to serve.
        carried.retain(|(linter, _)| Arc::strong_count(linter) > 1);
        for (linter, run) in runs.0 {
            if let Some(lint) = batch.iter().find(|lint| std::ptr::eq(&*lint.linter, linter)) {
                carried.push((lint.linter.clone(), run.carried()));
            }
        }
        shared.editing.lock().expect("lock").carried = carried;
    });
}

/// Lints a file on disk (from `cache` when it can) and publishes the
/// result, unless the editor owns the file. Returns the problem count.
fn publish_disk(linter: &Linter, run: &LintRun<'_>, cache: Option<&Cache>, shared: &Shared, out: &Outbox, path: &Path, current: impl Fn() -> bool) -> Option<usize> {
    let open = || shared.editor.lock().expect("lock").contains(path);
    if open() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let rel = linter.relative(path).to_string_lossy().replace('\\', "/");
    let hit = cache.and_then(|c| c.lookup(linter, &rel, &text).zip(c.saved_reads(&rel)));
    let diags = match hit {
        Some((diags, reads)) => {
            shared.reads.lock().expect("lock").insert(path.to_path_buf(), Arc::new(reads));
            diags
        }
        None => {
            let (diags, reads) = run.lint_tracked(path, &text);
            let reads = Arc::new(reads);
            if let Some(cache) = cache {
                cache.store(&rel, &text, &diags, reads.clone());
            }
            shared.reads.lock().expect("lock").insert(path.to_path_buf(), reads);
            diags
        }
    };
    // The editor may have opened the file while it was being linted; then
    // the buffer's diagnostics win.
    if current() && !open() {
        shared.publish(out, &uri::from_path(path), None, convert::diagnostics(&text, &diags, linter));
    }
    Some(diags.len())
}

/// Walks `roots` like the CLI does: `.gitignore` respected, hidden files
/// included, `.git` skipped, and directories `keep_dir` rejects skipped.
/// Roots inside other roots are dropped.
pub fn walker(roots: &[PathBuf], keep_dir: impl Fn(&Path) -> bool + Send + Sync + 'static) -> Option<WalkBuilder> {
    let mut sorted: Vec<&PathBuf> = roots.iter().collect();
    sorted.sort();
    let mut outer: Vec<&PathBuf> = Vec::new();
    for root in sorted {
        if !outer.iter().any(|o| root.starts_with(o)) {
            outer.push(root);
        }
    }
    let (first, rest) = outer.split_first()?;
    let mut walker = WalkBuilder::new(first);
    for root in rest {
        walker.add(root);
    }
    walker.hidden(false).filter_entry(move |e: &DirEntry| {
        e.file_name() != ".git" && (!e.file_type().is_some_and(|t| t.is_dir()) || keep_dir(e.path()))
    });
    Some(walker)
}

/// Whether a walk of the workspace would reach `path`, ie it is not
/// gitignored (or otherwise ignored). Only `path`'s ancestors are listed, and
/// an ignored ancestor stops the walk early. Paths outside every root count
/// as reached, and a path not on disk yet (an unsaved buffer) is judged by
/// its directory.
pub fn walk_reaches(roots: &[PathBuf], path: &Path) -> bool {
    let Some(root) = roots.iter().filter(|r| path.starts_with(r)).max_by_key(|r| r.components().count()) else {
        return true;
    };
    if !path.exists() {
        return path.parent().is_none_or(|dir| dir == root.as_path() || walk_reaches(roots, dir));
    }
    let target = path.to_path_buf();
    let Some(walk) = walker(std::slice::from_ref(root), move |dir| target.starts_with(dir)) else { return true };
    walk.build().flatten().any(|e| e.path() == path)
}

/// Where a scan walks: each workspace root that a config covers, else the
/// config directories inside it. Nothing outside a config gets linted.
fn scan_roots(roots: &[PathBuf], linters: &Linters) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        if linters.covers(root) {
            out.push(root.clone());
        } else {
            out.extend(linters.config_dirs().filter(|d| d.starts_with(root)).cloned());
        }
    }
    out
}

/// Directories the nearest config ignores hold nothing to lint, unless a
/// nested config lives below them.
fn worth_walking(linters: &Linters, dir: &Path) -> bool {
    match linters.for_dir(dir) {
        Some(l) if l.is_ignored(dir) => linters.config_dirs().any(|c| c.starts_with(dir)),
        _ => true,
    }
}

/// Scans `roots`, linting `tabs` first, then every other file breadth-first,
/// each with its nearest config.
pub fn start(linters: Linters, roots: Vec<PathBuf>, tabs: Vec<PathBuf>, shared: Arc<Shared>, out: Outbox, progress: bool) {
    let generation = shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
    *shared.priority.lock().expect("lock") = Priority { generation, queue: tabs.into() };
    lint_thread(move || Scan::new(linters, shared, out, progress, generation).run(&roots));
}

/// A thread that lints: worker-sized stack, as deep questions need, and an
/// owned pool, which the next lint thread takes over: one batch's free
/// blocks serve the next instead of each thread spreading over a new shard.
fn lint_thread(work: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().stack_size(crate::engine::WORKER_STACK).spawn(move || {
        let _pool = crate::alloc::own_pool();
        work()
    }).expect("lint thread");
}

struct Scan {
    linters: Linters,
    shared: Arc<Shared>,
    out: Outbox,
    progress: bool,
    generation: u64,
    token: String,
    /// Files already linted (or skipped) by this scan.
    done: Mutex<HashSet<PathBuf>>,
    /// URIs this scan published, for the stale sweep.
    touched: Mutex<HashSet<String>>,
    files: AtomicUsize,
    problems: AtomicUsize,
    /// Known once the walk is done.
    total: AtomicUsize,
    /// Each config's `.dxcache`, by config directory.
    caches: HashMap<PathBuf, Cache>,
}

impl Scan {
    fn new(linters: Linters, shared: Arc<Shared>, out: Outbox, progress: bool, generation: u64) -> Scan {
        let caches = linters.all().map(|l| (l.root.clone(), Cache::open(l))).collect();
        Scan {
            linters,
            shared,
            out,
            progress,
            generation,
            token: format!("dxlint-scan-{generation}"),
            done: Mutex::default(),
            touched: Mutex::default(),
            files: AtomicUsize::new(0),
            problems: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            caches,
        }
    }

    fn current(&self) -> bool {
        self.shared.generation.load(Ordering::SeqCst) == self.generation
    }

    fn run(self, roots: &[PathBuf]) {
        let (token, generation) = (&self.token, self.generation);
        if self.progress {
            self.out.request(&format!("progress-{generation}"), "window/workDoneProgress/create", json!({ "token": token }));
            let value = json!({ "kind": "begin", "title": "dxlint", "message": "scanning workspace", "cancellable": false, "percentage": 0 });
            self.out.notify("$/progress", json!({ "token": token, "value": value }));
        }
        // With no configs there is nothing to lint; the stale sweep below
        // still clears what earlier configs reported.
        let runs = Runs::background(self.linters.all().map(|l| &**l));
        if !self.linters.is_empty() {
            self.drain(&[], &runs);
            let files = self.collect(roots);
            self.total.store(files.len(), Ordering::Relaxed);
            self.drain(&files, &runs);
        }
        runs.finish();
        let finished = self.current();
        if finished {
            self.sweep();
        }
        if self.progress {
            let message = format!("{} files, {} problems", self.files.load(Ordering::Relaxed), self.problems.load(Ordering::Relaxed));
            self.out.notify("$/progress", json!({ "token": token, "value": { "kind": "end", "message": message } }));
        }
        // Only a whole scan's results are worth keeping. Files the editor
        // owns were skipped, so nothing is pruned.
        if finished {
            for cache in self.caches.into_values() {
                let _ = cache.save(false);
            }
        }
    }

    /// Every file some config wants, shallowest first.
    fn collect(&self, roots: &[PathBuf]) -> Vec<PathBuf> {
        let linters = self.linters.clone();
        let Some(walk) = walker(&scan_roots(roots, &self.linters), move |dir| worth_walking(&linters, dir)) else {
            return Vec::new();
        };
        let found = Mutex::new(Vec::new());
        walk.build_parallel().run(|| {
            Box::new(|entry| {
                if !self.current() {
                    return WalkState::Quit;
                }
                let Ok(entry) = entry else { return WalkState::Continue };
                let path = entry.path();
                if entry.file_type().is_some_and(|t| t.is_file()) && self.linters.for_path(path).is_some_and(|l| l.wants(path)) {
                    found.lock().expect("lock").push((entry.depth(), path.to_path_buf()));
                }
                WalkState::Continue
            })
        });
        let mut found = found.into_inner().expect("lock");
        found.sort();
        found.into_iter().map(|(_, path)| path).collect()
    }

    /// Lints priority files, then `files` in order, one at a time: the
    /// editor and the rest of the machine stay responsive, and the server
    /// never holds more than one file's lint (`.dxcache` makes most scans
    /// quick anyway).
    fn drain(&self, files: &[PathBuf], runs: &Runs<'_>) {
        let mut next = files.iter();
        while self.current() {
            let Some(path) = self.shared.next_priority(self.generation).or_else(|| next.next().cloned()) else { break };
            self.lint(&path, runs);
            crate::alloc::trim();
        }
    }

    fn lint(&self, path: &Path, runs: &Runs<'_>) {
        if !self.done.lock().expect("lock").insert(path.to_path_buf()) {
            return;
        }
        let Some(linter) = self.linters.for_path(path).filter(|l| l.wants(path)) else { return };
        let Some(run) = runs.of(linter) else { return };
        let cache = self.caches.get(&linter.root);
        let Some(problems) = publish_disk(linter, run, cache, &self.shared, &self.out, path, || self.current()) else { return };
        self.problems.fetch_add(problems, Ordering::Relaxed);
        self.touched.lock().expect("lock").insert(uri::from_path(path));
        let n = self.files.fetch_add(1, Ordering::Relaxed) + 1;
        if self.progress && n.is_multiple_of(100) {
            let total = self.total.load(Ordering::Relaxed);
            let value = if total == 0 {
                json!({ "kind": "report", "message": format!("{n} files") })
            } else {
                json!({ "kind": "report", "message": format!("{n}/{total} files"), "percentage": (n * 100 / total).min(100) })
            };
            self.out.notify("$/progress", json!({ "token": self.token, "value": value }));
        }
    }

    /// Files that showed problems before but were not seen now (deleted, or
    /// no longer owned by any language) get cleared.
    fn sweep(&self) {
        let touched = self.touched.lock().expect("lock");
        // By path: the editor's URIs for open files are spelled its own way.
        let editor = self.shared.editor.lock().expect("lock").clone();
        let owned = |u: &str| uri::to_path(u).is_some_and(|p| editor.contains(&p));
        let stale: Vec<String> =
            self.shared.published.lock().expect("lock").iter().filter(|u| !touched.contains(*u) && !owned(u)).cloned().collect();
        for u in stale {
            self.shared.publish(&self.out, &u, None, json!([]));
        }
    }
}

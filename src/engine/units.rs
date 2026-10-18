use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::grammar::Tree;

use super::{reads, Linter, Profile};

pub(super) struct ParsedFile {
    pub tree: Tree,
    pub front: Option<usize>,
    pub state: Option<Arc<dyn Any + Send + Sync>>,
}

pub(super) struct ParsePlan<'a> {
    pub lang: usize,
    pub front: Option<usize>,
    pub layers: bool,
    pub overlays: &'a [bool],
    /// Child layers are built only under nodes where `overlays` wants one.
    pub child_layers: bool,
    pub tokens: &'a [bool],
}

pub(super) struct Unit {
    pub path: PathBuf,
    pub rel: String,
    pub lang: usize,
    pub text: Arc<str>,
    pub parsed: ParsedFile,
    pub names: OnceLock<reads::Cached<Arc<crate::grammar::names::Resolved>>>,
    lines: OnceLock<crate::line_index::LineIndex>,
}

impl Unit {
    /// Line starts, built once on first use and shared by every world.
    pub fn lines(&self) -> &crate::line_index::LineIndex {
        self.lines.get_or_init(|| crate::line_index::LineIndex::new(&self.text))
    }
}

impl Linter {
    pub(super) fn parse(&self, path: &Path, text: &str, plan: ParsePlan<'_>) -> Result<ParsedFile, (u32, String)> {
        let started = std::time::Instant::now();
        let (tree, state, native_done, tokens_done) = match plan.front {
            Some(index) => {
                let front = &self.grammar.natives[index];
                let mut parsed = front.frontend.parse(text, path)?;
                let native_done = std::time::Instant::now();
                if plan.tokens.iter().any(|&wanted| wanted) {
                    front.frontend.attach_tokens(text, &mut parsed, plan.tokens);
                }
                let tokens_done = std::time::Instant::now();
                let nodes = front.remap(parsed.nodes);
                let tree = if plan.layers {
                    Tree::graft(&self.grammar, plan.lang, text, nodes, parsed.root, Some(plan.overlays), plan.child_layers, self.profile.as_ref())
                } else {
                    Tree::from_native(nodes, parsed.root)
                };
                (tree, parsed.state, native_done, tokens_done)
            }
            None => {
                let tree = Tree::parse_with(&self.grammar, plan.lang, text, Some(plan.overlays), self.profile.as_ref());
                (tree, None, started, started)
            }
        };
        let done = std::time::Instant::now();
        if let Some(profile) = &self.profile {
            Profile::add_time(&profile.parse_nanos, done - started);
            Profile::add_time(&profile.native_nanos, native_done - started);
            Profile::add_time(&profile.token_nanos, tokens_done - native_done);
            Profile::add_time(&profile.layer_nanos, done - tokens_done);
        }
        Ok(ParsedFile { tree, front: plan.front, state })
    }

    /// The parsed unit for `path`. Inside a run, a unit is read and validated
    /// once; later lookups replay its text read without touching the disk.
    pub(super) fn unit(&self, path: &Path, run: Option<&super::program::Snapshot>) -> Option<Arc<Unit>> {
        let revision = self.invalidation.load(std::sync::atomic::Ordering::Acquire);
        // Inside a run the text comes from the run's read of it, which name
        // candidate scans may already have made.
        let (hash, unit) = match run {
            Some(run) => run.unit(revision, path, || self.load_unit(path, run.text(revision, path))),
            None => self.load_unit(path, reads::read_unrecorded(path)),
        };
        reads::note(reads::Dep::Text(path.to_owned(), hash));
        unit
    }

    /// Loads the run's units for `paths` on several threads, so a following
    /// sequential pass (which records the reads) finds them ready. Each
    /// worker drains its share of the loaded units through `worker`.
    pub(super) fn prefetch_units(&self, paths: &[PathBuf], run: &super::program::Snapshot,
        worker: impl Fn(&mut dyn Iterator<Item = Arc<Unit>>) + Sync) {
        let workers = self.rule_jobs.min(paths.len());
        if workers <= 1 { return }
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    let _pool = crate::alloc::own_pool();
                    worker(&mut std::iter::from_fn(|| loop {
                        let path = paths.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))?;
                        if let Some(unit) = self.unit(path, Some(run)) { return Some(unit) }
                    }))
                });
            }
        });
    }

    /// `path`'s unit from its read `(text, hash)` ([`reads::MISSING`] when
    /// unreadable), reusing the previous run's unit while it still holds.
    fn load_unit(&self, path: &Path, (text, hash): (Option<Arc<str>>, u64)) -> (u64, Option<Arc<Unit>>) {
        let Some(text) = text else { return (hash, None) };
        let cached = self.units.lock().expect("unit cache").get(path).cloned();
        if let Some(unit) = cached.filter(|unit| unit.text == text
            && unit.names.get().is_none_or(|names| !names.reads.any(&mut |dep| !self.holds(dep)))) {
            return (hash, Some(unit));
        }
        let parse = || -> Option<Arc<Unit>> {
            let _owner = crate::alloc::heap::Owner::enter(crate::alloc::heap::UNITS);
            let rel = self.relative(path);
            let lang = self.grammar.lang_for(&rel)?;
            let front = self.grammar.natives.iter().position(|front| front.frontend.accepts(path));
            let tokens = front.and_then(|index| self.unit_token_kinds.get(index)).map_or(&[][..], Vec::as_slice);
            let plan = ParsePlan { lang, front, layers: true, overlays: &self.unit_overlays, child_layers: self.unit_child_layers, tokens };
            let parsed = self.parse(path, &text, plan).ok()?;
            let unit = Arc::new(Unit { path: path.to_owned(), rel: rel.to_string_lossy().replace('\\', "/"), lang, text, parsed, names: OnceLock::new(), lines: OnceLock::new() });
            self.units.lock().expect("unit cache").insert(path.to_owned(), unit.clone());
            Some(unit)
        };
        let unit = match &self.profile {
            Some(profile) => profile.time_unit_parse(parse),
            None => parse(),
        };
        (hash, unit)
    }
}

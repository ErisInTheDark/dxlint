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
    pub tokens: &'a [bool],
}

pub(super) struct Unit {
    pub path: PathBuf,
    pub rel: String,
    pub lang: usize,
    pub text: Arc<str>,
    pub hash: u64,
    pub parsed: ParsedFile,
    pub names: OnceLock<reads::Cached<Arc<crate::grammar::names::Resolved>>>,
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
                    Tree::graft(&self.grammar, plan.lang, text, nodes, parsed.root, Some(plan.overlays), self.profile.as_ref())
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
        if let Some(unit) = run.and_then(|run| run.unit(revision, path)) {
            reads::note(reads::Dep::Text(path.to_owned(), unit.hash));
            return Some(unit);
        }
        let (text, hash) = reads::read_unrecorded(path);
        reads::note(reads::Dep::Text(path.to_owned(), hash));
        let text = text?;
        let cached = self.units.lock().expect("unit cache").get(path).cloned();
        let unit = match cached.filter(|unit| unit.text == text
            && unit.names.get().is_none_or(|names| !names.reads.any(&mut |dep| !self.holds(dep)))) {
            Some(unit) => unit,
            None => {
                let rel = self.relative(path);
                let lang = self.grammar.lang_for(&rel)?;
                let front = self.grammar.natives.iter().position(|front| front.frontend.accepts(path));
                let tokens = front.map(|index| {
                    let frontend = self.grammar.natives[index].frontend;
                    (0..frontend.kinds().len()).map(|kind| self.unit_tokens && frontend.is_token(kind as u16)).collect::<Vec<_>>()
                }).unwrap_or_default();
                let parsed = self.parse(path, &text, ParsePlan { lang, front, layers: true, overlays: &self.unit_overlays, tokens: &tokens }).ok()?;
                let unit = Arc::new(Unit { path: path.to_owned(), rel: rel.to_string_lossy().replace('\\', "/"), lang, text, hash, parsed, names: OnceLock::new() });
                self.units.lock().expect("unit cache").insert(path.to_owned(), unit.clone());
                unit
            }
        };
        if let Some(run) = run { run.keep_unit(revision, path, &unit); }
        Some(unit)
    }
}

//! The files behind type questions (docs/design/type-layers.md "World:
//! files behind questions"): the one engine owner of trees, texts, names
//! and VM runs that type-layer engines see through [`World`].
//!
//! File 0 is the linted file. Other files load on module lookup, with
//! shared parse data and names, with per-lint facts and VM state.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::expr::{Ctx, Value};
use crate::expr::queries::Uncertainty;
use crate::grammar::names::question::{FrameId, Known, Leaf, NodeRef, ScopeRef, Target, World};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::modules::compile::Action;
use crate::grammar::names::modules::SharedQuestion;
use crate::grammar::names::modules::resolution::{parse_metadata, program_inputs, Configuration, DiscoveryRequest, Host, Json, Resolver, Settings};
use crate::grammar::names::question::QueryContext;
use crate::grammar::names::{Resolved, ScopeIndex};
use crate::grammar::{Grammar, Tree};
use crate::vm::{Session, VExpr};

#[derive(Default)]
enum Selection<'a> {
    #[default]
    Vacant,
    Busy,
    Ready(super::reads::Cached<Option<&'a Settings>>),
}

enum ModuleAnswer {
    Busy(u64),
    Ready(u64, super::reads::Cached<Option<ScopeRef>>),
}

type ModuleAnswers = crate::fx_hash::FxHashMap<Box<str>, ModuleAnswer>;

#[derive(Default)]
struct ProgramMemo<'a> {
    selected: RefCell<Selection<'a>>,
    inputs: RefCell<crate::fx_hash::FxHashMap<*const Settings, super::reads::Cached<Arc<Vec<PathBuf>>>>>,
    roots: RefCell<crate::fx_hash::FxHashMap<(*const Settings, Box<str>), super::reads::Cached<Known<Vec<ScopeRef>>>>>,
    catalogs: RefCell<crate::fx_hash::FxHashMap<(*const Settings, u64), super::reads::Cached<Arc<super::program::Catalog>>>>,
    modules: RefCell<crate::fx_hash::FxHashMap<(u32, *const Settings), ModuleAnswers>>,
    /// The linted text's hash: every run-shared answer this world computes
    /// records it, since the linted file's own tree is not a run unit.
    linted_hash: std::cell::OnceCell<u64>,
    /// Whether the linted text is what the run reads for its path, so this
    /// world may use and add run-shared answers.
    shares: std::cell::OnceCell<bool>,
    /// Per program and scope classes, the run's augmentation index (`None`
    /// when this world cannot share it).
    augmentations: RefCell<crate::fx_hash::FxHashMap<(*const Settings, u64), Option<super::reads::Cached<Arc<super::program::Augmentations>>>>>,
    /// Per program settings, the linted file when it is not an input.
    outside: RefCell<crate::fx_hash::FxHashMap<*const Settings, (u64, Option<PathBuf>)>>,
}

/// One lint's world. Engines hold it as `&dyn World` through [`Files`].
pub struct LintWorld<'a> {
    linter: &'a super::Linter,
    path: &'a Path,
    views: super::arena::Arena<'a>,
    files: RefCell<crate::fx_hash::FxHashMap<PathBuf, u32>>,
    program: ProgramMemo<'a>,
    snapshot: Option<&'a super::program::Snapshot>,
    /// File 0 outside any rule (no locals, no indents).
    ctx: Ctx<'a>,
    evaluation: Cell<*const ()>,
    semantic: crate::grammar::names::semantic::Session,
    /// Name-item conditions being decided, by node and condition.
    conditions: RefCell<Vec<(Target, u32)>>,
}

impl<'a> LintWorld<'a> {
    fn revision(&self) -> u64 {
        self.linter.invalidation.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn new(linter: &'a super::Linter, path: &'a Path, ctx: Ctx<'a>) -> Self {
        LintWorld {
            linter, path, views: super::arena::Arena::new(linter), files: Default::default(),
            program: Default::default(), snapshot: None, ctx: Ctx { node: ctx.tree.root, ..ctx },
            evaluation: Cell::new(std::ptr::null()), semantic: Default::default(), conditions: Default::default(),
        }
    }

    pub(super) fn in_run(linter: &'a super::Linter, path: &'a Path, ctx: Ctx<'a>, snapshot: &'a super::program::Snapshot) -> Self {
        Self { snapshot: Some(snapshot), ..Self::new(linter, path, ctx) }
    }

    fn file(&'a self, path: &Path) -> Option<u32> {
        if path == self.path { return Some(0); }
        if let Some(&file) = self.files.borrow().get(path) { return Some(file); }
        let load = || self.linter.unit(path, self.snapshot).map(|unit| self.views.insert(unit) + 1);
        let file = match &self.linter.profile {
            Some(profile) => profile.time_semantic_query("units.view", load),
            None => load(),
        }?;
        self.files.borrow_mut().insert(path.to_owned(), file);
        Some(file)
    }

    pub(crate) fn name_cache(&'a self, file: u32) -> Option<&'a std::sync::OnceLock<super::reads::Cached<std::sync::Arc<Resolved>>>> {
        Some(&self.views.get(file.checked_sub(1)?)?.unit.names)
    }
    /// File 0's context on `node`, able to reach this world.
    fn on(&'a self, file: u32, node: u32) -> Option<Ctx<'a>> {
        if file == 0 {
            self.ctx.tree.nodes.get(node as usize)?;
            return Some(Ctx { node, world: Some(self), ..self.ctx });
        }
        let view = self.views.with_native(file.checked_sub(1)?)?;
        let unit = &view.unit;
        unit.parsed.tree.nodes.get(node as usize)?;
        let ctx = self.linter.tree_ctx(unit.lang, &unit.parsed.tree, (&unit.text, unit.lines(), &unit.rel), &view.memo, &view.reindented, view.native());
        Some(Ctx { file_id: file, node, world: Some(self), ..ctx })
    }

    /// Runs `f` with `semantic` as the question in progress, so queries
    /// asked from the conditions `f` runs nest in it.
    fn asking<R>(&self, semantic: &Semantic<'_>, at: Leaf, f: impl FnOnce() -> Known<R>) -> Known<R> {
        let Some(_nested) = semantic.question.budget.nest() else { return Known::Unknown };
        self.evaluating(Some(ActiveQuestion { semantic, at }), Uncertainty::Strict, f)
    }

    fn evaluating<R>(&self, question: Option<ActiveQuestion<'_, '_>>, uncertainty: Uncertainty, f: impl FnOnce() -> Known<R>) -> Known<R> {
        let evaluation = ActiveEvaluation { question, unknown: Cell::new(false) };
        let before = self.evaluation.replace((&evaluation as *const ActiveEvaluation<'_, '_>).cast());
        let restore = Restore { slot: &self.evaluation, before };
        let result = f();
        drop(restore);
        if (uncertainty == Uncertainty::Strict && evaluation.unknown.get()) || matches!(result, Known::Unknown) {
            self.mark_unknown();
            Known::Unknown
        } else { result }
    }

    pub(crate) fn complete_attribute<R>(&self, f: impl FnOnce() -> R) -> Known<R> {
        let revision = self.revision();
        let question = self.question().map(|question| ActiveQuestion { semantic: question.semantic, at: question.at });
        let answer = self.evaluating(question, Uncertainty::Strict, || Known::Yes(f()));
        if self.revision() != revision {
            self.mark_unknown();
            Known::Unknown
        } else { answer }
    }

    pub(crate) fn cache_attribute(&self, memo: &crate::vm::FileVm) -> bool {
        self.question().is_none() && memo.sync_revision(self.revision(), self.active_evaluation().is_none())
    }

    fn mark_unknown(&self) {
        if let Some(evaluation) = self.active_evaluation() { evaluation.unknown.set(true); }
    }

    fn active_evaluation(&self) -> Option<&ActiveEvaluation<'_, '_>> {
        let at = self.evaluation.get();
        // SAFETY: `evaluating` restores the slot before its stack guard or
        // borrowed semantic can go away. Reads stay inside that call.
        (!at.is_null()).then(|| unsafe { &*at.cast::<ActiveEvaluation<'_, '_>>() })
    }

    fn question(&self) -> Option<&ActiveQuestion<'_, '_>> {
        self.active_evaluation()?.question.as_ref()
    }

    fn semantic_query<R>(&'a self, file: u32, node: u32, uncertainty: Uncertainty, run: impl FnOnce(&Semantic<'_>, Leaf) -> Known<R>) -> Known<R> {
        let target = Target::Node(NodeRef { file, node });
        if let Some(question) = self.question() {
            let frame = match question.at.target {
                Target::Node(at) if at.file == file => question.at.frame,
                Target::Scope(at) if at.file == file => question.at.frame,
                _ => FrameId::ROOT,
            };
            let target = match question.at.target {
                Target::Scope(scope) if scope.file == file && Files(self).names(file)
                    .and_then(|names| names.scopes.get(scope.scope as usize)).is_some_and(|scope| scope.node == node) => question.at.target,
                _ => target,
            };
            let ask = || match question.semantic.leaf(Leaf { target, frame }) {
                Known::Yes(at) => run(question.semantic, at),
                Known::Unknown => Known::Unknown,
            };
            let answer = match uncertainty {
                Uncertainty::Strict => ask(),
                Uncertainty::Explicit => self.evaluating(
                    Some(ActiveQuestion { semantic: question.semantic, at: Leaf { target, frame } }),
                    uncertainty, ask,
                ),
            };
            return match answer {
                Known::Yes(answer) => Known::Yes(answer),
                Known::Unknown => { self.mark_unknown(); Known::Unknown }
            };
        }
        let files = Files(self);
        let at = Leaf { target, frame: FrameId::ROOT };
        let answer = self.semantic.run(&files, &self.ctx.grammar.names, |semantic| {
            let Known::Yes(at) = semantic.leaf(at) else { return Known::Unknown };
            let Some(_nested) = semantic.question.budget.nest() else { return Known::Unknown };
            self.evaluating(Some(ActiveQuestion { semantic, at }), uncertainty, || run(semantic, at))
        });
        if matches!(answer, Known::Unknown) { self.mark_unknown(); }
        answer
    }

    pub fn query(&'a self, file: u32, node: u32, subject: &crate::grammar::names::Value, query: &crate::expr::queries::Query, mask: u64) -> bool {
        // could/always quantify over alternatives and answer Unknown themselves
        // unless a known alternative decides, so their known answers stand.
        let run = || self.semantic_query(file, node, Uncertainty::Explicit, |semantic, at| (query.call)(semantic, at, subject, mask));
        let answer = match self.ctx.profile {
            Some(profile) => profile.time_semantic_query(query.name, run),
            None => run(),
        };
        matches!(answer, Known::Yes(true))
    }

    pub fn query_function(&'a self, file: u32, node: u32, query: &crate::expr::queries::QueryFunction, args: &[Value<'static>]) -> Value<'static> {
        let run = || self.semantic_query(file, node, query.uncertainty(), |semantic, at| (query.call)(semantic, at, args));
        let answer = match self.ctx.profile {
            Some(profile) => profile.time_semantic_query(query.name, run),
            None => run(),
        };
        answer.known().unwrap_or(Value::Null)
    }

    pub fn query_handle(&'a self, file: u32, handle: crate::grammar::names::question::Handle, query: &crate::expr::queries::HandleFunction, args: &[Value<'static>]) -> Value<'static> {
        let run = || self.semantic_query(file, 0, Uncertainty::Explicit, |semantic, _| match semantic.question.handle_value(handle) {
            Some(value) => (query.call)(semantic, value, args),
            None => Known::Unknown,
        });
        let answer = match self.ctx.profile {
            Some(profile) => profile.time_semantic_query(query.name, run),
            None => run(),
        };
        answer.known().unwrap_or(Value::Null)
    }

    fn here(&'a self, at: Leaf) -> Option<Ctx<'a>> {
        match at.target {
            Target::Node(node) => self.on(node.file, node.node),
            Target::Scope(scope) => {
                let node = World::names(&Files(self), scope.file)?.scopes.get(scope.scope as usize)?.node;
                self.on(scope.file, node)
            }
            _ => None,
        }
    }

    fn key<'s>(&'a self, file: u32, text: &'s str) -> Cow<'s, str> {
        match self.on(file, 0).and_then(|ctx| ctx.native) {
            Some(native) => native.name_key(text),
            None => Cow::Borrowed(text),
        }
    }
}

struct ActiveQuestion<'s, 'a> {
    semantic: &'s Semantic<'a>,
    at: Leaf,
}

struct ActiveEvaluation<'s, 'a> {
    question: Option<ActiveQuestion<'s, 'a>>,
    unknown: Cell<bool>,
}

struct Restore<'c> {
    slot: &'c Cell<*const ()>,
    before: *const (),
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        self.slot.set(self.before);
    }
}

/// [`World`] over a lint's files, borrowed for the whole lint (conditions
/// rebuild contexts that point back at it).
#[derive(Clone, Copy)]
pub struct Files<'a>(pub &'a LintWorld<'a>);

struct NamedModule {
    scope: ScopeRef,
    name: String,
    source: Option<String>,
    classes: u64,
}

impl<'a> Files<'a> {
    fn file_leaf(self, file: u32) -> Option<Leaf> {
        let tree = World::tree(&self, file)?;
        let node = World::names(&self, file)?.scopes.first().map(|scope| scope.node)
            .or_else(|| tree.order.iter().copied().find(|&node| {
                self.0.ctx.grammar.names.kinds.get(tree.node(node).matcher as usize)
                    .is_some_and(|kind| kind.modules.items.iter().any(|item| matches!(item.action, Action::Uses(_))))
            })).unwrap_or(tree.root);
        Some(Leaf { target: Target::Node(NodeRef { file, node }), frame: FrameId::ROOT })
    }

    fn selected(self, at: Leaf, semantic: &Semantic<'_>) -> Known<Option<&'a Settings>> {
        let Some(ctx) = self.0.here(at) else { return Known::Unknown };
        let mut node = Some(ctx.node);
        while let Some(id) = node {
            if !semantic.question.budget.step() { return Known::Unknown; }
            let leaf = Leaf { target: Target::Node(NodeRef { file: ctx.file_id, node: id }), ..at };
            if let Some(kind) = ctx.grammar.names.kinds.get(ctx.tree.node(id).matcher as usize) {
                for item in &kind.modules.items {
                    let Action::Uses(name) = &item.action else { continue };
                    match item.when.map_or(Known::Yes(true), |when| self.condition(leaf, when, semantic)) {
                        Known::Yes(false) => continue,
                        Known::Unknown => return Known::Unknown,
                        Known::Yes(true) => {}
                    }
                    let Some(profile) = ctx.grammar.names.module_resolutions.iter().find(|profile| profile.name == *name) else { return Known::Unknown };
                    let mut settings = &profile.settings;
                    if !profile.modes.is_empty() {
                        let Known::Yes(context) = (Resolver { host: &self, settings }).context(ctx.file_id, semantic.question) else { return Known::Unknown };
                        let ctx = Ctx { node: id, ..ctx };
                        let session = Session::new(&ctx, id, &[]);
                        for mode in &profile.modes {
                            if !semantic.question.budget.step() { return Known::Unknown; }
                            let Some(expression) = ctx.grammar.names.operands.get(mode.when as usize).and_then(|operand| operand.lowered.get()) else { return Known::Unknown };
                            match self.0.asking(semantic, leaf, || session.holds_with(expression, &context).map_or(Known::Unknown, Known::Yes)) {
                                Known::Yes(true) => { settings = &mode.settings; break; }
                                Known::Yes(false) => {}
                                Known::Unknown => return Known::Unknown,
                            }
                        }
                    }
                    return Known::Yes(Some(settings));
                }
            }
            node = ctx.tree.parent(id);
        }
        Known::Yes(None)
    }

    fn program_selected(self) -> Known<Option<&'a Settings>> {
        {
            let mut selected = self.0.program.selected.borrow_mut();
            match &*selected {
                Selection::Ready(cached) => return Known::Yes(cached.get()),
                Selection::Busy => return Known::Unknown,
                Selection::Vacant => *selected = Selection::Busy,
            }
        }
        let cached = super::reads::Cached::compute(|| {
            let question = QueryContext::default();
            let semantic = Semantic::new(&self, &self.0.ctx.grammar.names, &question);
            let selected = (|| {
                let Known::Yes(program) = Host::program(&self) else { return Known::Unknown };
                let Some(at) = self.file_leaf(program) else { return Known::Unknown };
                self.selected(at, &semantic)
            })();
            if question.budget.limited() {
                if let Some(profile) = self.0.ctx.profile {
                    super::Profile::add(&profile.semantic_budget_limits, 1);
                }
            }
            selected
        });
        let answer = cached.get();
        let next = match &answer {
            Known::Yes(selected) => Selection::Ready(super::reads::Cached { value: *selected, reads: cached.reads }),
            Known::Unknown => Selection::Vacant,
        };
        self.0.program.selected.replace(next);
        answer
    }

    fn program_context(self, selected: &Settings) -> super::program::Context {
        super::program::Context {
            directory: self.0.path.parent().unwrap_or(Path::new("")).to_owned(),
            settings: selected as *const Settings as usize,
        }
    }

    fn input_paths(self, selected: &Settings) -> Known<Arc<Vec<PathBuf>>> {
        let key = selected as *const Settings;
        if let Some(cached) = self.0.program.inputs.borrow().get(&key) { return Known::Yes(cached.get()) }
        let cached = super::reads::Cached::compute(|| {
            let context = self.program_context(selected);
            let load = || self.0.linter.programs.paths(context.clone(), |dep| self.0.linter.holds(dep), || {
                let question = QueryContext::default();
                let answer = program_inputs(selected, &self, &question);
                if question.budget.limited() {
                    if let Some(profile) = self.0.ctx.profile {
                        super::Profile::add(&profile.semantic_budget_limits, 1);
                    }
                }
                answer
            });
            match self.0.snapshot {
                Some(snapshot) => snapshot.paths(self.0.revision(), context.clone(), load),
                None => load(),
            }
        });
        let answer = cached.get();
        if let Known::Yes(paths) = &answer {
            self.0.program.inputs.borrow_mut().insert(key, super::reads::Cached { value: paths.clone(), reads: cached.reads });
        }
        answer
    }

    /// `path`'s text, recorded: the linted text for the linted file.
    fn source_text(self, path: &Path) -> Option<Arc<str>> {
        if path == self.0.path {
            super::reads::note(super::reads::Dep::Text(path.to_owned(), self.linted_hash()));
            return Some(self.0.ctx.source.into());
        }
        let (text, hash) = match self.0.snapshot {
            Some(snapshot) => snapshot.text(self.0.revision(), path),
            None => super::reads::read_unrecorded(path),
        };
        super::reads::note(super::reads::Dep::Text(path.to_owned(), hash));
        text
    }

    /// The program's input paths whose text passes `keep` (all when the
    /// profile gives no hints), read but not parsed.
    fn candidate_inputs(self, paths: &[PathBuf], hinted: bool, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
        if !hinted { return paths.to_vec() }
        paths.iter().filter(|path| self.source_text(path).is_none_or(|text| keep(&text))).cloned().collect()
    }

    fn roots(self, selected: &Settings, name: &str) -> Known<Vec<ScopeRef>> {
        let hinted = !selected.globals.name_hints.is_empty();
        let key = (selected as *const Settings, if hinted { Box::from(name) } else { Box::from("") });
        if let Some(cached) = self.0.program.roots.borrow().get(&key) { return cached.get() }
        let cached = super::reads::Cached::compute(|| {
            let question = QueryContext::default();
            let roots = (|| {
                let Known::Yes(paths) = self.timed("modules.roots.inputs", || self.input_paths(selected)) else { return Known::Unknown };
                let paths: Arc<[PathBuf]> = match self.timed("modules.roots.candidates", || match (self.0.snapshot, hinted) {
                    (Some(snapshot), true) if self.shares(snapshot) => {
                        let Known::Yes((program, _)) = self.program_identity(selected) else { return Known::Unknown };
                        let settings = selected as *const Settings as usize;
                        let survey = self.timed("modules.roots.survey", || snapshot.survey((settings, program), || paths.iter().map(|path| (path.clone(), self.source_text(path))).collect()));
                        let shared = snapshot.candidates((settings, program, Box::from(name)), || {
                            let sources = survey.get();
                            let finder = memchr::memmem::Finder::new(name.as_bytes());
                            let hinted = |text: &str| finder.find(text.as_bytes()).is_some()
                                || selected.globals.name_hints.iter().any(|hint| text.contains(&**hint));
                            sources.iter().filter(|(_, text)| text.as_deref().is_none_or(hinted)).map(|(path, _)| path.clone()).collect()
                        });
                        Known::Yes(shared.get())
                    }
                    _ => Known::Yes(self.candidate_inputs(&paths, hinted, |text| selected.globals.may_publish(text, name)).into()),
                }) {
                    Known::Yes(paths) => paths,
                    Known::Unknown => return Known::Unknown,
                };
                self.timed("modules.roots.prefetch", || self.prefetch_inputs(selected, &paths));
                let attached = self.timed("modules.roots.attach", || {
                    let mut roots = Vec::new();
                    let mut seen = crate::fx_hash::FxHashSet::default();
                    for path in paths.iter() {
                        if !question.budget.step() { return None }
                        let file = self.0.file(path)?;
                        if seen.insert(file) { roots.push(ScopeRef { file, scope: 0 }); }
                    }
                    Some((roots, seen))
                });
                let Some((mut roots, mut seen)) = attached else { return Known::Unknown };
                let Known::Yes(program) = Host::program(&self) else { return Known::Unknown };
                if seen.insert(program) { roots.push(ScopeRef { file: program, scope: 0 }); }
                Known::Yes(roots)
            })();
            if question.budget.limited() {
                if let Some(profile) = self.0.ctx.profile {
                    super::Profile::add(&profile.semantic_budget_limits, 1);
                }
            }
            roots
        });
        let roots = cached.get();
        self.0.program.roots.borrow_mut().insert(key, cached);
        roots
    }

    /// Loads the program's input units once per run on all threads. Lint
    /// threads otherwise wait on one builder parsing them in sequence; its
    /// sequential pass after this records the reads.
    fn prefetch_inputs(self, selected: &Settings, paths: &[PathBuf]) {
        let (Some(snapshot), Known::Yes(_)) = (self.0.snapshot, self.program_identity(selected)) else { return };
        let mut hasher = crate::fx_hash::FxHasher::default();
        std::hash::Hash::hash(paths, &mut hasher);
        let linter = self.0.linter;
        snapshot.prefetch(std::hash::Hasher::finish(&hasher), || linter.prefetch_units(paths, snapshot, |units| units.for_each(drop)));
    }

    fn timed<T>(self, query: &'static str, run: impl FnOnce() -> T) -> T {
        match self.0.ctx.profile {
            Some(profile) => profile.time_semantic_query(query, run),
            None => run(),
        }
    }

    /// Every scope merged into module scope `scope` ([`World::module_scopes`]).
    fn merged_scopes(self, scope: ScopeRef, semantic: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
        let Some(names) = World::names(&self, scope.file) else { return Known::Unknown };
        let Some(info) = names.scopes.get(scope.scope as usize) else { return Known::Unknown };
        let selected = match self.program_selected() {
            Known::Yes(Some(selected)) => selected,
            Known::Yes(None) => return Known::Yes(vec![scope]),
            Known::Unknown => return Known::Unknown,
        };
        let classes = selected.ambient_scopes;
        if scope.scope != 0 && info.classes & classes == 0 { return Known::Yes(vec![scope]); }
        let Known::Yes(catalog) = self.timed("modules.scopes.catalog", || self.catalog(selected, classes, semantic)) else { return Known::Unknown };
        let Known::Yes(own) = self.timed("modules.scopes.named", || self.named_scope(scope, classes, semantic)) else { return Known::Unknown };
        let name = own.iter().find(|module| module.source.is_none()).map(|module| module.name.as_str());
        let mut merged = vec![scope];
        if name.is_none() && scope.file != 0 {
            if let Known::Yes(Some(index)) = self.timed("modules.scopes.augmentations", || self.augmentations(selected, classes, &catalog, semantic)) {
                let path = World::path(&self, scope.file).map(Path::to_owned);
                let found = path.and_then(|path| index.get(&(path, scope.scope)).cloned()).unwrap_or_default();
                let candidates: Option<Vec<ScopeRef>> = found.iter()
                    .map(|(path, scope)| self.0.file(path).filter(|&file| file != 0).map(|file| ScopeRef { file, scope: *scope }))
                    .collect();
                if let Some(candidates) = candidates {
                    for candidate in candidates {
                        if !merged.contains(&candidate) { merged.push(candidate); }
                    }
                    return Known::Yes(merged);
                }
            }
        }
        for declaration in catalog.candidates(name, classes) {
            let Known::Yes(candidate) = self.declaration_scope(declaration) else { return Known::Unknown };
            let same = match name {
                Some(_) => true,
                None => {
                    let at = Leaf { target: Target::Node(NodeRef { file: candidate.file, node: declaration.node }), frame: FrameId::ROOT };
                    match World::module(&self, at, declaration.source.as_deref().expect("source declaration"), semantic) {
                        Known::Yes(target) => target == Some(scope),
                        Known::Unknown => return Known::Unknown,
                    }
                }
            };
            if same && !merged.contains(&candidate) { merged.push(candidate); }
        }
        Known::Yes(merged)
    }

    /// The program's augmentation index (module scope -> augmenting scopes,
    /// by path), resolved once per run; `None` when it cannot be shared
    /// (an augmentation in or of the linted file, or no run).
    fn augmentations(self, selected: &Settings, classes: u64, catalog: &super::program::Catalog, semantic: &Semantic<'_>)
        -> Known<Option<Arc<super::program::Augmentations>>> {
        let key = (selected as *const Settings, classes);
        if let Some(known) = self.0.program.augmentations.borrow().get(&key) {
            return Known::Yes(known.as_ref().map(super::reads::Cached::get));
        }
        let Some(snapshot) = self.0.snapshot else { return Known::Yes(None) };
        let question = super::program::StableQuestion::Augmentations { classes };
        let (true, Some(run_key)) = (self.shares(snapshot), self.module_key(question)) else { return Known::Yes(None) };
        let shared = match snapshot.augmentations(&run_key) {
            Some(shared) => Some(shared),
            None => {
                let built = super::reads::Cached::compute(|| {
                    super::reads::note(super::reads::Dep::Text(self.0.path.to_owned(), self.linted_hash()));
                    self.build_augmentations(classes, catalog, semantic)
                });
                match built.value {
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(None) => None,
                    Known::Yes(Some(index)) => {
                        let shared = super::reads::Cached { value: Arc::new(index), reads: built.reads };
                        snapshot.keep_augmentations(run_key, shared.clone());
                        Some(shared)
                    }
                }
            }
        };
        self.0.program.augmentations.borrow_mut().insert(key, shared.clone());
        Known::Yes(shared.as_ref().map(super::reads::Cached::get))
    }

    /// Resolves every augmentation once; `None` when one is in or of the
    /// linted file (its scope ids are the lint tree's, not the run unit's).
    fn build_augmentations(self, classes: u64, catalog: &super::program::Catalog, semantic: &Semantic<'_>)
        -> Known<Option<super::program::Augmentations>> {
        let mut index = super::program::Augmentations::default();
        for declaration in catalog.candidates(None, classes) {
            let Known::Yes(candidate) = self.declaration_scope(declaration) else { return Known::Unknown };
            let at = Leaf { target: Target::Node(NodeRef { file: candidate.file, node: declaration.node }), frame: FrameId::ROOT };
            let target = match World::module(&self, at, declaration.source.as_deref().expect("source declaration"), semantic) {
                Known::Yes(Some(target)) => target,
                Known::Yes(None) => continue,
                Known::Unknown => return Known::Unknown,
            };
            let (Some(target_path), Some(candidate_path)) = (self.shared_path(target.file), self.shared_path(candidate.file)) else {
                return Known::Yes(None);
            };
            index.entry((target_path, target.scope)).or_default().push((candidate_path, candidate.scope));
        }
        Known::Yes(Some(index))
    }

    /// Whether this world may use and add run-shared answers: its linted
    /// text is what the run reads for its path.
    fn shares(self, snapshot: &super::program::Snapshot) -> bool {
        let linted = self.linted_hash();
        *self.0.program.shares.get_or_init(|| snapshot.text(self.0.revision(), self.0.path).1 == linted)
    }

    fn linted_hash(self) -> u64 {
        *self.0.program.linted_hash.get_or_init(|| super::reads::hash_bytes(self.0.ctx.source.as_bytes()))
    }

    /// Whether `path` is a file, recorded; probed once per run.
    fn probe_file(self, path: &Path) -> bool {
        let found = match self.0.snapshot {
            Some(snapshot) => snapshot.is_file(self.0.revision(), path),
            None => super::reads::is_file_unrecorded(path),
        };
        super::reads::note(super::reads::Dep::IsFile(path.to_owned(), found));
        found
    }

    /// A run unit's path, for questions about it shared across worlds; `None`
    /// for the linted file (see [`World::shared_modules`]).
    fn shared_path(self, file: u32) -> Option<PathBuf> {
        if file == 0 { return None }
        World::path(&self, file).map(Path::to_owned)
    }

    /// The run-shared answer to `question` under this world's program,
    /// computing (and sharing) it on a miss ([`World::shared_modules`]).
    fn shared(self, question: super::program::StableQuestion, compute: &mut dyn FnMut() -> Known<Vec<Target>>) -> Known<Vec<Target>> {
        let Some(snapshot) = self.0.snapshot else { return compute() };
        let linted = self.linted_hash();
        // Shared answers see every file as the run reads it; a world whose
        // linted text differs (a fix pass, an unsaved edit) keeps its own.
        let (true, Some(key)) = self.timed("modules.resolve.shared.key", || (self.shares(snapshot), self.module_key(question))) else { return compute() };
        if let Some(shared) = self.timed("modules.resolve.shared.lookup", || snapshot.module_answer(&key)) {
            if let Some(targets) = self.timed("modules.resolve.shared.targets", || self.local_targets(&shared.value)) {
                self.timed("modules.resolve.shared.record", || super::reads::note_group(&shared.reads));
                return Known::Yes(targets);
            }
        }
        let answer = super::reads::Cached::compute(|| {
            super::reads::note(super::reads::Dep::Text(self.0.path.to_owned(), linted));
            compute()
        });
        self.timed("modules.resolve.shared.record", || super::reads::note_group(&answer.reads));
        if let Known::Yes(targets) = &answer.value {
            self.timed("modules.resolve.shared.publish", || {
                if let Some(stable) = self.stable_targets(targets) {
                    snapshot.keep_module_answer(key, super::reads::Cached { value: stable, reads: answer.reads.clone() });
                }
            });
        }
        answer.value
    }

    /// `question` under this world's program; `None` while the program is
    /// not settled.
    fn module_key(self, question: super::program::StableQuestion) -> Option<super::program::ModuleKey> {
        let Known::Yes(Some(selected)) = self.timed("modules.resolve.shared.select", || self.program_selected()) else { return None };
        let Known::Yes((program, outside)) = self.timed("modules.resolve.shared.identity", || self.program_identity(selected)) else { return None };
        Some(super::program::ModuleKey {
            revision: self.0.revision(),
            settings: selected as *const Settings as usize,
            program,
            outside,
            question,
        })
    }

    /// The program under `selected` as run-shared caches know it: a hash of
    /// its input paths (worlds of one program share; other programs with the
    /// same settings profile do not), and the linted file when it is not an
    /// input.
    fn program_identity(self, selected: &Settings) -> Known<(u64, Option<PathBuf>)> {
        if let Some(known) = self.0.program.outside.borrow().get(&(selected as *const Settings)) {
            return Known::Yes(known.clone());
        }
        let Known::Yes(paths) = self.input_paths(selected) else { return Known::Unknown };
        // Worlds of one run share the run's input list (the snapshot holds
        // it for the run), so its address names the program without hashing
        // every path again in every world.
        let program = Arc::as_ptr(&paths) as usize as u64;
        let outside = (!paths.iter().any(|path| path == self.0.path)).then(|| self.0.path.to_owned());
        self.0.program.outside.borrow_mut().insert(selected as *const Settings, (program, outside.clone()));
        Known::Yes((program, outside))
    }

    fn stable_targets(self, targets: &[Target]) -> Option<Arc<[super::program::StableTarget]>> {
        targets.iter().map(|target| match *target {
            Target::Node(node) if node.file != 0 => Some(super::program::StableTarget::Node(World::path(&self, node.file)?.to_owned(), node.node)),
            Target::Scope(scope) if scope.file != 0 => Some(super::program::StableTarget::Scope(World::path(&self, scope.file)?.to_owned(), scope.scope)),
            _ => None,
        }).collect()
    }

    /// Shared targets in this world's file ids; `None` when one is in the
    /// linted file (see [`World::shared_modules`]).
    fn local_targets(self, targets: &[super::program::StableTarget]) -> Option<Vec<Target>> {
        targets.iter().map(|target| {
            let (path, index) = match target {
                super::program::StableTarget::Node(path, index) | super::program::StableTarget::Scope(path, index) => (path, *index),
            };
            let file = self.0.file(path).filter(|&file| file != 0)?;
            Some(match target {
                super::program::StableTarget::Node(..) => Target::Node(NodeRef { file, node: index }),
                super::program::StableTarget::Scope(..) => Target::Scope(ScopeRef { file, scope: index }),
            })
        }).collect()
    }

    fn catalog(self, selected: &Settings, classes: u64, semantic: &Semantic<'_>) -> Known<Arc<super::program::Catalog>> {
        let key = (selected as *const Settings, classes);
        if let Some(cached) = self.0.program.catalogs.borrow().get(&key) { return Known::Yes(cached.get()) }
        let cached = super::reads::Cached::compute(|| {
            let Known::Yes((program, outside)) = self.program_identity(selected) else { return Known::Unknown };
            let key = super::program::CatalogKey { settings: selected as *const Settings as usize, program, outside, classes };
            let current_hash = super::reads::hash_bytes(self.0.ctx.source.as_bytes());
            let load = || self.0.linter.programs.catalog(key.clone(), self.0.ctx.profile, |dep| {
                if let super::reads::Dep::Text(path, hash) = dep {
                    if path == self.0.path && *hash != current_hash { return false }
                }
                self.0.linter.holds(dep)
            }, || match self.0.complete_attribute(|| {
                let Known::Yes(paths) = self.input_paths(selected) else { return Known::Unknown };
                // Only files with a node that opens a module scope of these
                // classes contribute, so the others never resolve their names:
                // per kind, the conditions of its opening items.
                let opens: Vec<Vec<Option<u32>>> = self.0.ctx.grammar.names.kinds.iter().map(|kind| kind.modules.items.iter()
                    .filter(|item| matches!(item.action, Action::Scope { classes: opened, .. } if opened & classes != 0))
                    .map(|item| item.when).collect()).collect();
                let opening = |file: u32, tree: &Tree| tree.order.iter().any(|&node| {
                    let at = Leaf { target: Target::Node(NodeRef { file, node }), frame: FrameId::ROOT };
                    opens.get(tree.node(node).matcher as usize).is_some_and(|whens| whens.iter().any(|when| match when {
                        None => true,
                        Some(when) => World::condition(&self, at, *when, semantic) != Known::Yes(false),
                    }))
                });
                let mut declarations = Vec::new();
                let mut texts = crate::fx_hash::FxHashMap::default();
                // Files whose text cannot declare an ambient scope are read
                // (and recorded) but never parsed.
                let mut candidates = Vec::new();
                for path in paths.iter().chain(key.outside.iter()) {
                    let Some(text) = self.source_text(path) else { return Known::Unknown };
                    texts.insert(path.clone(), super::reads::hash_bytes(text.as_bytes()));
                    if selected.globals.may_declare_ambient(&text) { candidates.push(path.clone()) }
                }
                self.prefetch_inputs(selected, &candidates);
                for path in &candidates {
                    let Some(file) = self.0.file(path) else { return Known::Unknown };
                    let Some(tree) = World::tree(&self, file) else { return Known::Unknown };
                    if !opening(file, tree) { continue }
                    let Some(names) = World::names(&self, file) else { return Known::Unknown };
                    for (scope, info) in names.scopes.iter().enumerate() {
                        if classes & info.classes == 0 { continue }
                        let Known::Yes(items) = self.named_scope(ScopeRef { file, scope: scope as u32 }, classes, semantic) else { return Known::Unknown };
                        declarations.extend(items.into_iter().map(|item| super::program::Declaration {
                            path: path.clone(), scope: item.scope.scope, node: info.node,
                            name: item.name, source: item.source, classes: item.classes,
                        }));
                    }
                }
                Known::Yes(super::program::Catalog::new(declarations, texts))
            }) {
                Known::Yes(answer) => answer,
                Known::Unknown => Known::Unknown,
            });
            match self.0.snapshot {
                Some(snapshot) => snapshot.catalog(self.0.revision(), key.clone(), self.0.path, current_hash, load),
                None => load(),
            }
        });
        let answer = cached.get();
        if let Known::Yes(catalog) = &answer {
            self.0.program.catalogs.borrow_mut().insert(key, super::reads::Cached { value: catalog.clone(), reads: cached.reads });
        }
        answer
    }

    fn declaration_scope(self, declaration: &super::program::Declaration) -> Known<ScopeRef> {
        match self.0.file(&declaration.path) {
            Some(file) => Known::Yes(ScopeRef { file, scope: declaration.scope }),
            None => Known::Unknown,
        }
    }

    fn named_scope(self, scope: ScopeRef, classes: u64, semantic: &Semantic<'_>) -> Known<Vec<NamedModule>> {
        let Some(names) = World::names(&self, scope.file) else { return Known::Unknown };
        let Some(info) = names.scopes.get(scope.scope as usize) else { return Known::Unknown };
        if info.classes & classes == 0 { return Known::Yes(Vec::new()) }
        let Some(tree) = World::tree(&self, scope.file) else { return Known::Unknown };
        let at = Leaf { target: Target::Node(NodeRef { file: scope.file, node: info.node }), frame: FrameId::ROOT };
        let Some(kind) = self.0.ctx.grammar.names.kinds.get(tree.node(info.node).matcher as usize) else { return Known::Yes(Vec::new()) };
        let mut modules = Vec::new();
        for item in &kind.modules.items {
            let Action::Scope { classes: opened, name, module } = item.action else { continue };
            if opened & classes == 0 { continue }
            if !semantic.question.budget.step() { return Known::Unknown }
            match item.when.map_or(Known::Yes(true), |when| self.condition(at, when, semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            let Known::Yes(name) = self.operand(at, name, semantic) else { return Known::Unknown };
            let name = match name {
                Value::Str(name) => name.to_string(),
                Value::Node(_, name) => self.name_key(scope.file, &name).into_owned(),
                _ => return Known::Unknown,
            };
            let source = match module {
                Some(operand) => match self.operand(at, operand, semantic) {
                    Known::Yes(Value::Str(source)) => Some(source.to_string()),
                    _ => return Known::Unknown,
                },
                None => None,
            };
            modules.push(NamedModule { scope, name, source, classes: opened });
        }
        Known::Yes(modules)
    }

    fn ambient_with(self, name: &str, classes: u64, semantic: &Semantic<'_>) -> Known<Option<ScopeRef>> {
        let Known::Yes(Some(selected)) = self.program_selected() else { return Known::Unknown };
        let Known::Yes(catalog) = self.timed("modules.resolve.catalog", || self.catalog(selected, classes, semantic)) else { return Known::Unknown };
        match catalog.ambient(name, classes) {
            Some(declaration) => match self.declaration_scope(declaration) {
                Known::Yes(scope) => Known::Yes(Some(scope)),
                Known::Unknown => Known::Unknown,
            },
            None => Known::Yes(None),
        }
    }
}

impl World for Files<'_> {
    fn revision(&self) -> u64 {
        self.0.revision()
    }
    fn grammar(&self) -> &Grammar {
        self.0.ctx.grammar
    }

    fn profile(&self) -> Option<&crate::engine::Profile> { self.0.ctx.profile }

    fn path(&self, file: u32) -> Option<&Path> {
        if file == 0 { return Some(self.0.path); }
        Some(&self.0.views.get(file.checked_sub(1)?)?.unit.path)
    }

    fn tree(&self, file: u32) -> Option<&Tree> {
        if file == 0 { return Some(self.0.ctx.tree); }
        Some(&self.0.views.get(file.checked_sub(1)?)?.unit.parsed.tree)
    }

    fn text(&self, file: u32) -> Option<&str> {
        if file == 0 { return Some(self.0.ctx.source); }
        Some(&self.0.views.get(file.checked_sub(1)?)?.unit.text)
    }

    fn names(&self, file: u32) -> Option<&Resolved> {
        // A run unit's names resolved by any world are complete and shared:
        // read them without building this world's expression context.
        if let Some(shared) = self.0.name_cache(file).and_then(std::sync::OnceLock::get) {
            return Some(&**shared.as_ref());
        }
        let names = self.0.on(file, self.tree(file)?.root)?.names();
        if names.complete { Some(names) } else {
            self.0.mark_unknown();
            None
        }
    }

    fn index(&self, file: u32) -> Option<&ScopeIndex> {
        let ctx = self.0.on(file, self.tree(file)?.root)?;
        Some(ctx.names().index(ctx.tree, ctx.source, &|text| self.0.key(file, text)))
    }

    fn name_key<'s>(&self, file: u32, text: &'s str) -> Cow<'s, str> {
        self.0.key(file, text)
    }

    fn module(&self, at: Leaf, specifier: &str, semantic: &Semantic<'_>) -> Known<Option<ScopeRef>> {
        let Some(ctx) = self.0.here(at) else { return Known::Unknown };
        let Known::Yes(Some(selected)) = self.program_selected() else { return Known::Unknown };
        let key = (ctx.file_id, selected as *const Settings);
        let revision = self.0.revision();
        {
            let modules = self.0.program.modules.borrow();
            match modules.get(&key).and_then(|answers| answers.get(specifier)) {
                Some(ModuleAnswer::Ready(version, answer)) if *version == revision => return Known::Yes(answer.get()),
                Some(ModuleAnswer::Busy(version)) if *version == revision => return Known::Unknown,
                _ => {}
            }
        }
        self.0.program.modules.borrow_mut().entry(key).or_default().insert(specifier.into(), ModuleAnswer::Busy(revision));
        let resolve = || self.timed("modules.resolve.search", || self.0.asking(semantic, at, || Resolver { host: self, settings: selected }.resolve(ctx.file_id, specifier, &[], semantic.question)));
        // Resolution depends only on the importer's directory (not its tree
        // or name), so imports are shared by directory, the linted file's too.
        let answer = super::reads::Cached::compute(|| match World::path(self, ctx.file_id).and_then(Path::parent) {
            Some(directory) => {
                let question = super::program::StableQuestion::Resolve { directory: directory.to_owned(), specifier: specifier.into() };
                let shared = self.timed("modules.resolve.shared", || self.shared(question, &mut || match resolve() {
                    Known::Yes(scope) => Known::Yes(scope.into_iter().map(Target::Scope).collect()),
                    Known::Unknown => Known::Unknown,
                }));
                match shared {
                    Known::Yes(targets) => match targets.as_slice() {
                        [] => Known::Yes(None),
                        [Target::Scope(scope)] => Known::Yes(Some(*scope)),
                        _ => Known::Unknown,
                    },
                    Known::Unknown => Known::Unknown,
                }
            }
            None => resolve(),
        });
        let result = answer.get();
        let mut modules = self.0.program.modules.borrow_mut();
        let answers = modules.entry(key).or_default();
        match result {
            Known::Yes(value) => {
                answers.insert(specifier.into(), ModuleAnswer::Ready(revision, super::reads::Cached { value, reads: answer.reads }));
            }
            Known::Unknown => { answers.remove(specifier); }
        }
        result
    }

    fn program_option(&self, at: Leaf, name: &str, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        let Some(ctx) = self.0.here(at) else { return Known::Unknown };
        let Known::Yes(Some(settings)) = self.program_selected() else { return Known::Unknown };
        let context = self.0.asking(semantic, at, || Resolver { host: self, settings }.context(ctx.file_id, semantic.question));
        match context {
            Known::Yes(context) => Known::Yes(context.key("config").key(name)),
            Known::Unknown => Known::Unknown,
        }
    }

    fn origin(&self, file: u32) -> Known<crate::grammar::names::modules::resolution::Origin> {
        let Known::Yes(Some(selected)) = self.program_selected() else { return Known::Unknown };
        World::path(self, file).map_or(Known::Unknown, |path| Known::Yes(selected.origin(path)))
    }

    fn global_scopes(&self, _importer: u32, name: &str, _semantic: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
        let Known::Yes(Some(selected)) = self.program_selected() else { return Known::Unknown };
        self.roots(selected, name)
    }

    /// Shares answers across the run's worlds by file path. Answers about
    /// or pointing into the linted file stay in this world: its tree is
    /// parsed for linting, so its node ids are not the run unit's.
    fn shared_modules(&self, question: SharedQuestion<'_>, compute: &mut dyn FnMut() -> Known<Vec<Target>>) -> Known<Vec<Target>> {
        let question = match question {
            SharedQuestion::Publication { kind, scope, name, class } => {
                let Some(path) = self.shared_path(scope.file) else { return compute() };
                super::program::StableQuestion::Publication { kind, path, scope: scope.scope, name: name.into(), class }
            }
            SharedQuestion::Global { name, class } => super::program::StableQuestion::Global { name: name.into(), class },
        };
        self.shared(question, compute)
    }

    fn module_scopes(&self, scope: ScopeRef, semantic: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
        let Some(path) = self.shared_path(scope.file) else { return self.merged_scopes(scope, semantic) };
        let question = super::program::StableQuestion::Scopes { path, scope: scope.scope };
        let shared = self.shared(question, &mut || match self.merged_scopes(scope, semantic) {
            Known::Yes(scopes) => Known::Yes(scopes.into_iter().map(Target::Scope).collect()),
            Known::Unknown => Known::Unknown,
        });
        let Known::Yes(targets) = shared else { return Known::Unknown };
        let scopes: Option<Vec<ScopeRef>> = targets.into_iter().map(|target| match target {
            Target::Scope(scope) => Some(scope),
            _ => None,
        }).collect();
        scopes.map_or(Known::Unknown, Known::Yes)
    }

    fn module_star_excludes(&self, scope: ScopeRef, name: &str, _semantic: &Semantic<'_>) -> Known<bool> {
        let Some(names) = self.names(scope.file) else { return Known::Unknown };
        if names.scopes.get(scope.scope as usize).is_none() { return Known::Unknown }
        match self.program_selected() {
            Known::Yes(Some(selected)) => Known::Yes(selected.star_excludes.iter().any(|excluded| &**excluded == name)),
            Known::Yes(None) => Known::Yes(false),
            Known::Unknown => Known::Unknown,
        }
    }

    fn condition(&self, at: Leaf, condition: u32, semantic: &Semantic<'_>) -> Known<bool> {
        let world = self.0;
        let lowered = world.ctx.grammar.names.conditions.get(condition as usize).and_then(|c| c.lowered.get());
        let (Some(ctx), Some(e)) = (world.here(at), lowered) else { return Known::Unknown };
        // A condition asked again while it is being decided on the same
        // node (in any frame: each round can bind a new one) is a cycle.
        let key = (at.target, condition);
        if world.conditions.borrow().contains(&key) {
            world.mark_unknown();
            return Known::Unknown;
        }
        world.conditions.borrow_mut().push(key);
        let answer = world.asking(semantic, at, || {
            let held = Session::new(&ctx, ctx.node, &[]).holds_on(ctx.node, e);
            held.map_or(Known::Unknown, Known::Yes)
        });
        world.conditions.borrow_mut().pop();
        answer
    }

    fn operand(&self, at: Leaf, operand: u32, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        let world = self.0;
        let lowered = world.ctx.grammar.names.operands.get(operand as usize).and_then(|o| o.lowered.get());
        let (Some(ctx), Some(e)) = (world.here(at), lowered) else { return Known::Unknown };
        world.asking(semantic, at, || run(&ctx, e))
    }

    fn attribute(&self, at: Leaf, attr: u16, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        let world = self.0;
        let Some(ctx) = world.here(at) else { return Known::Unknown };
        world.asking(semantic, at, || run(&ctx, &VExpr::Attr(attr)))
    }
}

impl Host for Files<'_> {
    fn program(&self) -> Known<u32> {
        Known::Yes(0)
    }

    fn path(&self, file: u32) -> Known<PathBuf> {
        if file == 0 { return Known::Yes(self.0.path.to_owned()); }
        self.0.views.get(file.saturating_sub(1)).map_or(Known::Unknown, |view| Known::Yes(view.unit.path.clone()))
    }

    fn text(&self, path: &Path, _: &QueryContext) -> Known<Option<String>> {
        if path == self.0.path { return Known::Yes(Some(self.0.ctx.source.to_owned())); }
        let (text, hash) = match self.0.snapshot {
            Some(snapshot) => snapshot.text(self.0.revision(), path),
            None => super::reads::read_unrecorded(path),
        };
        super::reads::note(super::reads::Dep::Text(path.to_owned(), hash));
        Known::Yes(text.map(|text| text.to_string()))
    }

    fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> {
        Known::Yes(path == self.0.path || self.probe_file(path))
    }

    fn configuration(&self, settings: &Settings, directory: &Path, load: &mut dyn FnMut() -> Result<Arc<Configuration>, ()>) -> Result<Arc<Configuration>, ()> {
        match self.0.snapshot {
            Some(snapshot) if self.shares(snapshot) => {
                let key = (self.0.revision(), settings as *const Settings as usize, directory.to_owned());
                Ok(snapshot.configuration(key, load)?.get())
            }
            _ => load(),
        }
    }

    fn metadata(&self, path: &Path, question: &QueryContext) -> Result<Option<Arc<Json>>, ()> {
        let Some(snapshot) = self.0.snapshot.filter(|_| path != self.0.path) else {
            return parse_metadata(Host::text(self, path, question).known().ok_or(())?.as_deref());
        };
        let (text, hash) = snapshot.text(self.0.revision(), path);
        super::reads::note(super::reads::Dep::Text(path.to_owned(), hash));
        match text {
            Some(text) => snapshot.metadata(path, hash, &text).map(Some).ok_or(()),
            None => Ok(None),
        }
    }

    fn scope(&self, path: &Path, _: &QueryContext) -> Known<Option<ScopeRef>> {
        if path != self.0.path && !self.probe_file(path) { return Known::Yes(None); }
        self.0.file(path).map_or(Known::Unknown, |file| Known::Yes(Some(ScopeRef { file, scope: 0 })))
    }

    fn ambient(&self, _importer: u32, name: &str, classes: u64, question: &QueryContext) -> Known<Option<ScopeRef>> {
        if let Some(active) = self.0.question() { return self.ambient_with(name, classes, active.semantic); }
        let semantic = Semantic::new(self, &self.0.ctx.grammar.names, question);
        self.ambient_with(name, classes, &semantic)
    }

    fn discover(&self, request: &DiscoveryRequest, _: &QueryContext) -> Known<Vec<PathBuf>> {
        let mut discoveries = self.0.linter.discoveries.lock().expect("unit discoveries");
        let paths = discoveries.entry(request.clone()).or_insert_with(|| super::reads::Cached::compute(|| super::reads::requested(request))).get();
        paths.map_or(Known::Unknown, Known::Yes)
    }
}

/// `e`'s value on the context's node; unknown when the VM gives up.
fn run(ctx: &Ctx<'_>, e: &VExpr) -> Known<Value<'static>> {
    let session = Session::new(ctx, ctx.node, &[]);
    let value = session.value(e, None).map(Value::into_static);
    value.map_or(Known::Unknown, Known::Yes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::names::question::{FrameId, NodeRef, QueryContext, ScopeRef};
    use crate::engine::{reads, tests::linter};

    fn with_world<T>(linter: &super::super::Linter, f: impl FnOnce(Files<'_>) -> T) -> T {
        with_source_world(linter, Path::new("/root/main.t"), "()", f)
    }

    fn with_source_world<T>(linter: &super::super::Linter, path: &Path, source: &str, f: impl FnOnce(Files<'_>) -> T) -> T {
        with_snapshot_world(linter, path, source, None, f)
    }

    fn with_snapshot_world<T>(linter: &super::super::Linter, path: &Path, source: &str, snapshot: Option<&super::super::program::Snapshot>, f: impl FnOnce(Files<'_>) -> T) -> T {
        let tree = Tree::parse_with(&linter.grammar, 0, source, None, None);
        let lines = crate::line_index::LineIndex::new(source);
        let memo = Default::default();
        let reindented = Default::default();
        let rel = linter.relative(path);
        let file = rel.to_string_lossy();
        let ctx = linter.tree_ctx(0, &tree, (source, &lines, &file), &memo, &reindented, None);
        let world = match snapshot {
            Some(snapshot) => LintWorld::in_run(linter, path, ctx, snapshot),
            None => LintWorld::new(linter, path, ctx),
        };
        f(Files(&world))
    }

    #[test]
    fn program_catalog_reuses_declarations_across_questions_and_worlds_then_invalidates() {
        let mut linter = linter(r#"
            module_resolution program {
                config {
                    files ["catalog.json"]
                    format jsonc
                    source_files "files"
                }
                extensions [".t"]
                ambient_scopes [ambient]
                search [ambient]
            }
            in paren {
                scope module
                uses module_resolution program
                when text == "(library)" { scope ambient named "shared" }
                when text == "(unsaved)" { scope ambient named "local" }
            }
        "#);
        linter.enable_profile();
        let profile = linter.profile.as_ref().unwrap();
        let root = Path::new("/root/catalog-reuse");
        let main = root.join("main.t");
        let next = root.join("next.t");
        let library = root.join("library.t");
        let config = root.join("catalog.json");
        reads::set_buffer(&main, "()");
        reads::set_buffer(&next, "()");
        reads::set_buffer(&library, "(library)");
        let inputs = r#"{"files":["library.t","main.t","next.t"]}"#;
        reads::set_buffer(&config, inputs);
        let classes = linter.grammar.names.vocabulary.scope_class("ambient");
        with_source_world(&linter, &main, "()", |files| {
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            let Known::Yes(Some(scope)) = files.ambient_with("shared", classes, &semantic) else { panic!("ambient declaration exists") };
            assert_eq!(World::text(&files, scope.file), Some("(library)"));
            let remaining = question.budget.remaining();
            for _ in 0..20 {
                assert_eq!(files.ambient_with("shared", classes, &semantic), Known::Yes(Some(scope)));
            }
            assert_eq!(question.budget.remaining(), remaining, "completed catalog is not repeated question work");
            assert_eq!(super::super::Profile::get(&profile.program_catalogue_builds), 1);
            assert_eq!(super::super::Profile::get(&profile.catalogue_read_validations), 0, "freshly built catalog needs no reread");
        });
        with_source_world(&linter, &next, "()", |files| {
            files.0.file(&main).unwrap();
            let mut question = QueryContext::default();
            question.budget = crate::grammar::names::question::Budget::new(0);
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            let Known::Yes(Some(scope)) = files.ambient_with("shared", classes, &semantic) else { panic!("shared catalog is available without rebuilding") };
            assert_eq!(World::text(&files, scope.file), Some("(library)"), "shared descriptors must not retain another world's file ids");
            for _ in 0..20 {
                assert_eq!(files.ambient_with("shared", classes, &semantic), Known::Yes(Some(scope)));
            }
            assert_eq!(super::super::Profile::get(&profile.program_catalogue_builds), 1);
            assert_eq!(super::super::Profile::get(&profile.catalogue_read_validations), 1, "a shared catalog is validated once for this world");
        });
        let snapshot = super::super::program::Snapshot::default();
        for path in [&main, &next].into_iter().cycle().take(20) {
            let (_, dependencies) = reads::track(|| with_snapshot_world(&linter, path, "()", Some(&snapshot), |files| {
                let mut question = QueryContext::default();
                question.budget = crate::grammar::names::question::Budget::new(0);
                let semantic = Semantic::new(&files, &linter.grammar.names, &question);
                let Known::Yes(Some(scope)) = files.ambient_with("shared", classes, &semantic) else { panic!("run snapshot crosses files without rebuilding") };
                assert_eq!(World::text(&files, scope.file), Some("(library)"));
            }));
            assert!(!dependencies.is_empty());
        }
        assert_eq!(super::super::Profile::get(&profile.program_catalogue_builds), 1);
        assert_eq!(super::super::Profile::get(&profile.catalogue_read_validations), 2, "twenty new worlds in one run share one validation");
        for (source, present) in [("(unsaved)", true), ("()", false)] {
            with_snapshot_world(&linter, &main, source, Some(&snapshot), |files| {
                let question = QueryContext::default();
                let semantic = Semantic::new(&files, &linter.grammar.names, &question);
                let Known::Yes(answer) = files.ambient_with("local", classes, &semantic) else { panic!("provided file text is known") };
                assert_eq!(answer.is_some(), present, "the caller's text must not be replaced by a shared disk snapshot");
            });
        }
        for (metadata, present) in [(r#"{"files":["main.t","next.t"]}"#, false), (inputs, true)] {
            reads::set_buffer(&config, metadata);
            with_source_world(&linter, &main, "()", |files| {
                let question = QueryContext::default();
                let semantic = Semantic::new(&files, &linter.grammar.names, &question);
                let Known::Yes(answer) = files.ambient_with("shared", classes, &semantic) else { panic!("changed project inputs are known") };
                assert_eq!(answer.is_some(), present);
            });
        }
        reads::set_buffer(&library, "(changed)");
        linter.invalidate(std::slice::from_ref(&library));
        with_snapshot_world(&linter, &main, "()", Some(&snapshot), |files| {
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            assert_eq!(files.ambient_with("shared", classes, &semantic), Known::Yes(None));
        });
        reads::set_buffer(&library, "(library)");
        with_source_world(&linter, &main, "()", |files| {
            let mut bounded = QueryContext::default();
            bounded.budget = crate::grammar::names::question::Budget::new(0);
            let semantic = Semantic::new(&files, &linter.grammar.names, &bounded);
            assert_eq!(files.ambient_with("shared", classes, &semantic), Known::Unknown);
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            let Known::Yes(Some(scope)) = files.ambient_with("shared", classes, &semantic) else { panic!("unknown catalog is retried") };
            assert_eq!(World::text(&files, scope.file), Some("(library)"));
        });
        for path in [&main, &next, &library, &config] { reads::drop_buffer(path); }
    }

    #[test]
    fn unknown_scope_conditions_cannot_publish_partial_names_or_catalogs() {
        thread_local! { static CALLS: Cell<u32> = const { Cell::new(0) }; }
        static QUERY: crate::expr::queries::QueryFunction = crate::expr::queries::QueryFunction {
            name: "unresolved_scope",
            arity: 1,
            call: |_, _, _| {
                CALLS.with(|calls| calls.set(calls.get() + 1));
                Known::Unknown
            },
        };
        CALLS.with(|calls| calls.set(0));
        let mut linter = linter(r#"
            module_resolution program {
                config {
                    files ["catalog.json"]
                    format jsonc
                    source_files "files"
                }
                extensions [".t"]
                ambient_scopes [ambient]
            }
            in paren {
                scope module
                uses module_resolution program
                when text == "(library)" { scope ambient named "shared" }
            }
        "#);
        for condition in &mut linter.grammar.names.conditions {
            let original = condition.lowered.take().unwrap();
            condition.lowered.set(VExpr::Binary(crate::config::ast::BinOp::And,
                Box::new(original), Box::new(VExpr::QueryCall(&QUERY,
                    Box::new([VExpr::Builtin(crate::expr::Builtin::Node)]))))).unwrap();
        }
        let root = Path::new("/root/catalog-unknown");
        let main = root.join("main.t");
        let library = root.join("library.t");
        let config = root.join("catalog.json");
        reads::set_buffer(&main, "()");
        reads::set_buffer(&library, "(library)");
        reads::set_buffer(&config, r#"{"files":["library.t","main.t"]}"#);
        let classes = linter.grammar.names.vocabulary.scope_class("ambient");
        let mut previous = 0;
        for _ in 0..2 {
            with_source_world(&linter, &main, "()", |files| {
                let question = QueryContext::default();
                let semantic = Semantic::new(&files, &linter.grammar.names, &question);
                assert_eq!(files.ambient_with("shared", classes, &semantic), Known::Unknown);
                let calls = CALLS.with(Cell::get);
                assert!(calls > previous, "a new world must retry incomplete names");
                assert_eq!(files.ambient_with("shared", classes, &semantic), Known::Unknown);
                previous = calls;
            });
        }
        for path in [&main, &library, &config] { reads::drop_buffer(path); }
    }

    #[test]
    fn reentrant_queries_in_fresh_frames_share_the_question_depth_bound() {
        fn reenter<'a>(world: &'a LintWorld<'a>, semantic: &Semantic<'_>, at: Leaf, remaining: u32) -> Known<bool> {
            world.asking(semantic, at, || world.semantic_query(0, 0, Uncertainty::Strict, |semantic, at| {
                if remaining == 0 { return Known::Yes(true) }
                let frame = semantic.question.bind(at.frame, NodeRef::here(0), None, None);
                reenter(world, semantic, Leaf { frame, ..at }, remaining - 1)
            }))
        }
        let linter = linter("");
        with_world(&linter, |files| {
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            let at = Leaf { target: Target::Node(NodeRef::here(0)), frame: FrameId::ROOT };
            assert_eq!(reenter(files.0, &semantic, at, 2), Known::Yes(true));
            assert_eq!(reenter(files.0, &semantic, at, crate::grammar::names::question::Budget::DEPTH + 1), Known::Unknown);
            assert!(files.0.question().is_none());
        });
    }

    #[test]
    fn unknown_trait_answers_keep_name_conditions_unknown() {
        let linter = linter(r#"
            in str fits good
            in paren {
                unwraps it
                fits conditional when node could good
            }
        "#);
        with_world(&linter, |files| {
            let tree = files.tree(0).unwrap();
            let matcher = linter.grammar.matcher("paren").unwrap();
            let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
            let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            let trait_id = linter.grammar.names.vocabulary.traits.iter().position(|name| name == "conditional").unwrap() as u16;
            assert_eq!(semantic.values().fits(at, trait_id), Known::Unknown);
        });
    }

    #[test]
    fn complete_root_semantic_attributes_reuse_query_answers() {
        for shaped in [false, true] {
            let mut linter = linter("");
            let matcher = linter.grammar.matcher("paren").unwrap();
            let query = crate::expr::queries::lookup_function("name_role_classes").unwrap();
            let attr = 1;
            linter.attrs = crate::expr::Attributes::new(vec![
                crate::expr::Attr {
                    name: "classes".into(),
                    defs: vec![(matcher, crate::expr::CExpr::QueryCall(query, vec![
                        crate::expr::CExpr::Builtin(crate::expr::Builtin::Node), crate::expr::CExpr::Const(Value::from("declaration")),
                    ]))],
                },
                crate::expr::Attr { name: "alias".into(), defs: vec![(matcher, crate::expr::CExpr::Attr(0))] },
            ], &[], &linter.grammar);
            linter.profile = Some(super::super::Profile::new(linter.grammar.matchers.len(), linter.rules.len()));
            let expression = crate::expr::CExpr::Attr(attr);
            if shaped {
                linter.attrs.shape_records(&linter.grammar, vec![(crate::vm::KindSet::all(linter.grammar.matchers.len()), &expression)]);
            }
            with_world(&linter, |files| {
                let tree = files.tree(0).unwrap();
                let matcher = linter.grammar.matcher("paren").unwrap();
                let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
                let ctx = files.0.on(0, node).unwrap();
                let read = || Session::new(&ctx, node, &[]).value(&VExpr::Attr(attr), None).map(Value::into_static);
                assert_eq!(read(), Some(Value::Null));
                let calls = &linter.profile.as_ref().unwrap().semantic_queries["name_role_classes"].calls;
                assert_eq!(super::super::Profile::get(calls), 1);
                assert_eq!(read(), Some(Value::Null));
                assert_eq!(super::super::Profile::get(calls), 1, "shaped={shaped}");
            });
        }
    }

    #[test]
    fn unknown_root_attribute_fallbacks_retry_instead_of_becoming_answers() {
        thread_local! { static ATTEMPTS: Cell<u32> = const { Cell::new(0) }; }
        static QUERY: crate::expr::queries::QueryFunction = crate::expr::queries::QueryFunction {
            name: "transient",
            arity: 1,
            call: |_, _, _| ATTEMPTS.with(|attempts| {
                let before = attempts.replace(attempts.get() + 1);
                if before == 0 { Known::Unknown } else { Known::Yes(Value::Bool(false)) }
            }),
        };
        for shaped in [false, true] {
            ATTEMPTS.with(|attempts| attempts.set(0));
            let mut linter = linter("");
            let matcher = linter.grammar.matcher("paren").unwrap();
            linter.attrs = crate::expr::Attributes::new(vec![crate::expr::Attr {
                name: "answer".into(),
                defs: vec![(matcher, crate::expr::CExpr::Nullish(
                    Box::new(crate::expr::CExpr::QueryCall(&QUERY, vec![crate::expr::CExpr::Builtin(crate::expr::Builtin::Node)])),
                    Box::new(crate::expr::CExpr::Const(Value::Bool(true))),
                ))],
            }], &[], &linter.grammar);
            if shaped {
                linter.attrs.shape_records(&linter.grammar, vec![(crate::vm::KindSet::all(linter.grammar.matchers.len()), &crate::expr::CExpr::Attr(0))]);
            }
            with_world(&linter, |files| {
                let tree = files.tree(0).unwrap();
                let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
                let ctx = files.0.on(0, node).unwrap();
                let read = || Session::new(&ctx, node, &[]).value(&VExpr::Attr(0), None).map(Value::into_static);
                assert_eq!(read(), Some(Value::Bool(true)));
                assert_eq!(read(), Some(Value::Bool(false)));
                assert_eq!(read(), Some(Value::Bool(false)));
                assert_eq!(ATTEMPTS.with(Cell::get), 2, "shaped={shaped}");
            });
        }
    }

    #[test]
    fn trait_attributes_do_not_reuse_root_answers_inside_conditions() {
        let linter = linter(r#"
            in str fits good
            in paren {
                unwraps it
                let answer node could good
                let alias answer
                fits conditional when alias
            }
        "#);
        with_world(&linter, |files| {
            let tree = files.tree(0).unwrap();
            let matcher = linter.grammar.matcher("paren").unwrap();
            let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
            let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
            let ctx = files.0.here(at).unwrap();
            let condition = linter.grammar.names.conditions[0].lowered.get().unwrap();
            assert_eq!(Session::new(&ctx, node, &[]).holds_on(node, condition), Some(false));
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            let trait_id = linter.grammar.names.vocabulary.traits.iter().position(|name| name == "conditional").unwrap() as u16;
            assert_eq!(semantic.values().fits(at, trait_id), Known::Unknown);
        });
    }

    #[test]
    fn unit_vm_uses_the_target_file_and_keeps_stable_file_ids() {
        let linter = linter(r#"
            in paren {
                let identity text
                fits marked when text == "(leaf)"
                fits viewed in module when text == "(leaf)"
                scope module named text
            }
        "#);
        let path = Path::new("/root/unit-vm.t");
        reads::set_buffer(path, "(leaf)");
        with_world(&linter, |files| {
            let file = files.0.file(path).expect("unit loads");
            assert_ne!(file, 0);
            assert_eq!(files.0.file(path), Some(file));
            let tree = files.tree(file).expect("unit tree");
            let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == linter.grammar.matcher("paren").unwrap()).unwrap();
            let at = Leaf { target: Target::Node(NodeRef { file, node }), frame: FrameId::ROOT };
            let q = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &q);
            let scope = files.names(file).unwrap().scopes.iter().position(|scope| scope.node == node).unwrap() as u32;
            let frame = q.bind(FrameId::ROOT, NodeRef { file, node }, None, Some(vec![]));
            let scoped = Leaf { target: Target::Scope(ScopeRef { file, scope }), frame };
            let trait_id = linter.grammar.names.vocabulary.traits.iter().position(|name| name == "marked").unwrap() as u16;
            for subject in [at, scoped] {
                assert_eq!(semantic.values().fits(subject, trait_id), Known::Yes(true));
                assert_eq!(files.condition(subject, 0, &semantic), Known::Yes(true));
                let Known::Yes(operand) = files.operand(subject, 0, &semantic) else { panic!("operand answers") };
                assert_eq!(operand.text(), "(leaf)");
                let Known::Yes(value) = files.attribute(subject, 0, &semantic) else { panic!("attribute answers") };
                assert_eq!(value.text(), "(leaf)");
            }
            let viewed = linter.grammar.names.vocabulary.trait_bit("viewed");
            assert!(!files.0.query(file, node, &crate::grammar::names::Value::Path(vec![]), crate::expr::queries::query(crate::config::ast::Quantifier::Could), viewed));
            assert_eq!(files.0.asking(&semantic, scoped, || Known::Yes(files.0.query(file, node, &crate::grammar::names::Value::Path(vec![]), crate::expr::queries::query(crate::config::ast::Quantifier::Could), viewed))), Known::Yes(true));
            assert_eq!(files.0.asking(&semantic, scoped, || files.0.semantic_query(file, node, Uncertainty::Strict, |_, nested| Known::Yes(nested))), Known::Yes(scoped));
            let other = tree.root;
            assert_eq!(files.0.asking(&semantic, scoped, || files.0.semantic_query(file, other, Uncertainty::Strict, |_, nested| Known::Yes(nested))), Known::Yes(Leaf { target: Target::Node(NodeRef { file, node: other }), frame }));
            assert_eq!(files.0.asking(&semantic, scoped, || files.0.semantic_query(0, 0, Uncertainty::Strict, |_, nested| Known::Yes(nested.frame))), Known::Yes(FrameId::ROOT));
            let nodeless = Leaf { target: Target::Traits(0), frame };
            assert_eq!(files.condition(nodeless, 0, &semantic), Known::Unknown);
            assert_eq!(files.0.asking(&semantic, nodeless, || files.0.semantic_query(file, node, Uncertainty::Strict, |_, nested| Known::Yes(nested.frame))), Known::Yes(FrameId::ROOT));
            assert_eq!(World::text(&files, 0), Some("()"));
        });
        reads::drop_buffer(path);
    }

    #[test]
    fn loaded_units_share_names_and_replace_them_with_changed_text() {
        let linter = linter("in paren scope module");
        let path = Path::new("/root/shared-unit-names.t");
        reads::set_buffer(path, "(first)");
        with_world(&linter, |first| {
            let file = first.0.file(path).unwrap();
            let names = first.names(file).unwrap();
            assert_eq!(names.scopes.len(), 1);
            with_world(&linter, |second| {
                let file = second.0.file(path).unwrap();
                assert!(std::ptr::eq(names, second.names(file).unwrap()));
            });
            reads::set_buffer(path, "(second) (third)");
            with_world(&linter, |changed| {
                let file = changed.0.file(path).unwrap();
                let replacement = changed.names(file).unwrap();
                assert!(!std::ptr::eq(names, replacement));
                assert_eq!(replacement.scopes.len(), 2);
            });
        });
        reads::drop_buffer(path);
    }

    #[test]
    fn every_unit_consumer_records_text_and_changed_units_are_replaced() {
        let linter = linter("in paren fits marked");
        let path = Path::new("/root/unit-cache.t");
        reads::set_buffer(path, "(before)");
        let first = linter.unit(path, None).unwrap();
        let second = linter.unit(path, None).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        for _ in 0..2 {
            let (_, deps) = reads::track(|| with_world(&linter, |files| {
                let file = files.0.file(path).expect("unit loads");
                assert_eq!(World::text(&files, file), Some("(before)"));
            }));
            assert!(deps.any(&mut |dep| matches!(dep, reads::Dep::Text(p, _) if p == path)));
            assert!(!deps.any(&mut |dep| !linter.holds(dep)));
            reads::set_buffer(path, "(after)");
            assert!(deps.any(&mut |dep| !linter.holds(dep)));
            let stale = linter.invalidate(&[path.to_owned()]);
            assert!(linter.is_stale(&stale, &deps));
            reads::set_buffer(path, "(before)");
        }
        reads::set_buffer(path, "(after)");
        linter.invalidate(&[path.to_owned()]);
        with_world(&linter, |files| {
            let file = files.0.file(path).expect("unit reloads");
            assert_eq!(World::text(&files, file), Some("(after)"));
        });
        reads::drop_buffer(path);
    }

    #[test]
    fn quantified_trait_answers_survive_unrelated_unknown_alternatives_on_a_cold_ask() {
        let linter = linter(r#"
            in .t match incomplete /u/ {
                fits good when node always mystery
                fits mystery when node always mystery
            }
            in .t match complete /a/ { fits good }
            in .t match known left:incomplete "|" right:complete {
                unwraps left | right
                rule proven { when node could good; warn "proven" }
            }
            in .t match unknown left:incomplete "&" right:incomplete {
                unwraps left | right
                rule unproven { when node could good; warn "unknown proven" }
            }
        "#);
        let messages = |text| crate::engine::tests::lint(&linter, text).into_iter().map(|d| d.message).collect::<Vec<_>>();
        assert_eq!(messages("u|a"), ["proven"], "a known good alternative decides `could` on the first ask");
        assert!(messages("u&u").is_empty(), "only unknown alternatives prove nothing");
    }

    #[test]
    fn a_run_reads_each_unit_once_and_replays_its_text_read() {
        let linter = linter("in paren fits marked");
        let path = Path::new("/root/run-unit.t");
        reads::set_buffer(path, "(before)");
        let run = super::super::program::Snapshot::default();
        let first = linter.unit(path, Some(&run)).unwrap();
        reads::set_buffer(path, "(after)");
        let (again, deps) = reads::track(|| linter.unit(path, Some(&run)).unwrap());
        assert!(std::sync::Arc::ptr_eq(&first, &again), "a run keeps the unit it validated");
        let first_hash = reads::hash_bytes(first.text.as_bytes());
        assert!(deps.any(&mut |dep| matches!(dep, reads::Dep::Text(p, hash) if p == path && *hash == first_hash)));
        assert!(deps.any(&mut |dep| !linter.holds(dep)), "the replayed read still detects the edit for caching");
        let fresh = linter.unit(path, Some(&super::super::program::Snapshot::default())).unwrap();
        assert_eq!(&*fresh.text, "(after)");
        linter.invalidate(&[path.to_owned()]);
        assert_eq!(&*linter.unit(path, Some(&run)).unwrap().text, "(after)", "invalidation ends the run's snapshot");
        reads::drop_buffer(path);
    }

    #[test]
    fn program_profile_selection_is_a_recorded_world_snapshot() {
        let linter = linter(r#"
            module_resolution program {
                config {
                    files ["world-selection.json"]
                    format jsonc
                    options "compilerOptions"
                }
                when resolution.config.variant == "chosen" {
                    star_excludes ["chosen"]
                }
            }
            in paren {
                scope module
                uses module_resolution program
            }
        "#);
        let config = Path::new("/root/world-selection.json");
        reads::set_buffer(config, r#"{"compilerOptions":{"variant":"chosen"}}"#);
        with_world(&linter, |files| {
            let mut question = QueryContext::default();
            question.budget = crate::grammar::names::question::Budget::new(0);
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            assert_eq!(files.global_scopes(0, "x", &semantic), Known::Yes(vec![ScopeRef { file: 0, scope: 0 }]));
            let Known::Yes(Some(selected)) = files.program_selected() else { panic!("root selection is not question work") };
            assert_eq!(selected.star_excludes, [Box::<str>::from("chosen")]);
            reads::set_buffer(config, r#"{"compilerOptions":{"variant":"other"}}"#);
            let (answer, dependencies) = reads::track(|| files.program_selected());
            let Known::Yes(Some(repeated)) = answer else { panic!("completed root selection remains available") };
            assert!(std::ptr::eq(selected, repeated));
            assert!(dependencies.any(&mut |dep| !linter.holds(dep)));
        });
        with_world(&linter, |files| {
            let Known::Yes(Some(selected)) = files.program_selected() else { panic!("next world selects its own mode") };
            assert!(selected.star_excludes.is_empty());
        });
        reads::drop_buffer(config);
    }

    #[test]
    fn program_inputs_are_a_recorded_world_snapshot_not_question_work() {
        let linter = linter(r#"
            module_resolution program {
                config {
                    files ["world-inputs.json"]
                    format jsonc
                    source_files "files"
                }
                extensions [".t"]
            }
            in paren {
                scope module
                uses module_resolution program
            }
        "#);
        let config = Path::new("/root/world-inputs.json");
        let library = Path::new("/root/world-inputs-library.t");
        reads::set_buffer(config, r#"{"files":["world-inputs-library.t"]}"#);
        reads::set_buffer(library, "(library)");
        with_world(&linter, |files| {
            let file = files.0.file(library).unwrap();
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            let Known::Yes(Some(selected)) = files.selected(files.file_leaf(0).unwrap(), &semantic) else { panic!("program settings exist") };
            let mut bounded = QueryContext::default();
            bounded.budget = crate::grammar::names::question::Budget::new(0);
            let semantic = Semantic::new(&files, &linter.grammar.names, &bounded);
            let expected = Known::Yes(vec![ScopeRef { file, scope: 0 }, ScopeRef { file: 0, scope: 0 }]);
            let at = files.file_leaf(0).unwrap();
            assert_eq!(files.0.asking(&semantic, at, || files.roots(selected, "x")), expected);
            reads::set_buffer(config, r#"{"files":[]}"#);
            let (answer, dependencies) = reads::track(|| files.0.asking(&semantic, at, || files.roots(selected, "x")));
            assert_eq!(answer, expected);
            assert!(dependencies.any(&mut |dep| !linter.holds(dep)));
        });
        with_world(&linter, |files| {
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            assert_eq!(files.global_scopes(0, "x", &semantic), Known::Yes(vec![ScopeRef { file: 0, scope: 0 }]));
        });
        for path in [config, library] { reads::drop_buffer(path); }
    }

    #[test]
    fn foreign_queries_keep_program_settings_and_their_relative_importer() {
        let linter = linter(r#"
            module_resolution program {
                config {
                    files ["world-program.json"]
                    format jsonc
                    source_files "files"
                }
                extensions [".t"]
                candidates ["{{dir}}/{{name}}{{ext}}"]
                ambient_scopes [ambient]
                star_excludes ["program-only"]
                search [relative, ambient]
            }
            module_resolution foreign { search [] }
            in paren {
                scope module
                when text == "()" { uses module_resolution program }
                when text != "()" { uses module_resolution foreign }
                when text == "(library)" || text == "(foreign)" { scope ambient named "shared" }
            }
        "#);
        let config = Path::new("/root/world-program.json");
        let library = Path::new("/root/world-program-library.t");
        let foreign = Path::new("/root/world-program-foreign/importer.t");
        let target = Path::new("/root/world-program-foreign/target.t");
        reads::set_buffer(config, r#"{"files":["world-program-library.t"]}"#);
        reads::set_buffer(library, "(library)");
        reads::set_buffer(foreign, "(foreign)");
        reads::set_buffer(target, "(target)");
        with_world(&linter, |files| {
            let file = files.0.file(foreign).unwrap();
            let library_file = files.0.file(library).unwrap();
            let tree = files.tree(file).unwrap();
            let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == linter.grammar.matcher("paren").unwrap()).unwrap();
            let at = Leaf { target: Target::Node(NodeRef { file, node }), frame: FrameId::ROOT };
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            assert_eq!(files.global_scopes(file, "x", &semantic), Known::Yes(vec![ScopeRef { file: library_file, scope: 0 }, ScopeRef { file: 0, scope: 0 }]));
            assert_eq!(files.module_star_excludes(ScopeRef { file, scope: 0 }, "program-only", &semantic), Known::Yes(true));
            assert_eq!(files.module(files.file_leaf(0).unwrap(), "./target", &semantic), Known::Yes(None));
            let Known::Yes(Some(relative)) = files.module(at, "./target", &semantic) else { panic!("relative importer is preserved") };
            assert_eq!(World::text(&files, relative.file), Some("(target)"));
            let classes = linter.grammar.names.vocabulary.scope_class("ambient");
            let expected = ScopeRef { file: library_file, scope: 1 };
            assert_eq!(files.ambient_with("shared", classes, &semantic), Known::Yes(Some(expected)));
            let own = ScopeRef { file, scope: 1 };
            assert_eq!(files.module_scopes(own, &semantic), Known::Yes(vec![own, expected]));
        });
        for path in [config, library, foreign, target] { reads::drop_buffer(path); }
    }

    #[test]
    fn module_lookup_loads_a_unit_and_missing_candidates_invalidate_on_creation() {
        use crate::grammar::names::modules::compile::{Conditional, Profile};
        use crate::grammar::names::modules::resolution::{Search, Settings};
        static CREATED: crate::expr::queries::QueryFunction = crate::expr::queries::QueryFunction {
            name: "created",
            arity: 1,
            call: |semantic, at, _| match semantic.world.module(at, "./created-unit", semantic) {
                Known::Yes(scope) => Known::Yes(Value::Bool(scope.is_some())),
                Known::Unknown => Known::Unknown,
            },
        };
        let mut linter = linter("in paren fits marked");
        let matcher = linter.grammar.matcher("paren").unwrap();
        linter.grammar.names.kinds[matcher as usize].modules.items.push(Conditional { action: Action::Uses("unit-test".into()), when: None });
        linter.grammar.names.module_resolutions.push(Profile { name: "unit-test".into(), modes: Vec::new(), settings: Settings {
            search: vec![Search::Relative], extensions: vec![".t".into()], candidates: vec!["{dir}/{name}{ext}".into()], ..Settings::default()
        } });
        linter.attrs = crate::expr::Attributes::new(vec![
            crate::expr::Attr { name: "created".into(), defs: vec![(matcher, crate::expr::CExpr::QueryCall(
                &CREATED, vec![crate::expr::CExpr::Builtin(crate::expr::Builtin::Node)],
            ))] },
            crate::expr::Attr { name: "alias".into(), defs: vec![(matcher, crate::expr::CExpr::Attr(0))] },
        ], &[], &linter.grammar);
        linter.attrs.shape_records(&linter.grammar, vec![(crate::vm::KindSet::all(linter.grammar.matchers.len()), &crate::expr::CExpr::Attr(0))]);
        let path = Path::new("/root/module-unit.t");
        reads::set_buffer(path, "(remote)");
        with_world(&linter, |files| {
            let tree = files.tree(0).unwrap();
            let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
            let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
            let q = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &q);
            let Known::Yes(Some(scope)) = files.module(at, "./module-unit", &semantic) else { panic!("module reaches unit") };
            assert_ne!(scope.file, 0);
            assert_eq!(World::text(&files, scope.file), Some("(remote)"));
            let mut bounded = QueryContext::default();
            bounded.budget = crate::grammar::names::question::Budget::new(0);
            let repeated = Semantic::new(&files, &linter.grammar.names, &bounded);
            let (answer, dependencies) = reads::track(|| files.module(at, "./module-unit", &repeated));
            assert_eq!(answer, Known::Yes(Some(scope)));
            reads::set_buffer(path, "(updated)");
            assert!(dependencies.any(&mut |dep| !linter.holds(dep)));
            assert_eq!(files.module(at, "./created-unit", &repeated), Known::Unknown);
            let (answer, deps) = reads::track(|| files.module(at, "./created-unit", &semantic));
            assert_eq!(answer, Known::Yes(None));
            assert_eq!(files.module(at, "./created-unit", &repeated), Known::Yes(None));
            let ctx = files.0.here(at).unwrap();
            for attr in [0, 1] {
                assert_eq!(Session::new(&ctx, node, &[]).value(&VExpr::Attr(attr), None), Some(Value::Bool(false)));
            }
            let created = Path::new("/root/created-unit.t");
            reads::set_buffer(created, "()");
            assert!(deps.any(&mut |dep| !linter.holds(dep)));
            assert!(linter.is_stale(&linter.invalidate(&[created.to_owned()]), &deps));
            for attr in [0, 1] {
                assert_eq!(Session::new(&ctx, node, &[]).value(&VExpr::Attr(attr), None), Some(Value::Bool(true)));
            }
            let Known::Yes(Some(created)) = files.module(at, "./created-unit", &semantic) else { panic!("created module reaches unit") };
            assert_eq!(World::text(&files, created.file), Some("()"));
            reads::drop_buffer(Path::new("/root/created-unit.t"));
        });
        reads::drop_buffer(path);
    }

    #[test]
    fn native_unit_names_use_the_remote_frontend_after_more_units_arrive() {
        let linter = crate::engine::tests::native_linter("");
        let path = Path::new("/root/native-unit.ts");
        let text = r"const \u0061 = 1; a;";
        reads::set_buffer(path, text);
        with_world(&linter, |files| {
            let file = files.0.file(path).unwrap();
            let tree = files.tree(file).unwrap();
            let names = files.names(file).unwrap();
            assert_eq!(files.name_key(file, r"\u0061"), "a");
            let reference = tree.order.iter().copied().find(|&node| {
                let n = tree.node(node);
                &text[n.start as usize..n.end as usize] == "a"
            }).unwrap();
            let declared = names.declaration[&reference];
            let n = tree.node(declared);
            assert_eq!(&text[n.start as usize..n.end as usize], r"\u0061");
            let other = Path::new("/root/native-other-unit.ts");
            reads::set_buffer(other, "const other = 2;");
            let second = files.0.file(other).unwrap();
            assert_ne!(file, second);
            assert_eq!(files.names(file).unwrap().declaration[&reference], declared);
            assert!(files.index(file).unwrap().names(0).any(|(name, _)| name == "a"));
            reads::drop_buffer(other);
        });
        reads::drop_buffer(path);
    }
}

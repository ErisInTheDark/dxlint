//! The files behind type questions: the one engine owner of trees, texts, names
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
use crate::grammar::names::question::{Budget, FrameId, Known, Leaf, NodeRef, NodeValue, ScopeRef, Target, World};
use crate::grammar::names::semantic::{AttributeKey, Semantic};
use crate::grammar::names::modules::compile::Action;
use crate::grammar::names::modules::SharedQuestion;
use crate::grammar::names::modules::resolution::{parse_metadata, program_anchor, program_inputs, Configuration, Probe, DiscoveryRequest, Host, Json, Resolver, Settings};
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
    inputs: RefCell<crate::fx_hash::FxHashMap<*const Settings, super::reads::Cached<Option<Arc<Vec<PathBuf>>>>>>,
    roots: RefCell<crate::fx_hash::FxHashMap<(*const Settings, Box<str>), super::reads::Cached<Known<Vec<ScopeRef>>>>>,
    catalogs: RefCell<crate::fx_hash::FxHashMap<(*const Settings, u64), super::reads::Cached<Arc<super::program::Catalog>>>>,
    modules: RefCell<crate::fx_hash::FxHashMap<(u32, *const Settings), ModuleAnswers>>,
    /// The linted text's hash: a run-shared answer computed from the linted
    /// file's own tree (not a run unit) records it.
    linted_hash: std::cell::OnceCell<u64>,
    /// Reads of the linted file's tree, text, names or index so far.
    linted_reads: Cell<u64>,
    /// The run's unit for the linted file, when it holds the linted text.
    linted_unit: std::cell::OnceCell<Option<Arc<super::units::Unit>>>,
    /// Whether the linted text is what the run reads for its path, so this
    /// world may use and add run-shared answers.
    shares: std::cell::OnceCell<bool>,
    /// Per program and scope classes, the run's augmentation index (`None`
    /// when this world cannot share it).
    augmentations: RefCell<crate::fx_hash::FxHashMap<(*const Settings, u64), Option<super::reads::Cached<Arc<super::program::Augmentations>>>>>,
    /// Per program settings, the linted file when it is not an input.
    outside: RefCell<crate::fx_hash::FxHashMap<*const Settings, (u64, Option<PathBuf>)>>,
    /// Run-shared module answers this world used, by revision and question.
    shared_local: RefCell<crate::fx_hash::FxHashMap<(u64, super::program::StableQuestion), super::reads::Cached<Vec<Target>>>>,
    /// The global ones among them by (revision, class) and name.
    shared_globals: RefCell<crate::fx_hash::FxHashMap<(u64, u64), crate::fx_hash::FxHashMap<Box<str>, super::reads::Cached<Vec<Target>>>>>,
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
    /// A whole-file syntax scan is stepping through bodies
    /// ([`World::quiet_entering`]).
    quiet_entering: Cell<bool>,
}

impl<'a> LintWorld<'a> {
    fn revision(&self) -> u64 {
        self.linter.invalidation.load(std::sync::atomic::Ordering::Acquire)
    }

    fn note_linted_read(&self) {
        self.program.linted_reads.set(self.program.linted_reads.get() + 1);
    }

    fn linted_hash(&self) -> u64 {
        *self.program.linted_hash.get_or_init(|| super::reads::hash_bytes(self.ctx.source.as_bytes()))
    }

    /// The run's unit for the linted file when it holds the linted text: its
    /// bodies are the linted file's.
    fn linted_unit(&self) -> Option<&Arc<super::units::Unit>> {
        self.program.linted_unit.get_or_init(|| {
            let (hash, unit) = self.linter.unit_quiet(self.path, Some(self.snapshot?));
            unit.filter(|_| hash == self.linted_hash())
        }).as_ref()
    }

    /// What run-shared work that read the linted file's syntax read of it:
    /// its text outside its bodies, like any other file it reaches.
    fn linted_read(&self) -> super::reads::Dep {
        match self.linted_unit() {
            Some(unit) => self.unit_read(unit),
            None => super::reads::Dep::Text(super::reads::dep_path(self.path), self.linted_hash()),
        }
    }

    pub fn new(linter: &'a super::Linter, path: &'a Path, ctx: Ctx<'a>) -> Self {
        LintWorld {
            linter, path, views: super::arena::Arena::new(linter), files: Default::default(),
            program: Default::default(), snapshot: None, ctx: Ctx { node: ctx.tree.root, ..ctx },
            evaluation: Cell::new(std::ptr::null()), semantic: Default::default(), conditions: Default::default(), quiet_entering: Cell::new(false),
        }
    }

    pub(super) fn in_run(linter: &'a super::Linter, path: &'a Path, ctx: Ctx<'a>, snapshot: &'a super::program::Snapshot) -> Self {
        Self { snapshot: Some(snapshot), ..Self::new(linter, path, ctx) }
    }

    /// `path`'s file, its text read recorded on every lookup: work in any
    /// frame that reaches the file depends on its text, not only the frame
    /// that loaded it.
    fn file(&'a self, path: &Path) -> Option<u32> {
        if path == self.path { return Some(0); }
        let (file, read) = self.load(path);
        super::reads::note(read);
        file
    }

    /// [`LintWorld::file`] unrecorded, for scans (which record a
    /// [`super::reads::Scan`]) and work that records the file itself.
    fn file_quiet(&'a self, path: &Path) -> Option<u32> {
        if path == self.path { return Some(0); }
        self.load(path).0
    }

    /// `path`'s file, loaded once per world, and its text read.
    fn load(&'a self, path: &Path) -> (Option<u32>, super::reads::Dep) {
        if let Some(&file) = self.files.borrow().get(path) {
            let unit = &self.views.get(file - 1).expect("loaded view").unit;
            return (Some(file), self.unit_read(unit));
        }
        let load = || self.linter.unit_quiet(path, self.snapshot);
        let (hash, unit) = match &self.linter.profile {
            Some(profile) => profile.time_semantic_query("units.view", load),
            None => load(),
        };
        let Some(unit) = unit else { return (None, super::reads::Dep::Text(super::reads::dep_path(path), hash)) };
        let read = self.unit_read(&unit);
        let file = self.views.insert(unit) + 1;
        self.files.borrow_mut().insert(path.to_owned(), file);
        (Some(file), read)
    }

    /// What reaching `unit` reads: its text outside its bodies, until work
    /// steps inside one ([`LintWorld::note_entered`]).
    fn unit_read(&self, unit: &super::units::Unit) -> super::reads::Dep {
        match self.linter.bodies(unit) {
            Some(bodies) => super::reads::Dep::Outside(unit.path.clone(), bodies),
            None => super::reads::Dep::Text(unit.path.clone(), unit.hash),
        }
    }

    /// Work steps onto `at`: inside a file's body, it reads that body too.
    /// The linted file counts through the run's unit of the same text, for
    /// run-shared answers that read it.
    fn note_entered(&self, at: Leaf) {
        let file = match at.target {
            Target::Node(node) => node.file,
            Target::Scope(scope) => scope.file,
            _ => return,
        };
        let unit = match file.checked_sub(1) {
            Some(view) => match self.views.get(view) { Some(view) => &view.unit, None => return },
            None => match self.linted_unit() { Some(unit) => unit, None => return },
        };
        let node = match at.target {
            Target::Scope(scope) => {
                // A scope's names come before any question inside it.
                let Some(names) = unit.names.get() else {
                    if file == 0 { super::reads::note(super::reads::Dep::Text(unit.path.clone(), unit.hash)) }
                    return;
                };
                let Some(info) = names.value.scopes.get(scope.scope as usize) else { return };
                info.node
            }
            Target::Node(node) => node.node,
            _ => return,
        };
        let tree = if file == 0 { self.ctx.tree } else { &unit.parsed.tree };
        let Some(syntax) = tree.nodes.get(node as usize) else { return };
        if let Some(entered) = self.linter.entered(unit, syntax.start) {
            super::reads::note(super::reads::Dep::Outside(unit.path.clone(), entered));
        }
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

    /// The evaluation in progress read something unknown.
    pub(crate) fn mark_unknown(&self) {
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

    /// Where a query about `node` asked inside `question` stands: the
    /// question's frame for nodes of its file, and its scope when that scope
    /// stands for `node`.
    fn asked_at(&'a self, question: &ActiveQuestion<'_, '_>, file: u32, node: u32) -> Leaf {
        let frame = match question.at.target {
            Target::Node(at) if at.file == file => question.at.frame,
            Target::Scope(at) if at.file == file => question.at.frame,
            _ => FrameId::ROOT,
        };
        let target = match question.at.target {
            Target::Scope(scope) if scope.file == file && Files(self).names(file)
                .and_then(|names| names.scopes.get(scope.scope as usize)).is_some_and(|scope| scope.node == node) => question.at.target,
            _ => Target::Node(NodeRef { file, node }),
        };
        Leaf { target, frame }
    }

    /// A semantic attribute read inside a question, memoised per root
    /// session by where its queries stand (see [`Self::asked_at`]); only
    /// complete answers are kept. Outside a question `compute` runs as is.
    pub(crate) fn semantic_attribute(&'a self, file: u32, node: u32, attr: u16, compute: impl FnOnce() -> Option<crate::vm::V>) -> Option<crate::vm::V> {
        let Some(question) = self.question() else { return compute() };
        let semantic = question.semantic;
        // Like `asked_at`, without reading names: a scope target of this
        // file keys the entry as is (names may still be resolving here).
        let (frame, scope) = match question.at.target {
            Target::Scope(scope) if scope.file == file => (question.at.frame, Some(scope.scope)),
            Target::Node(at) if at.file == file => (question.at.frame, None),
            _ => (FrameId::ROOT, None),
        };
        let key = AttributeKey { file, node, frame, scope, attr };
        let values = semantic.values();
        let free = semantic.frame_free_attribute(key);
        // Attribute facts' identity for `Values::fact`: one memo, by key.
        static ATTRIBUTES: u8 = 0;
        let kept = |key: &AttributeKey| semantic.attribute(key).map(|(value, unknown)| {
            if unknown { self.mark_unknown() }
            values.read_kept(crate::grammar::names::values::fact_tag(&ATTRIBUTES, key));
            value
        });
        if let Some(value) = kept(&key) {
            // What its answer asked is asked again by whoever reads it.
            if !free { values.note_value_read() }
            return Some(value);
        }
        // One that asked no value question answers alike in every frame.
        if free && frame != FrameId::ROOT {
            if let Some(value) = kept(&AttributeKey { frame: FrameId::ROOT, ..key }) { return Some(value) }
        }
        // A fact of its own: what it answers never depends on who asked.
        let reads = values.value_reads();
        let mark = values.cycle_mark();
        let mut value = None;
        let answer = values.fact(crate::grammar::names::values::fact_tag(&ATTRIBUTES, &key), Budget::per_question(),
            || self.complete_attribute(|| { value = compute(); value }));
        let unpaid = answer == Err(crate::grammar::names::question::Cut::Unpaid);
        let unknown = !matches!(answer, Ok(Known::Yes(Some(_))));
        if unknown { self.mark_unknown() }
        // Unknown only because something further up is still undecided, or
        // never started: a later ask may know more.
        if let Some(found) = value.filter(|_| !(unknown && (unpaid || values.cycled_since(mark)))) {
            semantic.remember_attribute(key, found, unknown);
            if values.value_reads() == reads { semantic.remember_frame_free(key, found, unknown) }
        }
        value
    }

    /// Something read again while it is still being decided inside a
    /// question (an attribute, a file's names): a cycle, so the question's
    /// unknown answers computed meanwhile are not kept.
    pub(crate) fn note_cycle(&self) {
        if let Some(question) = self.question() { question.semantic.values().note_cycle_read() }
    }

    fn semantic_query<R>(&'a self, file: u32, node: u32, uncertainty: Uncertainty, run: impl FnOnce(&Semantic<'_>, Leaf) -> Known<R>) -> Known<R> {
        let target = Target::Node(NodeRef { file, node });
        if let Some(question) = self.question() {
            let Leaf { target, frame } = self.asked_at(question, file, node);
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
        let _root = self.ctx.profile.and(World::tree(&files, file))
            .map(|tree| crate::engine::Profile::root_scope(tree.node(node).matcher as usize));
        let answer = self.semantic.run(&files, &self.ctx.grammar.names, |semantic| {
            let Known::Yes(at) = semantic.leaf(at) else { return Known::Unknown };
            let answer = match semantic.question.budget.nest() {
                Some(_nested) => self.evaluating(Some(ActiveQuestion { semantic, at }), uncertainty, || run(semantic, at)),
                None => Known::Unknown,
            };
            let budget = &semantic.question.budget;
            if let (Some(profile), Some(tree)) = (self.ctx.profile, World::tree(&files, file)) {
                let used = u64::from(crate::grammar::names::question::Budget::per_question() - budget.remaining());
                profile.record_root(tree.node(node).matcher as usize, used, budget.limited());
                if let Some(path) = World::path(&files, file) { profile.note_root_outcome(path, node, budget.limited()); }
                if budget.limited() {
                    let syntax = tree.node(node);
                    let text = World::text(&files, file).and_then(|text| text.get(syntax.start as usize..syntax.end as usize));
                    let name = World::path(&files, file).and_then(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    profile.record_cut_root(&name, text.unwrap_or(""));
                }
            }
            answer
        });
        if matches!(answer, Known::Unknown) { self.mark_unknown(); }
        answer
    }

    pub fn query(&'a self, file: u32, node: u32, subject: &crate::grammar::names::Value, query: &crate::expr::queries::Query, mask: u64) -> bool {
        // could/always quantify over alternatives and answer Unknown themselves
        // unless a known alternative decides, so their known answers stand.
        let run = || self.semantic_query(file, node, Uncertainty::Explicit, |semantic, at| (query.call)(semantic, at, subject, mask));
        let root = self.question().is_none();
        let started = std::time::Instant::now();
        let answer = match self.ctx.profile {
            Some(profile) => profile.time_semantic_query(query.name, run),
            None => run(),
        };
        // Slots 4/5: known answers (count, ns); 6/7: unknown answers.
        if root { self.census(4, query.name, file, node, answer == Known::Unknown, started) }
        matches!(answer, Known::Yes(true))
    }

    /// [`crate::levers::Lever::Census`]: counts a root question's outcome
    /// and time from `slot`, and prints a root that ended unknown.
    fn census(&'a self, slot: usize, name: &str, file: u32, node: u32, unknown: bool, started: std::time::Instant) {
        if !crate::levers::on(crate::levers::Lever::Census) { return }
        let slot = if unknown { slot + 2 } else { slot };
        let elapsed = started.elapsed().as_nanos() as u64;
        crate::levers::count(slot);
        crate::levers::add(slot + 1, elapsed);
        if !unknown { return }
        let files = Files(self);
        let text = World::tree(&files, file).zip(World::text(&files, file))
            .and_then(|(tree, text)| { let n = tree.node(node); text.get(n.start as usize..n.end as usize) })
            .unwrap_or("").chars().take(90).collect::<String>().replace(['\n', '\r'], " ");
        let path = World::path(&files, file).and_then(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        eprintln!("UNK\t{name}\t{}\t{path}\t{text}", elapsed / 1000);
    }

    pub fn query_function(&'a self, file: u32, node: u32, query: &crate::expr::queries::QueryFunction, args: &[Value<'static>]) -> Value<'static> {
        let run = || self.semantic_query(file, node, query.uncertainty(), |semantic, at| (query.call)(semantic, at, args));
        let root = self.question().is_none();
        let started = std::time::Instant::now();
        let answer = match self.ctx.profile {
            Some(profile) => profile.time_semantic_query(query.name, run),
            None => run(),
        };
        // Slots 8/9: known answers (count, ns); 10/11: unknown answers.
        if root { self.census(8, query.name, file, node, matches!(answer, Known::Unknown), started) }
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
        if !self.quiet_entering.get() { self.note_entered(at) }
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

    /// The resolution context of the program around `at`: its config
    /// options and directory, package metadata and conditions.
    fn resolution_context(self, at: Leaf, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        let Some(ctx) = self.0.here(at) else { return Known::Unknown };
        let Known::Yes(Some(settings)) = self.program_selected() else { return Known::Unknown };
        self.0.asking(semantic, at, || Resolver { host: &self, settings }.context(ctx.file_id, semantic.question))
    }

    fn program_selected(self) -> Known<Option<&'a Settings>> {
        {
            let mut selected = self.0.program.selected.borrow_mut();
            match &*selected {
                Selection::Ready(cached) => return Known::Yes(cached.get()),
                Selection::Busy => {
                    self.0.note_cycle();
                    return Known::Unknown;
                }
                Selection::Vacant => *selected = Selection::Busy,
            }
        }
        let cached = super::reads::Cached::compute(|| {
            let question = QueryContext::once();
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

    fn input_paths(self, selected: &Settings) -> Known<Arc<Vec<PathBuf>>> {
        let key = selected as *const Settings;
        if let Some(cached) = self.0.program.inputs.borrow().get(&key) { return cached.get().map_or(Known::Unknown, Known::Yes) }
        let reentries = super::program::reentries();
        let cached = super::reads::Cached::compute(|| {
            let Known::Yes(anchor) = program_anchor(selected, &self, &QueryContext::once()) else { return Known::Unknown };
            let context = super::program::Context { anchor, settings: selected as *const Settings as usize };
            let load = || self.0.linter.programs.paths(context.clone(), |reads| self.0.linter.reads_hold(reads), || {
                let question = QueryContext::once();
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
        // Kept unknown too (see `program::Inputs`), unless only because the
        // list was still being collected.
        if super::program::reentries() != reentries {
            self.0.note_cycle();
        } else {
            self.0.program.inputs.borrow_mut().insert(key, super::reads::Cached { value: answer.clone().known(), reads: cached.reads });
        }
        answer
    }

    /// `path`'s text and hash, unrecorded: the linted text for the linted
    /// file. Scans record their reads as a [`super::reads::Scan`].
    fn source_text(self, path: &Path) -> (Option<Arc<str>>, u64) {
        if path == self.0.path { return (Some(self.0.ctx.source.into()), self.linted_hash()) }
        match self.0.snapshot {
            Some(snapshot) => snapshot.text(self.0.revision(), path),
            None => super::reads::read_unrecorded(path),
        }
    }

    /// The program's input paths whose text passes `keep` (all when the
    /// profile gives no hints), read but not parsed, and their text reads.
    fn candidate_inputs(self, paths: &[PathBuf], hinted: bool, keep: impl Fn(&str) -> bool) -> (Vec<PathBuf>, Arc<super::reads::Reads>) {
        let mut deps = Vec::with_capacity(paths.len());
        let candidates = paths.iter().filter(|path| {
            let (text, hash) = self.source_text(path);
            deps.push(super::reads::Dep::Text(super::reads::dep_path(path), hash));
            !hinted || text.is_none_or(|text| keep(&text))
        }).cloned().collect();
        (candidates, Arc::new(super::reads::Reads { deps, ..Default::default() }))
    }

    fn roots(self, selected: &Settings, name: &str) -> Known<Vec<ScopeRef>> {
        let hinted = !selected.globals.name_hints.is_empty();
        let key = (selected as *const Settings, if hinted { Box::from(name) } else { Box::from("") });
        if let Some(cached) = self.0.program.roots.borrow().get(&key) { return cached.get() }
        let cached = super::reads::Cached::compute(|| {
            let question = QueryContext::once();
            let roots = (|| {
                let Known::Yes(paths) = self.timed("modules.roots.inputs", || self.input_paths(selected)) else { return Known::Unknown };
                let (paths, files): (Arc<[PathBuf]>, Arc<super::reads::Reads>) = match self.timed("modules.roots.candidates", || match (self.0.snapshot, hinted) {
                    (Some(snapshot), true) if self.shares(snapshot) => {
                        let Known::Yes((program, _)) = self.program_identity(selected) else { return Known::Unknown };
                        let settings = selected as *const Settings as usize;
                        // Hints don't depend on the name: each text is checked
                        // for them once per program, not once per name.
                        let survey = self.timed("modules.roots.survey", || snapshot.survey((settings, program), || {
                            let mut deps = Vec::with_capacity(paths.len());
                            let sources = paths.iter().map(|path| {
                                let (text, hash) = self.source_text(path);
                                deps.push(super::reads::Dep::Text(super::reads::dep_path(path), hash));
                                let hinted = text.as_deref().is_none_or(|text| selected.globals.name_hints.iter().any(|hint| text.contains(&**hint)));
                                (path.clone(), text, hinted)
                            }).collect();
                            (sources, Arc::new(super::reads::Reads { deps, ..Default::default() }))
                        }));
                        let (sources, files) = survey.get();
                        // Kept as survey positions: a run asks thousands of
                        // names, each with up to every input as a candidate.
                        let shared = snapshot.candidates((settings, program, Box::from(name)), || {
                            // Only texts holding every trigram of the name are
                            // searched for it, not every input per name.
                            let sketches = snapshot.words((settings, program), || super::program::sketch_texts(&sources)).get();
                            let mut holding = vec![false; sources.len()];
                            sketches.mark(name, &sources, &mut holding);
                            sources.iter().zip(holding).enumerate()
                                .filter(|(_, ((_, _, hinted), holds))| *hinted || *holds)
                                .map(|(at, _)| u32::try_from(at).expect("inputs fit u32")).collect()
                        });
                        Known::Yes((shared.get().iter().map(|&at| sources[at as usize].0.clone()).collect(), files))
                    }
                    _ => {
                        let (candidates, files) = self.candidate_inputs(&paths, hinted, |text| selected.globals.may_publish(text, name));
                        Known::Yes((candidates.into(), files))
                    }
                }) {
                    Known::Yes(found) => found,
                    Known::Unknown => return Known::Unknown,
                };
                // Each input was asked only whether it can declare the name
                // (`depends_on_file` records the ones that can).
                let hints = selected.globals.name_hints.clone().into_boxed_slice();
                super::reads::note_scan(&Arc::new(super::reads::Scan { files, test: super::reads::ScanTest::Global { name: name.into(), hints } }));
                self.timed("modules.roots.prefetch", || self.prefetch_inputs(selected, &paths));
                let attached = self.timed("modules.roots.attach", || {
                    let mut roots = Vec::new();
                    let mut seen = crate::fx_hash::FxHashSet::default();
                    for path in paths.iter() {
                        if !question.budget.step() { return None }
                        let file = self.0.file_quiet(path)?;
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
        use crate::alloc::heap;
        let owner = if query.starts_with("modules.roots") { Some(heap::ROOTS) }
            else if query.ends_with(".catalog") { Some(heap::CATALOG) }
            else if query.starts_with("modules.resolve.shared") { Some(heap::SHARED) }
            else { None };
        let _owner = owner.map(heap::Owner::enter);
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
                let found = self.shared_path(scope.file).and_then(|path| index.get(&(path, scope.scope)).cloned()).unwrap_or_default();
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
                    super::reads::note(self.0.linted_read());
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
        self.0.linted_hash()
    }

    /// Whether `path` is a file, recorded; probed once per run.
    fn probe_file(self, path: &Path) -> bool {
        let found = match self.0.snapshot {
            Some(snapshot) => snapshot.is_file(self.0.revision(), path),
            None => super::reads::is_file_unrecorded(path),
        };
        super::reads::note(super::reads::Dep::IsFile(super::reads::dep_path(path), found));
        found
    }

    /// A run unit's path, for questions about it shared across worlds; `None`
    /// for the linted file (see [`World::shared_modules`]).
    fn shared_path(self, file: u32) -> Option<super::reads::DepPath> {
        Some(self.0.views.get(file.checked_sub(1)?)?.unit.path.clone())
    }

    /// The run-shared answer to `question` under this world's program,
    /// computing (and sharing) it on a miss ([`World::shared_modules`]).
    fn shared(self, question: super::program::StableQuestion, compute: &mut dyn FnMut() -> Known<Vec<Target>>) -> Known<Vec<Target>> {
        let Some(snapshot) = self.0.snapshot else { return compute() };
        // This world's own copy of every shared answer it used: a repeat
        // (the same global asked a million times a run) skips the program
        // key, the run-wide lookup and the path-to-file mapping.
        let local = (self.0.revision(), question.clone());
        if let Some(found) = self.0.program.shared_local.borrow().get(&local) {
            super::reads::note_group(&found.reads);
            return Known::Yes(found.value.clone());
        }
        // Shared answers see every file as the run reads it; a world whose
        // linted text differs (a fix pass, an unsaved edit) keeps its own.
        let (true, Some(key)) = self.timed("modules.resolve.shared.key", || (self.shares(snapshot), self.module_key(question))) else { return compute() };
        if let Some(shared) = self.timed("modules.resolve.shared.lookup", || snapshot.module_answer(&key)) {
            if let Some(targets) = self.timed("modules.resolve.shared.targets", || self.local_targets(&shared.value)) {
                self.timed("modules.resolve.shared.record", || super::reads::note_group(&shared.reads));
                self.0.program.shared_local.borrow_mut().insert(local, super::reads::Cached { value: targets.clone(), reads: shared.reads.clone() });
                return Known::Yes(targets);
            }
        }
        // Every other input is read as a run unit, which records its read;
        // the linted file's own tree only counts when the answer read it.
        let answer = super::reads::Cached::compute(|| {
            let before = self.0.program.linted_reads.get();
            let value = compute();
            if self.0.program.linted_reads.get() != before {
                super::reads::note(self.0.linted_read());
            }
            value
        });
        self.timed("modules.resolve.shared.record", || super::reads::note_group(&answer.reads));
        if let Known::Yes(targets) = &answer.value {
            self.timed("modules.resolve.shared.publish", || {
                if let Some(stable) = self.stable_targets(targets) {
                    if let Some(profile) = self.0.ctx.profile {
                        let kind = match &key.question {
                            super::program::StableQuestion::Publication { .. } => 0,
                            super::program::StableQuestion::Global { .. } => 1,
                            super::program::StableQuestion::Scopes { .. } => 2,
                            super::program::StableQuestion::Resolve { specifier, .. } if specifier.starts_with('.') => 3,
                            super::program::StableQuestion::Resolve { .. } => 4,
                            super::program::StableQuestion::Augmentations { .. } => 5,
                        };
                        super::Profile::add(&profile.shared_answers[kind], 1);
                        super::Profile::add(&profile.shared_answer_deps[kind], answer.reads.deps.len() as u64);
                        super::Profile::add(&profile.shared_answer_groups[kind], answer.reads.groups.len() as u64);
                    }
                    snapshot.keep_module_answer(key, super::reads::Cached { value: stable, reads: answer.reads.clone() });
                }
            });
            self.0.program.shared_local.borrow_mut().insert(local, super::reads::Cached { value: targets.clone(), reads: answer.reads.clone() });
        }
        answer.value
    }

    /// `question` under this world's program; `None` while the program is
    /// not settled.
    fn module_key(self, question: super::program::StableQuestion) -> Option<super::program::ModuleKey> {
        let Known::Yes(Some(selected)) = self.timed("modules.resolve.shared.select", || self.program_selected()) else { return None };
        if let super::program::StableQuestion::Resolve { specifier, .. } = &question {
            if selected.resolves_from_directory(specifier) {
                return Some(super::program::ModuleKey { revision: self.0.revision(), settings: selected as *const Settings as usize, program: 0, outside: None, question });
            }
        }
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
        let Known::Yes(paths) = self.timed("modules.identity.inputs", || self.input_paths(selected)) else { return Known::Unknown };
        // Worlds of one run share the run's input list (the snapshot holds
        // it for the run), so its address names the program without hashing
        // every path again in every world.
        let program = Arc::as_ptr(&paths) as usize as u64;
        // File names first: whole-path comparison parses every component.
        let name = self.0.path.file_name();
        let outside = self.timed("modules.identity.scan", || (!paths.iter().any(|path| path.file_name() == name && path == self.0.path))
            .then(|| self.0.path.to_owned()));
        self.0.program.outside.borrow_mut().insert(selected as *const Settings, (program, outside.clone()));
        Known::Yes((program, outside))
    }

    fn stable_targets(self, targets: &[Target]) -> Option<Arc<[super::program::StableTarget]>> {
        targets.iter().map(|target| match *target {
            Target::Node(node) => Some(super::program::StableTarget::Node(self.shared_path(node.file)?, node.node)),
            Target::Scope(scope) => Some(super::program::StableTarget::Scope(self.shared_path(scope.file)?, scope.scope)),
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

    /// Where the run keeps `leaf`'s value, when every linted file of this
    /// program would compute the same one.
    fn value_key(self, leaf: Leaf, semantic: &Semantic<'_>) -> Option<(&'a super::program::Snapshot, super::program::ValueKey)> {
        let snapshot = self.0.snapshot.filter(|&snapshot| self.shares(snapshot))?;
        let stable = self.stable_leaf(leaf, semantic)?;
        let Known::Yes(Some(selected)) = self.program_selected() else { return None };
        let Known::Yes((program, outside)) = self.program_identity(selected) else { return None };
        Some((snapshot, (self.0.revision(), selected as *const Settings as usize, program, outside, stable)))
    }

    fn taken_value(self, snapshot: &super::program::Snapshot, key: &super::program::ValueKey, semantic: &Semantic<'_>) -> Option<NodeValue> {
        let found = snapshot.value_answer(key)?;
        let value = match &found.value {
            Some(value) => self.local_value(value, semantic)?,
            None => NodeValue::Unknown,
        };
        if let Some(profile) = self.0.ctx.profile { super::Profile::add(&profile.shared_value_hits, 1) }
        super::reads::note_group(&found.reads);
        Some(value)
    }

    /// `leaf` by path, when every world builds its frame alike (none, or a
    /// declaration's shared frame) and it isn't in the linted file.
    fn stable_leaf(self, leaf: Leaf, semantic: &Semantic<'_>) -> Option<super::program::StableLeaf> {
        use super::program::StableLeaf;
        let frame = self.stable_frame(leaf.frame, semantic)?;
        match leaf.target {
            Target::Node(node) => Some(StableLeaf::Node(self.shared_path(node.file)?, node.node, frame)),
            Target::Scope(scope) => Some(StableLeaf::Scope(self.shared_path(scope.file)?, scope.scope, frame)),
            Target::Traits(bits) => Some(StableLeaf::Traits(bits, frame)),
            _ => None,
        }
    }

    fn stable_frame(self, frame: FrameId, semantic: &Semantic<'_>) -> Option<super::program::StableFrame> {
        use super::program::{StableBinding, StableFrame};
        if frame == FrameId::ROOT { return Some(StableFrame::Root) }
        let snapshot = self.0.snapshot?;
        let id = semantic.question.world_frame(frame, || {
            let binding = semantic.question.get(frame)?;
            let through = match binding.through {
                Some(leaf) => Some(self.stable_leaf(leaf, semantic)?),
                None => None,
            };
            let args = match binding.args {
                Some(id) => {
                    let (overlay, slots) = semantic.question.arguments(id)?;
                    let slots = slots.iter().map(|slot| match slot {
                        Some(value) => self.stable_value(value, semantic).map(Some),
                        None => Some(None),
                    }).collect::<Option<Box<[_]>>>()?;
                    Some((overlay, slots))
                }
                None => None,
            };
            let StableFrame::Bound(id) = snapshot.stable_frame(StableBinding {
                parent: self.stable_frame(binding.parent, semantic)?,
                owner: (self.shared_path(binding.owner.file)?, binding.owner.node),
                through,
                args,
                classes: binding.classes,
            }) else { return None };
            Some(u64::from(id))
        })?;
        Some(StableFrame::Bound(u32::try_from(id).ok()?))
    }

    fn local_frame(self, frame: super::program::StableFrame, semantic: &Semantic<'_>) -> Option<FrameId> {
        let super::program::StableFrame::Bound(id) = frame else { return Some(FrameId::ROOT) };
        let snapshot = self.0.snapshot?;
        semantic.question.frame_for_world(u64::from(id), || {
            let binding = snapshot.stable_binding(id)?;
            let parent = self.local_frame(binding.parent, semantic)?;
            let owner = NodeRef { file: self.0.file(&binding.owner.0).filter(|&file| file != 0)?, node: binding.owner.1 };
            let through = match &binding.through {
                Some(leaf) => Some(self.local_leaf(leaf, semantic)?),
                None => None,
            };
            let question = &semantic.question;
            Some(match &binding.args {
                None => question.bind_definition(parent, owner, through, None, binding.classes),
                Some((overlay, slots)) => {
                    let slots = slots.iter().map(|slot| match slot {
                        Some(value) => self.local_value(value, semantic).map(Some),
                        None => Some(None),
                    }).collect::<Option<Vec<_>>>()?;
                    if *overlay { question.bind_overlay(parent, owner, slots, binding.classes) }
                    else { question.bind_slots(parent, owner, through, slots, binding.classes) }
                }
            })
        })
    }

    fn stable_value(self, value: &NodeValue, semantic: &Semantic<'_>) -> Option<super::program::StableValue> {
        let NodeValue::Alternatives(rows) = value else { return None };
        let stable = rows.iter().map(|row| row.iter().map(|&leaf| self.stable_leaf(leaf, semantic)).collect::<Option<Box<[_]>>>())
            .collect::<Option<Arc<[_]>>>()?;
        Some((stable, rows.normal()))
    }

    fn local_leaf(self, leaf: &super::program::StableLeaf, semantic: &Semantic<'_>) -> Option<Leaf> {
        use super::program::StableLeaf;
        let (target, frame) = match leaf {
            StableLeaf::Node(path, node, frame) => (Target::Node(NodeRef { file: self.0.file(path).filter(|&file| file != 0)?, node: *node }), frame),
            StableLeaf::Scope(path, scope, frame) => (Target::Scope(ScopeRef { file: self.0.file(path).filter(|&file| file != 0)?, scope: *scope }), frame),
            StableLeaf::Traits(bits, frame) => (Target::Traits(*bits), frame),
        };
        Some(Leaf { target, frame: self.local_frame(*frame, semantic)? })
    }

    fn local_value(self, value: &super::program::StableValue, semantic: &Semantic<'_>) -> Option<NodeValue> {
        let (rows, normal) = value;
        let rows = rows.iter().map(|row| row.iter().map(|leaf| self.local_leaf(leaf, semantic)).collect::<Option<Vec<_>>>())
            .collect::<Option<Vec<_>>>()?;
        Some(NodeValue::Alternatives(crate::grammar::names::question::Rows::normalized(rows, *normal)))
    }

    fn catalog(self, selected: &Settings, classes: u64, semantic: &Semantic<'_>) -> Known<Arc<super::program::Catalog>> {
        let key = (selected as *const Settings, classes);
        if let Some(cached) = self.0.program.catalogs.borrow().get(&key) { return Known::Yes(cached.get()) }
        let reentries = super::program::reentries();
        let cached = super::reads::Cached::compute(|| {
            let Known::Yes((program, outside)) = self.program_identity(selected) else { return Known::Unknown };
            // The program's catalog is shared by every linted file, inside
            // its inputs or not: a file outside adds its own declarations on
            // top, instead of a whole catalog of its own.
            let key = super::program::CatalogKey { settings: selected as *const Settings as usize, program, classes };
            let current_hash = super::reads::hash_bytes(self.0.ctx.source.as_bytes());
            // A catalog read another text of the linted file than this lint's.
            let linted_differs = |dep: &super::reads::Dep| matches!(dep, super::reads::Dep::Text(path, hash) if **path == *self.0.path && *hash != current_hash);
            let load = || self.0.linter.programs.catalog(key.clone(), self.0.ctx.profile, |reads| reads.holds(
                &mut |dep| !linted_differs(dep) && dep.holds(),
                &mut |scan| !scan.files.deps.iter().any(linted_differs) && self.0.linter.scan_holds(scan),
            ), || match self.0.complete_attribute(|| {
                let Known::Yes(paths) = self.input_paths(selected) else { return Known::Unknown };
                let Known::Yes((declarations, texts)) = self.catalog_declarations(selected, classes, &paths, semantic) else { return Known::Unknown };
                Known::Yes(super::program::Catalog::new(declarations, texts))
            }) {
                Known::Yes(answer) => answer,
                Known::Unknown => Known::Unknown,
            });
            let Known::Yes(base) = (match self.0.snapshot {
                // The linted file's own text is checked only where the
                // catalog lists it: as an input.
                Some(snapshot) => snapshot.catalog(self.0.revision(), key.clone(), outside.is_none().then_some((self.0.path, current_hash)), load),
                None => load(),
            }) else { return Known::Unknown };
            let Some(outside) = outside else { return Known::Yes(base) };
            let Known::Yes(Known::Yes((declarations, texts))) = self.0.complete_attribute(|| self.catalog_declarations(selected, classes, std::slice::from_ref(&outside), semantic))
                else { return Known::Unknown };
            Known::Yes(Arc::new(super::program::Catalog::over(base, declarations, texts)))
        });
        let answer = cached.get();
        if let Known::Yes(catalog) = &answer {
            self.0.program.catalogs.borrow_mut().insert(key, super::reads::Cached { value: catalog.clone(), reads: cached.reads });
        }
        // Unknown because the catalog was still being built: a cycle.
        if super::program::reentries() != reentries { self.0.note_cycle() }
        answer
    }

    /// The ambient declarations of these classes in `paths`, with each
    /// path's text hash.
    fn catalog_declarations(self, selected: &Settings, classes: u64, paths: &[PathBuf], semantic: &Semantic<'_>)
        -> Known<(Vec<super::program::Declaration>, crate::fx_hash::FxHashMap<super::reads::DepPath, u64>)> {
        let mut declarations = Vec::new();
        let mut texts = crate::fx_hash::FxHashMap::default();
        // Files whose text cannot declare an ambient scope are read but
        // never parsed: they stay out of it while their text still cannot.
        let mut candidates = Vec::new();
        let mut scanned = Vec::with_capacity(paths.len());
        for path in paths {
            let (Some(text), hash) = self.source_text(path) else { return Known::Unknown };
            let path_read = super::reads::dep_path(path);
            texts.insert(path_read.clone(), hash);
            scanned.push(super::reads::Dep::Text(path_read, hash));
            if selected.globals.may_declare_ambient(&text) { candidates.push(path.clone()) }
        }
        let hints = selected.globals.ambient_hints.clone().into_boxed_slice();
        super::reads::note_scan(&Arc::new(super::reads::Scan {
            files: Arc::new(super::reads::Reads { deps: scanned, ..Default::default() }),
            test: super::reads::ScanTest::Ambient { hints, classes },
        }));
        self.prefetch_inputs(selected, &candidates);
        // Only files that open a module scope of these classes contribute;
        // the scan stands for the others while they still open none.
        for path in &candidates {
            let Some(file) = self.0.file_quiet(path) else { return Known::Unknown };
            if !self.opens_scopes(file, classes, semantic) { continue }
            World::depends_on_file(&self, file);
            let Some(names) = World::names(&self, file) else { return Known::Unknown };
            let shared_path = super::reads::dep_path(path);
            for (scope, info) in names.scopes.iter().enumerate() {
                if classes & info.classes == 0 { continue }
                let Known::Yes(items) = self.named_scope(ScopeRef { file, scope: scope as u32 }, classes, semantic) else { return Known::Unknown };
                declarations.extend(items.into_iter().map(|item| super::program::Declaration {
                    path: shared_path.clone(), scope: item.scope.scope, node: info.node,
                    name: item.name, source: item.source, classes: item.classes,
                }));
            }
        }
        Known::Yes((declarations, texts))
    }

    /// Whether `file` has a node opening a module scope of `classes`; one
    /// whose condition cannot be decided counts.
    fn opens_scopes(self, file: u32, classes: u64, semantic: &Semantic<'_>) -> bool {
        let Some(tree) = World::tree(&self, file) else { return true };
        let kinds = &self.0.ctx.grammar.names.kinds;
        tree.order.iter().any(|&node| {
            let Some(kind) = kinds.get(tree.node(node).matcher as usize) else { return false };
            let at = Leaf { target: Target::Node(NodeRef { file, node }), frame: FrameId::ROOT };
            kind.modules.items.iter()
                .filter(|item| matches!(item.action, Action::Scope { classes: opened, .. } if opened & classes != 0))
                .any(|item| item.when.is_none_or(|when| World::condition(&self, at, when, semantic) != Known::Yes(false)))
        })
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

    fn unknown_read(&self) -> Option<bool> {
        self.0.active_evaluation().map(|evaluation| evaluation.unknown.get())
    }

    fn forget_unknown_reads(&self, read: bool) {
        if let Some(evaluation) = self.0.active_evaluation() { evaluation.unknown.set(read) }
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
        if file == 0 { self.0.note_linted_read(); return Some(self.0.ctx.tree); }
        Some(&self.0.views.get(file.checked_sub(1)?)?.unit.parsed.tree)
    }

    fn text(&self, file: u32) -> Option<&str> {
        if file == 0 { self.0.note_linted_read(); return Some(self.0.ctx.source); }
        Some(&self.0.views.get(file.checked_sub(1)?)?.unit.text)
    }

    fn names(&self, file: u32) -> Option<&Resolved> {
        if file == 0 { self.0.note_linted_read() }
        // A run unit's names resolved by any world are complete and shared:
        // read them without building this world's expression context.
        if let Some(shared) = self.0.name_cache(file).and_then(std::sync::OnceLock::get) {
            return Some(&**shared.as_ref());
        }
        let ctx = self.0.on(file, self.tree(file)?.root)?;
        // A question asked by the file's own name conditions cannot see its
        // names: unknown, and a cycle so nothing computed from it is kept.
        if ctx.names_resolving() {
            self.0.note_cycle();
            self.0.mark_unknown();
            return None;
        }
        let names = {
            let _owner = crate::alloc::heap::Owner::enter(crate::alloc::heap::NAMES);
            ctx.names()
        };
        if names.complete { Some(names) } else {
            self.0.mark_unknown();
            None
        }
    }

    fn linted_reads(&self) -> u64 {
        self.0.program.linted_reads.get()
    }

    fn note_entered(&self, at: Leaf) {
        if !self.0.quiet_entering.get() { self.0.note_entered(at) }
    }

    fn quiet_entering(&self, quiet: bool) -> bool {
        self.0.quiet_entering.replace(quiet)
    }

    fn note_linted_read(&self) {
        self.0.note_linted_read()
    }

    fn index(&self, file: u32) -> Option<&ScopeIndex> {
        if file == 0 { self.0.note_linted_read() }
        let ctx = self.0.on(file, self.tree(file)?.root)?;
        Some(ctx.names().index(ctx.tree, ctx.source, &|text| self.0.key(file, text)))
    }

    fn name_key<'s>(&self, file: u32, text: &'s str) -> Cow<'s, str> {
        self.0.key(file, text)
    }

    fn depends_on_file(&self, file: u32) {
        let read = match file.checked_sub(1) {
            None => self.0.linted_read(),
            // What a file declares for everyone lies outside its bodies.
            Some(view) => {
                let Some(view) = self.0.views.get(view) else { return };
                self.0.unit_read(&view.unit)
            }
        };
        super::reads::note(read);
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
                Some(ModuleAnswer::Busy(version)) if *version == revision => {
                    self.0.note_cycle();
                    return Known::Unknown;
                }
                _ => {}
            }
        }
        self.0.program.modules.borrow_mut().entry(key).or_default().insert(specifier.into(), ModuleAnswer::Busy(revision));
        // Resolution depends only on the importer's directory (not its tree
        // or name), so imports are shared by directory, the linted file's too.
        let resolve = || self.timed("modules.resolve.search", || semantic.question.budget.sized(crate::grammar::names::question::Budget::DEFAULT,
            || self.0.asking(semantic, at, || Resolver { host: self, settings: selected }.resolve(ctx.file_id, specifier, &[], semantic.question)))
            .unwrap_or(Known::Unknown));
        let answer = super::reads::Cached::compute(|| match World::path(self, ctx.file_id).and_then(Path::parent) {
            Some(directory) => {
                let question = super::program::StableQuestion::Resolve { directory: super::reads::dep_path(directory), specifier: specifier.into() };
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
        match self.resolution_context(at, semantic) {
            Known::Yes(context) => Known::Yes(context.key("config").key(name)),
            Known::Unknown => Known::Unknown,
        }
    }

    fn program_directory(&self, at: Leaf, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        match self.resolution_context(at, semantic) {
            Known::Yes(context) => Known::Yes(context.key("directory")),
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
            SharedQuestion::Global { name, class } => {
                // The hottest module question (a million a run): repeats are
                // found by the borrowed name, building no key.
                let revision = self.0.revision();
                if let Some(found) = self.0.program.shared_globals.borrow().get(&(revision, class)).and_then(|names| names.get(name)) {
                    super::reads::note_group(&found.reads);
                    return Known::Yes(found.value.clone());
                }
                let question = super::program::StableQuestion::Global { name: name.into(), class };
                let answer = self.shared(question.clone(), compute);
                if let Some(found) = self.0.program.shared_local.borrow().get(&(revision, question)) {
                    self.0.program.shared_globals.borrow_mut().entry((revision, class)).or_default().insert(name.into(), found.clone());
                }
                return answer;
            }
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

    fn shared_value_kept(&self, leaf: Leaf, semantic: &Semantic<'_>) -> Option<NodeValue> {
        let (snapshot, key) = self.value_key(leaf, semantic)?;
        self.taken_value(snapshot, &key, semantic)
    }

    fn shared_value(&self, leaf: Leaf, semantic: &Semantic<'_>, compute: &mut dyn FnMut() -> NodeValue) -> NodeValue {
        let profile = self.0.ctx.profile;
        let started = profile.map(|_| super::Profile::thread_steps());
        let skip = |reason: usize| if let (Some(profile), Some(started)) = (profile, started) {
            super::Profile::add(&profile.shared_value_skips[reason], super::Profile::thread_steps() - started);
        };
        let Some((snapshot, key)) = self.value_key(leaf, semantic) else {
            let value = compute();
            skip(if self.stable_frame(leaf.frame, semantic).is_none() { 0 } else { 1 });
            return value;
        };
        if let Some(value) = self.taken_value(snapshot, &key, semantic) { return value }
        let values = semantic.values();
        let before = values.root_reads();
        let mark = values.cycle_mark();
        let answer = super::reads::Cached::compute(|| compute());
        super::reads::note_group(&answer.reads);
        // Only a value no step of which read the linted file is every
        // linted file's (a global augmentation there would change it). An
        // unknown one is too, unless it rests on something still undecided
        // (as in [`crate::grammar::names::values::Values::memoized`]).
        let stable = if values.root_reads() != before { Err(3) }
            else if semantic.question.budget.limited() { Err(4) }
            else if answer.value == NodeValue::Unknown { if values.cycled_since(mark) { Err(2) } else { Ok(None) } }
            else { self.stable_value(&answer.value, semantic).map(Some).ok_or(5) };
        match stable {
            Ok(stable) => {
                if let Some(profile) = profile { super::Profile::add(&profile.shared_value_publishes, 1) }
                snapshot.keep_value(key, super::reads::Cached { value: stable, reads: answer.reads.clone() });
            }
            Err(reason) => skip(reason),
        }
        answer.value
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

    fn refuted_by_syntax(&self, at: Leaf, condition: u32) -> bool {
        let world = self.0;
        let Some(item) = world.ctx.grammar.names.conditions.get(condition as usize) else { return false };
        let Some(lowered) = item.lowered.get() else { return false };
        let Some(guard) = item.guard.get_or_init(|| lowered.syntax_guard()) else { return false };
        let Some(ctx) = world.here(at) else { return false };
        let held = Session::new(&ctx, ctx.node, &[]).holds_on(ctx.node, guard);
        held == Some(false)
    }

    fn condition(&self, at: Leaf, condition: u32, semantic: &Semantic<'_>) -> Known<bool> {
        let world = self.0;
        let lowered = world.ctx.grammar.names.conditions.get(condition as usize).and_then(|c| c.lowered.get());
        let (Some(ctx), Some(e)) = (world.here(at), lowered) else { return Known::Unknown };
        // A condition asked again while it is being decided on the same
        // node (in any frame: each round can bind a new one) is a cycle.
        let key = (at.target, condition);
        if world.conditions.borrow().contains(&key) {
            semantic.values().note_cycle_read();
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
        self.0.views.get(file.saturating_sub(1)).map_or(Known::Unknown, |view| Known::Yes(view.unit.path.to_path_buf()))
    }

    fn text(&self, path: &Path, _: &QueryContext) -> Known<Option<String>> {
        if path == self.0.path { return Known::Yes(Some(self.0.ctx.source.to_owned())); }
        let (text, hash) = match self.0.snapshot {
            Some(snapshot) => snapshot.text(self.0.revision(), path),
            None => super::reads::read_unrecorded(path),
        };
        super::reads::note(super::reads::Dep::Text(super::reads::dep_path(path), hash));
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

    fn package(&self, settings: &Settings, root: &Path, subpath: &str, conditions: &[&str],
        probe: &mut dyn FnMut() -> Result<Probe<PathBuf>, ()>) -> Result<Probe<PathBuf>, ()> {
        match self.0.snapshot {
            Some(snapshot) if self.shares(snapshot) => {
                let key = (self.0.revision(), settings as *const Settings as usize, root.to_owned(), subpath.into(),
                    conditions.iter().map(|&condition| Box::from(condition)).collect());
                snapshot.package(key, probe)
            }
            _ => probe(),
        }
    }

    fn metadata(&self, path: &Path, question: &QueryContext) -> Result<Option<Arc<Json>>, ()> {
        let Some(snapshot) = self.0.snapshot.filter(|_| path != self.0.path) else {
            return parse_metadata(Host::text(self, path, question).known().ok_or(())?.as_deref());
        };
        let (text, hash) = snapshot.text(self.0.revision(), path);
        super::reads::note(super::reads::Dep::Text(super::reads::dep_path(path), hash));
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

/// Whether `world`'s linted file opens a module scope of `classes` (see
/// [`Files::opens_scopes`]).
pub(super) fn opens_scopes<'a>(world: &'a LintWorld<'a>, classes: u64) -> bool {
    let files = Files(world);
    let question = QueryContext::once();
    let semantic = Semantic::new(&files, &world.ctx.grammar.names, &question);
    files.opens_scopes(0, classes, &semantic)
}

/// Whether some of `items` (a node of the world's file, and the condition
/// on it) holds there or cannot be decided.
pub(super) fn any_holds<'a>(world: &'a LintWorld<'a>, items: &[(u32, Option<u32>)]) -> bool {
    let files = Files(world);
    let question = QueryContext::once();
    let semantic = Semantic::new(&files, &world.ctx.grammar.names, &question);
    items.iter().any(|&(node, when)| {
        let at = Leaf { target: Target::Node(NodeRef { file: 0, node }), frame: FrameId::ROOT };
        when.is_none_or(|when| World::condition(&files, at, when, &semantic) != Known::Yes(false))
    })
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
                    ]), 1)],
                },
                crate::expr::Attr { name: "alias".into(), defs: vec![(matcher, crate::expr::CExpr::Attr(0), 2)] },
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
    fn incomplete_root_attributes_are_computed_once() {
        thread_local! { static ATTEMPTS: Cell<u32> = const { Cell::new(0) }; }
        static QUERY: crate::expr::queries::QueryFunction = crate::expr::queries::QueryFunction {
            name: "unknowable",
            arity: 1,
            call: |_, _, _| ATTEMPTS.with(|attempts| {
                attempts.set(attempts.get() + 1);
                Known::Unknown
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
                ), 1)],
            }], &[], &linter.grammar);
            if shaped {
                linter.attrs.shape_records(&linter.grammar, vec![(crate::vm::KindSet::all(linter.grammar.matchers.len()), &crate::expr::CExpr::Attr(0))]);
            }
            with_world(&linter, |files| {
                let tree = files.tree(0).unwrap();
                let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
                let ctx = files.0.on(0, node).unwrap();
                let read = || Session::new(&ctx, node, &[]).value(&VExpr::Attr(0), None).map(Value::into_static);
                for _ in 0..3 { assert_eq!(read(), Some(Value::Bool(true))) }
                assert_eq!(ATTEMPTS.with(Cell::get), 1, "shaped={shaped}");
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
    fn every_lookup_of_a_loaded_file_records_its_text_and_quiet_ones_none() {
        let linter = linter("in paren fits marked");
        let path = Path::new("/root/unit-relookup.t");
        reads::set_buffer(path, "(text)");
        with_world(&linter, |files| {
            // A shared answer worked out after the load still depends on it.
            for _ in 0..2 {
                let (_, deps) = reads::track(|| files.0.file(path).expect("unit loads"));
                assert!(deps.any(&mut |dep| matches!(dep, reads::Dep::Text(p, _) if **p == *path)));
            }
            let (_, quiet) = reads::track(|| files.0.file_quiet(path).expect("unit loads"));
            assert!(quiet.is_empty());
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
            assert!(deps.any(&mut |dep| matches!(dep, reads::Dep::Text(p, _) if **p == *path)));
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
    fn stale_scans_judge_only_the_changed_files() {
        let linter = linter("in paren fits marked");
        let (declares, other) = (Path::new("/root/scan-declares.t"), Path::new("/root/scan-other.t"));
        reads::set_buffer(declares, "()");
        reads::set_buffer(other, "()");
        let text = |path: &Path| reads::Dep::Text(reads::dep_path(path), reads::read_unrecorded(path).1);
        let scan = reads::Scan {
            files: std::sync::Arc::new(reads::Reads { deps: vec![text(declares), text(other)], ..Default::default() }),
            test: reads::ScanTest::Global { name: "G".into(), hints: Box::new([]) },
        };
        let deps = reads::Reads { scans: vec![std::sync::Arc::new(scan)], ..Default::default() };
        // `declares` now spells G (and cannot be shown not to declare it),
        // but only `other` is reported changed.
        reads::set_buffer(declares, "(G)");
        reads::set_buffer(other, "( )");
        assert!(!linter.is_stale(&linter.invalidate(&[other.to_owned()]), &deps));
        assert!(linter.is_stale(&linter.invalidate(&[declares.to_owned()]), &deps));
        for path in [declares, other] { reads::drop_buffer(path) }
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
        assert!(deps.any(&mut |dep| matches!(dep, reads::Dep::Text(p, hash) if **p == *path && *hash == first_hash)));
        assert!(deps.any(&mut |dep| !linter.holds(dep)), "the replayed read still detects the edit for caching");
        let fresh = linter.unit(path, Some(&super::super::program::Snapshot::default())).unwrap();
        assert_eq!(&*fresh.text, "(after)");
        linter.invalidate(&[path.to_owned()]);
        assert_eq!(&*linter.unit(path, Some(&run)).unwrap().text, "(after)", "invalidation ends the run's snapshot");
        reads::drop_buffer(path);
    }

    #[test]
    fn a_carried_run_rereads_only_what_invalidations_since_changed() {
        let linter = linter("in paren fits marked");
        let (kept, edited) = (Path::new("/root/carry-kept.t"), Path::new("/root/carry-edited.t"));
        reads::set_buffer(kept, "(kept)");
        reads::set_buffer(edited, "(edited)");
        let run = linter.run();
        for path in [kept, edited] { linter.unit(path, Some(&run.programs)).unwrap(); }
        // Only invalidated files count as changed: the carried run keeps
        // its read of the other one.
        reads::set_buffer(kept, "(kept, unannounced)");
        reads::set_buffer(edited, "(edited again)");
        linter.invalidate(&[edited.to_owned()]);
        let next = linter.run_after(Some(run.carried()));
        assert_eq!(&*linter.unit(kept, Some(&next.programs)).unwrap().text, "(kept)");
        assert_eq!(&*linter.unit(edited, Some(&next.programs)).unwrap().text, "(edited again)");
        // A run older than the invalidations still kept carries nothing.
        let stale = next.carried();
        for _ in 0..=super::super::INVALIDATIONS_KEPT { linter.invalidate(&[]); }
        let fresh = linter.run_after(Some(stale));
        assert_eq!(&*linter.unit(kept, Some(&fresh.programs)).unwrap().text, "(kept, unannounced)");
        reads::drop_buffer(kept);
        reads::drop_buffer(edited);
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
            ), 1)] },
            crate::expr::Attr { name: "alias".into(), defs: vec![(matcher, crate::expr::CExpr::Attr(0), 2)] },
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

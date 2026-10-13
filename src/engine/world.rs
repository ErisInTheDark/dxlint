//! The files behind type questions (docs/design/type-layers.md "World:
//! files behind questions"): the one engine owner of trees, texts, names
//! and VM runs that type-layer engines see through [`World`].
//!
//! File 0 is the linted file. Other files load on module lookup, with
//! shared parse data and names, with per-lint facts and VM state.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use crate::expr::{Ctx, Value};
use crate::grammar::names::question::{FrameId, Known, Leaf, NodeRef, ScopeRef, Target, World};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::modules::compile::Action;
use crate::grammar::names::modules::resolution::{program_inputs, DiscoveryRequest, Host, Resolver, Settings};
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
    roots: RefCell<crate::fx_hash::FxHashMap<*const Settings, super::reads::Cached<Known<Vec<ScopeRef>>>>>,
    modules: RefCell<crate::fx_hash::FxHashMap<(u32, *const Settings), ModuleAnswers>>,
}

/// One lint's world. Engines hold it as `&dyn World` through [`Files`].
pub struct LintWorld<'a> {
    linter: &'a super::Linter,
    path: &'a Path,
    views: super::arena::Arena<'a>,
    files: RefCell<crate::fx_hash::FxHashMap<PathBuf, u32>>,
    program: ProgramMemo<'a>,
    /// File 0 outside any rule (no locals, no indents).
    ctx: Ctx<'a>,
    evaluation: Cell<*const ()>,
    semantic: crate::grammar::names::semantic::Session,
}

impl<'a> LintWorld<'a> {
    fn revision(&self) -> u64 {
        self.linter.invalidation.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn new(linter: &'a super::Linter, path: &'a Path, ctx: Ctx<'a>) -> Self {
        LintWorld {
            linter, path, views: super::arena::Arena::new(linter), files: Default::default(),
            program: Default::default(), ctx: Ctx { node: ctx.tree.root, ..ctx },
            evaluation: Cell::new(std::ptr::null()), semantic: Default::default(),
        }
    }

    fn file(&'a self, path: &Path) -> Option<u32> {
        if path == self.path { return Some(0); }
        if let Some(&file) = self.files.borrow().get(path) { return Some(file); }
        let unit = self.linter.unit(path)?;
        let file = self.views.insert(unit) + 1;
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
        let view = self.views.get(file.checked_sub(1)?)?;
        let unit = &view.unit;
        unit.parsed.tree.nodes.get(node as usize)?;
        let ctx = self.linter.tree_ctx(unit.lang, &unit.parsed.tree, (&unit.text, &view.lines, &unit.rel), &view.memo, &view.reindented, view.native());
        Some(Ctx { file_id: file, node, world: Some(self), ..ctx })
    }

    /// Runs `f` with `semantic` as the question in progress, so queries
    /// asked from the conditions `f` runs nest in it.
    fn asking<R>(&self, semantic: &Semantic<'_>, at: Leaf, f: impl FnOnce() -> Known<R>) -> Known<R> {
        let Some(_nested) = semantic.question.budget.nest() else { return Known::Unknown };
        self.evaluating(Some(ActiveQuestion { semantic, at }), f)
    }

    fn evaluating<R>(&self, question: Option<ActiveQuestion<'_, '_>>, f: impl FnOnce() -> Known<R>) -> Known<R> {
        let evaluation = ActiveEvaluation { question, unknown: Cell::new(false) };
        let before = self.evaluation.replace((&evaluation as *const ActiveEvaluation<'_, '_>).cast());
        let restore = Restore { slot: &self.evaluation, before };
        let result = f();
        drop(restore);
        if evaluation.unknown.get() || matches!(result, Known::Unknown) {
            self.mark_unknown();
            Known::Unknown
        } else { result }
    }

    pub(crate) fn complete_attribute<R>(&self, f: impl FnOnce() -> R) -> Known<R> {
        let revision = self.revision();
        let question = self.question().map(|question| ActiveQuestion { semantic: question.semantic, at: question.at });
        let answer = self.evaluating(question, || Known::Yes(f()));
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

    fn semantic_query<R>(&'a self, file: u32, node: u32, run: impl FnOnce(&Semantic<'_>, Leaf) -> Known<R>) -> Known<R> {
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
            let answer = match question.semantic.leaf(Leaf { target, frame }) {
                Known::Yes(at) => run(question.semantic, at),
                Known::Unknown => Known::Unknown,
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
            self.asking(semantic, at, || run(semantic, at))
        });
        if matches!(answer, Known::Unknown) { self.mark_unknown(); }
        answer
    }

    pub fn query(&'a self, file: u32, node: u32, subject: &crate::grammar::names::Value, query: &crate::expr::queries::Query, mask: u64) -> bool {
        let answer = self.semantic_query(file, node, |semantic, at| (query.call)(semantic, at, subject, mask));
        matches!(answer, Known::Yes(true))
    }

    pub fn query_function(&'a self, file: u32, node: u32, query: &crate::expr::queries::QueryFunction, args: &[Value<'static>]) -> Value<'static> {
        let run = || self.semantic_query(file, node, |semantic, at| (query.call)(semantic, at, args));
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
    at: Leaf,
    name: String,
    source: Option<String>,
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

    fn roots(self, selected: &Settings) -> Known<Vec<ScopeRef>> {
        let key = selected as *const Settings;
        if let Some(cached) = self.0.program.roots.borrow().get(&key) { return cached.get() }
        let cached = super::reads::Cached::compute(|| {
            let question = QueryContext::default();
            let roots = (|| {
                let Known::Yes(paths) = program_inputs(selected, &self, &question) else { return Known::Unknown };
                let mut roots = Vec::new();
                for path in paths {
                    if !question.budget.step() { return Known::Unknown }
                    let Some(file) = self.0.file(&path) else { return Known::Unknown };
                    let scope = ScopeRef { file, scope: 0 };
                    if !roots.contains(&scope) { roots.push(scope); }
                }
                let Known::Yes(program) = Host::program(&self) else { return Known::Unknown };
                let program = ScopeRef { file: program, scope: 0 };
                if !roots.contains(&program) { roots.push(program); }
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

    fn named_modules(self, roots: &[ScopeRef], classes: u64, semantic: &Semantic<'_>) -> Known<Vec<NamedModule>> {
        let mut modules = Vec::new();
        for root in roots {
            let Some(names) = World::names(&self, root.file) else { return Known::Unknown };
            for (scope, info) in names.scopes.iter().enumerate() {
                if classes & info.classes == 0 { continue }
                let Known::Yes(items) = self.named_scope(ScopeRef { file: root.file, scope: scope as u32 }, classes, semantic) else { return Known::Unknown };
                modules.extend(items);
            }
        }
        Known::Yes(modules)
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
            modules.push(NamedModule { scope, at, name, source });
        }
        Known::Yes(modules)
    }

    fn ambient_with(self, name: &str, classes: u64, semantic: &Semantic<'_>) -> Known<Option<ScopeRef>> {
        let Known::Yes(Some(selected)) = self.program_selected() else { return Known::Unknown };
        let Known::Yes(roots) = self.roots(selected) else { return Known::Unknown };
        let Known::Yes(modules) = self.named_modules(&roots, classes, semantic) else { return Known::Unknown };
        let exact = modules.iter().find(|module| module.source.is_none() && module.name == name);
        let found = exact.or_else(|| modules.iter().find(|module| module.source.is_none() && crate::path_globs::wildcard(&module.name, name)));
        Known::Yes(found.map(|module| module.scope))
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
        Some(self.0.on(file, self.tree(file)?.root)?.names())
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
        let answer = super::reads::Cached::compute(|| {
            self.0.asking(semantic, at, || Resolver { host: self, settings: selected }.resolve(ctx.file_id, specifier, &[], semantic.question))
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

    fn global_scopes(&self, _importer: u32, _semantic: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
        let Known::Yes(Some(selected)) = self.program_selected() else { return Known::Unknown };
        self.roots(selected)
    }

    fn module_scopes(&self, scope: ScopeRef, semantic: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
        let Some(names) = self.names(scope.file) else { return Known::Unknown };
        let Some(info) = names.scopes.get(scope.scope as usize) else { return Known::Unknown };
        let selected = match self.program_selected() {
            Known::Yes(Some(selected)) => selected,
            Known::Yes(None) => return Known::Yes(vec![scope]),
            Known::Unknown => return Known::Unknown,
        };
        let classes = selected.ambient_scopes;
        if scope.scope != 0 && info.classes & classes == 0 { return Known::Yes(vec![scope]); }
        let Known::Yes(roots) = self.roots(selected) else { return Known::Unknown };
        let Known::Yes(modules) = self.named_modules(&roots, classes, semantic) else { return Known::Unknown };
        let Known::Yes(own) = self.named_scope(scope, classes, semantic) else { return Known::Unknown };
        let name = own.iter().find(|module| module.source.is_none()).map(|module| module.name.as_str());
        let mut merged = vec![scope];
        for module in &modules {
            let same = match (name, &module.source) {
                (Some(name), _) => module.name == name,
                (None, Some(source)) => match self.module(module.at, source, semantic) {
                    Known::Yes(target) => target == Some(scope),
                    Known::Unknown => return Known::Unknown,
                },
                _ => false,
            };
            if same && !merged.contains(&module.scope) { merged.push(module.scope); }
        }
        Known::Yes(merged)
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
        world.asking(semantic, at, || {
            let held = Session::new(&ctx, ctx.node, &[]).holds_on(ctx.node, e);
            held.map_or(Known::Unknown, Known::Yes)
        })
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
        Known::Yes(super::reads::read(path).map(|text| text.to_string()))
    }

    fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> {
        Known::Yes(path == self.0.path || super::reads::is_file(path))
    }

    fn scope(&self, path: &Path, _: &QueryContext) -> Known<Option<ScopeRef>> {
        if path != self.0.path && !super::reads::is_file(path) { return Known::Yes(None); }
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
        let source = "()";
        let tree = Tree::parse_with(&linter.grammar, 0, source, None, None);
        let lines = crate::line_index::LineIndex::new(source);
        let memo = Default::default();
        let reindented = Default::default();
        let ctx = linter.tree_ctx(0, &tree, (source, &lines, "main.t"), &memo, &reindented, None);
        let world = LintWorld::new(linter, Path::new("/root/main.t"), ctx);
        f(Files(&world))
    }

    #[test]
    fn reentrant_queries_in_fresh_frames_share_the_question_depth_bound() {
        fn reenter<'a>(world: &'a LintWorld<'a>, semantic: &Semantic<'_>, at: Leaf, remaining: u32) -> Known<bool> {
            world.asking(semantic, at, || world.semantic_query(0, 0, |semantic, at| {
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
            assert_eq!(files.0.asking(&semantic, scoped, || files.0.semantic_query(file, node, |_, nested| Known::Yes(nested))), Known::Yes(scoped));
            let other = tree.root;
            assert_eq!(files.0.asking(&semantic, scoped, || files.0.semantic_query(file, other, |_, nested| Known::Yes(nested))), Known::Yes(Leaf { target: Target::Node(NodeRef { file, node: other }), frame }));
            assert_eq!(files.0.asking(&semantic, scoped, || files.0.semantic_query(0, 0, |_, nested| Known::Yes(nested.frame))), Known::Yes(FrameId::ROOT));
            let nodeless = Leaf { target: Target::Traits(0), frame };
            assert_eq!(files.condition(nodeless, 0, &semantic), Known::Unknown);
            assert_eq!(files.0.asking(&semantic, nodeless, || files.0.semantic_query(file, node, |_, nested| Known::Yes(nested.frame))), Known::Yes(FrameId::ROOT));
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
        let first = linter.unit(path).unwrap();
        let second = linter.unit(path).unwrap();
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
            assert_eq!(files.global_scopes(0, &semantic), Known::Yes(vec![ScopeRef { file: 0, scope: 0 }]));
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
            assert_eq!(files.0.asking(&semantic, at, || files.roots(selected)), expected);
            reads::set_buffer(config, r#"{"files":[]}"#);
            let (answer, dependencies) = reads::track(|| files.0.asking(&semantic, at, || files.roots(selected)));
            assert_eq!(answer, expected);
            assert!(dependencies.any(&mut |dep| !linter.holds(dep)));
        });
        with_world(&linter, |files| {
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &question);
            assert_eq!(files.global_scopes(0, &semantic), Known::Yes(vec![ScopeRef { file: 0, scope: 0 }]));
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
            assert_eq!(files.global_scopes(file, &semantic), Known::Yes(vec![ScopeRef { file: library_file, scope: 0 }, ScopeRef { file: 0, scope: 0 }]));
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

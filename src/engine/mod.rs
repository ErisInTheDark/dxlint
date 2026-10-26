//! The linter: a compiled grammar plus rules, run over files.

mod build;
mod arena;
mod units;
pub mod cache;
pub mod diagnostic;
mod fix;
pub mod frontend;
mod inspect;
pub(crate) mod language_options;
mod message;
mod profile;
mod program;
mod prune;
pub mod reads;
pub mod scopes;
mod suppress;
pub mod symbols;
pub mod world;
#[cfg(test)]
pub(crate) mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use build::{build, build_for_lint, Built};
pub use diagnostic::{Diagnostic, Edit, Fix, ENGINE_RULE};
pub use fix::{apply_fixes, Fixed};
pub use profile::{EffectBuild, Profile, Query, StepSite};
pub(crate) use profile::ValueEvaluation;

use crate::config::ast::{FixTarget, Report, Severity};
use crate::config::source::{FileId, SourceMap, Span};
use crate::expr::{Attributes, AttrMemo, CExpr, Ctx, Value};
use crate::grammar::{Grammar, Tree, ROOT, TEXT};
use crate::line_index::LineIndex;
use crate::path_globs::PathGlobs;
use reads::{Dep, Reads};
use frontend::{FileInput, Native};
use suppress::Suppressions;

pub struct OptionSlot {
    pub name: String,
    pub description: Option<String>,
    pub schema: crate::config::schema::Schema,
    pub default: Value<'static>,
    pub value: Value<'static>,
    /// `default option`: what `rule name set <value>` sets.
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Matcher(u32),
    Field(u32, u16),
    Nodes(u32, NodeSelection),
    /// `None` = every language (`in .*`).
    Lang(Option<usize>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeSelection {
    Attribute(u16),
}

pub struct Rule {
    pub name: String,
    pub span: Span,
    pub targets: Vec<Target>,
    pub target_text: String,
    /// `Off` when the rule is defined but not turned on.
    pub severity: Severity,
    /// The severity `rule name` (without one) turns it on at.
    pub default_severity: Severity,
    /// On for some file (outside or inside a `files` block): lowered and
    /// dispatched.
    pub(crate) live: bool,
    /// A `files` block copy whose options are read at runtime instead of
    /// folded in ([`scopes`]).
    pub(crate) unfolded: bool,
    options: Vec<OptionSlot>,
    files: PathGlobs,
    ignore: PathGlobs,
    /// With `each`: the `when`s not reading the item, checked once per node
    /// before the list is built.
    node_when: Vec<CExpr>,
    when: Vec<CExpr>,
    /// The rule's expressions lowered to the VM ([`Linter::lower_conditions`]):
    /// `when`s and clauses read the `each` item from frame slot 0.
    node_when_vm: Vec<crate::vm::VExpr>,
    when_vm: Vec<crate::vm::VExpr>,
    each_vm: Option<crate::vm::VExpr>,
    fix_when_vm: Option<crate::vm::VExpr>,
    fix_vm: Option<crate::vm::VExpr>,
    fix_at_vm: Option<crate::vm::VExpr>,
    report_at_vm: Option<crate::vm::VExpr>,
    /// `let` values, by slot.
    locals: Vec<CExpr>,
    /// `locals` lowered to the VM, read from a [`crate::vm::Session`].
    locals_vm: Vec<crate::vm::VExpr>,
    message: Option<CExpr>,
    /// Explicit message source, or the rule name's source for its fallback.
    pub(crate) message_file: FileId,
    message_plan: Option<message::Plan>,
    /// The message lowered to the VM as `[index, text]`.
    message_vm: Option<crate::vm::VExpr>,
    /// What the selected fix region should look like (`fix ... as expr`).
    fix: Option<CExpr>,
    /// `fix if cond as ...`: without it holding, report without the fix.
    fix_when: Option<CExpr>,
    fix_target: FixTarget,
    fix_at: Option<CExpr>,
    /// `indent [in] X by n` lines.
    indents: crate::expr::IndentPlan,
    indent_reset: Option<CExpr>,
    /// Layers it reads by name (`opener.x`, `descendants(x)`): their nodes
    /// must be built.
    layer_reads: Vec<u32>,
    /// Its targets or expressions require a successfully parsed native tree.
    requires_native: bool,
    /// Its expressions or consumed helpers need unfiltered token neighbours.
    all_tokens: bool,
    /// It targets or indents nodes' own tokens (`ts_member.token`): every
    /// token must be attached.
    token_fields: bool,
    /// The layers it targets when all its targets are layers (else empty):
    /// where its `let`s can be read from another node (`previous.rank`).
    target_layers: Vec<u32>,
    report: Report,
    /// `report at <offset>`
    report_at: Option<CExpr>,
    /// `each x in list`: the list; the other clauses see each item as `x`.
    each: Option<CExpr>,
}

impl Rule {
    /// The final configuration used to specialise this rule's expressions.
    pub fn options(&self) -> &[OptionSlot] {
        &self.options
    }

    /// Every compiled expression of the rule's checks, message and fix.
    pub fn expressions(&self) -> impl Iterator<Item = &CExpr> {
        self.locals.iter().chain(&self.node_when).chain(&self.when).chain(
            [&self.message, &self.fix, &self.fix_when, &self.fix_at, &self.report_at, &self.each, &self.indent_reset]
                .into_iter().flatten())
    }

    /// The layers the rule targets or reads by name.
    pub fn layers(&self) -> impl Iterator<Item = u32> + '_ {
        self.targets.iter().filter_map(|target| match *target {
            Target::Matcher(m) | Target::Field(m, _) | Target::Nodes(m, _) => Some(m),
            Target::Lang(_) => None,
        }).chain(self.layer_reads.iter().copied())
    }

    fn option_values(&self) -> Vec<Value<'static>> {
        self.options.iter().map(|o| o.value.clone()).collect()
    }

    fn specialise_options(&mut self, functions: &[crate::grammar::UserFn]) {
        // No values: option reads stay runtime reads.
        let options = if self.unfolded { Vec::new() } else { self.option_values() };
        let fold = |expr: &mut CExpr| crate::expr::fold::specialise(expr, &options, functions);
        self.locals.iter_mut().chain(&mut self.node_when).chain(&mut self.when).for_each(fold);
        for expr in [
            &mut self.fix, &mut self.fix_when, &mut self.fix_at,
            &mut self.report_at, &mut self.each, &mut self.indent_reset,
        ].into_iter().flatten() {
            fold(expr);
        }
        self.indents.specialise(&options, functions);
        self.message_plan = self.message.as_mut().map(|expr| message::Plan::new(expr, &options, functions));
    }

    /// The message with current option values filled in; holes that need a
    /// node (`{text}`) stay as written.
    pub fn message_template(&self) -> Option<String> {
        Some(Ctx::display(self.message.as_ref()?, &self.option_values()))
    }

    pub fn has_fix(&self) -> bool {
        self.fix.is_some()
    }

    /// A condition folded to a false constant by the configured options
    /// (`when len(paths) > 0` with no paths): the rule can never report, so
    /// it is left out of dispatch.
    pub(crate) fn never_reports(&self) -> bool {
        self.node_when.iter().chain(&self.when).any(|e| matches!(e, CExpr::Const(v) if !v.truthy()))
    }

    /// Whether the rule runs on `rel` at `severity` (its own, or the file's
    /// `files` block one).
    fn applies_to(&self, rel: &Path, lang: usize, severity: Severity) -> bool {
        if severity == Severity::Off {
            return false;
        }
        // A rule only on other languages' files has nothing to do here.
        if self.targets.iter().all(|t| matches!(t, Target::Lang(Some(l)) if *l != lang)) {
            return false;
        }
        (self.files.is_empty() || self.files.matches(rel)) && !self.ignore.matches(rel)
    }
}

/// A rule that is off and reads library layers nothing else needs: listed,
/// and turned on like any other, but not compiled while off.
pub struct InertRule {
    pub name: String,
    pub span: Span,
    pub target_text: String,
    pub default_severity: Severity,
}

/// Stack for threads that run rules: rule programs nest sequence consumers
/// inside their producers and semantic questions recurse through types,
/// deeper than the default 2 MiB thread stack on large typed projects.
pub const WORKER_STACK: usize = 64 << 20;

pub struct Linter {
    invalidation: std::sync::atomic::AtomicU64,
    units: std::sync::Mutex<crate::fx_hash::FxHashMap<PathBuf, std::sync::Arc<units::Unit>>>,
    discoveries: std::sync::Mutex<crate::fx_hash::FxHashMap<crate::grammar::names::modules::resolution::DiscoveryRequest, reads::Cached<Option<Vec<PathBuf>>>>>,
    programs: program::Programs,
    /// `files` blocks: per-file severities and option copies.
    pub scopes: scopes::Scopes,
    unit_overlays: Vec<bool>,
    /// Per front end, by native kind: the tokens semantic-only units attach.
    unit_token_kinds: Vec<Vec<bool>>,
    /// Whether units build child layers only where the semantic reads need
    /// one (unless some semantic read sees raw children).
    unit_child_layers: bool,
    /// The layers semantic tables read (name items, recipes, their helpers).
    semantic_layers: Vec<u32>,
    /// What asks units for tokens: helpers, layers or name items.
    unit_token_reasons: Vec<String>,
    pub grammar: Grammar,
    language_options: language_options::LanguageOptions,
    pub rules: Vec<Rule>,
    pub inert: Vec<InertRule>,
    pub sources: std::sync::Arc<SourceMap>,
    /// Paths are matched relative to this (the config file's directory).
    pub root: PathBuf,
    ignores: PathGlobs,
    /// See [`Grammar::reach`].
    reach: Vec<Vec<bool>>,
    /// Set by [`Linter::enable_profile`].
    pub profile: Option<Profile>,
    by_matcher: Vec<Vec<usize>>,
    by_field: crate::fx_hash::FxHashMap<(u32, u16), Vec<usize>>,
    by_selection: crate::fx_hash::FxHashMap<u32, Vec<(NodeSelection, Vec<usize>)>>,
    /// Whole-file rules and the language they want (`None`: any).
    by_lang: Vec<(usize, Option<usize>)>,
    /// `let` attributes, by id.
    pub(crate) attrs: Attributes,
    /// Identifies these rules: the tokens parsed (imports and macros
    /// expanded) and the project root.
    pub fingerprint: u64,
    /// Files at least this big run their rules in groups on several threads.
    pub(crate) split_at: usize,
    rule_jobs: usize,
}

static NO_INDENTS: crate::expr::IndentPlan = crate::expr::IndentPlan::EMPTY;

/// The files a [`Linter::invalidate`] forgot, for [`Linter::is_stale`].
pub struct Stale {
    changed: Vec<PathBuf>,
}

/// One read snapshot shared by files in a batch. Start another run after editing inputs.
pub struct LintRun<'a> {
    linter: &'a Linter,
    programs: program::Snapshot,
}

impl LintRun<'_> {
    pub fn lint(&self, path: &Path, text: &str) -> Vec<Diagnostic> {
        self.linter.lint_in(&self.programs, path, text)
    }

    pub fn lint_tracked(&self, path: &Path, text: &str) -> (Vec<Diagnostic>, Reads) {
        reads::track(|| self.lint(path, text))
    }
}

impl Linter {
    pub fn run(&self) -> LintRun<'_> {
        LintRun { linter: self, programs: Default::default() }
    }

    pub(crate) fn reset_runtime(&mut self) {
        self.units.get_mut().expect("unit cache").clear();
        self.discoveries.get_mut().expect("unit discoveries").clear();
        self.programs.clear();
        self.profile = None;
        *self.invalidation.get_mut() = 0;
        self.attrs.values.clear_runtime();
    }

    /// Lowers every enabled rule's expressions and reports config errors.
    pub(crate) fn lower_conditions(&mut self, config: &crate::config::ast::Config) -> Vec<crate::config::error::ConfigError> {
        use crate::config::error::ConfigError;
        let mut errors: Vec<_> = std::mem::take(&mut self.attrs.lowering_errors).into_iter().map(|(name, error)| {
            let span = config.attrs.iter().find(|attr| attr.name.text == name).expect("compiled attribute").name.span;
            ConfigError::new(span, error)
        }).collect();
        let functions = &self.grammar.functions;
        let values = &mut self.attrs.values;
        for rule in self.rules.iter_mut().filter(|rule| rule.live) {
            let item = rule.each.is_some();
            // Indent lines are read on other nodes: no locals, no item.
            rule.indents.lower(rule.indent_reset.as_ref(), functions, values);
            let locals = &rule.locals;
            let mut lower =|e: &CExpr, item: bool| crate::vm::lower_condition(e, functions, locals, values, item);
            rule.locals_vm = rule.locals.iter().map(|e| lower(e, false)).collect();
            // Guards run before the items, with the item slot null.
            rule.node_when_vm = rule.node_when.iter().map(|e| lower(e, item)).collect();
            rule.when_vm = rule.when.iter().map(|e| lower(e, item)).collect();
            rule.each_vm = rule.each.as_ref().map(|e| lower(e, false));
            rule.fix_when_vm = rule.fix_when.as_ref().map(|e| lower(e, item));
            rule.fix_vm = rule.fix.as_ref().map(|e| lower(e, item));
            rule.fix_at_vm = rule.fix_at.as_ref().map(|e| lower(e, item));
            rule.report_at_vm = rule.report_at.as_ref().map(|e| lower(e, item));
            rule.message_vm = rule.message_plan.as_ref().zip(rule.message.as_ref())
                .map(|(plan, message)| lower(&plan.indexed(message, u16::from(item)), item));
            let failed = values.take_lowering_errors(&format!("rule `{}`", rule.name));
            if !failed.is_empty() {
                rule.severity = Severity::Off;
                rule.live = false;
            }
            errors.extend(failed.into_iter().map(|error| ConfigError::new(rule.span, error)));
        }
        for condition in &self.grammar.names.conditions {
            if let Some(e) = condition.expr.get() {
                let body = crate::vm::lower(e, functions, values);
                let failed = values.take_lowering_errors("name condition");
                if failed.is_empty() { let _ = condition.lowered.set(body); }
                let span = config.names[condition.item].when.as_ref().expect("compiled name condition").span;
                errors.extend(failed.into_iter().map(|error| ConfigError::new(span, error)));
            }
        }
        for operand in &self.grammar.names.operands {
            if let Some(e) = operand.expr.get() {
                let item = config.operand_parameter(operand.of).is_some();
                let body = crate::vm::lower_condition(e, functions, &[], values, item);
                let failed = values.take_lowering_errors("item expression");
                if failed.is_empty() { let _ = operand.lowered.set(body); }
                let span = config.operand(operand.of, operand.part).expect("compiled item expression").span;
                errors.extend(failed.into_iter().map(|error| ConfigError::new(span, error)));
            }
        }
        errors
    }

    /// Sizes each node kind's VM record from what the enabled rules read on
    /// the kinds they target (after [`Linter::lower_conditions`]).
    pub(crate) fn shape_records(&mut self) {
        let fields = self.record_fields();
        if self.attrs.vm_stats {
            eprintln!("dxlint vm: {fields} record fields");
        }
    }

    /// [`Linter::shape_records`]; the number of record fields.
    fn record_fields(&mut self) -> usize {
        use crate::vm::KindSet;
        let g = &self.grammar;
        let n = g.matchers.len();
        let specialisations = |base: u32| {
            let mut set = KindSet::empty(n);
            for k in 0..n as u32 {
                if g.is_a(k, base) {
                    set.insert(k);
                }
            }
            set
        };
        let mut roots = Vec::new();
        for rule in self.rules.iter().filter(|rule| rule.live) {
            let mut kinds = KindSet::empty(n);
            for target in &rule.targets {
                match *target {
                    Target::Matcher(m) => kinds.extend(&specialisations(m)),
                    Target::Field(m, label) => {
                        for owner in specialisations(m).iter() {
                            let targets = g.field_targets(owner, label);
                            if targets.any {
                                kinds = KindSet::all(n);
                            }
                            for &k in &targets.kinds {
                                kinds.insert(k);
                            }
                        }
                    }
                    Target::Nodes(..) => kinds = KindSet::all(n),
                    Target::Lang(_) => {}
                }
            }
            // Whole-file rules still read other nodes (`descendants(x).y`).
            let exprs = rule.node_when.iter().chain(&rule.when).chain(&rule.locals)
                .chain([&rule.message, &rule.fix, &rule.fix_when, &rule.fix_at, &rule.indent_reset, &rule.report_at, &rule.each].into_iter().flatten());
            roots.extend(exprs.map(|e| (kinds.clone(), e)));
        }
        let compiled_rules = &self.rules;
        let selection_reads: Vec<_> = self.by_selection.iter().flat_map(|(&matcher, selections)| {
            selections.iter().filter_map(move |(selection, rules)| match selection {
                NodeSelection::Attribute(attr) if rules.iter().any(|&rule| compiled_rules[rule].live) => {
                    let mut kinds = KindSet::empty(n);
                    kinds.insert(matcher);
                    Some((kinds, CExpr::Attr(*attr)))
                },
                _ => None,
            })
        }).collect();
        roots.extend(selection_reads.iter().map(|(kinds, expr)| (kinds.clone(), expr)));
        self.attrs.shape_records(g, roots)
    }

    /// Whether a rule's `i`th condition holds.
    fn condition_holds(&self, session: &crate::vm::Session<'_, Ctx>, item: Option<crate::vm::V>, vm: &[crate::vm::VExpr], i: usize) -> Result<bool, String> {
        vm.get(i).and_then(|lowered| session.holds(lowered, item)).ok_or_else(|| session.error())
    }
    /// A rule clause's VM value.
    fn clause(&self, session: &crate::vm::Session<'_, Ctx>, item: Option<crate::vm::V>, vm: Option<&crate::vm::VExpr>) -> Result<Value<'static>, String> {
        vm.and_then(|vm| session.value(vm, item)).map(Value::into_static).ok_or_else(|| session.error())
    }


    /// Limit parallel rule groups per file; zero selects serial evaluation.
    pub fn set_rule_jobs(&mut self, jobs: usize) {
        self.rule_jobs = jobs.max(1);
    }

    /// Collect `--profile` counters from now on.
    pub fn enable_profile(&mut self) {
        self.profile = Some(Profile::new(self.grammar.matchers.len(), self.rules.len())
            .with_attributes(self.attrs.count()).with_functions(self.grammar.functions.len()));
    }

    pub fn relative<'p>(&self, path: &'p Path) -> std::borrow::Cow<'p, Path> {
        match path.strip_prefix(&self.root) {
            Ok(rel) => rel.into(),
            Err(_) => path.into(),
        }
    }

    /// Is this file linted at all (not ignored, owned by some language)?
    pub fn wants(&self, path: &Path) -> bool {
        let rel = self.relative(path);
        !self.ignores.matches(&rel) && self.grammar.lang_for(&rel).is_some()
    }

    pub fn is_ignored(&self, path: &Path) -> bool {
        self.ignores.matches(&self.relative(path))
    }

    /// [`Linter::lint`], plus what the lint read besides `text` (other
    /// files' texts, module resolution, tsconfigs): the result stays
    /// right while every dep holds ([`Linter::holds`]).
    pub fn lint_tracked(&self, path: &Path, text: &str) -> (Vec<Diagnostic>, Reads) {
        reads::track(|| self.lint(path, text))
    }

    pub fn holds(&self, dep: &Dep) -> bool {
        dep.holds()
    }

    /// Whether everything `reads` read still holds, scans included.
    pub fn reads_hold(&self, reads: &Reads) -> bool {
        reads.holds(&mut |dep| dep.holds(), &mut |scan| self.scan_holds(scan))
    }

    pub fn scan_holds(&self, scan: &reads::Scan) -> bool {
        scan.holds(|path| self.passes(path, reads::read_unrecorded(path).0.as_deref(), &scan.test))
    }

    /// Whether `path`, reading `text` now (`None`: no such file), passes a
    /// scan's test.
    pub fn passes(&self, path: &Path, text: Option<&str>, test: &reads::ScanTest) -> bool {
        let Some(text) = text else { return true };
        let holding = |needles: &[Box<str>]| needles.iter().any(|needle| text.contains(&**needle));
        match test {
            reads::ScanTest::Lacks(needles) => !holding(needles),
            reads::ScanTest::Global { name, hints } => !holding(hints) && (!text.contains(&**name) || !self.mentions_global(path, name)),
        }
    }

    /// Whether `path`'s module index mentions `name` as a global of any
    /// class; true when it does not parse.
    fn mentions_global(&self, path: &Path, name: &str) -> bool {
        let Some(unit) = self.unit_quiet(path, None).1 else { return true };
        let Some(front) = unit.parsed.front else { return true };
        let run = Run { path: &unit.path, lang: unit.lang, text: &unit.text, lines: unit.lines(), file: &unit.rel, active: &[], settings: None };
        let native = self.native_facts(front, &unit.parsed.tree, &run, (&*unit.path, Path::new(&unit.rel)), unit.parsed.state.clone());
        let index = unit.parsed.tree.module_index(&self.grammar.names, &unit.text, &|text| native.name_key(text), None);
        index.mentions(name, u64::MAX, crate::grammar::names::modules::index::PublicationKind::Globals)
    }

    /// Forgets what the `changed` files said; [`Linter::is_stale`] then
    /// tells which recorded deps they broke.
    pub fn invalidate(&self, changed: &[PathBuf]) -> Stale {
        self.units.lock().expect("unit cache").retain(|path, _| !changed.contains(path));
        self.discoveries.lock().expect("unit discoveries").retain(|request, _| !changed.iter().any(|path| path.starts_with(&request.root)));
        self.invalidation.fetch_add(1, std::sync::atomic::Ordering::Release);
        Stale { changed: changed.to_vec() }
    }

    pub fn is_stale(&self, stale: &Stale, reads: &Reads) -> bool {
        let touched = |dep: &Dep| stale.changed.iter().any(|path| dep.touches(path));
        // Every scan of a program shares its file list: look through it once.
        let mut files_touched = crate::fx_hash::FxHashMap::default();
        !reads.holds(
            &mut |dep| !touched(dep) || dep.holds(),
            &mut |scan| !*files_touched.entry(std::sync::Arc::as_ptr(&scan.files)).or_insert_with(|| scan.files.any(&mut |dep| touched(dep))) || self.scan_holds(scan),
        )
    }

    /// One parsed file's native facts, from front end `front`.
    fn native_facts<'t>(
        &'t self,
        front: usize,
        tree: &'t Tree,
        run: &Run<'t>,
        (path, rel): (&'t Path, &Path),
        state: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Box<dyn Native + 't> {
        let native = &self.grammar.natives[front];
        let options: Vec<_> = self.language_options.values(run.lang, rel).collect();
        native.frontend.file(FileInput {
            tree,
            text: run.text,
            front: native,
            path,
            options: &options,
            state,
        })
    }

    pub fn lint(&self, path: &Path, text: &str) -> Vec<Diagnostic> {
        self.run().lint(path, text)
    }

    fn lint_in(&self, programs: &program::Snapshot, path: &Path, text: &str) -> Vec<Diagnostic> {
        let rel = self.relative(path);
        if self.ignores.matches(&rel) {
            return Vec::new();
        }
        let Some(lang) = self.grammar.lang_for(&rel) else { return Vec::new() };
        let settings = self.scopes.for_file(&rel, &self.rules);
        let severity = |i: usize| settings.as_ref().map_or(self.rules[i].severity, |s| s.severity[i]);
        let active: Vec<bool> = self.rules.iter().enumerate().map(|(i, f)| f.applies_to(&rel, lang, severity(i))).collect();
        if !active.iter().any(|a| *a) {
            return Vec::new();
        }

        // Overlays only matter when an active rule can see inside them, and
        // each tree only when an active rule targets its layers.
        let mut targets = vec![false; self.grammar.matchers.len()];
        let (mut wants_layers, mut wants_native) = (false, false);
        // The native front end reading this file, if any.
        let accepting = self.grammar.natives.iter().position(|front| front.frontend.accepts(path));
        // Token kinds the active rules use (a node per token costs): the
        // ones they target, and brackets for `opener.x`. All of them when a
        // read would see the difference (`children.x`, `previous.x` on a token).
        let kind_count = accepting.map_or(0, |front| self.grammar.natives[front].frontend.kinds().len());
        let mut token_kinds = vec![false; kind_count];
        let (mut all_tokens, mut token_fields, mut lang_layers) = (false, false, false);
        let token_kind = |m: u32| {
            self.grammar.native_kind(m).filter(|k| Some(usize::from(k.front)) == accepting && k.is_token()).map(|k| usize::from(k.kind))
        };
        // A layer some active rule targets or reads by name (`opener.x`,
        // `descendants(x)`): its nodes must be built.
        let mut need = |m: u32, targets: &mut [bool]| {
            if self.grammar.native_kind(m).is_some() {
                if let Some(k) = token_kind(m) {
                    token_kinds[k] = true;
                }
                return;
            }
            targets[m as usize] = true;
            wants_layers = true;
            // Pattern layers inside token layers need those nodes too.
            for &h in &self.grammar.native_hosts[m as usize] {
                if let Some(k) = token_kind(h) {
                    token_kinds[k] = true;
                }
            }
        };
        for (f, _) in self.rules.iter().zip(&active).filter(|(_, a)| **a) {
            wants_native |= f.requires_native;
            for &m in &f.layer_reads {
                need(m, &mut targets);
            }
            for t in &f.targets {
                match *t {
                    Target::Matcher(m) | Target::Field(m, _) | Target::Nodes(m, _) => need(m, &mut targets),
                    Target::Lang(_) => lang_layers = true,
                }
            }
            all_tokens |= f.all_tokens;
            token_fields |= f.token_fields;
        }
        // Type questions about this file read what imported files are
        // parsed with (reference directives). When no rule wants layers,
        // only those are built, so rules see the tree they saw without them.
        let rule_layers = lang_layers || targets.iter().any(|&wanted| wanted);
        if wants_native {
            for &m in &self.semantic_layers {
                need(m, &mut targets);
            }
        }
        wants_layers |= lang_layers;
        if token_kinds.iter().any(|&k| k) && all_tokens || token_fields {
            if let Some(front) = accepting {
                let frontend = self.grammar.natives[front].frontend;
                for (kind, wanted) in token_kinds.iter_mut().enumerate() {
                    *wanted |= frontend.is_token(kind as u16);
                }
            }
        }
        let lines = LineIndex::new(text);
        let file = rel.to_string_lossy().replace('\\', "/");
        let run = Run { path, lang, text, lines: &lines, file: &file, active: &active, settings: settings.as_deref() };
        let mut out = Vec::new();

        let overlays: Vec<bool> = self.reach.iter().map(|r| r.iter().zip(&targets).any(|(r, t)| *r && *t)).collect();
        // One tree per file: where a front end reads the file, its tree with
        // the pattern layers grafted in; elsewhere the pattern layers' own.
        let front = match wants_layers || wants_native {
            true => accepting.map(|index| (index, &self.grammar.natives[index])),
            false => None,
        };
        let mut pattern_tree = wants_layers;
        let mut fallback_active = None;
        if let Some((index, _)) = front {
            let parsed = self.parse(path, text, units::ParsePlan { lang, front: Some(index), layers: wants_layers, overlays: &overlays, child_layers: !rule_layers, tokens: &token_kinds });
            match parsed {
                Ok(parsed) => {
                    let tree = parsed.tree;
                    let rules_started = std::time::Instant::now();
                    let groups = self.rule_groups(&active, text.len());
                    if groups.len() <= 1 {
                        let facts = self.native_facts(index, &tree, &run, (path, &rel), parsed.state);
                        self.check_tree(&tree, &run, programs, Some(&*facts), &mut out);
                    } else {
                        // A big file sets the floor of a whole run: its rules
                        // run in groups on several threads, each with its own
                        // facts (the parsed tree and token stream are shared).
                        std::thread::scope(|s| {
                            let tree = &tree;
                            let rel = &rel;
                            let handles: Vec<_> = groups
                                .iter()
                                .map(|mask| {
                                    let state = parsed.state.clone();
                                    let run = Run { active: mask, ..run };
                                    // Reads are tracked per thread: pass them on.
                                    std::thread::Builder::new().stack_size(WORKER_STACK).spawn_scoped(s, move || {
                                        let _pool = crate::alloc::own_pool();
                                        reads::track(|| {
                                            let facts = self.native_facts(index, tree, &run, (path, rel), state);
                                            let mut part = Vec::new();
                                            self.check_tree(tree, &run, programs, Some(&*facts), &mut part);
                                            part
                                        })
                                    }).expect("rule group thread")
                                })
                                .collect();
                            for h in handles {
                                let (part, group_reads) = h.join().expect("rule group");
                                reads::note_group(&std::sync::Arc::new(group_reads));
                                out.extend(part);
                            }
                        });
                    }
                    if tree.too_deep {
                        out.push(engine_warning(0, "nesting is too deep to follow; some structure in this file was skipped".into()));
                    }
                    if let Some(p) = &self.profile {
                        Profile::add_time(&p.rules_nanos, rules_started.elapsed());
                        p.note_file(&file, rules_started.elapsed());
                    }
                    pattern_tree = false;
                }
                // Only native-free rules can use root-only pattern layers.
                Err((at, msg)) if wants_native => {
                    fallback_active = Some(self.rules.iter().zip(&active).map(|(rule, active)| *active && !rule.requires_native).collect::<Vec<_>>());
                    out.push(engine_warning(at as usize, format!("could not parse this file as TypeScript: {msg}; native-dependent checks are incomplete")));
                }
                Err(_) => {}
            }
        }

        if pattern_tree {
            let run = Run { active: fallback_active.as_deref().unwrap_or(&active), ..run };
            let tree = self.parse(path, text, units::ParsePlan { lang, front: None, layers: true, overlays: &overlays, child_layers: false, tokens: &[] }).expect("pattern parse").tree;
            let rules_started = std::time::Instant::now();
            self.check_tree(&tree, &run, programs, None, &mut out);
            if tree.too_deep {
                out.push(engine_warning(0, "nesting is too deep to follow; some structure in this file was skipped".into()));
            }
            if let Some(p) = &self.profile {
                Profile::add_time(&p.rules_nanos, rules_started.elapsed());
                p.note_file(&file, rules_started.elapsed());
            }
        }
        let suppressions = Suppressions::parse(text, &lines);
        if !suppressions.is_empty() {
            out.retain(|d| suppressions.allows(d, &lines));
        }
        Diagnostic::dedup(&mut out);
        out
    }

    /// The active rules split into masks to run side by side: one group
    /// unless the file is big enough to be worth the duplicated attribute
    /// and fact work.
    fn rule_groups(&self, active: &[bool], len: usize) -> Vec<Vec<bool>> {
        let on: Vec<usize> = (0..active.len()).filter(|&i| active[i]).collect();
        let k = if len < self.split_at { 1 } else { (len / self.split_at.max(1)).clamp(2, 4).min(self.rule_jobs).min(on.len().max(1)) };
        if k <= 1 {
            return vec![active.to_vec()];
        }
        let mut groups = vec![vec![false; active.len()]; k];
        for (i, &fi) in on.iter().enumerate() {
            groups[i % k][fi] = true;
        }
        groups
    }

    /// The compiled parse plan for units read only for semantic questions:
    /// how many token kinds are attached, how many matchers of all the
    /// layer mask enables, and whether child layers follow that mask.
    pub fn unit_plan(&self) -> (usize, usize, usize, bool) {
        let tokens = self.unit_token_kinds.iter().flatten().filter(|&&on| on).count();
        (tokens, self.unit_overlays.iter().filter(|&&on| on).count(), self.unit_overlays.len(), self.unit_child_layers)
    }

    /// Why [`Linter::unit_plan`] attaches tokens.
    pub fn unit_token_reasons(&self) -> &[String] {
        &self.unit_token_reasons
    }

    /// A context on `tree`'s root outside any rule: for the tree's names,
    /// which native facts answer name questions from.
    pub(super) fn tree_ctx<'a>(
        &'a self,
        lang: usize,
        tree: &'a Tree,
        (source, lines, file): (&'a str, &'a crate::line_index::LineIndex, &'a str),
        memo: &'a AttrMemo<'a>,
        reindented: &'a RefCell<HashMap<usize, String>>,
        native: Option<&'a (dyn Native + 'a)>,
    ) -> Ctx<'a> {
        Ctx {
            grammar: &self.grammar,
            lang,
            tree,
            node: tree.root,
            source,
            lines,
            file,
            file_id: 0,
            options: &[],
            language_options: Some(&self.language_options),
            reindented,
            attrs: &self.attrs,
            memo,
            locals: None,
            native,
            indents: &NO_INDENTS,
            indent_reset: None,
            profile: self.profile.as_ref(),
            world: None,
        }
    }

    /// Runs every active rule on the nodes of `tree` they target.
    fn check_tree<'a>(&'a self, tree: &'a Tree, run: &Run<'a>, programs: &'a program::Snapshot, native: Option<&'a (dyn Native + 'a)>, out: &mut Vec<Diagnostic>) {
        let _owner = crate::alloc::heap::Owner::enter(crate::alloc::heap::RULES);
        // Nodes run in document order, so a rule's indentation fix of a line
        // is known before it checks the lines below. Per rule: what one rule
        // fixes never changes what another sees, so rules can run apart.
        let reindented: Vec<RefCell<HashMap<usize, String>>> = self.rules.iter().map(|_| RefCell::default()).collect();
        let memo = AttrMemo::default();
        let no_reindent = RefCell::default();
        let tree_ctx = self.tree_ctx(run.lang, tree, (run.text, run.lines, run.file), &memo, &no_reindent, native);
        let world = world::LintWorld::in_run(self, run.path, tree_ctx, programs);
        // Resolve reachability before dispatch: a reference may select a
        // declaration earlier in the tree, and several owners can reach it.
        let mut linked: crate::fx_hash::FxHashMap<u32, Vec<usize>> = Default::default();
        let selection_owners = if self.by_selection.values().flatten().any(|(_, rules)| rules.iter().any(|&fi| run.active[fi])) {
            tree.order.as_slice()
        } else { &[] };
        for &owner in selection_owners {
            let Some(selections) = self.by_selection.get(&tree.node(owner).matcher) else { continue };
            for (selection, rules) in selections {
                if !rules.iter().any(|&fi| run.active[fi]) { continue; }
                let nodes = match selection {
                    NodeSelection::Attribute(attr) => {
                        memo.enter_node(owner);
                        let ctx = Ctx { node: owner, world: Some(&world), ..tree_ctx };
                        let session = crate::vm::Session::new(&ctx, owner, &[]);
                        let selected = session.value(&crate::vm::VExpr::Attr(*attr), None);
                        match selected {
                            Some(Value::Null) => Ok(Vec::new()),
                            Some(Value::Node(node, _)) => Ok(vec![node]),
                            Some(Value::List(values)) => values.iter().filter(|value| !matches!(value, Value::Null))
                                .map(|value| match value {
                                    Value::Node(node, _) => Ok(*node),
                                    _ => Err("expected a node, a list of nodes, or null".into()),
                                }).collect(),
                            Some(_) => Err("expected a node, a list of nodes, or null".into()),
                            None => Err(session.error()),
                        }
                    }
                };
                match nodes {
                    Ok(nodes) => for node in nodes {
                        if tree.nodes.get(node as usize).is_none() { continue; }
                        let selected = linked.entry(node).or_default();
                        for &fi in rules.iter().filter(|&&fi| run.active[fi]) {
                            if !selected.contains(&fi) { selected.push(fi); }
                        }
                    },
                    Err(error) => {
                        let names = rules.iter().filter(|&&fi| run.active[fi]).map(|&fi| self.rules[fi].name.as_str()).collect::<Vec<_>>().join(", ");
                        let node = tree.node(owner);
                        out.push(Diagnostic {
                            rule: ENGINE_RULE.into(), severity: Severity::Error,
                            message: format!("node selection for `{names}` failed here: {error}"),
                            message_index: None, start: node.start as usize, end: node.end as usize, fix: None,
                        });
                    },
                }
            }
        }
        for rules in linked.values_mut() { rules.sort_unstable(); }
        for &id in &tree.order {
            let node = tree.node(id);
            let field_rules = tree.label(id).zip(tree.owner(id))
                .and_then(|(label, owner)| self.by_field.get(&(tree.node(owner).matcher, label)));
            memo.enter_node(id);
            let mut apply = |fi: usize| {
                if run.active[fi] {
                    memo.enter_rule(fi as u32);
                    let ctx = Ctx {
                        grammar: &self.grammar,
                        lang: run.lang,
                        tree,
                        node: id,
                        source: run.text,
                        lines: run.lines,
                        file: run.file,
                        file_id: 0,
                        options: run.settings.and_then(|s| s.options[fi].as_deref()).unwrap_or(&[]),
                        language_options: Some(&self.language_options),
                        reindented: &reindented[fi],
                        attrs: &self.attrs,
                        memo: &memo,
                        locals: Some((fi as u32, &self.rules[fi].locals_vm, &self.rules[fi].target_layers)),
                        native,
                        indents: &self.rules[fi].indents,
                        indent_reset: self.rules[fi].indent_reset.as_ref(),
                        profile: self.profile.as_ref(),
                        world: Some(&world),
                    };
                    let found = match &self.profile {
                        Some(profile) => profile.time_rule(fi, || self.apply_each(&self.rules[fi], &ctx)),
                        None => self.apply_each(&self.rules[fi], &ctx),
                    };
                    for mut d in found {
                        if let Some(settings) = run.settings {
                            d.severity = settings.severity[fi];
                        }
                        if let Some([edit]) = d.fix.as_ref().map(|fix| fix.edits.as_slice()) {
                            record_reindent(&mut reindented[fi].borrow_mut(), run.text, run.lines, edit);
                        }
                        out.push(d);
                    }
                }
            };
            match node.matcher {
                ROOT => self.by_lang.iter().filter(|(_, l)| l.is_none_or(|l| l == run.lang)).for_each(|&(fi, _)| apply(fi)),
                TEXT => {}
                m => self.by_matcher[m as usize].iter().for_each(|&fi| apply(fi)),
            }
            if let Some(list) = field_rules {
                list.iter().for_each(|&fi| apply(fi));
            }
            if let Some(list) = linked.get(&id) {
                for &fi in list {
                    let direct = match node.matcher {
                        ROOT => self.by_lang.iter().any(|&(rule, lang)| rule == fi && lang.is_none_or(|lang| lang == run.lang)),
                        TEXT => false,
                        m => self.by_matcher[m as usize].contains(&fi),
                    };
                    if !direct && field_rules.is_none_or(|rules| !rules.contains(&fi)) { apply(fi); }
                }
            }
        }
        self.attrs.report_vm_stats(&memo, tree, run.file);
    }

    /// The rule's diagnostics on one node: one, or one per `each` item.
    fn apply_each(&self, f: &Rule, ctx: &Ctx) -> Vec<Diagnostic> {
        // Rule `let`s never read the item: one session serves every item.
        let session = crate::vm::Session::new(ctx, ctx.node, &f.locals_vm);
        if f.each.is_none() { return self.apply(f, ctx, &session).into_iter().collect() }
        let failed = |e: String| {
            let node = ctx.tree.node(ctx.node);
            let message = format!("rule `{}` failed here: {e}", f.name);
            vec![Diagnostic { rule: f.name.clone(), severity: Severity::Error, message, message_index: None, start: node.start as usize, end: node.end as usize, fix: None }]
        };
        if !f.node_when.is_empty() {
            // Guard lambdas are compiled after the rule's item slot even
            // though these guards do not read that item yet.
            let ready = (|| {
                for i in 0..f.node_when.len() {
                    if !self.condition_holds(&session, Some(crate::vm::V::Null), &f.node_when_vm, i)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            })();
            match ready {
                Ok(true) => {}
                Ok(false) => return Vec::new(),
                Err(e) => return failed(e),
            }
        }
        let mut out = Vec::new();
        let vm_items = f.each_vm.as_ref().and_then(|e| session.items(e));
        match vm_items {
            Some(items) => {
                for item in items {
                    out.extend(self.apply_item(f, ctx, &session, Some(item)));
                }
            }
            None => {
                return failed(session.error());
            }
        }
        out
    }

    fn apply(&self, f: &Rule, ctx: &Ctx, session: &crate::vm::Session<'_, Ctx>) -> Option<Diagnostic> {
        self.apply_item(f, ctx, session, None)
    }

    /// One report, optionally bound to an `each` item.
    fn apply_item(&self, f: &Rule, ctx: &Ctx, session: &crate::vm::Session<'_, Ctx>, item: Option<crate::vm::V>) -> Option<Diagnostic> {
        let node = ctx.tree.node(ctx.node);
        let (start, end) = (node.start as usize, node.end as usize);
        let failed = |e: String| Diagnostic {
            rule: f.name.clone(),
            severity: Severity::Error,
            message: format!("rule `{}` failed here: {e}", f.name),
            message_index: None,
            start,
            end,
            fix: None,
        };
        for i in 0..f.when.len() {
            match self.condition_holds(session, item, &f.when_vm, i) {
                Ok(true) => {}
                Ok(false) => return None,
                Err(e) => return Some(failed(e)),
            }
        }
        let fix_enabled = match &f.fix_when {
            Some(_) => match self.clause(session, item, f.fix_when_vm.as_ref()) {
                Ok(v) => v.truthy(),
                Err(e) => return Some(failed(e)),
            },
            None => true,
        };
        let fix = match (&f.fix, fix_enabled) {
            (Some(_), true) => match self.clause(session, item, f.fix_vm.as_ref()) {
                Ok(v) => {
                    let want = v.text();
                    let (from, to) = match &f.fix_at {
                        Some(_) => match self.clause(session, item, f.fix_at_vm.as_ref()).and_then(|value| explicit_fix_region(ctx, value)) {
                            Ok(span) => span,
                            Err(e) => return Some(failed(e)),
                        },
                        None => fix_region(ctx.source, start, end, f.fix_target),
                    };
                    if want == ctx.source[from..to] {
                        return None;
                    }
                    Some(Fix { range: from..to, edits: vec![fix::minimal_edit(ctx.source, from, to, &want)] })
                }
                Err(e) => return Some(failed(e)),
            },
            _ => None,
        };
        let (start, end) = match (f.report, &fix) {
            (Report::Change, Some(fix)) => (fix.edits[0].start, fix.edits[0].end),
            (Report::End, _) => (end, end),
            _ => (start, end),
        };
        // Offsets locate points; only records and nodes select a span.
        let (start, end) = match &f.report_at {
            Some(_) => {
                let boundary = |n: f64| {
                    let mut at = (n.max(0.0) as usize).min(ctx.source.len());
                    while !ctx.source.is_char_boundary(at) {
                        at -= 1;
                    }
                    at
                };
                let span = self.clause(session, item, f.report_at_vm.as_ref()).and_then(|v| match v {
                    Value::Node(id, _) => {
                        let n = ctx.tree.node(id);
                        Ok((n.start as usize, n.end as usize))
                    }
                    Value::Record(_) => {
                        let s = boundary(v.key("start").num()?);
                        let e = match v.key("end") {
                            Value::Null => end.max(s),
                            e => boundary(e.num()?).max(s),
                        };
                        Ok((s, e))
                    }
                    v => v.num().map(|n| {
                        let s = boundary(n);
                        (s, s)
                    }),
                });
                match span {
                    Ok(span) => span,
                    Err(e) => return Some(failed(e)),
                }
            }
            None => (start, end),
        };
        // Through the VM: `[index, text]` in one expression.
        let message_vm = f.message_vm.as_ref();
        let lowered = message_vm.and_then(|vm| match session.value(vm, item)? {
            Value::List(pair) => Some((pair.get(1)?.text().into_owned(), pair.first()?.num().ok()? as usize)),
            _ => None,
        });
        let (message, message_index) = match (&f.message, lowered) {
            (Some(_), Some((text, index))) => (text, Some(index)),
            (Some(_), None) => return Some(failed(session.error())),
            (None, _) => (f.name.clone(), Some(0)),
        };
        Some(Diagnostic { rule: f.name.clone(), severity: f.severity, message, message_index, start, end, fix })
    }
}

/// What every rule run over one file shares.
struct Run<'a> {
    path: &'a Path,
    lang: usize,
    text: &'a str,
    lines: &'a LineIndex,
    file: &'a str,
    active: &'a [bool],
    /// Inside a `files` block: the severities and runtime options there.
    settings: Option<&'a scopes::FileSettings>,
}

fn engine_warning(at: usize, message: String) -> Diagnostic {
    Diagnostic { rule: ENGINE_RULE.into(), severity: Severity::Warn, message, message_index: None, start: at, end: at, fix: None }
}

/// The text a fix rewrites: the node, or the whitespace just before or
/// after it (stopping at a comment or code).
fn fix_region(source: &str, start: usize, end: usize, target: FixTarget) -> (usize, usize) {
    match target {
        FixTarget::Node => (start, end),
        FixTarget::SpaceBefore => {
            let ws = source[..start].len() - source[..start].trim_end().len();
            (start - ws, start)
        }
        FixTarget::SpaceAfter => {
            let ws = source[end..].len() - source[end..].trim_start().len();
            (end, end + ws)
        }
    }
}

fn explicit_fix_region(ctx: &Ctx<'_>, value: Value<'_>) -> Result<(usize, usize), String> {
    let boundary = |value: Value<'_>| {
        let Value::Num(number) = value else { return Err("fix span offsets must be numbers".to_owned()) };
        if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > ctx.source.len() as f64 {
            return Err("fix span offsets must be finite integers within the file".to_owned());
        }
        let offset = number as usize;
        if !ctx.source.is_char_boundary(offset) {
            return Err("fix span offsets must be UTF-8 boundaries".to_owned());
        }
        Ok(offset)
    };
    let (start, end) = match value {
        Value::Node(id, _) => {
            let node = ctx.tree.node(id);
            (boundary(Value::Num(node.start as f64))?, boundary(Value::Num(node.end as f64))?)
        }
        Value::Record(_) => (boundary(value.key("start"))?, boundary(value.key("end"))?),
        value => {
            let at = boundary(value)?;
            (at, at)
        }
    };
    if start > end {
        return Err("fix span start must not follow its end".to_owned());
    }
    Ok((start, end))
}

/// Remembers a fix that only rewrites a line's leading whitespace, as that
/// line's indentation once fixed.
fn record_reindent(reindented: &mut HashMap<usize, String>, source: &str, lines: &LineIndex, edit: &Edit) {
    if !edit.text.bytes().all(|b| b == b' ' || b == b'\t') {
        return;
    }
    let line = lines.line_of(edit.start);
    let line_start = lines.line_start(line);
    let text = lines.line_text(source, line);
    let ws_end = line_start + text.len() - text.trim_start_matches([' ', '\t']).len();
    // A second fix on the same line overlaps the first, which wins.
    if edit.end > ws_end || reindented.contains_key(&line) {
        return;
    }
    let fixed = format!("{}{}{}", &source[line_start..edit.start], edit.text, &source[edit.end..ws_end]);
    reindented.insert(line, fixed);
}

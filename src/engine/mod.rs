//! The linter: a compiled grammar plus rules, run over files.

mod build;
pub mod cache;
pub mod diagnostic;
mod fix;
mod inspect;
mod language_options;
mod profile;
mod prune;
mod suppress;
pub mod symbols;
#[cfg(test)]
pub(crate) mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use build::{build, build_for_lint, Built};
pub use diagnostic::{Diagnostic, Edit, ENGINE_RULE};
pub use fix::{apply_fixes, Fixed};
pub use profile::Profile;

use crate::config::ast::{FixTarget, Report, Severity};
use crate::config::source::{SourceMap, Span};
use crate::expr::{Attr, AttrMemo, CExpr, Ctx, Value};
use crate::grammar::{Grammar, Tree, ROOT, TEXT};
use crate::line_index::LineIndex;
use crate::native::ts::facts::{Facts, NativeFile};
use crate::native::ts::modules::Stale;
use crate::native::ts::reads::{self, Dep, Reads};
use crate::native::ts::syntax::Syntax;
use crate::path_globs::PathGlobs;
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
    /// Nodes reached through a registered link from this matcher.
    Link(u32, usize),
    /// `None` = every language (`in .*`).
    Lang(Option<usize>),
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
    pub options: Vec<OptionSlot>,
    files: PathGlobs,
    ignore: PathGlobs,
    /// With `each`: the `when`s not reading the item, checked once per node
    /// before the list is built.
    node_when: Vec<CExpr>,
    when: Vec<CExpr>,
    /// `let` values, by slot.
    locals: Vec<CExpr>,
    message: Option<CExpr>,
    /// What the node (or the space next to it) should look like (`fix as expr`).
    fix: Option<CExpr>,
    /// `fix if cond as ...`: without it holding, report without the fix.
    fix_when: Option<CExpr>,
    fix_target: FixTarget,
    /// `indent [in] X by n` lines.
    indents: Vec<crate::expr::IndentRule>,
    indent_reset: Option<CExpr>,
    /// Layers it reads by name (`opener.x`, `descendants(x)`): their nodes
    /// must be built.
    layer_reads: Vec<u32>,
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
    fn option_values(&self) -> Vec<Value<'static>> {
        self.options.iter().map(|o| o.value.clone()).collect()
    }

    /// The message with current option values filled in; holes that need a
    /// node (`{text}`) stay as written.
    pub fn message_template(&self) -> Option<String> {
        Some(Ctx::display(self.message.as_ref()?, &self.option_values()))
    }

    pub fn has_fix(&self) -> bool {
        self.fix.is_some()
    }

    fn applies_to(&self, rel: &Path, lang: usize) -> bool {
        if self.severity == Severity::Off {
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

pub struct Linter {
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
    by_field: HashMap<(u32, u16), Vec<usize>>,
    /// Owner matcher -> links and their rule indexes.
    by_link: crate::fx_hash::FxHashMap<u32, Vec<(usize, Vec<usize>)>>,
    /// Whole-file rules and the language they want (`None`: any).
    by_lang: Vec<(usize, Option<usize>)>,
    /// `let` attributes, by id.
    attrs: Vec<Attr>,
    /// `in a forward x from field`: (matcher, attribute, label).
    forwards: Vec<(u32, u16, u16)>,
    /// Other files' exports, read on demand by native rules.
    project: crate::native::ts::modules::Project,
    /// Identifies these rules: the tokens parsed (imports and macros
    /// expanded) and the project root.
    pub fingerprint: u64,
    /// Files at least this big run their rules in groups on several threads.
    pub(crate) split_at: usize,
    rule_jobs: usize,
}

impl Linter {
    /// Limit parallel rule groups per file; zero selects serial evaluation.
    pub fn set_rule_jobs(&mut self, jobs: usize) {
        self.rule_jobs = jobs.max(1);
    }

    /// Collect `--profile` counters from now on.
    pub fn enable_profile(&mut self) {
        self.profile = Some(Profile::new(self.grammar.matchers.len(), self.rules.len()));
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
    /// files' summaries, module resolution, tsconfigs): the result stays
    /// right while every dep holds ([`Linter::holds`]).
    pub fn lint_tracked(&self, path: &Path, text: &str) -> (Vec<Diagnostic>, Reads) {
        reads::track(|| self.lint(path, text))
    }

    pub fn holds(&self, dep: &Dep) -> bool {
        self.project.holds(dep)
    }

    /// Forgets what the `changed` files said (see `Project::invalidate`);
    /// [`Linter::is_stale`] then tells which recorded deps they broke.
    pub fn invalidate(&self, changed: &[PathBuf]) -> Stale {
        self.project.invalidate(changed)
    }

    pub fn is_stale(&self, stale: &Stale, reads: &Reads) -> bool {
        self.project.is_stale(stale, reads)
    }

    pub fn lint(&self, path: &Path, text: &str) -> Vec<Diagnostic> {
        let rel = self.relative(path);
        if self.ignores.matches(&rel) {
            return Vec::new();
        }
        let Some(lang) = self.grammar.lang_for(&rel) else { return Vec::new() };
        let active: Vec<bool> = self.rules.iter().map(|f| f.applies_to(&rel, lang)).collect();
        if !active.iter().any(|a| *a) {
            return Vec::new();
        }

        // Overlays only matter when an active rule can see inside them, and
        // each tree only when an active rule targets its layers.
        let mut targets = vec![false; self.grammar.matchers.len()];
        let (mut wants_layers, mut wants_native) = (false, false);
        // Token kinds the active rules use (a node per token costs): the
        // ones they target, and brackets for `opener.x`. All of them when a
        // read would see the difference (`children.x`, `previous.x` on a token).
        use crate::native::ts::kinds::ALL_KINDS;
        let mut token_kinds = [false; ALL_KINDS.len()];
        let (mut all_tokens, mut token_fields, mut lang_layers) = (false, false, false);
        let token_kind = |m: u32| self.grammar.native_kind(m).map(|k| k.kind as usize).filter(|&k| ALL_KINDS.get(k).is_some_and(|k| k.is_token()));
        // A layer some active rule targets or reads by name (`opener.x`,
        // `descendants(x)`): its nodes must be built.
        let mut need = |m: u32, target: bool, targets: &mut [bool]| {
            if self.grammar.native_kind(m).is_some() {
                wants_native |= target;
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
            for &m in &f.layer_reads {
                need(m, false, &mut targets);
            }
            for t in &f.targets {
                match *t {
                    Target::Matcher(m) | Target::Field(m, _) | Target::Link(m, _) => need(m, true, &mut targets),
                    Target::Lang(_) => lang_layers = true,
                }
            }
            all_tokens |= f.all_tokens;
            token_fields |= f.token_fields;
        }
        wants_layers |= lang_layers;
        if token_kinds.iter().any(|&k| k) && all_tokens || token_fields {
            crate::native::ts::tokens::add_all_tokens(&mut token_kinds);
        }
        let wants_tokens = token_kinds.iter().any(|&k| k);
        let lines = LineIndex::new(text);
        let file = rel.to_string_lossy().replace('\\', "/");
        let options: Vec<Vec<Value<'static>>> = self.rules.iter().map(Rule::option_values).collect();
        let run = Run { lang, text, lines: &lines, file: &file, active: &active, options: &options };
        let mut out = Vec::new();

        let overlays: Vec<bool> = self.reach.iter().map(|r| r.iter().zip(&targets).any(|(r, t)| *r && *t)).collect();
        // One tree per file: where a front end reads the file, its tree with
        // the pattern layers grafted in; elsewhere the pattern layers' own.
        let front = match wants_layers || wants_native {
            true => self.grammar.natives.first().zip(crate::native::ts::reads(path)),
            false => None,
        };
        let mut pattern_tree = wants_layers;
        if let Some((front, jsx)) = front {
            let parse_started = std::time::Instant::now();
            let mut parsed = crate::native::ts::parse(text, jsx);
            let native_done = std::time::Instant::now();
            // Token layers cost a node per token: only the kinds rules use.
            if let (true, Ok(parsed)) = (wants_tokens, parsed.as_mut()) {
                crate::native::ts::tokens::attach_selected(text, parsed, &token_kinds);
            }
            let tokens_done = std::time::Instant::now();
            match parsed {
                Ok(parsed) => {
                    let nodes = front.remap(parsed.nodes);
                    let tree = match wants_layers {
                        true => Tree::graft(&self.grammar, lang, text, nodes, parsed.root, Some(&overlays), self.profile.as_ref()),
                        false => Tree::from_native(nodes, parsed.root),
                    };
                    let rules_started = std::time::Instant::now();
                    if let Some(p) = &self.profile {
                        Profile::add_time(&p.parse_nanos, rules_started - parse_started);
                        Profile::add_time(&p.native_nanos, native_done - parse_started);
                        Profile::add_time(&p.token_nanos, tokens_done - native_done);
                        Profile::add_time(&p.layer_nanos, rules_started - tokens_done);
                    }
                    let groups = self.rule_groups(&active, text.len());
                    if groups.len() <= 1 {
                        let facts = NativeFile::new(Syntax::new(&tree, text, front), path, &self.project)
                            .with_environment(crate::native::ts::environment::Environment::from_values(self.language_options.values(lang, &rel)))
                            .with_tokens(parsed.tokens);
                        self.check_tree(&tree, &run, Some(&facts), &mut out);
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
                                    let tokens = parsed.tokens.clone();
                                    let run = Run { active: mask, ..run };
                                    // Reads are tracked per thread: pass them on.
                                    s.spawn(move || {
                                        reads::track(|| {
                                            let facts = NativeFile::new(Syntax::new(tree, text, front), path, &self.project)
                                                .with_environment(crate::native::ts::environment::Environment::from_values(self.language_options.values(lang, rel)))
                                                .with_tokens(tokens);
                                            let mut part = Vec::new();
                                            self.check_tree(tree, &run, Some(&facts), &mut part);
                                            part
                                        })
                                    })
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
                    }
                    pattern_tree = false;
                }
                // Text layers still apply to a file the front end cannot
                // read: from the file's root alone.
                Err((at, msg)) if wants_native => out.push(engine_warning(at as usize, format!("could not parse this file as TypeScript: {msg}"))),
                Err(_) => {}
            }
        }

        if pattern_tree {
            let parse_started = std::time::Instant::now();
            let tree = Tree::parse_with(&self.grammar, lang, text, Some(&overlays), self.profile.as_ref());
            let rules_started = std::time::Instant::now();
            if let Some(p) = &self.profile {
                Profile::add_time(&p.parse_nanos, rules_started - parse_started);
                Profile::add_time(&p.layer_nanos, rules_started - parse_started);
            }
            self.check_tree(&tree, &run, None, &mut out);
            if tree.too_deep {
                out.push(engine_warning(0, "nesting is too deep to follow; some structure in this file was skipped".into()));
            }
            if let Some(p) = &self.profile {
                Profile::add_time(&p.rules_nanos, rules_started.elapsed());
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

    /// Runs every active rule on the nodes of `tree` they target.
    fn check_tree<'a>(&'a self, tree: &'a Tree, run: &Run<'a>, native: Option<&'a dyn Facts>, out: &mut Vec<Diagnostic>) {
        // Nodes run in document order, so a rule's indentation fix of a line
        // is known before it checks the lines below. Per rule: what one rule
        // fixes never changes what another sees, so rules can run apart.
        let reindented: Vec<RefCell<HashMap<usize, String>>> = self.rules.iter().map(|_| RefCell::default()).collect();
        let memo = AttrMemo::default();
        // Resolve reachability before dispatch: a reference may select a
        // declaration earlier in the tree, and several owners can reach it.
        let mut linked: crate::fx_hash::FxHashMap<u32, Vec<usize>> = Default::default();
        if let Some(native) = native.filter(|_| self.by_link.values().flatten().any(|(_, rules)| rules.iter().any(|&fi| run.active[fi]))) {
            for &owner in &tree.order {
                let Some(links) = self.by_link.get(&tree.node(owner).matcher) else { continue };
                for (link, rules) in links {
                    if !rules.iter().any(|&fi| run.active[fi]) { continue; }
                    for node in (crate::expr::facts::LINKS[*link].get)(native, owner) {
                        let selected = linked.entry(node).or_default();
                        for &fi in rules.iter().filter(|&&fi| run.active[fi]) {
                            if !selected.contains(&fi) { selected.push(fi); }
                        }
                    }
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
                        options: &run.options[fi],
                        reindented: &reindented[fi],
                        attrs: &self.attrs,
                        forwards: &self.forwards,
                        memo: &memo,
                        locals: Some((fi as u32, &self.rules[fi].locals, &self.rules[fi].target_layers)),
                        native,
                        indents: &self.rules[fi].indents,
                        indent_reset: self.rules[fi].indent_reset.as_ref(),
                        profile: self.profile.as_ref(),
                    };
                    let started = self.profile.as_ref().map(|_| std::time::Instant::now());
                    for d in self.apply_each(&self.rules[fi], &ctx) {
                        if let Some([edit]) = d.fix.as_deref() {
                            record_reindent(&mut reindented[fi].borrow_mut(), run.text, run.lines, edit);
                        }
                        out.push(d);
                    }
                    if let (Some(p), Some(s)) = (&self.profile, started) {
                        Profile::add_time(&p.rule_nanos[fi], s.elapsed());
                        Profile::add(&p.rule_evals[fi], 1);
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
    }

    /// The rule's diagnostics on one node: one, or one per `each` item.
    fn apply_each(&self, f: &Rule, ctx: &Ctx) -> Vec<Diagnostic> {
        let Some(list) = &f.each else { return self.apply(f, ctx).into_iter().collect() };
        let failed = |e: String| {
            let node = ctx.tree.node(ctx.node);
            let message = format!("rule `{}` failed here: {e}", f.name);
            vec![Diagnostic { rule: f.name.clone(), severity: Severity::Error, message, start: node.start as usize, end: node.end as usize, fix: None }]
        };
        for w in &f.node_when {
            match ctx.eval(w) {
                Ok(v) if v.truthy() => {}
                Ok(_) => return Vec::new(),
                Err(e) => return failed(e),
            }
        }
        let items = match ctx.eval(list) {
            Ok(v) => v.items(),
            Err(e) => return failed(e),
        };
        let mut out = Vec::new();
        for item in items {
            ctx.push_param(item);
            out.extend(self.apply(f, ctx));
            ctx.pop_param();
        }
        out
    }

    fn apply(&self, f: &Rule, ctx: &Ctx) -> Option<Diagnostic> {
        let node = ctx.tree.node(ctx.node);
        let (start, end) = (node.start as usize, node.end as usize);
        let failed = |e: String| Diagnostic {
            rule: f.name.clone(),
            severity: Severity::Error,
            message: format!("rule `{}` failed here: {e}", f.name),
            start,
            end,
            fix: None,
        };
        for w in &f.when {
            match ctx.eval(w) {
                Ok(v) if v.truthy() => {}
                Ok(_) => return None,
                Err(e) => return Some(failed(e)),
            }
        }
        let fix = match &f.fix {
            Some(expr) => match ctx.eval(expr) {
                Ok(v) => {
                    let want = v.text();
                    let (from, to) = fix_region(ctx.source, start, end, f.fix_target);
                    if want == ctx.source[from..to] {
                        return None;
                    }
                    Some(fix::minimal_edit(ctx.source, from, to, &want))
                }
                Err(e) => return Some(failed(e)),
            },
            None => None,
        };
        let (start, end) = match (f.report, &fix) {
            (Report::Change, Some(edit)) => (edit.start, edit.end),
            (Report::End, _) => (end, end),
            _ => (start, end),
        };
        // `report at`: an offset (the start), a `{start, end}` record or a node.
        let (start, end) = match &f.report_at {
            Some(at) => {
                let boundary = |n: f64| {
                    let mut at = (n.max(0.0) as usize).min(ctx.source.len());
                    while !ctx.source.is_char_boundary(at) {
                        at -= 1;
                    }
                    at
                };
                let span = ctx.eval(at).and_then(|v| match v {
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
                        (s, end.max(s))
                    }),
                });
                match span {
                    Ok(span) => span,
                    Err(e) => return Some(failed(e)),
                }
            }
            None => (start, end),
        };
        let fix = match &f.fix_when {
            Some(cond) => match ctx.eval(cond) {
                Ok(v) if v.truthy() => fix,
                Ok(_) => None,
                Err(e) => return Some(failed(e)),
            },
            None => fix,
        };
        let fix = fix.map(|edit| vec![edit]);
        let message = match &f.message {
            Some(m) => match ctx.eval(m) {
                Ok(v) => v.text().into_owned(),
                Err(e) => return Some(failed(e)),
            },
            None => f.name.clone(),
        };
        Some(Diagnostic { rule: f.name.clone(), severity: f.severity, message, start, end, fix })
    }
}

/// What every rule run over one file shares.
struct Run<'a> {
    lang: usize,
    text: &'a str,
    lines: &'a LineIndex,
    file: &'a str,
    active: &'a [bool],
    options: &'a [Vec<Value<'static>>],
}

fn engine_warning(at: usize, message: String) -> Diagnostic {
    Diagnostic { rule: ENGINE_RULE.into(), severity: Severity::Warn, message, start: at, end: at, fix: None }
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

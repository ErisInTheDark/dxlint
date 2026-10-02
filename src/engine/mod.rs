//! The linter: a compiled grammar plus rules, run over files.

mod build;
pub mod diagnostic;
mod fix;
mod profile;
mod suppress;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use build::{build, Built};
pub use diagnostic::{Diagnostic, Edit, ENGINE_RULE};
pub use fix::{apply_fixes, Fixed};
pub use profile::Profile;

use crate::config::ast::{Report, Severity};
use crate::config::source::{SourceMap, Span};
use crate::expr::{CExpr, Ctx, Value};
use crate::grammar::{Grammar, Tree, ROOT, TEXT};
use crate::line_index::LineIndex;
use crate::path_globs::PathGlobs;
use suppress::Suppressions;

pub struct OptionSlot {
    pub name: String,
    pub description: Option<String>,
    pub default: Value<'static>,
    pub value: Value<'static>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Matcher(u32),
    Field(u32, u16),
    /// `None` = every language (`in .*`).
    Lang(Option<usize>),
}

pub struct Rule {
    pub name: String,
    pub span: Span,
    pub targets: Vec<Target>,
    pub target_text: String,
    pub severity: Severity,
    pub options: Vec<OptionSlot>,
    files: PathGlobs,
    ignore: PathGlobs,
    when: Vec<CExpr>,
    message: Option<CExpr>,
    /// What the node should look like (`fix expr`).
    fix: Option<CExpr>,
    report: Report,
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

pub struct Linter {
    pub grammar: Grammar,
    pub rules: Vec<Rule>,
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
    /// Whole-file rules and the language they want (`None`: any).
    by_lang: Vec<(usize, Option<usize>)>,
}

impl Linter {
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

        // Overlays only matter when an active rule can see inside them.
        let mut targets = vec![false; self.grammar.matchers.len()];
        for (f, _) in self.rules.iter().zip(&active).filter(|(_, a)| **a) {
            for t in &f.targets {
                if let Target::Matcher(m) | Target::Field(m, _) = *t {
                    targets[m as usize] = true;
                }
            }
        }
        let overlays: Vec<bool> = self.reach.iter().map(|r| r.iter().zip(&targets).any(|(r, t)| *r && *t)).collect();
        let parse_started = std::time::Instant::now();
        let tree = Tree::parse_with(&self.grammar, lang, text, Some(&overlays), self.profile.as_ref());
        let rules_started = std::time::Instant::now();
        if let Some(p) = &self.profile {
            Profile::add_time(&p.parse_nanos, rules_started - parse_started);
        }
        let lines = LineIndex::new(text);
        let file = rel.to_string_lossy().replace('\\', "/");
        let options: Vec<Vec<Value<'static>>> = self.rules.iter().map(Rule::option_values).collect();
        let mut out = Vec::new();

        for &id in &tree.order {
            let node = tree.node(id);
            let mut run = |fi: usize| {
                if active[fi] {
                    let ctx = Ctx { grammar: &self.grammar, lang, tree: &tree, node: id, source: text, lines: &lines, file: &file, options: &options[fi] };
                    let started = self.profile.as_ref().map(|_| std::time::Instant::now());
                    if let Some(d) = self.apply(&self.rules[fi], &ctx) {
                        out.push(d);
                    }
                    if let (Some(p), Some(s)) = (&self.profile, started) {
                        Profile::add_time(&p.rule_nanos[fi], s.elapsed());
                        Profile::add(&p.rule_evals[fi], 1);
                    }
                }
            };
            match node.matcher {
                ROOT => self.by_lang.iter().filter(|(_, l)| l.is_none_or(|l| l == lang)).for_each(|&(fi, _)| run(fi)),
                TEXT => {}
                m => self.by_matcher[m as usize].iter().for_each(|&fi| run(fi)),
            }
            if let (Some(label), Some(owner)) = (tree.label(id), tree.owner(id)) {
                let owner_matcher = tree.node(owner).matcher;
                if let Some(list) = self.by_field.get(&(owner_matcher, label)) {
                    list.iter().for_each(|&fi| run(fi));
                }
            }
        }

        if tree.too_deep {
            out.push(Diagnostic {
                rule: ENGINE_RULE.into(),
                severity: Severity::Warn,
                message: "nesting is too deep to follow; some structure in this file was skipped".into(),
                start: 0,
                end: 0,
                fix: None,
            });
        }

        if let Some(p) = &self.profile {
            Profile::add_time(&p.rules_nanos, rules_started.elapsed());
        }
        let suppressions = Suppressions::parse(text, &lines);
        if !suppressions.is_empty() {
            out.retain(|d| suppressions.allows(d, &lines));
        }
        out.sort_by(|a, b| (a.start, a.end, &a.rule).cmp(&(b.start, b.end, &b.rule)));
        out.dedup_by(|a, b| a.start == b.start && a.end == b.end && a.rule == b.rule);
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
                    if want == ctx.source[start..end] {
                        return None;
                    }
                    Some(fix::minimal_edit(ctx.source, start, end, &want))
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

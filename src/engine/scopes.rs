//! `files "glob" { ... }` blocks (and nested configs folded into them): the
//! severity and options each file's rules run with.
//!
//! Severity and on/off are data: a file's table is the config's rule lines
//! replayed with only the blocks it is in. Option values are folded into
//! compiled rules, so a rule gets one compiled copy per distinct set of
//! option values the blocks can give it; past [`MAX_COPIES`] one copy
//! reads options at runtime instead.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::config::ast::Severity;
use crate::expr::Value;
use crate::fx_hash::FxHashMap;
use crate::path_globs::PathGlobs;

/// Distinct option sets one rule may compile before its block copies read
/// options at runtime.
pub(super) const MAX_COPIES: usize = 8;
/// Blocks setting one rule's options that are enumerated in combination;
/// beyond, the rule's block copy reads options at runtime.
pub(super) const MAX_COMBINED: usize = 10;

/// How one `rule ...` line changes severity.
#[derive(Clone, Copy, Debug)]
pub(super) enum Change {
    Set(Severity),
    /// `rule name`: on at its default, unless it is on already.
    On,
    /// `severity <level>`: only if it is on at this point.
    OnlyIfOn(Severity),
}

/// One configuring line as it applies to one rule definition.
#[derive(Clone, Debug)]
pub(super) struct Setting {
    pub scope: Option<usize>,
    pub change: Change,
    /// `set` lines owned by this rule: (config line, option line).
    pub options: Vec<(usize, usize)>,
}

/// A definition whose severity or options some block changes.
pub(super) struct Scoped {
    /// Compiled copies: the first is the one outside every block.
    pub copies: Vec<usize>,
    pub default: Severity,
    pub defined_only: bool,
    pub settings: Vec<Setting>,
    /// Per setting, its converted option values by slot.
    pub values: Vec<Vec<(usize, Value<'static>)>>,
    /// The last setting of each option slot that applies, by copy.
    pub by_key: FxHashMap<Vec<Option<usize>>, usize>,
    /// The copy reading options at runtime, if one was needed.
    pub unfolded: Option<usize>,
    pub slots: usize,
}

impl Scoped {
    /// The severity after the settings `applies` lets through.
    pub fn severity(&self, applies: impl Fn(Option<usize>) -> bool) -> Severity {
        let mut configured: Option<Severity> = None;
        let enabled = |configured: Option<Severity>| match configured {
            Some(s) => s != Severity::Off,
            None => !self.defined_only && self.default != Severity::Off,
        };
        for setting in self.settings.iter().filter(|s| applies(s.scope)) {
            match setting.change {
                Change::Set(s) => configured = Some(s),
                Change::On if !enabled(configured) => configured = Some(self.default),
                Change::On => {}
                Change::OnlyIfOn(s) if enabled(configured) => configured = Some(s),
                Change::OnlyIfOn(_) => {}
            }
        }
        match configured {
            Some(s) => s,
            None if self.defined_only => Severity::Off,
            None => self.default,
        }
    }

    /// Per option slot, the index of the last applying setting that sets it.
    pub fn key(&self, applies: impl Fn(Option<usize>) -> bool) -> Vec<Option<usize>> {
        let mut key = vec![None; self.slots];
        for (i, values) in self.values.iter().enumerate() {
            if !applies(self.settings[i].scope) { continue }
            for (slot, _) in values { key[*slot] = Some(i) }
        }
        key
    }

    /// The option values a key gives, over the `base` values.
    pub fn options(&self, key: &[Option<usize>], base: &[Value<'static>]) -> Vec<Value<'static>> {
        base.iter().enumerate().map(|(slot, value)| match key.get(slot).copied().flatten() {
            Some(setting) => self.values[setting].iter().rev().find(|(s, _)| *s == slot).map_or_else(|| value.clone(), |(_, v)| v.clone()),
            None => value.clone(),
        }).collect()
    }

    /// The blocks whose settings set an option.
    pub fn option_scopes(&self) -> Vec<usize> {
        let mut scopes: Vec<usize> = self.settings.iter().zip(&self.values)
            .filter(|(_, values)| !values.is_empty()).filter_map(|(s, _)| s.scope).collect();
        scopes.sort_unstable();
        scopes.dedup();
        scopes
    }
}

/// What one file's rules run with.
pub struct FileSettings {
    /// By compiled rule.
    pub severity: Vec<Severity>,
    /// By compiled rule: option values for copies reading them at runtime.
    pub options: Vec<Option<Vec<Value<'static>>>>,
}

#[derive(Default)]
pub struct Scopes {
    globs: Vec<PathGlobs>,
    /// Each block's globs as written, for listings.
    labels: Vec<String>,
    scoped: Vec<Scoped>,
    tables: Mutex<FxHashMap<Vec<bool>, Arc<FileSettings>>>,
}

/// How one `files` block changes a rule, as a listing shows it.
pub struct BlockSettings<'a> {
    pub globs: &'a str,
    pub severity: Severity,
    /// The options it gives other values than outside every block.
    pub options: Vec<(usize, Value<'static>)>,
}

impl Scopes {
    pub(super) fn new(globs: Vec<PathGlobs>, labels: Vec<String>, scoped: Vec<Scoped>) -> Self {
        Scopes { globs, labels, scoped, tables: Mutex::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.globs.is_empty()
    }

    /// Compiled copies beyond each rule's own.
    pub fn copies(&self) -> usize {
        self.scoped.iter().map(|s| s.copies.len() - 1).sum()
    }

    /// A compiled copy for files inside some block, not a rule of its own.
    pub fn is_copy(&self, rule: usize) -> bool {
        self.scoped.iter().any(|s| s.copies[1..].contains(&rule))
    }

    /// For listings: what each block configuring `rule` (its copy outside
    /// every block) changes, block by block in config order.
    pub fn blocks_of(&self, rule: usize, rules: &[super::Rule]) -> Vec<BlockSettings<'_>> {
        let Some(scoped) = self.scoped.iter().find(|s| s.copies[0] == rule) else { return Vec::new() };
        let own = rules[rule].option_values();
        let mut blocks: Vec<usize> = Vec::new();
        for scope in scoped.settings.iter().filter_map(|s| s.scope) {
            if !blocks.contains(&scope) { blocks.push(scope) }
        }
        blocks.into_iter().map(|block| {
            let applies = |scope: Option<usize>| scope.is_none_or(|s| s == block);
            let options = scoped.options(&scoped.key(applies), &own).into_iter().enumerate()
                .filter(|(slot, value)| *value != own[*slot]).collect();
            BlockSettings { globs: &self.labels[block], severity: scoped.severity(applies), options }
        }).collect()
    }

    /// The settings of a file inside some block; `None` outside every block
    /// (each rule's own severity and folded options).
    pub(super) fn for_file(&self, rel: &Path, rules: &[super::Rule]) -> Option<Arc<FileSettings>> {
        if self.globs.is_empty() {
            return None;
        }
        let inside: Vec<bool> = self.globs.iter().map(|g| g.matches(rel)).collect();
        if !inside.iter().any(|&i| i) {
            return None;
        }
        if let Some(table) = self.tables.lock().expect("settings tables").get(&inside) {
            return Some(table.clone());
        }
        let applies = |scope: Option<usize>| scope.is_none_or(|s| inside[s]);
        let mut severity: Vec<Severity> = rules.iter().map(|r| if r.live { r.severity } else { Severity::Off }).collect();
        let mut options: Vec<Option<Vec<Value<'static>>>> = vec![None; rules.len()];
        for scoped in &self.scoped {
            let level = scoped.severity(applies);
            let key = scoped.key(applies);
            let copy = scoped.by_key.get(&key).copied().or(scoped.unfolded).unwrap_or(scoped.copies[0]);
            for &c in &scoped.copies {
                severity[c] = if c == copy && rules[c].live { level } else { Severity::Off };
            }
            if Some(copy) == scoped.unfolded {
                options[copy] = Some(scoped.options(&key, &rules[copy].option_values()));
            }
        }
        let table = Arc::new(FileSettings { severity, options });
        self.tables.lock().expect("settings tables").insert(inside, table.clone());
        Some(table)
    }
}

//! Loaded configs. Each file is linted by its nearest `.zlint` config; a
//! nested config fully replaces its parents for the files below it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use super::transport::Outbox;
use super::{convert, scan};
use crate::config::source::{normalize_path, FileId, SourceProvider};
use crate::config::{self, CONFIG_NAME};
use crate::engine::symbols::Symbols;
use crate::engine::{build, Linter};

#[derive(Default)]
struct Project {
    /// The last good linter; a broken config keeps it.
    linter: Option<Arc<Linter>>,
    /// Every name of the config as last parsed (even when broken).
    symbols: Option<Arc<Symbols>>,
    /// Every `.zlint` file the config reads, imports included.
    files: HashSet<PathBuf>,
    /// Config errors by the URI of the file they point at.
    errors: HashMap<String, Vec<Value>>,
}

#[derive(Default)]
pub struct Projects {
    /// Keyed by config directory.
    by_dir: HashMap<PathBuf, Project>,
    /// URIs currently showing config errors.
    error_uris: HashSet<String>,
}

/// Linters by config directory, snapshotted for background scans.
#[derive(Clone, Default)]
pub struct Linters(Arc<HashMap<PathBuf, Option<Arc<Linter>>>>);

impl Linters {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn for_path(&self, path: &Path) -> Option<&Arc<Linter>> {
        nearest(&self.0, path.parent()?)?.as_ref()
    }

    /// The linter of the nearest config at or above `dir`.
    pub fn for_dir(&self, dir: &Path) -> Option<&Arc<Linter>> {
        nearest(&self.0, dir)?.as_ref()
    }

    /// Whether a config (even a broken one) covers `dir`.
    pub fn covers(&self, dir: &Path) -> bool {
        nearest(&self.0, dir).is_some()
    }

    pub fn config_dirs(&self) -> impl Iterator<Item = &PathBuf> {
        self.0.keys()
    }

    /// Every working linter.
    pub fn all(&self) -> impl Iterator<Item = &Arc<Linter>> {
        self.0.values().flatten()
    }
}

/// The value for the nearest config directory at or above `dir`. A broken
/// nearest config still wins; its files never fall back to a parent config.
fn nearest<'a, T>(by_dir: &'a HashMap<PathBuf, T>, dir: &Path) -> Option<&'a T> {
    dir.ancestors().find_map(|d| by_dir.get(d))
}

/// Configs under `roots` (walked like the scan walks them), plus the nearest
/// one above each root.
pub fn discover(roots: &[PathBuf]) -> HashSet<PathBuf> {
    let mut found: HashSet<PathBuf> = roots.iter().filter_map(|r| config::find(r)).collect();
    if let Some(walker) = scan::walker(roots, |_| true) {
        for entry in walker.build().flatten() {
            if entry.file_name() == CONFIG_NAME && entry.file_type().is_some_and(|t| t.is_file()) {
                found.insert(normalize_path(entry.path()));
            }
        }
    }
    found
}

impl Projects {
    pub fn linter_for(&self, path: &Path) -> Option<&Arc<Linter>> {
        nearest(&self.by_dir, path.parent()?)?.linter.as_ref()
    }

    pub fn linters(&self) -> Linters {
        Linters(Arc::new(self.by_dir.iter().map(|(dir, p)| (dir.clone(), p.linter.clone())).collect()))
    }

    pub fn dirs(&self) -> Vec<PathBuf> {
        self.by_dir.keys().cloned().collect()
    }

    pub fn is_config(&self, path: &Path) -> bool {
        path.file_name().is_some_and(|n| n == CONFIG_NAME) || self.by_dir.values().any(|p| p.files.contains(path))
    }

    /// Configs that read `file`: itself if it is a `.zlint` config, plus every
    /// loaded config importing it.
    pub fn owners_of(&self, file: &Path) -> Vec<PathBuf> {
        let mut owners: Vec<PathBuf> =
            self.by_dir.iter().filter(|(_, p)| p.files.contains(file)).map(|(dir, _)| dir.join(CONFIG_NAME)).collect();
        if file.file_name().is_some_and(|n| n == CONFIG_NAME) && !owners.iter().any(|o| o == file) {
            owners.push(file.to_path_buf());
        }
        owners
    }

    /// The symbols of a config that reads `file` (a `.zlint` file, or one of
    /// the read-only std copies), and the file's id in it.
    pub fn config_file(&self, file: &Path) -> Option<(Arc<Symbols>, FileId)> {
        let std = super::config_docs::std_module(file);
        self.by_dir.values().filter_map(|p| p.symbols.clone()).find_map(|symbols| {
            let id = symbols.sources.files().find(|(_, f)| match &std {
                Some(name) => f.name == *name,
                None => f.path.as_deref() == Some(file),
            })?.0;
            Some((symbols, id))
        })
    }

    pub fn is_loaded(&self, config: &Path) -> bool {
        config.parent().is_some_and(|dir| self.by_dir.contains_key(dir))
    }

    pub fn has_errors(&self, uri: &str) -> bool {
        self.error_uris.contains(uri)
    }

    /// Drops configs not in `keep`. Returns whether any were dropped.
    pub fn retain(&mut self, keep: &HashSet<PathBuf>) -> bool {
        let before = self.by_dir.len();
        self.by_dir.retain(|dir, _| keep.contains(&dir.join(CONFIG_NAME)));
        self.by_dir.len() != before
    }

    /// (Re)loads one config. Returns whether the linters changed: a new good
    /// build, or a config that no longer exists.
    pub fn load(&mut self, config: &Path, sources: &dyn SourceProvider, uri_of: &dyn Fn(&Path) -> String) -> bool {
        let Some(dir) = config.parent() else { return false };
        if !config.is_file() {
            return self.by_dir.remove(dir).is_some();
        }
        let built = build(config::load_file(config, sources), dir.to_path_buf());
        let project = self.by_dir.entry(dir.to_path_buf()).or_default();
        project.files = built.sources.files().filter_map(|(_, f)| f.path.clone()).collect();
        project.symbols = built.symbols.clone();
        project.errors.clear();
        let file_uri = |id| built.sources.get(id).path.as_deref().map(|p: &Path| uri_of(p));
        for e in &built.errors {
            let file = e.span.and_then(|s| built.sources.get(s.file).path.clone()).unwrap_or_else(|| config.to_path_buf());
            project.errors.entry(uri_of(&file)).or_default().push(convert::config_error(&built.sources, e, &file_uri));
        }
        let Some(linter) = built.linter else { return false };
        project.linter = Some(Arc::new(linter));
        true
    }

    /// Publishes every config's errors in the files they point at, clearing
    /// files that no longer have any.
    pub fn publish_errors(&mut self, out: &Outbox) {
        let mut by_uri: HashMap<&str, Vec<Value>> = HashMap::new();
        for project in self.by_dir.values() {
            for (u, errors) in &project.errors {
                let list = by_uri.entry(u).or_default();
                // Files imported by several configs report each error once.
                for e in errors {
                    if !list.contains(e) {
                        list.push(e.clone());
                    }
                }
            }
        }
        for stale in self.error_uris.iter().filter(|u| !by_uri.contains_key(u.as_str())) {
            out.notify("textDocument/publishDiagnostics", json!({ "uri": stale, "diagnostics": [] }));
        }
        self.error_uris = by_uri.keys().map(|u| u.to_string()).collect();
        for (u, diags) in by_uri {
            out.notify("textDocument/publishDiagnostics", json!({ "uri": u, "diagnostics": diags }));
        }
    }
}

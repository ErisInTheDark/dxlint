//! The `.dx` front end: lexing, macro expansion, imports and parsing into
//! a [`Config`](ast::Config). Compiling that into a runnable linter lives in
//! [`crate::engine`].

pub mod ast;
pub mod error;
pub mod lexer;
mod parser;
pub use parser::build_one_config;
pub mod schema;
pub mod source;
pub mod std_lib;
mod stream;

use std::path::{Path, PathBuf};

/// The project config file name dxlint looks for.
pub const CONFIG_NAME: &str = ".dx";

/// The nearest `.dx` in `start` or any parent directory.
pub fn find(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(source::normalize_path(start));
    while let Some(d) = dir {
        let candidate = d.join(CONFIG_NAME);
        if crate::engine::reads::is_file(&candidate) {
            return Some(candidate);
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

pub fn not_found_message() -> String {
    format!("no {CONFIG_NAME} found in this directory or any parent; pass one with --config")
}

use ast::Config;
use error::ConfigError;
use source::{SourceMap, SourceProvider};

pub struct Loaded {
    pub config: Config,
    pub sources: SourceMap,
    pub errors: Vec<ConfigError>,
    /// A hash of every token parsed, imports and macros expanded: equal
    /// fingerprints mean the same rules (comments and spacing aside).
    pub fingerprint: u64,
}

pub fn load_file(path: &Path, provider: &dyn SourceProvider) -> Loaded {
    let mut sources = SourceMap::default();
    let mut parser = parser::Parser::new(&mut sources, provider);
    parser.parse_entry(path);
    let fingerprint = parser.finish();
    let (config, errors) = (std::mem::take(&mut parser.config), std::mem::take(&mut parser.errors));
    Loaded { config, sources, errors, fingerprint }
}

/// `root` with the nested configs below it that fold into it: one whose
/// first item imports its nearest ancestor config (directly or through
/// plain files) and that otherwise holds only what a `files` block can
/// becomes `files "<its folder>/**" { ... }` after that ancestor, deeper
/// folders last. `nested` lists the candidates, given the root's `ignore`
/// globs (folders they ignore hold nothing to lint, so their configs never
/// fold). Returns the folded configs too; the others stay separate.
pub fn load_tree(root: &Path, nested: impl FnOnce(&crate::path_globs::PathGlobs) -> Vec<PathBuf>, provider: &dyn SourceProvider) -> (Loaded, Vec<PathBuf>) {
    let root = source::normalize_path(root);
    let root_dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut sources = SourceMap::default();
    let mut parser = parser::Parser::new(&mut sources, provider);
    parser.parse_entry(&root);
    let ignores = crate::path_globs::PathGlobs::new(parser.config.ignores.iter().map(|(g, _)| g.as_str())).unwrap_or_default();
    let mut nested: Vec<PathBuf> = nested(&ignores).iter().map(|p| source::normalize_path(p))
        .filter(|p| *p != root && p.parent().and_then(|dir| dir.strip_prefix(&root_dir).ok()).is_some_and(|rel| !ignores.matches(rel)))
        .collect();
    nested.sort_by_key(|p| p.components().count());
    // What each folded config (the root included) read: the files its
    // nested configs' imports of it skip.
    let mut read: Vec<(PathBuf, std::collections::HashSet<String>)> = vec![(root_dir.clone(), parser.imported().clone())];
    let mut folded = Vec::new();
    for config in &nested {
        let Some(dir) = config.parent() else { continue };
        // Its nearest ancestor config, which must itself be part of this tree.
        let ancestor = dir.ancestors().skip(1).find_map(|d| {
            let candidate = d.join(CONFIG_NAME);
            (candidate == root || nested.contains(&candidate)).then_some(d.to_path_buf())
        });
        let Some(parent) = ancestor.and_then(|d| read.iter().find(|(r, _)| *r == d).map(|(_, files)| files.clone())) else { continue };
        let Some(own) = parser::Parser::folds(config, &parent, provider) else { continue };
        let rel = dir.strip_prefix(&root_dir).unwrap_or(dir).to_string_lossy().replace('\\', "/");
        parser.parse_folded(config, format!("{rel}/**"), parent.clone());
        read.push((dir.to_path_buf(), parent.union(&own).cloned().collect()));
        folded.push(config.clone());
    }
    let fingerprint = parser.finish();
    let (config, errors) = (std::mem::take(&mut parser.config), std::mem::take(&mut parser.errors));
    (Loaded { config, sources, errors, fingerprint }, folded)
}

pub fn load_text(name: &str, text: &str, provider: &dyn SourceProvider) -> Loaded {
    let mut sources = SourceMap::default();
    let mut parser = parser::Parser::new(&mut sources, provider);
    parser.parse_text(name, text);
    let fingerprint = parser.finish();
    let (config, errors) = (std::mem::take(&mut parser.config), std::mem::take(&mut parser.errors));
    Loaded { config, sources, errors, fingerprint }
}

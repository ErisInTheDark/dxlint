//! The `.zlint` front end: lexing, macro expansion, imports and parsing into
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

/// The project config file name zlint looks for.
pub const CONFIG_NAME: &str = ".zlint";

/// What the config file used to be called.
pub const OLD_CONFIG_NAME: &str = "zoomie.zlint";

/// The nearest `.zlint` in `start` or any parent directory. A nearer config
/// under its old name stops the search: it needs renaming ([`find_old`]),
/// and a config further up is not the project's.
pub fn find(start: &Path) -> Option<PathBuf> {
    nearest(start).filter(|p| p.file_name().is_some_and(|n| n == CONFIG_NAME))
}

/// The nearest config, when it still has its old name.
pub fn find_old(start: &Path) -> Option<PathBuf> {
    nearest(start).filter(|p| p.file_name().is_some_and(|n| n == OLD_CONFIG_NAME))
}

fn nearest(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(source::normalize_path(start));
    while let Some(d) = dir {
        for name in [CONFIG_NAME, OLD_CONFIG_NAME] {
            let candidate = d.join(name);
            if crate::engine::reads::is_file(&candidate) {
                return Some(candidate);
            }
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

/// "no .zlint found", with a hint when an old `zoomie.zlint` sits around.
pub fn not_found_message(start: &Path) -> String {
    match find_old(start) {
        Some(old) => format!("no {CONFIG_NAME} found, but {} is there: config files are named `{CONFIG_NAME}` now, rename it", old.display()),
        None => format!("no {CONFIG_NAME} found in this directory or any parent; pass one with --config"),
    }
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

pub fn load_text(name: &str, text: &str, provider: &dyn SourceProvider) -> Loaded {
    let mut sources = SourceMap::default();
    let mut parser = parser::Parser::new(&mut sources, provider);
    parser.parse_text(name, text);
    let fingerprint = parser.finish();
    let (config, errors) = (std::mem::take(&mut parser.config), std::mem::take(&mut parser.errors));
    Loaded { config, sources, errors, fingerprint }
}

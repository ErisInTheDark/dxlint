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

pub fn load_text(name: &str, text: &str, provider: &dyn SourceProvider) -> Loaded {
    let mut sources = SourceMap::default();
    let mut parser = parser::Parser::new(&mut sources, provider);
    parser.parse_text(name, text);
    let fingerprint = parser.finish();
    let (config, errors) = (std::mem::take(&mut parser.config), std::mem::take(&mut parser.errors));
    Loaded { config, sources, errors, fingerprint }
}

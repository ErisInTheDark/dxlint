//! The language-neutral runtime: rule expressions compiled and evaluated, and
//! the lowered VM running attributes and rule conditions.
//!
//! The lint engine above owns files, rules and fixes; everything here answers
//! questions about a tree, a node or a file it is handed.

pub mod expr;
pub mod language_options;
pub mod vm;

pub use dx_config as config;
pub use dx_core::{fx_hash, path_globs, reads};
pub use dx_grammar as grammar;
pub use dx_names as names;
pub use dx_pattern as pattern;
pub use dx_tree::{footprint, line_index};
#[cfg(feature = "work")]
pub use dx_core::work;

//! zoomie-lint: an extremely fast linter whose rules are written in the
//! `.zlint` language.
//!
//! - [`config`] reads `.zlint` files (macros, imports, parsing)
//! - [`pattern`] is the regex subset matchers are written in
//! - [`grammar`] turns matchers into layers and parses files into trees
//! - [`expr`] evaluates formatter expressions
//! - [`engine`] runs formatters over trees and applies fixes
//! - `vm` evaluates attributes and rule conditions lowered to typed code

pub mod alloc;
pub mod cli;
pub mod config;
pub mod engine;
pub mod expr;
pub mod fx_hash;
pub mod grammar;
pub mod line_index;
pub mod lsp;
pub mod native;
pub mod path_globs;
pub mod pattern;
pub mod sampler;
pub(crate) mod js_regex;
pub(crate) mod vm;

/// The native front ends this build has (`native ts`).
pub(crate) static FRONTENDS: &[&dyn engine::frontend::Frontend] = &[&native::ts::frontend::TS];

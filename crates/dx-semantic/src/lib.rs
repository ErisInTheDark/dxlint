//! Semantic evaluation over the names layer: values, calls, effects,
//! narrowing and modules, and the query functions the VM dispatches to.
//!
//! The names crate holds the tables, compile and resolution; this crate
//! answers questions over them. It is a sibling of the expression runtime:
//! the runtime reaches it only through [`crate::queries`]' ids and specs.

pub use dx_config as config;
pub use dx_core::fx_hash;
pub use dx_grammar as grammar;
pub use dx_names as names;
pub use dx_tree::footprint;

pub mod calls;
pub mod effects;
pub mod memo;
pub mod modules;
pub mod name_queries;
pub mod narrowing;
pub mod question;
pub mod queries;
pub mod semantic;
pub mod values;

pub use dx_names::*;

/// Builds plain names in tests: no builtin names and no constants.
#[cfg(test)]
pub(crate) fn plain_env() -> dx_grammar::BuildEnv<'static> {
    dx_grammar::BuildEnv::plain()
}

/// Builds a grammar and its names in tests, with no builtin names and no
/// constants.
#[cfg(test)]
pub(crate) fn build_plain(config: &dx_config::ast::Config, errors: &mut Vec<dx_config::error::ConfigError>) -> (dx_grammar::Grammar, dx_names::Names) {
    let env = plain_env();
    let grammar = dx_grammar::Grammar::build(config, errors, &env);
    let names = dx_names::Names::build(config, &grammar, errors, env.constant);
    (grammar, names)
}

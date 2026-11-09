//! Modules: the page `import`s and `exports` are compiled from. Publication
//! walks and cross-file lookups over them live in `dx-semantic`.

pub mod compile;
pub mod resolution;
pub mod index;
mod metadata;
mod configuration;
mod profiles;
mod context;
mod inputs;

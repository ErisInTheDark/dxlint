//! Compiled expressions and VM programs as data: expression values, semantic
//! subjects, pattern tests and lowered programs. Evaluation lives above.

pub mod builtins;
pub mod expr;
pub mod functions;
pub mod ops;
pub mod pipeline;
pub mod re;
pub mod semantic;
pub mod value;
pub mod vm;

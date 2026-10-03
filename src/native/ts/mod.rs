//! The TypeScript/JavaScript front end: a hand-written parser whose nodes
//! become `ts_*` layers.

mod bind;
mod check;
mod class_members;
mod comments;
mod constants;
mod docs;
mod equality;
pub mod facts;
mod flow;
mod globals;
pub mod kinds;
mod lexer;
pub mod modules;
mod parse;
mod paths;
pub mod reads;
mod regex;
mod returns;
mod roles;
mod statics;
mod summary;
pub mod syntax;
mod tokens;
pub mod topics;
mod types;
pub mod unused;
mod values;
mod view;
#[cfg(test)]
mod tests;

use std::path::Path;

use crate::grammar::Node;

pub use regex::flags_error as regex_flags_error;
pub use topics::FACT_GROUPS;
pub use regex::is_mark as is_combining_mark;

pub struct Parsed {
    /// Nodes with `matcher` = [`kinds::Kind`] and edge labels =
    /// [`kinds::Field`] (plain text nodes use `TEXT`).
    pub nodes: Vec<Node>,
    pub root: u32,
}

/// Which files this front end reads, and whether they may contain JSX.
pub fn reads(path: &Path) -> Option<bool> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "ts" | "mts" | "cts" | "js" | "mjs" | "cjs" => Some(false),
        "tsx" | "jsx" => Some(true),
        _ => None,
    }
}

/// Parses a file, or says where (byte offset) and why it could not.
pub fn parse(src: &str, jsx: bool) -> Result<Parsed, (u32, String)> {
    let mut p = parse::Parser::new(src, jsx);
    match p.parse_program() {
        Ok(root) => Ok(Parsed { nodes: std::mem::take(&mut p.nodes), root }),
        Err(_) => Err(p.error.take().unwrap_or((0, "syntax error".into()))),
    }
}

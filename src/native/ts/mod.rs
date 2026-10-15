//! The TypeScript/JavaScript front end: a hand-written parser whose nodes
//! become `ts_*` layers.

mod bind;
mod check;
mod comments;
mod docs;
mod equality;
pub(crate) mod environment;
pub mod facts;
mod flow;
pub mod frontend;
mod globals;
pub mod kinds;
mod lexer;
pub mod modules;
mod parse;
mod paths;
pub mod reads;
mod returns;
pub(crate) mod statics;
mod summary;
pub mod syntax;
pub(crate) mod tokens;
pub mod topics;
mod types;
mod values;
mod view;
#[cfg(test)]
mod tests;

use std::path::Path;

use crate::grammar::Node;

pub struct Parsed {
    /// Nodes with `matcher` = [`kinds::Kind`] and edge labels =
    /// [`kinds::Field`] (plain text nodes use `TEXT`).
    pub nodes: Vec<Node>,
    pub root: u32,
    pub(crate) tokens: Option<tokens::Lexed>,
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
        Ok(root) => Ok(Parsed { nodes: std::mem::take(&mut p.nodes), root, tokens: None }),
        Err(_) => Err(p.error.take().unwrap_or((0, "syntax error".into()))),
    }
}

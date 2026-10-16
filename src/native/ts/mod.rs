//! The TypeScript/JavaScript front end: a hand-written parser whose nodes
//! become `ts_*` layers, and the token streams rules scan. Every semantic
//! (names, types, modules) lives in `std/ts` on the engine's queries.

mod comments;
pub mod file;
pub mod frontend;
pub mod kinds;
mod lexer;
mod parse;
pub mod syntax;
pub(crate) mod tokens;
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

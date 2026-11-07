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
    parse_with(src, jsx, &Extensions::default())
}

/// Syntax a language adds to TypeScript for its transforms to map away.
#[derive(Default)]
pub struct Extensions<'a> {
    /// More words a class member may start with.
    pub member_modifiers: &'a [&'a str],
    /// More tokens that may introduce a class member's initializer, as `=` does.
    pub member_initializers: &'a [&'a str],
}

/// [`parse`], with a language's [`Extensions`].
pub fn parse_with(src: &str, jsx: bool, extensions: &Extensions<'_>) -> Result<Parsed, (u32, String)> {
    let mut p = parse::Parser::new(src, jsx);
    p.member_modifiers = extensions.member_modifiers.iter().map(|&word| word.to_owned()).collect();
    p.member_initializers = extensions.member_initializers.iter().map(|&token| token.to_owned()).collect();
    match p.parse_program() {
        Ok(root) => Ok(Parsed { nodes: std::mem::take(&mut p.nodes), root, tokens: None }),
        Err(_) => Err(p.error.take().unwrap_or((0, "syntax error".into()))),
    }
}

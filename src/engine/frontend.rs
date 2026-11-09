//! Front ends: declared parsers (`parser <name> in <lang>`) owning their
//! language's root, for syntax a pattern grammar cannot own. `native <name>`
//! in a config enables one. The engine knows them only through [`Frontend`]:
//! their node kinds and fields, parsing, and one object per parsed file.

use std::any::Any;
use std::path::Path;
use std::sync::Arc;

use crate::expr::Value;
use crate::grammar::{NativeFront, Node, Tree};

/// What one parsed file answers about its tokens and names.
pub trait Native {
    /// Byte `at` lies strictly inside one token or comment (a line of a
    /// multi-line string or block comment starts there).
    fn inside_token(&self, at: usize) -> bool;
    /// The file's code tokens as the parser read them (comments left out;
    /// strings, templates and regexes whole), for `token(...)`.
    fn tokens(&self) -> Option<std::rc::Rc<Tokens>> {
        None
    }
    /// What a name's text means for name resolution (escapes decoded).
    fn name_key<'s>(&self, text: &'s str) -> std::borrow::Cow<'s, str> {
        std::borrow::Cow::Borrowed(text)
    }
}

/// A file's code tokens: byte spans in source order.
pub struct Tokens(pub Box<[(u32, u32)]>);

impl Tokens {
    /// Tokens wholly inside `start..end`.
    pub fn within(&self, start: u32, end: u32) -> &[(u32, u32)] {
        let from = self.0.partition_point(|&(token, _)| token < start);
        let to = self.0.partition_point(|&(_, token)| token <= end).max(from);
        &self.0[from..to]
    }

    /// The last token ending at or before `at`.
    pub fn before(&self, at: u32) -> Option<usize> {
        self.0.partition_point(|&(_, end)| end <= at).checked_sub(1)
    }

    /// The first token starting at or after `at`.
    pub fn after(&self, at: u32) -> Option<usize> {
        let index = self.0.partition_point(|&(start, _)| start < at);
        (index < self.0.len()).then_some(index)
    }

    /// The token matching the bracket-like token at `index` (an opener
    /// pairs forward, a closer backward), by `pairs` of opener and closer
    /// texts. A closer with no open partner is skipped; one closing an
    /// outer opener drops the unclosed openers inside it.
    pub fn partner(&self, index: usize, source: &str, pairs: &[(&str, &str)]) -> Option<usize> {
        let text = |at: usize| {
            let (start, end) = self.0[at];
            source.get(start as usize..end as usize).unwrap_or("")
        };
        let own = text(index);
        let forward = pairs.iter().any(|&(open, _)| open == own);
        if !forward && !pairs.iter().any(|&(_, close)| close == own) { return None }
        // Walking backward, closers open and openers close.
        let pairs: Vec<(&str, &str)> = if forward { pairs.to_vec() } else { pairs.iter().map(|&(open, close)| (close, open)).collect() };
        let mut stack = vec![own];
        let mut closes = |at: usize| {
            let found = text(at);
            if pairs.iter().any(|&(open, _)| open == found) {
                stack.push(found);
            } else if let Some(depth) = stack.iter().rposition(|&open| pairs.contains(&(open, found))) {
                stack.truncate(depth);
                return stack.is_empty();
            }
            false
        };
        if forward {
            (index + 1..self.0.len()).find(|&at| closes(at))
        } else {
            (0..index).rev().find(|&at| closes(at))
        }
    }
}

/// Which node kinds may fill a field of one node kind.
#[derive(Default)]
pub struct FieldShape {
    /// Front-end kind numbers.
    pub kinds: Vec<u16>,
    /// Plain text can fill it.
    pub text: bool,
    /// Anything can (the front end does not say).
    pub any: bool,
}

/// A parsed file: nodes numbered by the front end's own kinds and fields
/// ([`NativeFront::remap`] turns them into grammar ids), and whatever the
/// front end keeps for its fact object.
pub struct Parsed {
    pub nodes: Vec<Node>,
    pub root: u32,
    pub state: Option<Arc<dyn Any + Send + Sync>>,
}

/// One file's facts are built from these.
pub struct FileInput<'t, 'o> {
    pub tree: &'t Tree,
    pub text: &'t str,
    pub front: &'t NativeFront,
    pub path: &'t Path,
    /// Language options (`language ts { globals ... }`) for this file.
    pub options: &'o [(&'o str, &'o Value<'static>)],
    pub state: Option<Arc<dyn Any + Send + Sync>>,
}

pub trait Frontend: Send + Sync {

    /// Node kind names, by kind number: the layers it defines.
    fn kinds(&self) -> &[String];
    /// Field names, by field number.
    fn fields(&self) -> &[String];
    /// The fields a node kind has.
    fn kind_fields(&self, kind: u16) -> Vec<u16>;
    /// The field holding a node's own punctuation and keywords.
    fn token_field(&self) -> u16;
    /// Token nodes (punctuation, keywords), built only when rules need them.
    fn is_token(&self, kind: u16) -> bool;
    /// Literal text: no code (and so no comment) inside.
    fn is_literal_text(&self, kind: u16) -> bool;
    /// The kind of a whole file's root node.
    fn root_kind(&self) -> u16;
    fn field_shape(&self, kind: u16, field: u16) -> FieldShape;

    /// Parses a file (`path` relative to the config) under its language's
    /// options, or says where (byte offset) and why it could not.
    fn parse(&self, text: &str, path: &Path, options: &[(&str, &Value<'static>)]) -> Result<Parsed, (u32, String)>;
    /// Adds token nodes of the `wanted` kinds (by kind number).
    fn attach_tokens(&self, text: &str, parsed: &mut Parsed, wanted: &[bool]);
    /// The object of one parsed file.
    fn file<'t>(&'t self, input: FileInput<'t, '_>) -> Box<dyn Native + 't>;
}

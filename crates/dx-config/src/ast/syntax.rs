//! A declared parser and the language whose root it owns.

use super::Name;
use dx_pattern::syntax::Definition;

#[derive(Clone, Debug)]
pub struct ParserDecl {
    pub name: Name,
    pub language: Name,
    pub definition: Definition,
    /// Only these flags read same-name boolean language options initially.
    pub option_flags: Vec<String>,
    /// `name_escape /regex/`: escapes a name may hold; a match stands for the
    /// character whose hex code point its first matching group captured.
    pub name_escapes: Vec<String>,
    /// Flags on in files matching their globs (config-relative), from
    /// `extend parser x { files ... }`.
    pub file_flags: Vec<(String, Vec<String>)>,
}

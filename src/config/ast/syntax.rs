//! A declared parser and the language whose root it owns.

use super::Name;
use crate::pattern::syntax::Definition;

#[derive(Clone, Debug)]
pub struct ParserDecl {
    pub name: Name,
    pub language: Name,
    pub definition: Definition,
}

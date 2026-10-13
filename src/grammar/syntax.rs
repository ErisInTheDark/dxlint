//! Executes declared grammars without language-specific parser code.

mod compile;
mod captures;
mod run;
mod shapes;
mod values;
mod registry;

pub use values::ValueSource;
pub use registry::Parsers;

use crate::pattern::syntax::{Definition, Node as NodeSchema, Token};
use super::Node;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Context {
    pub goal: u16,
    pub flags: u64,
}

/// Local node kinds and their fields, mapped to the engine's grammar ids.
pub struct Registration {
    pub matchers: Vec<u32>,
    pub labels: Vec<Vec<u16>>,
    pub text_matcher: u32,
}

#[derive(Clone, Debug)]
pub struct TokenSpan {
    pub token: u16,
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug)]
pub struct TriviaSpan {
    pub start: u32,
    pub end: u32,
    pub line_break: bool,
}

pub struct Parsed {
    pub nodes: Vec<Node>,
    pub root: u32,
    pub tokens: Vec<TokenSpan>,
    pub trivia: Vec<TriviaSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub offset: u32,
    pub expected: Vec<String>,
    pub committed: bool,
}

pub struct CompiledParser {
    definition: Definition,
    program: compile::Program,
    caches: run::Pool,
}

impl CompiledParser {
    pub fn compile(definition: &Definition) -> Result<Self, String> {
        let definition = shapes::resolve(definition)?;
        let program = compile::Program::new(&definition)?;
        Ok(Self { definition, program, caches: run::Pool::default() })
    }

    pub fn nodes(&self) -> &[NodeSchema] {
        &self.definition.nodes
    }

    pub fn tokens(&self) -> &[Token] {
        &self.definition.tokens
    }

    pub fn flags(&self) -> &[String] {
        &self.definition.flags
    }

    pub fn parse_root(&self, text: &str, context: Context, registration: &Registration) -> Result<Parsed, ParseError> {
        run::Run::new(self, text).parse(context, registration)
    }
}

#[cfg(test)]
mod tests;

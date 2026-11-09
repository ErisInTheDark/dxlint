//! Executes declared grammars without language-specific parser code.

mod compile;
mod captures;
mod frontend;
mod phases;
mod run;
mod shapes;
mod values;
mod registry;

pub use frontend::DeclaredFrontend;
pub use phases::{set_phase_profiling, take_phase_totals, PhaseTotal};
pub use values::ValueSource;
pub use registry::Parsers;

use std::sync::{Arc, Mutex};

use dx_pattern::syntax::{Definition, Node as NodeSchema, Token};
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

/// Definitions compiled lately in this process, newest last: configurations
/// built one after another (fixtures, tests, a language server's rebuilds,
/// nested configs) mostly declare the same parsers.
static COMPILED: Mutex<Vec<(Definition, Arc<CompiledParser>)>> = Mutex::new(Vec::new());
const KEPT_COMPILED: usize = 4;

impl CompiledParser {
    pub fn compile(definition: &Definition) -> Result<Self, String> {
        let definition = shapes::resolve(definition)?;
        let program = compile::Program::new(&definition)?;
        Ok(Self { definition, program, caches: run::Pool::default() })
    }

    /// `definition` compiled, shared with whoever compiled it lately.
    pub fn shared(definition: &Definition) -> Result<Arc<Self>, String> {
        let kept = |compiled: &mut Vec<(Definition, Arc<Self>)>| {
            let at = compiled.iter().position(|(known, _)| known == definition)?;
            let entry = compiled.remove(at);
            let parser = entry.1.clone();
            compiled.push(entry);
            Some(parser)
        };
        if let Some(parser) = kept(&mut COMPILED.lock().expect("compiled parsers")) { return Ok(parser) }
        let parser = Arc::new(Self::compile(definition)?);
        let mut compiled = COMPILED.lock().expect("compiled parsers");
        if let Some(raced) = kept(&mut compiled) { return Ok(raced) }
        if compiled.len() == KEPT_COMPILED { compiled.remove(0); }
        compiled.push((definition.clone(), parser.clone()));
        Ok(parser)
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
        let parsed = phases::phase("parse", || run::Run::new(self, text).parse(context, registration));
        phases::flush();
        parsed
    }
}

#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod tests;

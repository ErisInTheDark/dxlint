use std::ops::{Deref, DerefMut};
use std::sync::Mutex;

use regex_automata::meta::Cache;
use crate::pattern::dfa::LexerCache;

use crate::grammar::syntax::compile::Program;

pub(super) struct RegexCaches {
    pub tokens: Vec<Cache>,
    pub trivia: Vec<Cache>,
    pub line: Cache,
    pub predicates: Vec<Cache>,
    pub lexers: Vec<Option<LexerCache>>,
    pub trivia_scan: Option<LexerCache>,
}

impl RegexCaches {
    fn new(program: &Program) -> Self {
        Self {
            tokens: program.tokens.iter().map(|dfa| dfa.cache()).collect(),
            trivia: program.trivia.iter().map(|dfa| dfa.cache()).collect(),
            line: program.line_break.cache(),
            predicates: program.predicates.iter().map(|dfa| dfa.cache()).collect(),
            lexers: program.lexers.iter().map(|lexer| lexer.as_ref().map(|lexer| lexer.cache())).collect(),
            trivia_scan: program.trivia_lexer.as_ref().map(|lexer| lexer.cache()),
        }
    }
}

pub(super) struct Storage {
    pub regex: RegexCaches,
    pub scratch: super::Scratch,
}

#[derive(Default)]
pub(crate) struct Pool(Mutex<Vec<Storage>>);

impl Pool {
    pub(super) fn lease(&self, program: &Program) -> Lease<'_> {
        let cached = { self.0.lock().expect("parser cache pool lock").pop() };
        Lease { pool: self, storage: Some(cached.unwrap_or_else(|| Storage {
            regex: RegexCaches::new(program), scratch: super::Scratch::default(),
        })) }
    }
}

pub(super) struct Lease<'a> {
    pool: &'a Pool,
    storage: Option<Storage>,
}

impl Deref for Lease<'_> {
    type Target = Storage;
    fn deref(&self) -> &Self::Target { self.storage.as_ref().expect("active parser storage lease") }
}

impl DerefMut for Lease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target { self.storage.as_mut().expect("active parser storage lease") }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.pool.0.lock().expect("parser cache pool lock")
            .push(self.storage.take().expect("active parser storage lease"));
    }
}

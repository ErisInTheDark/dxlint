//! One root question's engines. Borrowed handles share its memo state.

use super::question::{QueryContext, World};
use super::values::{State, Values};
use super::Names;

pub struct Semantic<'a> {
    pub world: &'a dyn World,
    pub tables: &'a Names,
    pub question: &'a QueryContext,
    pub(crate) values: State,
}

impl<'a> Semantic<'a> {
    pub fn new(world: &'a dyn World, tables: &'a Names, question: &'a QueryContext) -> Self {
        Self { world, tables, question, values: State::default() }
    }

    pub fn values(&self) -> Values<'_> {
        Values { semantic: self }
    }
}

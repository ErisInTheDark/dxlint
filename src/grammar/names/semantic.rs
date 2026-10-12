//! One root question's engines. Borrowed handles share its memo state.

use std::cell::{Ref, RefCell};

use super::question::{Leaf, QueryContext, World};
use super::values::{State, Values};
use super::Names;

pub struct Semantic<'a> {
    pub world: &'a dyn World,
    pub tables: &'a Names,
    pub question: &'a QueryContext,
    pub(crate) values: State,
    pub(crate) calls: super::calls::State,
    defects: RefCell<Vec<Defect>>,
}

pub struct Defect {
    pub item: u32,
    pub at: Leaf,
    pub message: Box<str>,
}

impl<'a> Semantic<'a> {
    pub fn new(world: &'a dyn World, tables: &'a Names, question: &'a QueryContext) -> Self {
        Self { world, tables, question, values: State::default(), calls: super::calls::State::default(), defects: RefCell::default() }
    }

    pub fn values(&self) -> Values<'_> {
        Values { semantic: self }
    }

    pub fn calls(&self) -> super::calls::Calls<'_> {
        super::calls::Calls { semantic: self }
    }

    pub fn defect(&self, item: u32, at: Leaf, message: impl Into<Box<str>>) {
        Defect::record(&mut self.defects.borrow_mut(), Defect { item, at, message: message.into() });
    }

    pub fn defects(&self) -> Ref<'_, [Defect]> {
        Ref::map(self.defects.borrow(), Vec::as_slice)
    }
}

impl Defect {
    fn record(_defects: &mut Vec<Defect>, _defect: Defect) {}
}

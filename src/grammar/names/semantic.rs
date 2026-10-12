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
    pub(crate) effects: super::effects::State,
    pub(crate) modules: super::modules::queries::State,
    defects: RefCell<Vec<Defect>>,
}

pub struct Defect {
    pub item: u32,
    pub at: Leaf,
    pub message: Box<str>,
}

impl<'a> Semantic<'a> {
    pub fn new(world: &'a dyn World, tables: &'a Names, question: &'a QueryContext) -> Self {
        Self { world, tables, question, values: State::default(), calls: super::calls::State::default(), effects: super::effects::State::default(), modules: super::modules::queries::State::default(), defects: RefCell::default() }
    }

    pub fn values(&self) -> Values<'_> {
        Values { semantic: self }
    }

    pub fn calls(&self) -> super::calls::Calls<'_> {
        super::calls::Calls { semantic: self }
    }

    pub fn effects(&self) -> super::effects::Effects<'_> {
        super::effects::Effects { semantic: self }
    }

    pub fn modules(&self) -> super::modules::queries::Queries<'_> {
        super::modules::queries::Queries { semantic: self }
    }

    pub fn narrowing(&self) -> super::narrowing::Narrowing<'_> {
        super::narrowing::Narrowing { semantic: self }
    }

    pub fn defect(&self, item: u32, at: Leaf, message: impl Into<Box<str>>) {
        Defect::record(&mut self.defects.borrow_mut(), Defect { item, at, message: message.into() });
    }

    pub fn defects(&self) -> Ref<'_, [Defect]> {
        Ref::map(self.defects.borrow(), Vec::as_slice)
    }
}

impl Defect {
    pub fn file(&self) -> u32 {
        match self.at.target {
            super::question::Target::Node(node) => node.file,
            super::question::Target::Scope(scope) => scope.file,
        }
    }

    fn record(defects: &mut Vec<Defect>, defect: Defect) {
        if !defects.iter().any(|previous| previous.item == defect.item && previous.file() == defect.file()) {
            defects.push(defect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::names::question::{FrameId, NodeRef, ScopeRef, Target};

    #[test]
    fn defects_deduplicate_per_item_and_file_not_reference_or_view() {
        let first = Leaf { target: Target::Node(NodeRef::here(0)), frame: FrameId::ROOT };
        let sites = [
            (7, first),
            (7, Leaf { target: Target::Node(NodeRef::here(1)), frame: FrameId::ROOT }),
            (7, Leaf { target: Target::Scope(ScopeRef { file: 0, scope: 0 }), frame: FrameId::ROOT }),
            (8, first),
            (7, Leaf { target: Target::Node(NodeRef { file: 1, node: 0 }), frame: FrameId::ROOT }),
        ];
        let mut defects = Vec::new();
        for (item, at) in sites {
            Defect::record(&mut defects, Defect { item, at, message: "conflict".into() });
        }
        assert_eq!(defects.len(), 3);
        assert_eq!(defects[0].at, first);
        assert_eq!(defects.iter().filter(|defect| defect.item == 7).count(), 2);
    }
}

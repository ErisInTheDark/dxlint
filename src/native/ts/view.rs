//! One way to read native nodes, whether they still use the front end's
//! kind and field numbers (a file summarised for other files) or a
//! grammar's ids (the file being linted).

use super::kinds::{Field, Kind, ALL_KINDS};
use super::syntax::Syntax;
use crate::grammar::Node;

pub trait View {
    fn kind(&self, id: u32) -> Option<Kind>;
    fn get(&self, id: u32, field: Field) -> Option<u32>;
    fn all(&self, id: u32, field: Field) -> Vec<u32>;
    fn children(&self, id: u32) -> impl DoubleEndedIterator<Item = u32> + '_;
    fn text(&self, id: u32) -> &str;
    fn start(&self, id: u32) -> u32;
    fn src(&self) -> &str;

    fn is(&self, id: u32, kind: Kind) -> bool {
        self.kind(id) == Some(kind)
    }

    fn has_word(&self, id: u32, field: Field, word: &str) -> bool {
        self.all(id, field).into_iter().any(|n| self.text(n) == word)
    }
}

/// Only external import aliases make a file a module; `import X = A.B` does not.
pub fn external_module_statement(v: &impl View, node: u32) -> bool {
    matches!(v.kind(node), Some(Kind::Import | Kind::Export | Kind::ExportAssign | Kind::ImportEquals))
        && (!v.is(node, Kind::ImportEquals) || v.get(node, Field::Value).is_some_and(|value| v.is(value, Kind::Call)))
}

pub fn global_augmentation(v: &impl View, root: u32) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match v.kind(node) {
            Some(Kind::Namespace) => {
                if v.get(node, Field::Kind).is_some_and(|kind| v.text(kind) == "global") { return true }
                pending.extend(v.get(node, Field::Body));
            }
            Some(Kind::Program | Kind::Block) => pending.extend(v.all(node, Field::Body)),
            Some(Kind::Export) => pending.extend(v.children(node)),
            _ => {}
        }
    }
    false
}

/// Nodes straight from the parser.
pub struct Raw<'a> {
    pub src: &'a str,
    pub nodes: &'a [Node],
}

impl View for Raw<'_> {
    fn kind(&self, id: u32) -> Option<Kind> {
        ALL_KINDS.get(self.nodes[id as usize].matcher as usize).copied()
    }

    fn get(&self, id: u32, field: Field) -> Option<u32> {
        self.nodes[id as usize].children.iter().find(|e| e.label == field as u16).map(|e| e.node)
    }

    fn all(&self, id: u32, field: Field) -> Vec<u32> {
        self.nodes[id as usize].children.iter().filter(|e| e.label == field as u16).map(|e| e.node).collect()
    }

    fn children(&self, id: u32) -> impl DoubleEndedIterator<Item = u32> + '_ {
        self.nodes[id as usize].children.iter().map(|edge| edge.node)
    }

    fn text(&self, id: u32) -> &str {
        let n = &self.nodes[id as usize];
        &self.src[n.start as usize..n.end as usize]
    }

    fn start(&self, id: u32) -> u32 {
        self.nodes[id as usize].start
    }

    fn src(&self) -> &str {
        self.src
    }
}

impl View for Syntax<'_> {
    fn kind(&self, id: u32) -> Option<Kind> {
        Syntax::kind(self, id)
    }

    fn get(&self, id: u32, field: Field) -> Option<u32> {
        Syntax::get(self, id, field)
    }

    fn all(&self, id: u32, field: Field) -> Vec<u32> {
        Syntax::all(self, id, field).collect()
    }

    fn children(&self, id: u32) -> impl DoubleEndedIterator<Item = u32> + '_ {
        self.tree.node(id).children.iter().map(|edge| edge.node)
    }

    fn text(&self, id: u32) -> &str {
        Syntax::text(self, id)
    }

    fn start(&self, id: u32) -> u32 {
        self.tree.node(id).start
    }

    fn src(&self) -> &str {
        self.src
    }
}

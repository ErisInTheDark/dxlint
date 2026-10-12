//! Values: `unwraps`, `fits`, `could` / `always`, `through` generics
//! (docs/design/type-layers.md). Owner: Violet.

use std::cell::RefCell;

use crate::fx_hash::FxHashMap;

use super::question::{Known, Leaf, NodeRef, NodeValue, Target};
use super::semantic::Semantic;
use super::Value;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Text(Box<str>),
    Ordinal(u32),
    Symbol(NodeRef),
}

/// One root question. Nested value, call and narrowing queries use this
/// controller and its context rather than opening another work budget.
#[derive(Default)]
pub(crate) struct State {
    values: RefCell<FxHashMap<Leaf, NodeValue>>,
    traits: RefCell<FxHashMap<(Leaf, u16), Known<bool>>>,
}

pub struct Values<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
}

impl<'a> std::ops::Deref for Values<'a> {
    type Target = Semantic<'a>;

    fn deref(&self) -> &Self::Target {
        self.semantic
    }
}

impl Values<'_> {
    pub fn values(&self, leaf: Leaf) -> NodeValue {
        if let Some(value) = self.semantic.values.values.borrow().get(&leaf) {
            return value.clone();
        }
        if !self.question.budget.step() {
            return NodeValue::Unknown;
        }
        let value = NodeValue::leaf(leaf);
        self.semantic.values.values.borrow_mut().insert(leaf, value.clone());
        value
    }

    pub fn fits(&self, leaf: Leaf, trait_id: u16) -> Known<bool> {
        if let Some(&answer) = self.semantic.values.traits.borrow().get(&(leaf, trait_id)) {
            return answer;
        }
        if trait_id >= 64 || !self.question.budget.step() {
            return Known::Unknown;
        }
        // A pending question cuts recursion without a separate cycle set.
        self.semantic.values.traits.borrow_mut().insert((leaf, trait_id), Known::Unknown);
        let answer = match leaf.target {
            Target::Scope(_) => Known::Yes(false),
            Target::Node(node) => self.direct_fits(leaf, node, trait_id),
        };
        self.semantic.values.traits.borrow_mut().insert((leaf, trait_id), answer);
        answer
    }

    fn direct_fits(&self, leaf: Leaf, node: NodeRef, trait_id: u16) -> Known<bool> {
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(node_data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(node_data.matcher as usize) else { return Known::Yes(false) };
        let mut unknown = false;
        for &(bits, when) in &kind.fits {
            if !self.question.budget.step() {
                return Known::Unknown;
            }
            if bits & (1 << trait_id) == 0 {
                continue;
            }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(leaf, condition, self.semantic)) {
                Known::Yes(true) => return Known::Yes(true),
                Known::Unknown => unknown = true,
                Known::Yes(false) => {}
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }

    pub fn could(&self, _value: &NodeValue, _traits: u64) -> Known<bool> {
        Known::Unknown
    }

    pub fn always(&self, _value: &NodeValue, _traits: u64) -> Known<bool> {
        Known::Unknown
    }

    pub fn member(&self, _value: &NodeValue, _key: &Key) -> NodeValue {
        NodeValue::Unknown
    }

    pub fn member_sites(&self, _value: &NodeValue, _key: &Key) -> Known<Vec<Leaf>> {
        Known::Unknown
    }

    pub fn evaluate(&self, _at: Leaf, _expression: &Value) -> NodeValue {
        NodeValue::Unknown
    }

    pub fn key(&self, _at: Leaf, _expression: &Value) -> Known<Key> {
        Known::Unknown
    }

    pub fn sites(&self, _at: Leaf, _expression: &Value) -> Known<Vec<Leaf>> {
        Known::Unknown
    }

    pub fn arguments(&self, _at: Leaf, _path: &[u16]) -> Known<Vec<NodeValue>> {
        Known::Unknown
    }

    pub fn follow(&self, _at: Leaf, _path: &[u16]) -> Known<Vec<Leaf>> {
        Known::Unknown
    }
}

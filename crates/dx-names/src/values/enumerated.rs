//! `enumerates`: a value that stays one leaf everywhere (an enum type) but
//! spells out its finite alternatives where something enumerates them
//! (mapped type keys, case arms).

use super::{Known, Leaf, NodeValue, Target, Values};

impl Values<'_> {
    /// `value` with every lone leaf that declares `enumerates` replaced by
    /// the alternatives it enumerates (one level); other alternatives stay.
    pub fn enumerated(&self, value: &NodeValue) -> NodeValue {
        let NodeValue::Alternatives(rows) = value else { return NodeValue::Unknown };
        if !rows.iter().any(|row| matches!(row.as_slice(), [leaf] if self.may_enumerate(*leaf))) { return value.clone() }
        let mut alternatives = Vec::with_capacity(rows.len());
        for row in rows {
            if !self.question.budget.step() { return NodeValue::Unknown }
            let [leaf] = row.as_slice() else { alternatives.push(NodeValue::rows(vec![row.clone()])); continue };
            match self.enumeration(*leaf) {
                Known::Yes(Some(spelled)) => alternatives.push(spelled),
                Known::Yes(None) => alternatives.push(NodeValue::leaf(*leaf)),
                Known::Unknown => return NodeValue::Unknown,
            }
        }
        super::eval::combine(self, alternatives, false)
    }

    /// Whether `leaf`'s kind declares `enumerates` at all (no evaluation).
    fn may_enumerate(&self, leaf: Leaf) -> bool {
        let Known::Yes((owner, _)) = self.member_context(leaf) else { return false };
        self.world.tree(owner.file).and_then(|tree| tree.nodes.get(owner.node as usize))
            .and_then(|node| self.tables.kinds.get(node.matcher as usize))
            .is_some_and(|kind| !kind.enumerates.is_empty())
    }

    /// The alternatives `leaf` enumerates; `None` when it declares none here.
    fn enumeration(&self, leaf: Leaf) -> Known<Option<NodeValue>> {
        let Known::Yes(leaf) = self.semantic.leaf(leaf) else { return Known::Unknown };
        let Known::Yes((owner, scope)) = self.member_context(leaf) else { return Known::Yes(None) };
        let Some(node) = self.world.tree(owner.file).and_then(|tree| tree.nodes.get(owner.node as usize)) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(node.matcher as usize) else { return Known::Yes(None) };
        let at = Leaf { target: Target::Node(owner), frame: leaf.frame };
        for (recipe, views, when) in &kind.enumerates {
            if !self.question.budget.step() { return Known::Unknown }
            let applies = match scope {
                Some((_, classes)) => *views != 0 && classes & views != 0,
                None => *views == 0,
            };
            if !applies { continue }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                Known::Yes(true) => return Known::Yes(Some(self.evaluate(at, recipe))),
                Known::Yes(false) => {}
                Known::Unknown => return Known::Unknown,
            }
        }
        Known::Yes(None)
    }
}

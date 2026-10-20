use super::super::question::{Known, Leaf, NodeValue};
use super::{Identity, Values};

impl Values<'_> {
    pub fn reduce(&self, declared: &NodeValue, written: &NodeValue) -> NodeValue {
        let (NodeValue::Alternatives(declared), NodeValue::Alternatives(written)) = (declared, written) else {
            return NodeValue::Unknown;
        };
        // Literal alternatives (enum members, literal types) overlap exactly
        // when their values are equal: matched by value up front instead of
        // normalizing every pair (a 77-member enum written back to itself
        // took ~3,000 normalizations).
        let scalar = |row: &[Leaf]| match self.row_identity(row) {
            Known::Yes(Some(identity)) if scalar_identity(&identity) => Some(identity),
            _ => None,
        };
        let written_scalars: Vec<Option<Identity>> = written.iter().map(|row| scalar(row)).collect();
        let mut retained = Vec::new();
        for declaration in declared {
            let declared_scalar = scalar(declaration);
            if let Some(identity) = &declared_scalar {
                if written_scalars.iter().any(|written| written.as_ref() == Some(identity)) {
                    retained.push(declaration.clone());
                    continue;
                }
            }
            // Keeping an alternative the write may not reach only widens the
            // answer; dropping the whole value for it lost every read after a
            // write of a type with generic methods (`const p = make()`). So an
            // alternative goes only when every write provably misses it.
            let mut reached = false;
            for (write, written_scalar) in written.iter().zip(&written_scalars) {
                // Two different literals never overlap.
                if declared_scalar.is_some() && written_scalar.is_some() { continue }
                if !self.question.budget.step() { return NodeValue::Unknown }
                if !self.disjoint(declaration, write) { reached = true; break }
            }
            if reached { retained.push(declaration.clone()) }
        }
        NodeValue::rows(retained)
    }

    /// Whether a declared alternative and a written one provably share no
    /// value: their intersection normalizes to never, or both are distinct
    /// literals. Anything else (overlapping, or undecided) may share one; a
    /// structural comparison could only tell those two apart, and both keep
    /// the alternative.
    fn disjoint(&self, declared: &[Leaf], written: &[Leaf]) -> bool {
        let mut joint = declared.to_vec();
        joint.extend_from_slice(written);
        match self.normalize(NodeValue::rows(vec![joint])) {
            NodeValue::Alternatives(rows) if rows.is_empty() => return true,
            NodeValue::Unknown => return false,
            _ => {}
        }
        if declared == written { return false }
        matches!((self.row_identity(declared), self.row_identity(written)),
            (Known::Yes(Some(left)), Known::Yes(Some(right))) if left != right && scalar_identity(&left) && scalar_identity(&right))
    }

    fn row_identity(&self, row: &[Leaf]) -> Known<Option<Identity>> {
        let mut identity = None;
        for &leaf in row {
            if !self.question.budget.step() { return Known::Unknown }
            match self.identity(leaf) {
                Known::Yes(Some(value)) => {
                    if identity.as_ref().is_some_and(|previous| previous != &value) { return Known::Unknown }
                    identity = Some(value);
                }
                Known::Yes(None) => {}
                Known::Unknown => return Known::Unknown,
            }
        }
        Known::Yes(identity)
    }
}

fn scalar_identity(identity: &Identity) -> bool {
    matches!(identity, Identity::Value(crate::expr::Value::Null
        | crate::expr::Value::Bool(_) | crate::expr::Value::Num(_) | crate::expr::Value::Str(_)))
}

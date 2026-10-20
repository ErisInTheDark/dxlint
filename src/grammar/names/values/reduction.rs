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
            let mut possible = false;
            let mut uncertain = false;
            let declared_scalar = scalar(declaration);
            if let Some(identity) = &declared_scalar {
                if written_scalars.iter().any(|written| written.as_ref() == Some(identity)) {
                    retained.push(declaration.clone());
                    continue;
                }
            }
            for (write, written_scalar) in written.iter().zip(&written_scalars) {
                // Two different literals never overlap.
                if declared_scalar.is_some() && written_scalar.is_some() { continue }
                if !self.question.budget.step() { return NodeValue::Unknown }
                match self.overlap(declaration, write) {
                    Known::Yes(true) => { possible = true; break }
                    Known::Yes(false) => {}
                    Known::Unknown => uncertain = true,
                }
            }
            // Keeping an alternative the write may not reach only widens the
            // answer; dropping the whole value for it lost every read after a
            // write of a type with generic methods (`const p = make()`).
            if possible || uncertain {
                retained.push(declaration.clone());
            }
        }
        NodeValue::rows(retained)
    }

    fn overlap(&self, declared: &[Leaf], written: &[Leaf]) -> Known<bool> {
        let mut joint = declared.to_vec();
        joint.extend_from_slice(written);
        match self.normalize(NodeValue::rows(vec![joint])) {
            NodeValue::Alternatives(rows) if rows.is_empty() => return Known::Yes(false),
            NodeValue::Unknown => return Known::Unknown,
            _ => {}
        }
        // A non-empty value overlaps itself (a written value of the declared
        // type), without a structural comparison of what it contains.
        if declared == written { return Known::Yes(true) }
        if let (Known::Yes(Some(left)), Known::Yes(Some(right))) = (
            self.row_identity(declared), self.row_identity(written),
        ) {
            if left == right { return Known::Yes(true) }
            if scalar_identity(&left) && scalar_identity(&right) { return Known::Yes(false) }
        }
        let declared = NodeValue::rows(vec![declared.to_vec()]);
        let written = NodeValue::rows(vec![written.to_vec()]);
        let calls = self.semantic.calls();
        if calls.accepts(&declared, &written) == Known::Yes(true)
            || calls.accepts(&written, &declared) == Known::Yes(true) {
            Known::Yes(true)
        } else {
            Known::Unknown
        }
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

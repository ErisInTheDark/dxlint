use super::{Known, Leaf, NodeValue, Values};

impl Values<'_> {
    pub fn expand(&self, value: &NodeValue) -> NodeValue {
        let Some(_nested) = self.question.budget.nest() else { return NodeValue::Unknown };
        let NodeValue::Alternatives(rows) = value else { return NodeValue::Unknown };
        let mut alternatives = Vec::with_capacity(rows.len());
        for row in rows {
            let mut parts = Vec::with_capacity(row.len());
            for &leaf in row {
                if !self.question.budget.step() { return NodeValue::Unknown }
                parts.push(self.values(leaf));
            }
            alternatives.push(super::eval::combine(self, parts, true));
        }
        super::eval::combine(self, alternatives, false)
    }

    pub fn top(&self, leaf: Leaf) -> Known<bool> {
        self.classify(leaf, self.tables.top | self.tables.dynamic)
    }

    pub fn dynamic(&self, leaf: Leaf) -> Known<bool> {
        self.classify(leaf, self.tables.dynamic)
    }

    fn classify(&self, leaf: Leaf, mask: u64) -> Known<bool> {
        let mut unknown = false;
        for trait_id in 0..64 {
            if mask & (1 << trait_id) == 0 { continue }
            match self.fits(leaf, trait_id) {
                Known::Yes(true) => return Known::Yes(true),
                Known::Unknown => unknown = true,
                Known::Yes(false) => {}
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }

    pub(crate) fn normalize(&self, value: NodeValue) -> NodeValue {
        let NodeValue::Alternatives(alternatives) = value else { return NodeValue::Unknown };
        let mask = self.tables.exclusive.iter().fold(0, |all, group| all | group);
        let mut possible = Vec::new();
        let mut absorbing = Vec::new();
        let mut dynamic = false;
        'alternative: for alternative in alternatives {
            let mut proven = 0;
            for &leaf in &alternative {
                for trait_id in 0..64 {
                    if mask & (1 << trait_id) == 0 { continue }
                    if !self.question.budget.step() { return NodeValue::Unknown }
                    if self.fits(leaf, trait_id) == Known::Yes(true) {
                        proven |= 1 << trait_id;
                        if self.tables.exclusive.iter().any(|group| (proven & group).count_ones() > 1) {
                            continue 'alternative;
                        }
                    }
                    if self.question.budget.exhausted() { return NodeValue::Unknown }
                }
            }
            let mut constrained = Vec::new();
            let mut tops = Vec::new();
            for leaf in alternative {
                if !self.question.budget.step() { return NodeValue::Unknown }
                if self.top(leaf) == Known::Yes(true) {
                    tops.push(leaf);
                } else {
                    constrained.push(leaf);
                }
                if self.question.budget.exhausted() { return NodeValue::Unknown }
            }
            if !constrained.is_empty() || tops.is_empty() {
                possible.push(constrained);
                continue;
            }
            let mut dynamic_tops = Vec::new();
            for &leaf in &tops {
                if self.dynamic(leaf) == Known::Yes(true) { dynamic_tops.push(leaf) }
                if self.question.budget.exhausted() { return NodeValue::Unknown }
            }
            if dynamic_tops.is_empty() {
                if !dynamic { absorbing.push(tops) }
            } else {
                if !dynamic { absorbing.clear(); dynamic = true }
                absorbing.push(dynamic_tops);
            }
        }
        let mut rows = if absorbing.is_empty() { possible } else { absorbing };
        let mut seen = crate::fx_hash::FxHashSet::default();
        rows.retain(|row| seen.insert(row.clone()));
        NodeValue::Alternatives(rows)
    }
}

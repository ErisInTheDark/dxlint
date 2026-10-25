use super::{Known, Leaf, NodeValue, Values};
use super::super::question::{Normal, Rows};

impl Values<'_> {
    pub fn expand(&self, value: &NodeValue) -> NodeValue {
        let Some(_nested) = self.question.budget.nest() else { return NodeValue::Unknown };
        let NodeValue::Alternatives(rows) = value else { return NodeValue::Unknown };
        // Fully normal rows whose leaves each expand to themselves are
        // already their own expansion: normalizing them again proves nothing.
        let normal = rows.normal() == Normal::Full;
        let mut unchanged = normal;
        let mut alternatives = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let mut parts = Vec::with_capacity(row.len());
            for &leaf in row {
                if !self.question.budget.step() { return NodeValue::Unknown }
                parts.push(self.values(leaf));
            }
            let same = normal && parts.iter().zip(row).all(|(part, &leaf)| *part == NodeValue::leaf(leaf));
            unchanged &= same;
            alternatives.push(if same {
                NodeValue::Alternatives(Rows::normalized(vec![row.clone()], Normal::Full))
            } else {
                super::eval::combine(self, parts, true)
            });
        }
        if unchanged { return value.clone() }
        super::eval::combine(self, alternatives, false)
    }

    pub fn top(&self, leaf: Leaf) -> Known<bool> {
        self.classify(leaf, self.tables.top | self.tables.dynamic)
    }

    pub fn dynamic(&self, leaf: Leaf) -> Known<bool> {
        self.classify(leaf, self.tables.dynamic)
    }

    /// An alternative of only top values (`any`, `unknown`): narrowing can
    /// never prove it is wholly inside or outside a set, so it stays.
    pub fn wholly_top(&self, alternative: &[Leaf]) -> bool {
        !alternative.is_empty() && alternative.iter().all(|&leaf| self.top(leaf) == Known::Yes(true))
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

    /// The traits `leaf` can fit at all: a trait set's bits, or every trait
    /// its kind declares a fit for (open fits never prove one); all traits
    /// where only evaluation can tell (products, modified values, scopes).
    fn possible_traits(&self, leaf: Leaf) -> u64 {
        let Known::Yes(leaf) = self.semantic.leaf(leaf) else { return u64::MAX };
        match leaf.target {
            super::Target::Traits(bits) => bits,
            super::Target::Node(node) => self.world.tree(node.file)
                .and_then(|tree| tree.nodes.get(node.node as usize))
                .map_or(u64::MAX, |data| self.tables.kinds.get(data.matcher as usize)
                    .map_or(0, |kind| kind.fits.iter().fold(0, |all, &(bits, _, _)| all | bits))),
            _ => u64::MAX,
        }
    }

    pub(crate) fn normalize(&self, value: NodeValue) -> NodeValue {
        self.normalize_rows(value, true)
    }

    /// [`Self::normalize`] for a union of values: a union makes no new
    /// intersection, so its rows aren't checked for contradictions (the
    /// leaf that settles the union normalizes it fully); only absorbing tops
    /// and duplicates are settled.
    pub(crate) fn normalize_union(&self, value: NodeValue) -> NodeValue {
        self.normalize_rows(value, false)
    }

    fn normalize_rows(&self, value: NodeValue, contradictions: bool) -> NodeValue {
        let NodeValue::Alternatives(alternatives) = value else { return NodeValue::Unknown };
        let level = if contradictions { Normal::Full } else { Normal::Union };
        if alternatives.normal() >= level { return NodeValue::Alternatives(alternatives) }
        if contradictions {
            if let Some(settled) = alternatives.fully_normal() { return NodeValue::Alternatives(settled) }
        }
        let mask = self.tables.exclusive.iter().fold(0, |all, group| all | group);
        let mut possible = Vec::new();
        let mut absorbing = Vec::new();
        let mut dynamic = false;
        let single = alternatives.len() == 1;
        'alternative: for alternative in alternatives {
            let trait_set = |leaf: &Leaf| matches!(leaf.target, super::Target::Traits(_));
            // A lone value is never absorbed by another alternative and can
            // only contradict itself through its own trait bits: the common
            // case asks no trait question at all.
            if single && alternative.len() == 1 && !trait_set(&alternative[0]) {
                possible.push(alternative);
                continue;
            }
            let mut proven = 0;
            // One node site can't prove two exclusive traits by itself; only
            // an intersection (or a trait set) can contradict.
            let contradicts = contradictions && (alternative.len() > 1 || alternative.first().is_some_and(trait_set));
            let fittable: Vec<u64> = alternative.iter().filter(|_| contradicts).map(|&leaf| self.possible_traits(leaf)).collect();
            // Only a group two of whose traits some leaf may fit can
            // contradict: the others' (often typed) fits are never asked.
            let union = fittable.iter().fold(0, |all, bits| all | bits);
            let checked = self.tables.exclusive.iter().filter(|&&group| (union & group).count_ones() > 1)
                .fold(0, |all, group| all | group) & mask;
            for (&leaf, fittable) in alternative.iter().zip(fittable) {
                // One step a leaf: its trait answers are memoised lookups,
                // and a row of merged declarations (lib `Array`) has many.
                if checked & fittable != 0 && !self.question.budget.step() { return NodeValue::Unknown }
                for trait_id in 0..64 {
                    if checked & fittable & (1 << trait_id) == 0 { continue }
                    if self.declared_trait(leaf, trait_id) == Known::Yes(true) {
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
        NodeValue::Alternatives(Rows::normalized(rows, level))
    }
}

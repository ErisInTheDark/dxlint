use super::{Match, Value};
use crate::fx_hash::FxHashMap;
use crate::grammar::names::question::{Known, Leaf, NodeValue, Target};
use crate::grammar::names::values::{Key, Values};

pub(super) fn accepts(expected: &NodeValue, actual: &NodeValue, values: &Values<'_>) -> Known<bool> {
    let Known::Yes(mut run) = Run::new(values, &[], &[]) else { return Known::Unknown };
    run.compare(expected, actual)
}

#[derive(Clone, Default)]
struct Candidate {
    locked: Option<NodeValue>,
    covariant: Vec<NodeValue>,
    contravariant: Vec<NodeValue>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode { Accept, Infer }

pub(super) struct Run<'a, 'world> {
    pub(super) values: &'a Values<'world>,
    positions: FxHashMap<super::NodeRef, usize>,
    candidates: Vec<Candidate>,
    pending: Vec<(NodeValue, NodeValue)>,
    expanding: Vec<Leaf>,
    validating: bool,
}

impl<'a, 'world> Run<'a, 'world> {
    pub(super) fn new(values: &'a Values<'world>, slots: &[Leaf], supplied: &[Option<NodeValue>]) -> Known<Self> {
        let mut positions = FxHashMap::default();
        for (position, slot) in slots.iter().enumerate() {
            if !values.question.budget.step() { return Known::Unknown }
            let Target::Node(node) = slot.target else { return Known::Unknown };
            if positions.insert(node, position).is_some() { return Known::Unknown }
        }
        let candidates = slots.iter().enumerate().map(|(position, _)| Candidate {
            locked: supplied.get(position).cloned().flatten(), ..Candidate::default()
        }).collect();
        Known::Yes(Self { values, positions, candidates, pending: Vec::new(), expanding: Vec::new(), validating: false })
    }

    fn slot(&self, leaf: Leaf) -> Option<usize> {
        match leaf.target { Target::Node(node) => self.positions.get(&node).copied(), _ => None }
    }

    pub(super) fn capturing(&self) -> bool {
        self.mode() == Mode::Infer && !self.validating
    }

    pub(super) fn mode(&self) -> Mode {
        if self.positions.is_empty() { Mode::Accept } else { Mode::Infer }
    }

    pub(super) fn recipe(&mut self, at: Leaf, recipe: &Value) -> NodeValue {
        if self.positions.is_empty() { return self.values.evaluate(at, recipe) }
        if !self.values.question.budget.step() { return NodeValue::Unknown }
        match recipe {
            Value::Current => self.raw_leaf(at),
            Value::Captured(value) => (**value).clone(),
            Value::Product(recipe) => {
                let values = self.values;
                values.produce_product(at, recipe, |site| match &recipe.value {
                    Some(value) => self.recipe(site, value),
                    None => self.raw_leaf(site),
                })
            }
            // A value question (`it.inferred ?? fallback`) has no syntax
            // sites: its answer is a finished value, else the fallback's.
            Value::Either(left, right) if matches!(&**left, Value::Path(path) if path.last().is_some_and(|&step| crate::grammar::names::query_step(step))) => {
                match self.values.evaluate_optional(at, left) {
                    Known::Yes(Some(value)) => value,
                    Known::Yes(None) => self.recipe(at, right),
                    Known::Unknown => NodeValue::Unknown,
                }
            }
            Value::Path(_) | Value::Either(..) | Value::Scope { .. } | Value::From(..) => {
                let Known::Yes(sites) = self.values.sites(at, recipe) else { return NodeValue::Unknown };
                if sites.is_empty() { return NodeValue::Unknown }
                let parts: Vec<_> = sites.into_iter().map(|site| self.raw_leaf(site)).collect();
                self.values.combine(parts, false)
            }
            Value::Union(parts) | Value::Intersection(parts) => {
                let inputs: Vec<_> = parts.iter().map(|part| self.recipe(at, part)).collect();
                self.values.combine(inputs, matches!(recipe, Value::Intersection(_)))
            }
            Value::Modified { source, required, readonly, flags } => {
                let source = self.recipe(at, source);
                self.values.modify(source, *required, *readonly, flags)
            }
            Value::Spread { intersection, path, value } => {
                let Known::Yes(sites) = self.values.follow(at, path) else { return NodeValue::Unknown };
                let inputs: Vec<_> = sites.into_iter().map(|site| match value {
                    Some(value) => self.recipe(site, value),
                    None => self.raw_leaf(site),
                }).collect();
                self.values.combine(inputs, *intersection)
            }
            _ => self.values.evaluate(at, recipe),
        }
    }

    fn raw_leaf(&mut self, leaf: Leaf) -> NodeValue {
        if self.positions.is_empty() { return self.values.values(leaf) }
        let Known::Yes(leaf) = self.values.semantic.leaf(leaf) else { return NodeValue::Unknown };
        if self.slot(leaf).is_some() { return NodeValue::leaf(leaf) }
        if self.expanding.contains(&leaf) { return NodeValue::Unknown }
        let Some(_nest) = self.values.question.budget.nest() else { return NodeValue::Unknown };
        self.expanding.push(leaf);
        let answer = match self.values.unwrap_recipe(leaf) {
            Known::Yes(None | Some(Value::Current)) => NodeValue::leaf(leaf),
            Known::Yes(Some(recipe)) => self.recipe(leaf, recipe),
            Known::Unknown => NodeValue::Unknown,
        };
        self.expanding.pop();
        answer
    }

    fn expand(&mut self, value: &NodeValue) -> NodeValue {
        if self.positions.is_empty() { return self.values.expand(value) }
        let NodeValue::Alternatives(rows) = value else { return NodeValue::Unknown };
        let mut alternatives = Vec::new();
        for row in rows {
            let parts: Vec<_> = row.iter().map(|&leaf| self.raw_leaf(leaf)).collect();
            alternatives.push(self.values.combine(parts, true));
        }
        self.values.combine(alternatives, false)
    }

    /// [`Run::relate`] for an inference input that may itself wait on this
    /// inference (an untyped callback reads its parameters from it): an
    /// undecidable relation leaves the captures as they were.
    pub(super) fn relate_or_skip(&mut self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        let saved = self.candidates.clone();
        let answer = self.relate(expected, actual);
        if answer == Known::Unknown { self.candidates = saved; }
        answer
    }

    pub(super) fn relate(&mut self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        if self.positions.is_empty() { return self.values.calls().accepts(expected, actual) }
        let Some(_nest) = self.values.question.budget.nest() else { return Known::Unknown };
        let pair = (expected.clone(), actual.clone());
        if self.pending.contains(&pair) { return Known::Unknown }
        self.pending.push(pair);
        let result = self.compare(expected, actual);
        self.pending.pop();
        result
    }

    fn compare(&mut self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        if self.positions.is_empty() && matches!(actual, NodeValue::Alternatives(rows) if rows.is_empty()) {
            return Known::Yes(true);
        }
        let expected = self.expand(expected);
        if matches!(&expected, NodeValue::Alternatives(rows)
            if rows.iter().any(|row| universal_target(row, self.values) == Known::Yes(true))) {
            return Known::Yes(true);
        }
        let actual = self.expand(actual);
        if let NodeValue::Alternatives(rows) = &expected {
            if rows.len() == 1 && rows[0].len() == 1 {
                if let Some(slot) = self.slot(rows[0][0]) {
                    return self.capture(slot, &actual, true);
                }
            }
        }
        if let NodeValue::Alternatives(rows) = &actual {
            if rows.len() == 1 && rows[0].len() == 1 {
                if let Some(slot) = self.slot(rows[0][0]) {
                    return self.capture(slot, &expected, false);
                }
            }
        }
        self.alternatives(&expected, &actual)
    }

    fn capture(&mut self, slot: usize, value: &NodeValue, covariant: bool) -> Known<bool> {
        if let Some(locked) = self.candidates[slot].locked.clone() {
            if !self.validating { return Known::Yes(true) }
            return if covariant { self.relate(&locked, value) } else { self.relate(value, &locked) };
        }
        if self.validating { return Known::Unknown }
        let candidates = if covariant { &mut self.candidates[slot].covariant } else { &mut self.candidates[slot].contravariant };
        if !candidates.contains(value) { candidates.push(value.clone()) }
        Known::Yes(true)
    }

    pub(super) fn arguments(&self) -> Known<Vec<Option<NodeValue>>> {
        let mut arguments = Vec::new();
        for candidate in &self.candidates {
            if let Some(value) = &candidate.locked { arguments.push(Some(value.clone())); continue }
            let covariant = (!candidate.covariant.is_empty()).then(|| self.values.combine(candidate.covariant.clone(), false));
            let contravariant = (!candidate.contravariant.is_empty()).then(|| self.values.combine(candidate.contravariant.clone(), true));
            let selected = match (covariant, contravariant) {
                (Some(covariant), Some(contravariant)) => {
                    let mut fits = true;
                    for constraint in &candidate.contravariant {
                        match self.values.calls().accepts(constraint, &covariant) {
                            Known::Yes(false) => fits = false,
                            Known::Unknown => return Known::Unknown,
                            Known::Yes(true) => {}
                        }
                    }
                    Some(if fits { covariant } else { contravariant })
                }
                (covariant, contravariant) => covariant.or(contravariant),
            };
            arguments.push(selected);
        }
        Known::Yes(arguments)
    }

    pub(super) fn match_value(&mut self, at: Leaf, pattern: &Value, actual: &NodeValue, slots: &[Leaf]) -> Known<Match> {
        let pattern = self.recipe(at, pattern);
        let Known::Yes(accepted) = self.relate(&pattern, actual) else { return Known::Unknown };
        if self.positions.is_empty() { return Known::Yes(Match { accepted, arguments: Vec::new() }) }
        let Known::Yes(arguments) = self.arguments() else { return Known::Unknown };
        let locked: Vec<_> = self.candidates.iter().map(|candidate| candidate.locked.is_some()).collect();
        let Known::Yes(Some(arguments)) = self.values.calls().constrained_arguments(at, None, slots, arguments, &locked) else { return Known::Unknown };
        for (candidate, argument) in self.candidates.iter_mut().zip(&arguments) {
            candidate.locked = argument.clone();
        }
        self.validating = true;
        let Known::Yes(accepted) = self.relate(&pattern, actual) else { return Known::Unknown };
        Known::Yes(Match { accepted, arguments })
    }

    fn alternatives(&mut self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        if matches!(actual, NodeValue::Alternatives(arms) if arms.is_empty()) {
            return Known::Yes(true);
        }
        let NodeValue::Alternatives(expected) = expected else { return Known::Unknown };
        let NodeValue::Alternatives(actual) = actual else { return Known::Unknown };
        let mut unknown = false;
        for actual in actual {
            match self.choose(expected, |run, expected| run.alternative(expected, actual)) {
                Known::Yes(false) => return Known::Yes(false),
                Known::Unknown => unknown = true,
                Known::Yes(true) => {}
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(true) }
    }

    fn alternative(&mut self, expected: &[Leaf], actual: &[Leaf]) -> Known<bool> {
        let values = self.values;
        if !values.question.budget.step() { return Known::Unknown }
        if expected.len() == 1 {
            if let Some(slot) = self.slot(expected[0]) {
                return self.capture(slot, &NodeValue::Alternatives(vec![actual.to_vec()]), true);
            }
        }
        if actual.len() == 1 {
            if let Some(slot) = self.slot(actual[0]) {
                return self.capture(slot, &NodeValue::Alternatives(vec![expected.to_vec()]), false);
            }
        }
        if self.positions.is_empty() && expected == actual { return Known::Yes(true) }
        if let ([expected], [actual]) = (expected, actual) {
            match self.products(*expected, *actual) {
                Known::Yes(Some(answer)) => return Known::Yes(answer),
                Known::Yes(None) => {}
                Known::Unknown => return Known::Unknown,
            }
        }
        // Merged declarations make one row of several leaves: pair them up.
        if self.positions.is_empty() && !expected.is_empty() && expected.len() == actual.len()
            && expected.iter().zip(actual).all(|(expected, actual)| self.same_instantiation(*expected, *actual)) {
            return Known::Yes(true);
        }
        let target = universal_target(expected, values);
        if target == Known::Yes(true) { return Known::Yes(true) }
        let source = universal_source(actual, values);
        if source == Known::Yes(true) { return Known::Yes(true) }
        if target == Known::Unknown || source == Known::Unknown { return Known::Unknown }
        let expected_value = NodeValue::Alternatives(vec![expected.to_vec()]);
        let actual_value = NodeValue::Alternatives(vec![actual.to_vec()]);
        let mut described = false;
        let mut unknown = false;
        let categories = values.tables.exclusive.iter().fold(0, |all, group| all | group);
        for trait_id in 0..64 {
            let bit = 1 << trait_id;
            if categories & bit == 0 { continue }
            match values.always(&expected_value, bit) {
                Known::Yes(false) => {}
                Known::Unknown => unknown = true,
                Known::Yes(true) => {
                    described = true;
                    match values.always(&actual_value, bit) {
                        Known::Yes(false) => return Known::Yes(false),
                        Known::Unknown => unknown = true,
                        Known::Yes(true) => {}
                    }
                }
            }
        }
        for &leaf in expected {
            if matches!(leaf.target, Target::Traits(_)) { continue }
            match identity(leaf, actual, values) {
                Known::Yes(Some(false)) => return Known::Yes(false),
                Known::Yes(Some(true)) => described = true,
                Known::Yes(None) => {}
                Known::Unknown => unknown = true,
            }
            match values.member_shape(leaf) {
                Known::Yes(true) => {
                    described = true;
                    // A shape (even `{}`) needs a value that has members:
                    // a nullable one has none to read.
                    let nullable = values.tables.vocabulary.trait_bit("nullable");
                    if nullable != 0 && values.always(&actual_value, nullable) == Known::Yes(true) { return Known::Yes(false) }
                    match self.members(&NodeValue::leaf(leaf), &actual_value) {
                        Known::Yes(false) => return Known::Yes(false),
                        Known::Unknown => unknown = true,
                        Known::Yes(true) => {}
                    }
                }
                Known::Unknown => unknown = true,
                Known::Yes(false) => {}
            }
        }
        match super::signature_accepts::accepts(&expected_value, &actual_value, self) {
            Known::Yes(Some(false)) => return Known::Yes(false),
            Known::Yes(Some(true)) => described = true,
            Known::Yes(None) => {}
            Known::Unknown => unknown = true,
        }
        if unknown || !described { Known::Unknown } else { Known::Yes(true) }
    }

    /// Whether both are instances of the same generic declaration with
    /// arguments that accept each other both ways (`Map<string, string>`
    /// reached from two sites): the same type, whatever its members say.
    fn same_instantiation(&mut self, expected: Leaf, actual: Leaf) -> bool {
        let values = self.values;
        // A modified value (`Readonly<T>`) reports its source's carrier.
        if matches!(expected.target, Target::Modified(_)) || matches!(actual.target, Target::Modified(_)) { return false }
        let (Known::Yes(Some(left)), Known::Yes(Some(right))) = (values.carrier(expected), values.carrier(actual)) else { return false };
        if left.declaration != right.declaration || left.arguments.len() != right.arguments.len() { return false }
        left.arguments.iter().zip(&right.arguments).all(|(left, right)| left == right
            || self.relate(left, right) == Known::Yes(true) && self.relate(right, left) == Known::Yes(true))
    }

    /// Arrays and tuples relate by their elements: an array accepts any
    /// product whose every element its item accepts (an empty one too); a
    /// tuple accepts a tuple of its length, position by position. A readonly
    /// product never fits a mutable one. None when either isn't a product.
    fn products(&mut self, expected: Leaf, actual: Leaf) -> Known<Option<bool>> {
        let values = self.values;
        let (Known::Yes(Some(expected)), Known::Yes(Some(actual))) = (values.product_info(expected), values.product_info(actual)) else {
            return match (values.product_info(expected), values.product_info(actual)) {
                (Known::Unknown, _) | (_, Known::Unknown) => Known::Unknown,
                _ => Known::Yes(None),
            };
        };
        if actual.readonly && !expected.readonly { return Known::Yes(Some(false)) }
        let open = |element: &crate::grammar::names::question::ProductElement| element.repeated || element.values;
        let mut unknown = false;
        if !expected.elements.is_empty() && expected.elements.iter().all(open) {
            let item = values.combine(expected.elements.iter().map(|element| element.value.clone()), false);
            for element in &actual.elements {
                if !values.question.budget.step() { return Known::Unknown }
                match self.relate(&item, &element.value) {
                    Known::Yes(false) => return Known::Yes(Some(false)),
                    Known::Unknown => unknown = true,
                    Known::Yes(true) => {}
                }
            }
            return if unknown { Known::Unknown } else { Known::Yes(Some(true)) };
        }
        if expected.elements.iter().any(open) || actual.elements.iter().any(open) { return Known::Yes(None) }
        if actual.elements.len() > expected.elements.len() { return Known::Yes(Some(false)) }
        for (position, slot) in expected.elements.iter().enumerate() {
            if !values.question.budget.step() { return Known::Unknown }
            let Some(element) = actual.elements.get(position) else {
                if slot.optional { continue }
                return Known::Yes(Some(false));
            };
            if element.optional && !slot.optional { return Known::Yes(Some(false)) }
            match self.relate(&slot.value, &element.value) {
                Known::Yes(false) => return Known::Yes(Some(false)),
                Known::Unknown => unknown = true,
                Known::Yes(true) => {}
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(Some(true)) }
    }

    fn members(&mut self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        let Known::Yes(rows) = self.values.shape_members(expected) else { return Known::Unknown };
        let mut unknown = false;
        // An index signature (`Record<string, never>`) constrains every
        // named member the actual value has in its key domain.
        let indexers: Vec<_> = rows.iter().flatten().filter(|member| matches!(member.key, Key::Domain(_))).cloned().collect();
        if !indexers.is_empty() {
            let Known::Yes(actual_rows) = self.values.shape_members(actual) else { return Known::Unknown };
            for indexer in &indexers {
                for named in actual_rows.iter().flatten().filter(|named| !matches!(named.key, Key::Domain(_)) && indexer.key.may_alias(&named.key)) {
                    if !self.values.question.budget.step() { return Known::Unknown }
                    let value = self.member_value(named);
                    for source in &indexer.sources {
                        let expected = match &source.value {
                            Some(recipe) => self.recipe(source.site, recipe),
                            None => self.raw_leaf(source.site),
                        };
                        match self.relate(&expected, &value) {
                            Known::Yes(false) => return Known::Yes(false),
                            Known::Unknown => unknown = true,
                            Known::Yes(true) => {}
                        }
                    }
                }
            }
        }
        for member in rows.into_iter().flatten() {
            let Known::Yes(available) = self.values.keyed_members(actual, &member.key) else { return Known::Unknown };
            for available in available {
                let Some(available) = available else {
                    match member.required {
                        Known::Yes(false) => continue,
                        Known::Yes(true) if self.values.closed(actual) == Known::Yes(true) => return Known::Yes(false),
                        _ => { unknown = true; continue }
                    }
                };
                match (member.required, available.required) {
                    (Known::Yes(true), Known::Yes(false)) => return Known::Yes(false),
                    (Known::Unknown, _) | (Known::Yes(true), Known::Unknown) => unknown = true,
                    _ => {}
                }
                let actual = self.member_value(&available);
                for source in &member.sources {
                    let expected = match &source.value {
                        Some(recipe) => self.recipe(source.site, recipe),
                        None => self.raw_leaf(source.site),
                    };
                    match self.relate(&expected, &actual) {
                        Known::Yes(false) => return Known::Yes(false),
                        Known::Unknown => unknown = true,
                        Known::Yes(true) => {}
                    }
                }
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(true) }
    }

    fn member_value(&mut self, member: &crate::grammar::names::values::Member) -> NodeValue {
        let mut parts = Vec::new();
        for source in &member.sources {
            parts.push(match &source.value {
                Some(recipe) => self.recipe(source.site, recipe),
                None => self.raw_leaf(source.site),
            });
        }
        self.values.combine(parts, true)
    }

    pub(super) fn branch(&mut self, f: impl FnOnce(&mut Self) -> Known<bool>) -> Known<bool> {
        let original = self.candidates.clone();
        let result = f(self);
        if result != Known::Yes(true) { self.candidates = original }
        result
    }

    pub(super) fn choose<T>(&mut self, choices: &[T], mut test: impl FnMut(&mut Self, &T) -> Known<bool>) -> Known<bool> {
        let original = self.candidates.clone();
        let mut failed = None;
        let mut unknown = false;
        for choice in choices {
            self.candidates = original.clone();
            match test(self, choice) {
                Known::Yes(true) => return Known::Yes(true),
                Known::Yes(false) => { if failed.is_none() { failed = Some(self.candidates.clone()) } }
                Known::Unknown => unknown = true,
            }
        }
        self.candidates = failed.unwrap_or(original);
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }
}

fn universal_target(row: &[Leaf], values: &Values<'_>) -> Known<bool> {
    if row.is_empty() { return Known::Yes(false) }
    let mut unknown = false;
    for &leaf in row {
        match values.top(leaf) {
            Known::Yes(false) => return Known::Yes(false),
            Known::Unknown => unknown = true,
            Known::Yes(true) => {}
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(true) }
}

fn universal_source(row: &[Leaf], values: &Values<'_>) -> Known<bool> {
    match universal_target(row, values) {
        Known::Yes(true) => {}
        answer => return answer,
    }
    let mut unknown = false;
    for &leaf in row {
        match values.dynamic(leaf) {
            Known::Yes(true) => return Known::Yes(true),
            Known::Unknown => unknown = true,
            Known::Yes(false) => {}
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(false) }
}

fn identity(expected: Leaf, actual: &[Leaf], values: &Values<'_>) -> Known<Option<bool>> {
    let expected = match values.identity(expected) {
        Known::Yes(Some(identity)) => identity,
        Known::Yes(None) => return Known::Yes(None),
        Known::Unknown => return Known::Unknown,
    };
    let mut unknown = false;
    for &actual in actual {
        if !values.question.budget.step() { return Known::Unknown }
        match values.identity(actual) {
            Known::Yes(Some(actual)) if expected == actual => return Known::Yes(Some(true)),
            Known::Unknown => unknown = true,
            _ => {}
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(Some(false)) }
}

use super::*;

enum Work<'a> {
    Apply { at: Leaf, proof: &'a Value, negative: bool, input: NodeValue },
    Compose { at: Leaf, parts: Vec<&'a Value>, conjunction: bool, negative: bool, input: NodeValue },
    Then { at: Leaf, parts: Vec<&'a Value>, negative: bool },
    Union(usize),
}

impl Narrowing<'_> {
    pub(super) fn apply(&self, at: Leaf, proof: &Value, subject: &Place, base: NodeValue) -> NodeValue {
        let mut pending = vec![Work::Apply { at, proof, negative: false, input: base }];
        let mut results = Vec::new();
        let values = self.semantic.values();
        while let Some(work) = pending.pop() {
            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
            match work {
                Work::Apply { at, proof, negative, input } => match proof {
                    Value::Not(proof) => pending.push(Work::Apply { at, proof, negative: !negative, input }),
                    Value::Union(parts) | Value::Intersection(parts) => pending.push(Work::Compose {
                        at, parts: parts.iter().collect(), conjunction: matches!(proof, Value::Intersection(_)) != negative,
                        negative, input,
                    }),
                    Value::Path(path) => {
                        let Known::Yes(sites) = values.follow(at, path) else {
                            results.push(input);
                            continue;
                        };
                        let [site] = sites.as_slice() else {
                            results.push(input);
                            continue;
                        };
                        let Target::Node(node) = site.target else {
                            results.push(input);
                            continue;
                        };
                        let Some(kind) = self.semantic.world.tree(node.file)
                            .and_then(|tree| tree.nodes.get(node.node as usize))
                            .and_then(|data| self.semantic.tables.kinds.get(data.matcher as usize)) else {
                            results.push(input);
                            continue;
                        };
                        let Some(truthy) = self.semantic.tables.vocabulary.effect("truthy") else {
                            results.push(input);
                            continue;
                        };
                        let mut parts = Vec::new();
                        let mut opaque = false;
                        for item in &kind.unwraps {
                            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
                            if item.effect != Some(truthy) { continue }
                            match item.when.map_or(Known::Yes(true), |condition| self.semantic.world.condition(*site, condition, self.semantic)) {
                                Known::Yes(false) => continue,
                                Known::Unknown => { opaque = true; continue }
                                Known::Yes(true) => {}
                            }
                            match &item.value {
                                Some(value) => parts.push(value),
                                None => opaque = true,
                            }
                        }
                        if negative && opaque {
                            results.push(input);
                        } else {
                            pending.push(Work::Compose { at: *site, parts, conjunction: !negative, negative, input });
                        }
                    }
                    Value::Excludes { traits, from } => results.push(self.excludes(at, from.as_deref(), subject, input, *traits, negative)),
                    Value::Includes { value, from } => results.push(self.includes(at, from.as_deref(), subject, input, value, negative)),
                    _ => results.push(input),
                },
                Work::Compose { at, mut parts, conjunction, negative, input } => {
                    if parts.is_empty() {
                        results.push(input);
                    } else if conjunction {
                        parts.reverse();
                        let proof = parts.pop().expect("the proof list is nonempty");
                        pending.push(Work::Then { at, parts, negative });
                        pending.push(Work::Apply { at, proof, negative, input });
                    } else {
                        pending.push(Work::Union(parts.len()));
                        for proof in parts.into_iter().rev() {
                            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
                            pending.push(Work::Apply { at, proof, negative, input: input.clone() });
                        }
                    }
                }
                Work::Then { at, mut parts, negative } => {
                    if let Some(proof) = parts.pop() {
                        let input = results.pop().expect("the preceding proof produced a value");
                        pending.push(Work::Then { at, parts, negative });
                        pending.push(Work::Apply { at, proof, negative, input });
                    }
                }
                Work::Union(count) => {
                    let mut alternatives = Vec::new();
                    let mut unknown = false;
                    for _ in 0..count {
                        match results.pop().expect("each alternative proof produced a value") {
                            NodeValue::Alternatives(parts) => alternatives.extend(parts),
                            NodeValue::Unknown => unknown = true,
                        }
                    }
                    results.push(if unknown { NodeValue::Unknown } else { NodeValue::Alternatives(alternatives) });
                }
            }
        }
        results.pop().expect("the proof produced a value")
    }

    fn projection(&self, at: Leaf, from: Option<&Value>, subject: &Place) -> Known<Vec<Key>> {
        let values = self.semantic.values();
        let site = match from {
            None => at,
            Some(value) => {
                let Known::Yes(sites) = values.sites(at, value) else { return Known::Unknown };
                let [site] = sites.as_slice() else { return Known::Unknown };
                *site
            }
        };
        let Known::Yes(tested) = self.place(site) else { return Known::Unknown };
        if tested.root != subject.root || !tested.path.starts_with(&subject.path) { return Known::Unknown }
        Known::Yes(tested.path[subject.path.len()..].to_vec())
    }

    fn projected(&self, mut value: NodeValue, path: &[Key]) -> NodeValue {
        for key in path {
            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
            value = self.semantic.values().member(&value, key);
        }
        value
    }

    fn excludes(&self, at: Leaf, from: Option<&Value>, subject: &Place, base: NodeValue, traits: u64, negative: bool) -> NodeValue {
        let Known::Yes(path) = self.projection(at, from, subject) else { return base };
        let values = self.semantic.values();
        let NodeValue::Alternatives(alternatives) = base else { return NodeValue::Unknown };
        let mut kept = Vec::new();
        for alternative in alternatives {
            let projected = self.projected(NodeValue::Alternatives(vec![alternative.clone()]), &path);
            let mut keep = !negative;
            for trait_id in 0..64 {
                if traits & (1 << trait_id) == 0 { continue }
                let present = if negative {
                    values.could(&projected, 1 << trait_id) != Known::Yes(false)
                } else {
                    values.always(&projected, 1 << trait_id) == Known::Yes(true)
                };
                if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
                if present {
                    keep = negative;
                    break;
                }
            }
            if keep { kept.push(alternative) }
        }
        NodeValue::Alternatives(kept)
    }

    fn includes(&self, at: Leaf, from: Option<&Value>, subject: &Place, base: NodeValue, value: &Value, negative: bool) -> NodeValue {
        let Known::Yes(path) = self.projection(at, from, subject) else { return base };
        let expected = self.semantic.values().evaluate(at, value);
        if matches!(expected, NodeValue::Unknown) { return base }
        let NodeValue::Alternatives(alternatives) = base else {
            return if !negative && path.is_empty() { expected } else { NodeValue::Unknown };
        };
        let mut kept = Vec::new();
        for alternative in alternatives {
            let actual = self.projected(NodeValue::Alternatives(vec![alternative.clone()]), &path);
            let included = self.included(&expected, &actual);
            if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
            if negative {
                if !included { kept.push(alternative) }
            } else if included {
                kept.push(alternative);
            } else if !self.proven_disjoint(&expected, &actual) {
                if path.is_empty() && self.semantic.calls().accepts(&actual, &expected) == Known::Yes(true) {
                    let NodeValue::Alternatives(refined) = &expected else { unreachable!() };
                    kept.extend(refined.iter().cloned());
                } else {
                    kept.push(alternative);
                }
            }
            if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
        }
        NodeValue::Alternatives(kept)
    }

    fn literal(&self, alternative: &[Leaf]) -> Option<crate::expr::Value<'static>> {
        for &leaf in alternative {
            if !self.semantic.question.budget.step() { return None }
            if let Known::Yes(Some(super::super::values::Identity::Value(value))) = self.semantic.values().identity(leaf) {
                if matches!(value, crate::expr::Value::Null | crate::expr::Value::Bool(_) | crate::expr::Value::Num(_) | crate::expr::Value::Str(_)) {
                    return Some(value);
                }
            }
        }
        None
    }

    fn included(&self, expected: &NodeValue, actual: &NodeValue) -> bool {
        if self.semantic.calls().accepts(expected, actual) == Known::Yes(true) { return true }
        let (NodeValue::Alternatives(expected), NodeValue::Alternatives(actual)) = (expected, actual) else { return false };
        actual.iter().all(|actual| {
            let Some(literal) = self.literal(actual) else { return false };
            expected.iter().any(|expected| expected.len() == 1 && self.literal(expected).is_some_and(|value| value == literal))
        })
    }

    fn proven_disjoint(&self, expected: &NodeValue, actual: &NodeValue) -> bool {
        let (NodeValue::Alternatives(expected), NodeValue::Alternatives(actual)) = (expected, actual) else { return false };
        let values = self.semantic.values();
        actual.iter().all(|actual| expected.iter().all(|expected| {
            if let (Some(actual), Some(expected)) = (self.literal(actual), self.literal(expected)) {
                if actual != expected { return true }
            }
            let actual = NodeValue::Alternatives(vec![actual.clone()]);
            let expected = NodeValue::Alternatives(vec![expected.clone()]);
            for &group in &self.semantic.tables.exclusive {
                for actual_id in 0..64 {
                    let actual_bit = 1 << actual_id;
                    if group & actual_bit == 0 || values.always(&actual, actual_bit) != Known::Yes(true) { continue }
                    for expected_id in 0..64 {
                        let expected_bit = 1 << expected_id;
                        if group & expected_bit != 0 && expected_bit != actual_bit
                            && values.always(&expected, expected_bit) == Known::Yes(true) { return true }
                    }
                }
            }
            false
        }))
    }
}

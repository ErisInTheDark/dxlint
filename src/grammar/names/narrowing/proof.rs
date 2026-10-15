use super::*;

struct ProofContext<'a> {
    subject: &'a Place,
    region: Option<&'a Region<'a>>,
}

enum Work<'a> {
    Apply { at: Leaf, proof: &'a Value, negative: bool, input: NodeValue },
    Test { at: Leaf, negative: bool, input: NodeValue },
    Compose { at: Leaf, parts: Vec<&'a Value>, conjunction: bool, negative: bool, input: NodeValue },
    Then { at: Leaf, parts: Vec<&'a Value>, negative: bool },
    Choices { sites: Vec<Leaf>, negative: bool, input: NodeValue },
    AfterCall(Vec<Leaf>),
    Union(usize),
}

impl Narrowing<'_> {
    #[cfg(test)]
    pub(super) fn apply(&self, at: Leaf, proof: &Value, subject: &Place, base: NodeValue) -> NodeValue {
        self.apply_in(at, proof, subject, base, None)
    }

    pub(super) fn apply_in(&self, at: Leaf, proof: &Value, subject: &Place, base: NodeValue, region: Option<&Region<'_>>) -> NodeValue {
        let Known::Yes(at) = self.semantic.leaf(at) else { return NodeValue::Unknown };
        let context = ProofContext { subject, region };
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
                        pending.push(Work::Test { at: *site, negative, input });
                    }
                    Value::Excludes { traits, from } => results.push(self.excludes(at, from.as_deref(), &context, input, *traits, negative)),
                    Value::Includes { value, from } => results.push(self.includes(at, from.as_deref(), &context, input, value, negative)),
                    Value::Has { key, from } => results.push(self.has(at, from.as_deref(), &context, input, key, negative)),
                    _ => results.push(input),
                },
                Work::Test { at, negative, input } => {
                    let Known::Yes(at) = self.semantic.leaf(at) else {
                        results.push(input);
                        continue;
                    };
                    let Target::Node(node) = at.target else {
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
                        match item.when.map_or(Known::Yes(true), |condition| self.semantic.world.condition(at, condition, self.semantic)) {
                            Known::Yes(false) => continue,
                            Known::Unknown => { opaque = true; continue }
                            Known::Yes(true) => {}
                        }
                        match &item.value {
                            Some(value) if self.relevant_atom(at, value, &context) => parts.push(value),
                            Some(_) => {}
                            None => opaque = true,
                        }
                    }
                    let calls = if kind.calls.invokes.is_empty() { Known::Yes(None) } else { self.predicate_sites(at) };
                    let sites = match calls {
                        Known::Yes(sites) => sites,
                        Known::Unknown => { opaque = true; None }
                    };
                    if negative && opaque {
                        results.push(input);
                    } else if let Some(sites) = sites {
                        if negative {
                            if parts.is_empty() {
                                pending.push(Work::Choices { sites, negative, input });
                            } else {
                                pending.push(Work::Union(2));
                                pending.push(Work::Choices { sites, negative, input: input.clone() });
                                pending.push(Work::Compose { at, parts, conjunction: false, negative, input });
                            }
                        } else {
                            pending.push(Work::AfterCall(sites));
                            pending.push(Work::Compose { at, parts, conjunction: true, negative, input });
                        }
                    } else {
                        pending.push(Work::Compose { at, parts, conjunction: !negative, negative, input });
                    }
                },
                Work::Choices { sites, negative, input } => {
                    pending.push(Work::Union(sites.len()));
                    for at in sites.into_iter().rev() {
                        if !self.semantic.question.budget.step() { return NodeValue::Unknown }
                        pending.push(Work::Test { at, negative, input: input.clone() });
                    }
                }
                Work::AfterCall(sites) => {
                    let input = results.pop().expect("the caller proof produced a value");
                    pending.push(Work::Choices { sites, negative: false, input });
                }
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

    fn predicate_sites(&self, at: Leaf) -> Known<Option<Vec<Leaf>>> {
        let selection = match self.semantic.calls().selection(at) {
            Known::Yes(Some(selection)) if !selection.uncertain => selection,
            Known::Yes(None) => return Known::Yes(None),
            _ => return Known::Unknown,
        };
        let mut sites = Vec::new();
        for chosen in selection.choices {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let header = chosen.header;
            let Known::Yes(results) = self.semantic.calls().header_result_sites(header) else { return Known::Unknown };
            if results.is_empty() { return Known::Unknown }
            sites.extend(results);
        }
        if sites.is_empty() { Known::Unknown } else { Known::Yes(Some(sites)) }
    }

    fn relevant_atom(&self, at: Leaf, mut proof: &Value, context: &ProofContext<'_>) -> bool {
        while let Value::Not(inner) = proof { proof = inner; }
        let from = match proof {
            Value::Excludes { from, .. } | Value::Includes { from, .. } | Value::Has { from, .. } => from.as_deref(),
            _ => return true,
        };
        matches!(self.projection(at, from, context, None), Known::Yes(_))
    }

    fn projection(&self, at: Leaf, from: Option<&Value>, context: &ProofContext<'_>, key: Option<&Key>) -> Known<Vec<Key>> {
        let values = self.semantic.values();
        let site = match from {
            None => at,
            Some(value) => {
                let Known::Yes(sites) = values.sites(at, value) else { return Known::Unknown };
                let [site] = sites.as_slice() else { return Known::Unknown };
                *site
            }
        };
        let Known::Yes(mut tested) = self.place(site) else { return Known::Unknown };
        if tested.root != context.subject.root || !tested.path.starts_with(&context.subject.path) { return Known::Unknown }
        let path = tested.path[context.subject.path.len()..].to_vec();
        if let Some(key) = key { tested.path.push(key.clone()) }
        if context.region.is_some_and(|region| region.valid(self, &tested) != Known::Yes(true)) { return Known::Unknown }
        Known::Yes(path)
    }

    fn projected(&self, mut value: NodeValue, path: &[Key]) -> NodeValue {
        for key in path {
            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
            value = self.semantic.values().member(&value, key);
        }
        value
    }

    /// `value` is only top values (`any`, `unknown`), which no comparison
    /// proves inside or outside a set.
    fn wholly_top(&self, value: &NodeValue) -> bool {
        let values = self.semantic.values();
        matches!(value, NodeValue::Alternatives(rows) if !rows.is_empty() && rows.iter().all(|row| values.wholly_top(row)))
    }

    fn excludes(&self, at: Leaf, from: Option<&Value>, context: &ProofContext<'_>, base: NodeValue, traits: u64, negative: bool) -> NodeValue {
        let Known::Yes(path) = self.projection(at, from, context, None) else { return base };
        let values = self.semantic.values();
        let NodeValue::Alternatives(alternatives) = base else { return NodeValue::Unknown };
        let mut kept = Vec::new();
        for alternative in alternatives {
            let projected = self.projected(NodeValue::Alternatives(vec![alternative.clone()]), &path);
            if !negative && self.wholly_top(&projected) {
                kept.push(alternative);
                continue;
            }
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

    fn includes(&self, at: Leaf, from: Option<&Value>, context: &ProofContext<'_>, base: NodeValue, value: &Value, negative: bool) -> NodeValue {
        let Known::Yes(path) = self.projection(at, from, context, None) else { return base };
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
                if !included || self.wholly_top(&actual) { kept.push(alternative) }
            } else if included {
                kept.push(alternative);
            } else if !self.proven_disjoint(&expected, &actual) {
                if path.is_empty() {
                    let NodeValue::Alternatives(refined) = &expected else { unreachable!() };
                    for refinement in refined {
                        if !self.semantic.question.budget.step() { return NodeValue::Unknown }
                        let narrowed = NodeValue::Alternatives(vec![refinement.clone()]);
                        if self.proven_disjoint(&narrowed, &actual) { continue }
                        let mut intersection = alternative.clone();
                        intersection.extend(refinement.iter().copied());
                        kept.push(intersection);
                    }
                } else {
                    kept.push(alternative);
                }
            }
            if self.semantic.question.budget.exhausted() { return NodeValue::Unknown }
        }
        NodeValue::Alternatives(kept)
    }

    fn has(&self, at: Leaf, from: Option<&Value>, context: &ProofContext<'_>, base: NodeValue, key: &Value, negative: bool) -> NodeValue {
        let values = self.semantic.values();
        let Known::Yes(key) = values.key(at, key) else { return base };
        if !negative && !matches!(key, Key::Text(_)) { return base }
        let Known::Yes(path) = self.projection(at, from, context, Some(&key)) else { return base };
        let NodeValue::Alternatives(alternatives) = base else { return NodeValue::Unknown };
        let mut kept = Vec::new();
        for alternative in alternatives {
            if !self.semantic.question.budget.step() { return NodeValue::Unknown }
            let projected = self.projected(NodeValue::Alternatives(vec![alternative.clone()]), &path);
            let excluded = match values.members(&projected) {
                Known::Yes(rows) if negative => !rows.is_empty() && rows.iter().all(|row| row.iter()
                    .any(|member| member.key == key && member.required == Known::Yes(true))),
                Known::Yes(rows) => values.closed(&projected) == Known::Yes(true)
                    && rows.iter().all(|row| row.iter().all(|member| member.key != key)),
                _ => false,
            };
            if !excluded { kept.push(alternative) }
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

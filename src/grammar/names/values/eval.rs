//! Iterative value expansion: aliases never consume the Rust call stack.

use super::{Known, Leaf, NodeValue, Target, Value, Values};

struct Answer {
    exists: bool,
    value: NodeValue,
}

impl Answer {
    fn unknown() -> Self { Self { exists: true, value: NodeValue::Unknown } }
    fn absent() -> Self { Self { exists: false, value: NodeValue::Unknown } }
}

enum Task<'a> {
    Leaf(Leaf),
    Save(Leaf),
    Profile(crate::engine::ValueEvaluation<'a>),
    Expression(Leaf, &'a Value),
    Fallback(Leaf, &'a Value),
    Combine { count: usize, intersection: bool },
}

struct Run<'a, 'world> {
    values: &'a Values<'world>,
    tasks: Vec<Task<'a>>,
    answers: Vec<Answer>,
}

pub(super) fn values(values: &Values<'_>, leaf: Leaf) -> NodeValue {
    Run { values, tasks: vec![Task::Leaf(leaf)], answers: Vec::new() }.run()
}

pub(super) fn evaluate(values: &Values<'_>, at: Leaf, expression: &Value) -> NodeValue {
    Run { values, tasks: vec![Task::Expression(at, expression)], answers: Vec::new() }.run()
}

pub(super) fn evaluate_optional(values: &Values<'_>, at: Leaf, expression: &Value) -> Known<Option<NodeValue>> {
    let answer = Run { values, tasks: vec![Task::Expression(at, expression)], answers: Vec::new() }.run_answer();
    match (answer.exists, answer.value) {
        (false, _) => Known::Yes(None),
        (true, NodeValue::Unknown) => Known::Unknown,
        (true, value) => Known::Yes(Some(value)),
    }
}

pub(super) fn sites(values: &Values<'_>, at: Leaf, mut expression: &Value) -> Known<Vec<Leaf>> {
    let mut fallbacks = Vec::new();
    loop {
        if !values.question.budget.step() { return Known::Unknown }
        match expression {
            // Filtering a value narrows what it may hold, never where it lives.
            Value::Excludes { from: Some(inner), .. } => expression = inner,
            Value::Current | Value::Excludes { from: None, .. } => {
                return match values.semantic.leaf(at) {
                    Known::Yes(at) => Known::Yes(vec![at]),
                    Known::Unknown => Known::Unknown,
                };
            }
            Value::Either(left, right) => {
                fallbacks.push(&**right);
                expression = left;
            }
            Value::Path(path) => {
                let answer = values.follow(at, path);
                if matches!(&answer, Known::Yes(leaves) if leaves.is_empty()) {
                    if let Some(right) = fallbacks.pop() {
                        expression = right;
                        continue;
                    }
                }
                return answer;
            }
            Value::Spread { path, .. } => return values.follow(at, path),
            Value::Scope { classes, from } => return values.scopes(at, from, *classes),
            Value::From(base, key, classes) if *classes == 0 => {
                let base = Run { values, tasks: vec![Task::Expression(at, base)], answers: Vec::new() }.run_answer();
                let answer = if !base.exists {
                    Known::Yes(Vec::new())
                } else {
                    match values.key(at, key) {
                        Known::Yes(key) => values.member_sites(&base.value, &key),
                        Known::Unknown => Known::Unknown,
                    }
                };
                if matches!(&answer, Known::Yes(sites) if sites.is_empty()) {
                    if let Some(right) = fallbacks.pop() {
                        expression = right;
                        continue;
                    }
                }
                return answer;
            }
            _ => return Known::Unknown,
        }
    }
}

impl<'a, 'world> Run<'a, 'world> {
    fn run(self) -> NodeValue {
        self.run_answer().value
    }

    fn run_answer(mut self) -> Answer {
        while let Some(task) = self.tasks.pop() {
            if !matches!(&task, Task::Profile(_)) && !self.values.question.budget.step() {
                return Answer::unknown();
            }
            match task {
                Task::Profile(timer) => drop(timer),
                Task::Leaf(leaf) => self.leaf(leaf),
                Task::Save(leaf) => {
                    if let Some(answer) = self.answers.last_mut() {
                        answer.value = match self.values.question.budget.nest() {
                            Some(_nested) => self.values.normalize(self.values.semantic.narrowing().refine(leaf, answer.value.clone())),
                            None => NodeValue::Unknown,
                        };
                        let value = answer.value.clone();
                        self.settle(leaf, value);
                    }
                }
                Task::Expression(at, expression) => self.expression(at, expression),
                Task::Fallback(at, right) => {
                    if self.answers.last().is_some_and(|answer| !answer.exists) {
                        self.answers.pop();
                        self.tasks.push(Task::Expression(at, right));
                    }
                }
                Task::Combine { count, intersection } => self.combine(count, intersection),
            }
        }
        self.answers.pop().unwrap_or_else(Answer::unknown)
    }

    fn leaf(&mut self, leaf: Leaf) {
        if let Some(profile) = self.values.world.profile() {
            if let Target::Node(node) = leaf.target {
                if let Some(timer) = self.values.world.tree(node.file)
                    .and_then(|tree| tree.nodes.get(node.node as usize))
                    .and_then(|node| profile.begin_value(node.matcher as usize)) {
                    self.tasks.push(Task::Profile(timer));
                }
            }
        }
        let Known::Yes(leaf) = self.values.semantic.leaf(leaf) else {
            self.answers.push(Answer::unknown());
            return;
        };
        let state = &self.values.semantic.values;
        if let Some(value) = state.values.get(&leaf) {
            if value == NodeValue::Unknown && state.evaluating.borrow().iter().any(|&(active, _)| active == leaf) {
                state.cycle_reads.set(state.cycle_reads.get() + 1);
            }
            self.answers.push(Answer { exists: true, value });
            return;
        }
        state.values.insert(self.values.semantic.epoch, leaf, NodeValue::Unknown);
        state.evaluating.borrow_mut().push((leaf, state.cycle_reads.get()));
        let Target::Node(_) = leaf.target else {
            self.answers.push(Answer { exists: true, value: NodeValue::leaf(leaf) });
            self.tasks.push(Task::Save(leaf));
            return;
        };
        match self.values.unwrap_recipe(leaf) {
            Known::Unknown => {
                self.settle(leaf, NodeValue::Unknown);
                self.answers.push(Answer::unknown());
                return;
            }
            Known::Yes(Some(expression)) => {
                self.tasks.push(Task::Save(leaf));
                self.tasks.push(Task::Expression(leaf, expression));
                return;
            }
            Known::Yes(None) => {}
        }
        self.answers.push(Answer { exists: true, value: NodeValue::leaf(leaf) });
        self.tasks.push(Task::Save(leaf));
    }

    /// Ends `leaf`'s evaluation with `value`: kept, unless it is unknown
    /// after reading a leaf still being evaluated. That unknown may only be
    /// the unfinished cycle, so a later ask evaluates it afresh.
    fn settle(&self, leaf: Leaf, value: NodeValue) {
        let state = &self.values.semantic.values;
        let started = {
            let mut evaluating = state.evaluating.borrow_mut();
            match evaluating.iter().rposition(|&(active, _)| active == leaf) {
                Some(index) => {
                    let started = evaluating[index].1;
                    evaluating.truncate(index);
                    Some(started)
                }
                None => None,
            }
        };
        let cyclic = started.is_none_or(|started| state.cycle_reads.get() > started);
        if value == NodeValue::Unknown && cyclic {
            state.values.remove(self.values.semantic.epoch, &leaf);
        } else {
            state.values.insert(self.values.semantic.epoch, leaf, value);
        }
    }

    fn expression(&mut self, at: Leaf, expression: &'a Value) {
        let Known::Yes(at) = self.values.semantic.leaf(at) else {
            self.answers.push(Answer::unknown());
            return;
        };
        match expression {
            Value::Current => {
                self.answers.push(Answer { exists: true, value: NodeValue::leaf(at) });
            }
            Value::Unknown => self.answers.push(Answer::unknown()),
            Value::Captured(value) => {
                self.answers.push(Answer { exists: true, value: (**value).clone() });
            }
            Value::Traits(traits) => {
                let leaf = Leaf { target: Target::Traits(*traits), frame: at.frame };
                self.answers.push(Answer { exists: true, value: self.values.normalize(NodeValue::leaf(leaf)) });
            }
            Value::Returned { default } => match returned(self.values, at, Some(default)) {
                Known::Yes(Some(value)) => self.answers.push(Answer { exists: true, value }),
                Known::Yes(None) => self.answers.push(Answer::absent()),
                Known::Unknown => self.answers.push(Answer::unknown()),
            },
            Value::Path(path) => self.path(at, path, false, false),
            Value::Spread { intersection, path, value: None } => self.path(at, path, *intersection, true),
            Value::Spread { intersection, path, value: Some(value) } => {
                let value = match self.values.arguments_using(at, path, Some(value)) {
                    Known::Yes(values) => combine(self.values, values, *intersection),
                    Known::Unknown => NodeValue::Unknown,
                };
                self.answers.push(Answer { exists: true, value });
            }
            Value::Either(left, right) => {
                self.tasks.push(Task::Fallback(at, right));
                self.tasks.push(Task::Expression(at, left));
            }
            Value::Union(parts) | Value::Intersection(parts) => {
                self.tasks.push(Task::Combine { count: parts.len(), intersection: matches!(expression, Value::Intersection(_)) });
                self.tasks.extend(parts.iter().rev().map(|part| Task::Expression(at, part)));
            }
            Value::Scope { classes, from } => match self.values.scopes(at, from, *classes) {
                Known::Unknown => self.answers.push(Answer::unknown()),
                Known::Yes(leaves) if leaves.is_empty() => self.answers.push(Answer::absent()),
                Known::Yes(leaves) => {
                    self.tasks.push(Task::Combine { count: leaves.len(), intersection: false });
                    self.tasks.extend(leaves.into_iter().rev().map(Task::Leaf));
                }
            },
            Value::From(base, key, classes) if *classes == 0 => {
                let base = Run { values: self.values, tasks: vec![Task::Expression(at, base)], answers: Vec::new() }.run_answer();
                if !base.exists {
                    self.answers.push(base);
                    return;
                }
                let value = match self.values.key(at, key) {
                    Known::Yes(key) => self.values.member(&base.value, &key),
                    Known::Unknown => NodeValue::Unknown,
                };
                self.answers.push(Answer { exists: true, value });
            }
            Value::Instance { name, classes, arguments } => {
                self.answers.push(Answer { exists: true, value: self.values.instance(at, name, *classes, arguments) });
            }
            Value::Select(selection) => {
                self.answers.push(Answer { exists: true, value: self.values.select(at, selection) });
            }
            Value::Product(recipe) => {
                let value = self.values.produce_product(at, recipe, |site| {
                    recipe.value.as_ref().map_or_else(|| self.values.values(site), |value| self.values.evaluate(site, value))
                });
                self.answers.push(Answer { exists: true, value });
            }
            Value::Excludes { traits, from: Some(from) } => {
                let mut answer = Run { values: self.values, tasks: vec![Task::Expression(at, from)], answers: Vec::new() }.run_answer();
                if answer.exists { answer.value = self.excludes(answer.value, *traits) }
                self.answers.push(answer);
            }
            Value::Modified { source, required, readonly, flags } => {
                let mut answer = Run { values: self.values, tasks: vec![Task::Expression(at, source)], answers: Vec::new() }.run_answer();
                if answer.exists { answer.value = self.values.modify(answer.value, *required, *readonly, flags) }
                self.answers.push(answer);
            }
            Value::Text(_) | Value::From(..) | Value::Not(_) | Value::Excludes { from: None, .. }
                | Value::Includes { .. } | Value::Has { .. } | Value::Ordinal(_) => self.answers.push(Answer::unknown()),
        }
    }

    /// The leaves `path` leads to, and whether they are one merged value:
    /// the declarations of one entity (overloads, interface and namespace
    /// merging) combine as one value, never as runtime alternatives.
    fn follow_merged(&self, at: Leaf, path: &[u16]) -> Known<(Vec<Leaf>, bool)> {
        let merged = match path.split_last() {
            Some((&super::super::DECLARATION, prefix)) => match self.values.follow(at, prefix) {
                Known::Yes(owners) => owners.len() == 1,
                Known::Unknown => return Known::Unknown,
            },
            _ => false,
        };
        // One walk from `at`: the declarations keep the reference they were
        // reached through (its type arguments).
        match self.values.follow(at, path) {
            Known::Yes(leaves) => Known::Yes((leaves, merged)),
            Known::Unknown => Known::Unknown,
        }
    }

    fn path(&mut self, at: Leaf, path: &[u16], intersection: bool, spread: bool) {
        if path == [super::super::PRODUCT_ITEMS] {
            let value = match self.values.product_items(at) {
                Known::Yes(items) => combine(self.values, items, intersection),
                Known::Unknown => NodeValue::Unknown,
            };
            self.answers.push(Answer { exists: true, value });
            return;
        }
        if let Some((&query, prefix)) = path.split_last().filter(|(last, _)| super::super::query_step(**last)) {
            self.query_path(at, prefix, query, intersection, spread);
            return;
        }
        match self.follow_merged(at, path) {
            Known::Unknown => self.answers.push(Answer::unknown()),
            Known::Yes((leaves, _)) if leaves.is_empty() && !spread => self.answers.push(Answer::absent()),
            Known::Yes((leaves, merged)) => {
                self.tasks.push(Task::Combine { count: leaves.len(), intersection: intersection || merged });
                self.tasks.extend(leaves.into_iter().rev().map(Task::Leaf));
            }
        }
    }

    fn query_path(&mut self, at: Leaf, prefix: &[u16], query: u16, intersection: bool, spread: bool) {
        let Some(_nested) = self.values.question.budget.nest() else {
            self.answers.push(Answer::unknown());
            return;
        };
        if prefix.iter().any(|&label| super::super::query_step(label)) {
            self.query_value_path(at, prefix, query, spread);
            return;
        }
        let Known::Yes((leaves, merged)) = self.follow_merged(at, prefix) else {
            self.answers.push(Answer::unknown());
            return;
        };
        let intersection = intersection || merged;
        let mut answers = Vec::new();
        let mut absent = 0;
        for leaf in leaves {
            if !self.values.question.budget.step() {
                self.answers.push(Answer::unknown());
                return;
            }
            let answer = self.query(leaf, query);
            match answer {
                Known::Yes(Some(value)) => answers.push(Answer { exists: true, value }),
                Known::Yes(None) => absent += 1,
                Known::Unknown => {
                    self.answers.push(Answer::unknown());
                    return;
                }
            }
        }
        if answers.is_empty() {
            self.answers.push(if spread { Answer { exists: true, value: NodeValue::never() } } else { Answer::absent() });
        } else if absent != 0 {
            self.answers.push(Answer::unknown());
        } else {
            let count = answers.len();
            self.answers.extend(answers);
            self.combine(count, intersection);
        }
    }

    fn query(&self, leaf: Leaf, query: u16) -> Known<Option<NodeValue>> {
        let calls = self.values.semantic.calls();
        match query {
            super::super::RESULT => calls.result(leaf),
            super::super::EXPECTED => calls.expected(leaf),
            super::super::INFERRED => calls.inferred(leaf),
            super::super::RETURNED => returned(self.values, leaf, None),
            super::super::WIDENED => Known::Yes(Some(widened(self.values, leaf))),
            super::super::RECEIVER => receiver(self.values, leaf),
            _ => Known::Unknown,
        }
    }

    fn query_value_path(&mut self, at: Leaf, prefix: &[u16], query: u16, spread: bool) {
        let expression = Value::Path(prefix.to_vec());
        let answer = Run { values: self.values, tasks: vec![Task::Expression(at, &expression)], answers: Vec::new() }.run_answer();
        if !answer.exists {
            self.answers.push(answer);
            return;
        }
        let NodeValue::Alternatives(alternatives) = answer.value else {
            self.answers.push(Answer::unknown());
            return;
        };
        if alternatives.is_empty() {
            self.answers.push(Answer { exists: true, value: NodeValue::never() });
            return;
        }
        let mut results = Vec::new();
        let mut absent = 0;
        for alternative in alternatives {
            let mut conjunction = Vec::new();
            let mut missing = 0;
            for leaf in alternative {
                if !self.values.question.budget.step() {
                    self.answers.push(Answer::unknown());
                    return;
                }
                match self.query(leaf, query) {
                    Known::Yes(Some(value)) => conjunction.push(value),
                    Known::Yes(None) => missing += 1,
                    Known::Unknown => {
                        self.answers.push(Answer::unknown());
                        return;
                    }
                }
            }
            if conjunction.is_empty() {
                absent += 1;
            } else if missing != 0 {
                self.answers.push(Answer::unknown());
                return;
            } else {
                results.push(combine(self.values, conjunction, true));
            }
        }
        let answer = if results.is_empty() {
            if spread { Answer { exists: true, value: NodeValue::never() } } else { Answer::absent() }
        } else if absent != 0 {
            Answer::unknown()
        } else {
            Answer { exists: true, value: combine(self.values, results, false) }
        };
        self.answers.push(answer);
    }

    /// The alternatives having none of `traits` (each trait excludes on its own).
    /// The alternatives not wholly of these traits: an alternative that
    /// only may have one (`any`, `unknown`, `string` for `falsy`) stays.
    fn excludes(&self, value: NodeValue, traits: u64) -> NodeValue {
        let NodeValue::Alternatives(alternatives) = value else { return NodeValue::Unknown };
        let mut kept = Vec::new();
        for alternative in alternatives {
            let row = NodeValue::Alternatives(vec![alternative]);
            let mut excluded = Known::Yes(false);
            for bit in (0..u64::BITS).map(|i| 1u64 << i).filter(|bit| traits & bit != 0) {
                if !self.values.question.budget.step() { return NodeValue::Unknown }
                match self.values.always(&row, bit) {
                    Known::Yes(true) => {
                        excluded = Known::Yes(true);
                        break;
                    }
                    Known::Yes(false) => {}
                    Known::Unknown => excluded = Known::Unknown,
                }
            }
            match excluded {
                Known::Yes(true) => {}
                Known::Yes(false) => {
                    let NodeValue::Alternatives(mut row) = row else { unreachable!() };
                    kept.append(&mut row);
                }
                Known::Unknown => return NodeValue::Unknown,
            }
        }
        NodeValue::Alternatives(kept)
    }

    fn combine(&mut self, count: usize, intersection: bool) {
        let start = self.answers.len() - count;
        let inputs: Vec<_> = self.answers.drain(start..).map(|answer| answer.value).collect();
        self.answers.push(Answer { exists: true, value: combine(self.values, inputs, intersection) });
    }
}

fn widened(values: &Values<'_>, leaf: Leaf) -> NodeValue {
    let Known::Yes(leaf) = values.semantic.leaf(leaf) else { return NodeValue::Unknown };
    if let Some(answer) = values.semantic.values.widened.get(&leaf) { return answer }
    values.semantic.values.widened.insert(values.semantic.epoch, leaf, NodeValue::Unknown);
    let mark = values.cycle_mark();
    let answer = widen(values, leaf);
    if answer == NodeValue::Unknown && values.cycled_since(mark) {
        values.semantic.values.widened.remove(values.semantic.epoch, &leaf);
    } else {
        values.semantic.values.widened.insert(values.semantic.epoch, leaf, answer.clone());
    }
    answer
}

fn widen(values: &Values<'_>, leaf: Leaf) -> NodeValue {
    let Some(_nested) = values.question.budget.nest() else { return NodeValue::Unknown };
    if let Target::Node(node) = leaf.target {
        let Some(tree) = values.world.tree(node.file) else { return NodeValue::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return NodeValue::Unknown };
        if let Some(kind) = values.tables.kinds.get(syntax.matcher as usize) {
            for (recipe, when) in &kind.widens {
                if !values.question.budget.step() { return NodeValue::Unknown }
                match when.map_or(Known::Yes(true), |condition| values.world.condition(leaf, condition, values.semantic)) {
                    Known::Yes(true) => return values.evaluate(leaf, recipe),
                    Known::Yes(false) => {}
                    Known::Unknown => return NodeValue::Unknown,
                }
            }
        }
    }
    let NodeValue::Alternatives(rows) = values.values(leaf) else { return NodeValue::Unknown };
    let mut alternatives = Vec::new();
    for row in rows {
        let mut parts = Vec::new();
        for part in row {
            if !values.question.budget.step() { return NodeValue::Unknown }
            parts.push(if part == leaf { NodeValue::leaf(part) } else { widened(values, part) });
        }
        alternatives.push(combine(values, parts, true));
    }
    combine(values, alternatives, false)
}

fn receiver(values: &Values<'_>, at: Leaf) -> Known<Option<NodeValue>> {
    let Some(_nested) = values.question.budget.nest() else { return Known::Unknown };
    let value = values.values(at);
    match &value {
        NodeValue::Unknown => return Known::Unknown,
        NodeValue::Alternatives(rows) if rows.is_empty() => return Known::Yes(Some(NodeValue::never())),
        _ => {}
    }
    let calls = values.semantic.calls();
    let Known::Yes(groups) = calls.headers(&value, super::super::calls::InvokeKind::Call) else { return Known::Unknown };
    let mut receivers = Vec::new();
    let mut absent = false;
    for group in groups {
        if !values.question.budget.step() { return Known::Unknown }
        absent |= group.is_empty();
        for header in group {
            match calls.receiver(header) {
                Known::Yes(Some(site)) => receivers.push(values.values(site)),
                Known::Yes(None) => absent = true,
                Known::Unknown => return Known::Unknown,
            }
        }
    }
    if receivers.is_empty() { return Known::Yes(None) }
    if absent { return Known::Unknown }
    Known::Yes(Some(combine(values, receivers, false)))
}

fn returned(values: &Values<'_>, owner: Leaf, default: Option<&Value>) -> Known<Option<NodeValue>> {
    let Some(_nested) = values.question.budget.nest() else { return Known::Unknown };
    let effects = values.semantic.effects();
    let Known::Yes(entries) = effects.entries(owner) else { return Known::Unknown };
    if entries.is_empty() { return Known::Yes(None) }
    let Some(effect) = values.tables.vocabulary.effect("returned") else { return Known::Unknown };
    let mut payloads = Vec::new();
    let mut needs_default = false;
    for entry in entries {
        if !values.question.budget.step() { return Known::Unknown }
        let Known::Yes(normal) = effects.normal(entry) else { return Known::Unknown };
        needs_default |= normal;
        let Known::Yes(returns) = effects.declared(entry, effect) else { return Known::Unknown };
        for returned in returns {
            if returned.possible && returned.value.is_none() { return Known::Unknown }
            match returned.value {
                Some(payload) => payloads.push(values.values(payload)),
                None => needs_default = true,
            }
        }
    }
    if needs_default {
        let Some(default) = default else { return Known::Unknown };
        payloads.push(values.evaluate(owner, default));
    }
    Known::Yes(Some(combine(values, payloads, false)))
}

pub(super) fn combine(values: &Values<'_>, inputs: impl IntoIterator<Item = NodeValue>, intersection: bool) -> NodeValue {
    let mut result = if intersection { vec![Vec::new()] } else { Vec::new() };
    let mut unknown = false;
    for input in inputs {
        let NodeValue::Alternatives(alternatives) = input else {
            unknown = true;
            continue;
        };
        if intersection && alternatives.is_empty() { return NodeValue::never() }
        if intersection {
            let mut product = Vec::new();
            for left in &result {
                for right in &alternatives {
                    if !values.question.budget.step() {
                        return NodeValue::Unknown;
                    }
                    let mut conjunction = left.clone();
                    for &leaf in right {
                        if !values.question.budget.step() {
                            return NodeValue::Unknown;
                        }
                        if !conjunction.contains(&leaf) { conjunction.push(leaf) }
                    }
                    product.push(conjunction);
                }
            }
            result = product;
        } else {
            for alternative in alternatives {
                if !values.question.budget.step() {
                    return NodeValue::Unknown;
                }
                result.push(alternative);
            }
        }
    }
    let result = values.normalize(NodeValue::Alternatives(result));
    if !unknown { return result }
    if !intersection {
        if let NodeValue::Alternatives(rows) = &result {
            if rows.iter().any(|row| !row.is_empty() && row.iter().all(|&leaf| values.top(leaf) == Known::Yes(true))) {
                return result;
            }
        }
    }
    NodeValue::Unknown
}

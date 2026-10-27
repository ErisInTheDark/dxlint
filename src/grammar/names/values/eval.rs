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
    /// Leaves this run opened and has not settled, innermost last, with the
    /// task and answer counts when each opened: running out inside one
    /// unwinds just that leaf (unknown) and its asker carries on.
    opened: Vec<(Leaf, usize, usize)>,
}

impl<'a, 'world> Run<'a, 'world> {
    fn new(values: &'a Values<'world>, tasks: Vec<Task<'a>>) -> Self {
        Run { values, tasks, answers: Vec::new(), opened: Vec::new() }
    }
}

pub(super) fn values(values: &Values<'_>, leaf: Leaf) -> NodeValue {
    if values.world.profile().is_none() {
        if !values.question.budget.step() { return NodeValue::Unknown }
        let Known::Yes(leaf) = values.semantic.leaf(leaf) else { return NodeValue::Unknown };
        if let Some(value) = kept(values, leaf) { return value }
        let mut run = Run::new(values, Vec::new());
        run.evaluate_leaf(leaf);
        return run.run();
    }
    Run::new(values, vec![Task::Leaf(leaf)]).run()
}

/// The kept, known value of canonical `leaf`.
pub(super) fn kept_value(values: &Values<'_>, leaf: Leaf) -> Option<NodeValue> {
    values.semantic.values.values.get(&leaf).filter(|value| *value != NodeValue::Unknown)
}

/// The kept value of canonical `leaf`, if any; reading one still being
/// evaluated counts as a cycle read.
fn kept(values: &Values<'_>, leaf: Leaf) -> Option<NodeValue> {
    let state = &values.semantic.values;
    let value = state.values.get(&leaf)?;
    let tag = super::fact_tag(&state.values, &leaf);
    if value != NodeValue::Unknown {
        values.read_kept(tag);
    } else if state.evaluating.borrow().iter().any(|&(active, ..)| active == leaf) {
        state.cycle_reads.set(state.cycle_reads.get() + 1);
    } else {
        match values.read_provisional(tag) {
            Some(true) => {}
            Some(false) => {
                state.values.remove(&leaf);
                return None;
            }
            None => values.read_kept(tag),
        }
    }
    Some(value)
}

pub(super) fn evaluate(values: &Values<'_>, at: Leaf, expression: &Value) -> NodeValue {
    Run::new(values, vec![Task::Expression(at, expression)]).run()
}

pub(super) fn evaluate_optional(values: &Values<'_>, at: Leaf, expression: &Value) -> Known<Option<NodeValue>> {
    let answer = Run::new(values, vec![Task::Expression(at, expression)]).run_answer();
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
                let base = Run::new(values, vec![Task::Expression(at, base)]).run_answer();
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

/// Spreads over more parts than this run on their own allowance.
const SPREAD_DETACHED: usize = 64;
/// Steps per part a detached spread may spend (each part's own value).
const SPREAD_STEPS_PER_PART: u32 = 256;

impl<'a, 'world> Run<'a, 'world> {
    fn run(self) -> NodeValue {
        self.run_answer().value
    }

    fn run_answer(mut self) -> Answer {
        let _site = self.values.world.profile().map(|_| crate::engine::StepSite::enter("values.evaluate"));
        while let Some(task) = self.tasks.pop() {
            if !matches!(&task, Task::Profile(_)) && !self.values.question.budget.step() {
                // The innermost leaf this run opened ran out of its own
                // allowance: it is unknown, and it cuts its asker in turn
                // (see `settle`).
                let Some(&(leaf, tasks, answers)) = self.opened.last() else { return Answer::unknown() };
                self.tasks.truncate(tasks);
                self.answers.truncate(answers);
                self.settle(leaf, NodeValue::Unknown);
                self.answers.push(Answer::unknown());
                continue;
            }
            match task {
                Task::Profile(timer) => drop(timer),
                Task::Leaf(leaf) => self.leaf(leaf),
                Task::Save(leaf) => {
                    if let Some(answer) = self.answers.last_mut() {
                        let values = self.values;
                        let save = |value: NodeValue| values.normalize(values.semantic.narrowing().refine(leaf, value));
                        // A value of many alternatives (an enum's members) is
                        // normalized once and kept: on an allowance for its
                        // size, like the spread that built it.
                        let rows = match &answer.value { NodeValue::Alternatives(rows) => rows.len(), _ => 0 };
                        answer.value = match values.question.budget.nest() {
                            Some(_nested) if rows > SPREAD_DETACHED => {
                                let allowance = u32::try_from(rows).unwrap_or(u32::MAX).saturating_mul(SPREAD_STEPS_PER_PART);
                                let value = answer.value.clone();
                                values.question.budget.sized(allowance, || save(value)).unwrap_or(NodeValue::Unknown)
                            }
                            Some(_nested) => save(answer.value.clone()),
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
        let root = matches!(leaf.target, Target::Node(super::NodeRef { file: 0, .. }) | Target::Scope(super::ScopeRef { file: 0, .. }));
        if root {
            state.root_reads.set(state.root_reads.get() + 1);
        }
        if let Some(value) = kept(self.values, leaf) {
            self.answers.push(Answer { exists: true, value });
            return;
        }
        if !root && super::share_values() {
            if let Some(value) = self.values.world.shared_value_kept(leaf, self.values.semantic) {
                state.values.insert(leaf, value.clone());
                self.answers.push(Answer { exists: true, value });
                return;
            }
        }
        self.evaluate_leaf(leaf);
    }

    /// Starts evaluating canonical `leaf`, which has no kept value.
    fn evaluate_leaf(&mut self, leaf: Leaf) {
        let state = &self.values.semantic.values;
        // A node already being evaluated many times further out, each time
        // in another frame, is regressing (narrowing a read re-reads the
        // same read through a fresh frame), as TypeScript stops excessively
        // deep instantiation: answer unknown instead of nesting until the
        // budget runs out. Real nesting of one declaration (`Readonly<
        // Readonly<T>>`) stays well below the limit.
        const NESTING: usize = 8;
        if state.evaluating.borrow().iter().filter(|(active, ..)| active.target == leaf.target).nth(NESTING - 1).is_some() {
            state.cycle_reads.set(state.cycle_reads.get() + 1);
            self.answers.push(Answer::unknown());
            return;
        }
        state.values.insert(leaf, NodeValue::Unknown);
        // The leaf's value is a fact of its own, on its own allowance.
        let asker = self.values.question.budget.begin_fact(super::super::question::Budget::per_question());
        state.evaluating.borrow_mut().push((leaf, self.values.cycle_mark(), asker));
        self.opened.push((leaf, self.tasks.len(), self.answers.len()));
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
                // A declaration spreading many parts (an enum's members) is
                // evaluated once and kept for every later question: on an
                // allowance for its size, not what the asking question has
                // left (a 993-member enum was cut and redone question after
                // question).
                if let Value::Spread { path, value: None, .. } = expression {
                    if let Known::Yes(sites) = self.values.follow(leaf, path) {
                        if sites.len() > SPREAD_DETACHED {
                            let allowance = u32::try_from(sites.len()).unwrap_or(u32::MAX).saturating_mul(SPREAD_STEPS_PER_PART);
                            let value = self.values.question.budget.sized(allowance, || self.values.evaluate(leaf, expression));
                            self.answers.push(value.ok().map_or_else(Answer::unknown, |value| Answer { exists: true, value }));
                            return;
                        }
                    }
                }
                self.tasks.push(Task::Expression(leaf, expression));
                return;
            }
            Known::Yes(None) => {}
        }
        self.answers.push(Answer { exists: true, value: NodeValue::leaf(leaf) });
        self.tasks.push(Task::Save(leaf));
    }

    /// Ends `leaf`'s evaluation with `value`: kept, but an unknown that read
    /// a leaf still being evaluated only provisionally
    /// ([`Values::keep_provisional`]): it may only be the unfinished cycle.
    fn settle(&mut self, leaf: Leaf, value: NodeValue) {
        if let Some(at) = self.opened.iter().rposition(|&(open, ..)| open == leaf) { self.opened.truncate(at) }
        let state = &self.values.semantic.values;
        let budget = &self.values.question.budget;
        let opened = {
            let mut evaluating = state.evaluating.borrow_mut();
            match evaluating.iter().rposition(|&(active, ..)| active == leaf) {
                Some(index) => {
                    // Leaves opened inside it and never settled end with it.
                    for &(_, _, inner) in evaluating[index + 1..].iter().rev() { budget.end_fact(inner); }
                    let (_, started, asker) = evaluating[index];
                    evaluating.truncate(index);
                    Some((started, asker))
                }
                None => None,
            }
        };
        let tag = super::fact_tag(&state.values, &leaf);
        // A leaf that ran out cuts its asker, as it does every later reader
        // of its kept value.
        if let Some((_, asker)) = opened {
            let cut = budget.end_fact(asker);
            self.values.note_ended(tag, cut);
            if cut == Some(super::super::question::Cut::RanOut) { budget.exhaust() }
        }
        match opened {
            Some((started, asker)) if value == NodeValue::Unknown && self.values.cycled_since(started) => {
                state.values.insert(leaf, value);
                self.values.keep_provisional(tag, asker.epoch);
            }
            None if value == NodeValue::Unknown => { state.values.remove(&leaf); }
            _ => {
                self.values.settle_provisional(tag);
                state.values.insert(leaf, value);
            }
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
                let base = Run::new(self.values, vec![Task::Expression(at, base)]).run_answer();
                if !base.exists {
                    self.answers.push(base);
                    return;
                }
                let value = match self.values.key_alternatives(at, key) {
                    // Several literal keys read one member each (`T["a" | "b"]`,
                    // `T[keyof T]`); only a wide key reads indexers.
                    Known::Yes(keys) if keys.len() > 1 && keys.iter().all(|key| !matches!(key, super::Key::Domain(_))) => {
                        combine(self.values, keys.iter().map(|key| self.values.member(&base.value, key)).collect::<Vec<_>>(), false)
                    }
                    Known::Yes(keys) => match Values::widened_key(keys) {
                        Known::Yes(key) => self.values.member(&base.value, &key),
                        Known::Unknown => NodeValue::Unknown,
                    },
                    Known::Unknown => NodeValue::Unknown,
                };
                self.answers.push(Answer { exists: true, value });
            }
            Value::Keys { of } => {
                let operand = Run::new(self.values, vec![Task::Expression(at, of)]).run_answer();
                let value = if operand.exists { self.values.keys(&operand.value) } else { NodeValue::Unknown };
                self.answers.push(Answer { exists: true, value });
            }
            Value::Argument { index, of } => {
                let operand = Run::new(self.values, vec![Task::Expression(at, of)]).run_answer();
                let value = if operand.exists { self.values.carrier_argument(&operand.value, *index as usize) } else { NodeValue::Unknown };
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
                let mut answer = Run::new(self.values, vec![Task::Expression(at, from)]).run_answer();
                if answer.exists { answer.value = self.excludes(answer.value, *traits) }
                self.answers.push(answer);
            }
            Value::Modified { source, required, readonly, flags } => {
                let mut answer = Run::new(self.values, vec![Task::Expression(at, source)]).run_answer();
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
            super::super::FIRST_PARAMETER => first_parameter(self.values, leaf),
            _ => Known::Unknown,
        }
    }

    fn query_value_path(&mut self, at: Leaf, prefix: &[u16], query: u16, spread: bool) {
        let expression = Value::Path(prefix.to_vec());
        let answer = Run::new(self.values, vec![Task::Expression(at, &expression)]).run_answer();
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
        // A contextual callable's result comes from the arms that can be
        // called: `null | (() => T)` gives `T` (as TypeScript's contextual
        // signature skips non-callable arms).
        let callable_arms_only = query == super::super::RESULT;
        let answer = if results.is_empty() {
            if spread { Answer { exists: true, value: NodeValue::never() } } else { Answer::absent() }
        } else if absent != 0 && !callable_arms_only {
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
        for alternative in &alternatives {
            let row = NodeValue::rows(vec![alternative.clone()]);
            let mut excluded = Known::Yes(false);
            for bit in (0..u64::BITS).map(|i| 1u64 << i).filter(|bit| traits & bit != 0) {
                if !self.values.question.budget.step() { return NodeValue::Unknown }
                match self.values.always(&row, bit) {
                    Known::Yes(true) => {
                        excluded = Known::Yes(true);
                        break;
                    }
                    Known::Yes(false) => {}
                    // An open part (a type parameter) can't settle it; a
                    // part exclusive with the trait (its constraint) does.
                    Known::Unknown if alternative.len() > 1
                        && self.values.row_excludes(alternative, bit.trailing_zeros() as u16) == Known::Yes(true) => {}
                    Known::Unknown => excluded = Known::Unknown,
                }
            }
            match excluded {
                Known::Yes(true) => {}
                Known::Yes(false) => kept.push(alternative.clone()),
                Known::Unknown => return NodeValue::Unknown,
            }
        }
        // Nothing excluded: the value passes on as it was (shared, normalized).
        if kept.len() == alternatives.len() { return NodeValue::Alternatives(alternatives) }
        NodeValue::rows(kept)
    }

    fn combine(&mut self, count: usize, intersection: bool) {
        let start = self.answers.len() - count;
        let inputs: Vec<_> = self.answers.drain(start..).map(|answer| answer.value).collect();
        self.answers.push(Answer { exists: true, value: combine(self.values, inputs, intersection) });
    }
}

fn widened(values: &Values<'_>, leaf: Leaf) -> NodeValue {
    let Known::Yes(leaf) = values.semantic.leaf(leaf) else { return NodeValue::Unknown };
    let tag = super::fact_tag(&values.semantic.values.widened, &leaf);
    if let Some(answer) = values.semantic.values.widened.get(&leaf) {
        values.read_kept(tag);
        return answer;
    }
    values.semantic.values.widened.insert(leaf, NodeValue::Unknown);
    let mark = values.cycle_mark();
    let answer = values.fact(tag, super::super::question::Budget::per_question(), || widen(values, leaf));
    let unpaid = answer.as_ref().err() == Some(&super::super::question::Cut::Unpaid);
    let answer = answer.unwrap_or(NodeValue::Unknown);
    if answer == NodeValue::Unknown && (unpaid || values.cycled_since(mark)) {
        values.semantic.values.widened.remove(&leaf);
    } else {
        values.semantic.values.widened.insert(leaf, answer.clone());
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

/// The first parameter of every call signature `at`'s value has (a
/// component's props): absent when every signature takes none; unknown
/// when some do and some don't, or the value has no call signatures.
fn first_parameter(values: &Values<'_>, at: Leaf) -> Known<Option<NodeValue>> {
    let Some(_nested) = values.question.budget.nest() else { return Known::Unknown };
    let value = values.values(at);
    if value == NodeValue::Unknown { return Known::Unknown }
    let calls = values.semantic.calls();
    let Known::Yes(groups) = calls.headers(&value, super::super::calls::InvokeKind::Call) else { return Known::Unknown };
    let mut parameters = Vec::new();
    let mut absent = false;
    for group in groups {
        if !values.question.budget.step() { return Known::Unknown }
        if group.is_empty() { return Known::Unknown }
        for header in group {
            match calls.parameters(header) {
                Known::Yes(found) => match found.first() {
                    Some(&first) => parameters.push(values.values(first)),
                    None => absent = true,
                },
                Known::Unknown => return Known::Unknown,
            }
        }
    }
    match (parameters.is_empty(), absent) {
        (true, true) => Known::Yes(None),
        (true, false) | (false, true) => Known::Unknown,
        (false, false) => Known::Yes(Some(combine(values, parameters, false))),
    }
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
    // `X & X` is `X`: a member merged from many declarations of one type
    // (11 sources of three rows) must not multiply into 3^11 rows.
    let mut distinct: Vec<NodeValue> = Vec::new();
    for input in inputs {
        if !intersection || !distinct.contains(&input) { distinct.push(input) }
    }
    let mut inputs = distinct.into_iter().peekable();
    let first = inputs.next();
    // One input is its own union, and its own intersection when no row
    // repeats a leaf: it passes on shared, normalized at most once.
    if let (Some(NodeValue::Alternatives(rows)), None) = (&first, inputs.peek()) {
        if !intersection {
            return values.normalize_union(NodeValue::Alternatives(rows.clone()));
        }
        if rows.iter().all(|row| row.len() <= 1) {
            if let Some(rows) = rows.fully_normal() { return NodeValue::Alternatives(rows) }
        }
    }
    let inputs = first.into_iter().chain(inputs);
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
    let result = if intersection && result.len() > SPREAD_DETACHED {
        // Checking each of many rows for contradictions (`E & C` for an
        // enum `E`) is kept with the value it builds: on an allowance for
        // its size, like the members themselves.
        let allowance = u32::try_from(result.len()).unwrap_or(u32::MAX).saturating_mul(SPREAD_STEPS_PER_PART);
        values.question.budget.sized(allowance, || values.normalize(NodeValue::rows(result))).unwrap_or(NodeValue::Unknown)
    } else if intersection {
        values.normalize(NodeValue::rows(result))
    } else {
        values.normalize_union(NodeValue::rows(result))
    };
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

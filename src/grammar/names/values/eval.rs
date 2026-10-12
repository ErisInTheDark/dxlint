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

impl<'a, 'world> Run<'a, 'world> {
    fn run(mut self) -> NodeValue {
        while let Some(task) = self.tasks.pop() {
            if !self.values.question.budget.step() {
                return NodeValue::Unknown;
            }
            match task {
                Task::Leaf(leaf) => self.leaf(leaf),
                Task::Save(leaf) => {
                    if let Some(answer) = self.answers.last_mut() {
                        answer.value = self.values.normalize(answer.value.clone());
                        self.values.semantic.values.values.borrow_mut().insert(leaf, answer.value.clone());
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
        self.answers.pop().map_or(NodeValue::Unknown, |answer| answer.value)
    }

    fn leaf(&mut self, leaf: Leaf) {
        if let Some(value) = self.values.semantic.values.values.borrow().get(&leaf) {
            self.answers.push(Answer { exists: true, value: value.clone() });
            return;
        }
        self.values.semantic.values.values.borrow_mut().insert(leaf, NodeValue::Unknown);
        let Target::Node(node) = leaf.target else {
            self.answers.push(Answer { exists: true, value: NodeValue::leaf(leaf) });
            self.tasks.push(Task::Save(leaf));
            return;
        };
        let Some(tree) = self.values.world.tree(node.file) else {
            self.answers.push(Answer::unknown());
            return;
        };
        let Some(data) = tree.nodes.get(node.node as usize) else {
            self.answers.push(Answer::unknown());
            return;
        };
        let kind = self.values.tables.kinds.get(data.matcher as usize);
        if let Some(kind) = kind {
            for unwrap in &kind.unwraps {
                if unwrap.effect.is_some() {
                    continue;
                }
                let applies = unwrap.when.map_or(Known::Yes(true), |condition| self.values.world.condition(leaf, condition, self.values.semantic));
                match applies {
                    Known::Yes(false) => continue,
                    Known::Unknown => {
                        self.answers.push(Answer::unknown());
                        return;
                    }
                    Known::Yes(true) => {
                        self.tasks.push(Task::Save(leaf));
                        if let Some(expression) = &unwrap.value {
                            self.tasks.push(Task::Expression(leaf, expression));
                        } else {
                            self.answers.push(Answer::unknown());
                        }
                        return;
                    }
                }
            }
        }
        self.answers.push(Answer { exists: true, value: NodeValue::leaf(leaf) });
        self.tasks.push(Task::Save(leaf));
    }

    fn expression(&mut self, at: Leaf, expression: &'a Value) {
        match expression {
            Value::Path(path) => self.path(at, path, false, false),
            Value::Spread { intersection, path } => self.path(at, path, *intersection, true),
            Value::Either(left, right) => {
                self.tasks.push(Task::Fallback(at, right));
                self.tasks.push(Task::Expression(at, left));
            }
            Value::Union(parts) | Value::Intersection(parts) => {
                self.tasks.push(Task::Combine { count: parts.len(), intersection: matches!(expression, Value::Intersection(_)) });
                self.tasks.extend(parts.iter().rev().map(|part| Task::Expression(at, part)));
            }
            // Member, scope and truth questions have their own owners.
            Value::Text(_) | Value::From(..) | Value::Scope { .. } | Value::Not(_) | Value::Excludes { .. }
                | Value::Includes { .. } | Value::Has { .. } | Value::Ordinal(_) | Value::Instance { .. } => self.answers.push(Answer::unknown()),
        }
    }

    fn path(&mut self, at: Leaf, path: &[u16], intersection: bool, spread: bool) {
        match self.values.follow(at, path) {
            Known::Unknown => self.answers.push(Answer::unknown()),
            Known::Yes(leaves) if leaves.is_empty() && !spread => self.answers.push(Answer::absent()),
            Known::Yes(leaves) => {
                self.tasks.push(Task::Combine { count: leaves.len(), intersection });
                self.tasks.extend(leaves.into_iter().rev().map(Task::Leaf));
            }
        }
    }

    fn combine(&mut self, count: usize, intersection: bool) {
        let start = self.answers.len() - count;
        let inputs: Vec<_> = self.answers.drain(start..).collect();
        let mut result = if intersection { vec![Vec::new()] } else { Vec::new() };
        for answer in inputs {
            let NodeValue::Alternatives(alternatives) = answer.value else {
                self.answers.push(Answer::unknown());
                return;
            };
            if intersection {
                let mut product = Vec::new();
                for left in &result {
                    for right in &alternatives {
                        if !self.values.question.budget.step() {
                            self.answers.push(Answer::unknown());
                            return;
                        }
                        let mut conjunction = left.clone();
                        for &leaf in right {
                            if !self.values.question.budget.step() {
                                self.answers.push(Answer::unknown());
                                return;
                            }
                            if !conjunction.contains(&leaf) { conjunction.push(leaf) }
                        }
                        product.push(conjunction);
                    }
                }
                result = product;
            } else {
                for alternative in alternatives {
                    if !self.values.question.budget.step() {
                        self.answers.push(Answer::unknown());
                        return;
                    }
                    result.push(alternative);
                }
            }
        }
        self.answers.push(Answer { exists: true, value: self.values.normalize(NodeValue::Alternatives(result)) });
    }
}

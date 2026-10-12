//! Calls and signatures: overloads, `expected`, call-site inference
//! (docs/design/type-layers.md "Calls and signatures"). Owner: Juniper.

use std::cell::RefCell;

use crate::config::ast::calls::Variance;
use crate::fx_hash::FxHashMap;

use super::question::{Known, Leaf, NodeValue};
use super::semantic::Semantic;
use super::Value;

mod accepts;
pub mod compile;

pub type Path = Vec<u16>;

#[derive(Clone, Debug)]
pub struct Conditional<T> {
    pub item: T,
    pub when: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Signature {
    pub parameters: Path,
    pub result: Value,
}

#[derive(Clone, Debug)]
pub struct Invoke {
    pub callee: Value,
    pub arguments: Path,
    pub construct: bool,
}

#[derive(Clone, Debug)]
pub struct Expected {
    pub field: Path,
    pub value: Value,
}

#[derive(Clone, Debug, Default)]
pub struct KindCalls {
    pub signatures: Vec<Conditional<Signature>>,
    pub sources: Vec<Conditional<Value>>,
    pub invokes: Vec<Conditional<Invoke>>,
    pub expected: Vec<Conditional<Expected>>,
    pub discards: Vec<Option<u32>>,
    pub identities: Vec<Conditional<u32>>,
    pub optional: Vec<Option<u32>>,
    pub variances: Vec<Conditional<Variance>>,
}

/// A value question's answer: `Yes(None)` when there is none to ask about.
type ValueAnswer = Known<Option<NodeValue>>;

#[derive(Default)]
pub(crate) struct State {
    pub(super) acceptance: RefCell<FxHashMap<(NodeValue, NodeValue), Known<bool>>>,
    values: RefCell<FxHashMap<(Leaf, ValueQuestion), ValueAnswer>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ValueQuestion { Result, Expected, Inferred }

pub struct Calls<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
}

impl<'a> std::ops::Deref for Calls<'a> {
    type Target = Semantic<'a>;

    fn deref(&self) -> &Self::Target {
        self.semantic
    }
}

impl Calls<'_> {
    /// Whether every value `actual` may be is one `expected` allows,
    /// memoised per question; a pending comparison (a cycle) is unknown.
    pub fn accepts(&self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        let key = (expected.clone(), actual.clone());
        if let Some(&known) = self.semantic.calls.acceptance.borrow().get(&key) {
            return known;
        }
        self.semantic.calls.acceptance.borrow_mut().insert(key.clone(), Known::Unknown);
        let answer = accepts::accepts(expected, actual, &self.semantic.values());
        self.semantic.calls.acceptance.borrow_mut().insert(key, answer);
        answer
    }

    pub fn result(&self, at: Leaf) -> Known<Option<NodeValue>> {
        self.value(at, ValueQuestion::Result)
    }

    pub fn expected(&self, at: Leaf) -> Known<Option<NodeValue>> {
        self.value(at, ValueQuestion::Expected)
    }

    pub fn inferred(&self, at: Leaf) -> Known<Option<NodeValue>> {
        self.value(at, ValueQuestion::Inferred)
    }

    fn value(&self, at: Leaf, question: ValueQuestion) -> Known<Option<NodeValue>> {
        let key = (at, question);
        if let Some(answer) = self.semantic.calls.values.borrow().get(&key) {
            return answer.clone();
        }
        if !self.question.budget.step() {
            return Known::Unknown;
        }
        self.semantic.calls.values.borrow_mut().insert(key, Known::Unknown);
        let answer = Known::Unknown;
        self.semantic.calls.values.borrow_mut().insert(key, answer.clone());
        answer
    }
}

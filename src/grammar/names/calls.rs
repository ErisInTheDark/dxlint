//! Calls and signatures: overloads, `expected`, call-site inference
//! (docs/design/type-layers.md "Calls and signatures"). Owner: Juniper.

use std::cell::RefCell;

use crate::config::ast::calls::Variance;
use crate::fx_hash::FxHashMap;

use super::question::{Known, Leaf, NodeRef, NodeValue, Target};
use super::semantic::Semantic;
use super::values::Values;
use super::{KindNames, Value};

mod accepts;
mod headers;
mod result;
mod select;
pub mod compile;

#[cfg(test)]
mod tests;

pub type Path = Vec<u16>;

#[derive(Clone, Debug)]
pub struct Conditional<T> {
    pub item: T,
    pub when: Option<u32>,
}

fn expanded(value: &NodeValue, values: &Values<'_>) -> NodeValue {
    let NodeValue::Alternatives(arms) = value else { return NodeValue::Unknown };
    let mut expanded = Vec::new();
    for arm in arms {
        let mut products = vec![Vec::new()];
        for &leaf in arm {
            let NodeValue::Alternatives(parts) = values.values(leaf) else { return NodeValue::Unknown };
            let mut next = Vec::new();
            for product in &products {
                for part in &parts {
                    if !values.question.budget.step() { return NodeValue::Unknown }
                    let mut combined = product.clone();
                    combined.extend_from_slice(part);
                    next.push(combined);
                }
            }
            products = next;
        }
        expanded.extend(products);
    }
    NodeValue::Alternatives(expanded)
}

fn kind<'a>(leaf: Leaf, values: &'a Values<'_>) -> Option<&'a KindNames> {
    let node = node(leaf, values)?;
    let node = values.world.tree(node.file)?.nodes.get(node.node as usize)?;
    values.tables.kinds.get(node.matcher as usize)
}

fn node(leaf: Leaf, values: &Values<'_>) -> Option<NodeRef> {
    Some(match leaf.target {
        Target::Node(node) => node,
        Target::Scope(scope) => {
            let info = values.world.names(scope.file)?.scopes.get(scope.scope as usize)?;
            NodeRef { file: scope.file, node: info.node }
        }
    })
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvokeKind { Call, Construct }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    pub site: Leaf,
    pub signature: u32,
}

#[derive(Clone, Debug)]
pub struct Chosen {
    pub header: Header,
    /// Formal leaves use the selected frame; actual leaves keep caller frames.
    pub mapping: Vec<(Leaf, Vec<Leaf>)>,
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub choices: Vec<Chosen>,
    pub uncertain: bool,
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
type HeaderAnswer = Known<Vec<Vec<Header>>>;

#[derive(Default)]
pub(crate) struct State {
    pub(super) acceptance: RefCell<FxHashMap<(NodeValue, NodeValue), Known<bool>>>,
    headers: RefCell<FxHashMap<(NodeValue, InvokeKind), HeaderAnswer>>,
    candidates: RefCell<FxHashMap<Leaf, select::Answer>>,
    values: RefCell<FxHashMap<(Leaf, ValueQuestion), ValueAnswer>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ValueQuestion { Result, Expected }

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
    /// One overload set per value alternative; intersections combine headers.
    pub fn headers(&self, value: &NodeValue, kind: InvokeKind) -> Known<Vec<Vec<Header>>> {
        let key = (value.clone(), kind);
        if let Some(answer) = self.semantic.calls.headers.borrow().get(&key) {
            return answer.clone();
        }
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        self.semantic.calls.headers.borrow_mut().insert(key.clone(), Known::Unknown);
        let answer = headers::headers(value, kind, &self.semantic.values());
        self.semantic.calls.headers.borrow_mut().insert(key, answer.clone());
        answer
    }

    pub fn selection(&self, call: Leaf) -> Known<Option<Selection>> {
        select::selection(call, &self.semantic.values())
    }

    pub fn argument_sites(&self, formal_binding: Leaf) -> Known<Option<Vec<Leaf>>> {
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        select::argument_sites(formal_binding, &self.semantic.values())
    }

    fn candidates(&self, call: Leaf) -> select::Answer {
        if let Some(answer) = self.semantic.calls.candidates.borrow().get(&call) {
            return answer.clone();
        }
        if !self.question.budget.step() { return Known::Unknown }
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        self.semantic.calls.candidates.borrow_mut().insert(call, Known::Unknown);
        let answer = select::candidates(call, &self.semantic.values());
        self.semantic.calls.candidates.borrow_mut().insert(call, answer.clone());
        answer
    }

    /// Whether every value `actual` may be is one `expected` allows,
    /// memoised per question; a pending comparison (a cycle) is unknown.
    pub fn accepts(&self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        let key = (expected.clone(), actual.clone());
        if let Some(&known) = self.semantic.calls.acceptance.borrow().get(&key) {
            return known;
        }
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
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
        self.semantic.values().bound_argument(at)
    }

    fn value(&self, at: Leaf, question: ValueQuestion) -> Known<Option<NodeValue>> {
        let key = (at, question);
        if let Some(answer) = self.semantic.calls.values.borrow().get(&key) {
            return answer.clone();
        }
        if !self.question.budget.step() {
            return Known::Unknown;
        }
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        self.semantic.calls.values.borrow_mut().insert(key, Known::Unknown);
        let answer = match question {
            ValueQuestion::Result => result::result(at, &self.semantic.values()),
            ValueQuestion::Expected => Known::Unknown,
        };
        self.semantic.calls.values.borrow_mut().insert(key, answer.clone());
        answer
    }
}

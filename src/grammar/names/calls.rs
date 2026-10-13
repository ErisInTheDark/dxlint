//! Calls and signatures: overloads, `expected`, call-site inference
//! (docs/design/type-layers.md "Calls and signatures"). Owner: Juniper.

use crate::config::ast::calls::{ExpectedAt, Variance};

use super::memo::{Epoch, Memo};
use super::question::{Known, Leaf, NodeRef, NodeValue, Target};
use super::semantic::Semantic;
use super::values::Values;
use super::{KindNames, Value};

mod accepts;
mod headers;
mod expected;
mod infer;
mod result;
mod select;
mod signature_accepts;
pub mod compile;

#[cfg(test)]
mod tests;

pub type Path = Vec<u16>;

#[derive(Clone, Debug)]
pub struct Conditional<T> {
    pub item: T,
    pub when: Option<u32>,
}

fn kind<'a>(leaf: Leaf, values: &'a Values<'_>) -> Option<&'a KindNames> {
    let node = node(leaf, values)?;
    let node = values.world.tree(node.file)?.nodes.get(node.node as usize)?;
    values.tables.kinds.get(node.matcher as usize)
}

fn rest(at: Leaf, values: &Values<'_>) -> Known<bool> {
    values.tables.vocabulary.traits.iter().position(|name| name == "rest")
        .map_or(Known::Yes(false), |id| values.fits(at, id as u16))
}

fn node(leaf: Leaf, values: &Values<'_>) -> Option<NodeRef> {
    let Known::Yes(leaf) = values.semantic.leaf(leaf) else { return None };
    Some(match leaf.target {
        Target::Node(node) => node,
        Target::Scope(scope) => {
            let info = values.world.names(scope.file)?.scopes.get(scope.scope as usize)?;
            NodeRef { file: scope.file, node: info.node }
        }
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct Signature {
    pub parameters: Path,
    pub receiver: Option<Path>,
    pub result: Value,
}

fn signature<'a>(header: Header, values: &'a Values<'_>) -> Option<(Leaf, &'a Signature)> {
    let Known::Yes(site) = values.semantic.leaf(header.site) else { return None };
    Some((site, &kind(site, values)?.calls.signatures.get(header.signature as usize)?.item))
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
    pub at: ExpectedAt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvokeKind { Call, Construct }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub accepted: bool,
    pub arguments: Vec<Option<NodeValue>>,
}

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
    pub infer_last: Vec<Option<u32>>,
}

/// A value question's answer: `Yes(None)` when there is none to ask about.
type ValueAnswer = Known<Option<NodeValue>>;
type HeaderAnswer = Known<Vec<Vec<Header>>>;

#[derive(Default)]
pub(crate) struct State {
    pub(super) acceptance: Memo<(NodeValue, NodeValue), Known<bool>>,
    headers: Memo<(NodeValue, InvokeKind), HeaderAnswer>,
    candidates: Memo<Leaf, select::Answer>,
    values: Memo<(Leaf, ValueQuestion), ValueAnswer>,
}

impl State {
    pub(super) fn finish(&self, epoch: Epoch, failed: bool) -> u64 {
        self.acceptance.finish(epoch, failed) + self.headers.finish(epoch, failed)
            + self.candidates.finish(epoch, failed) + self.values.finish(epoch, failed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ValueQuestion { Result, Expected }

#[derive(Clone, Copy)]
pub(crate) enum ProfileQuery { Headers, Result, Select, Expected, Accept, Match, Infer }

impl ProfileQuery {
    pub(crate) const ALL: &[Self] = &[
        Self::Headers, Self::Result, Self::Select, Self::Expected, Self::Accept, Self::Match, Self::Infer,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Headers => "calls.headers",
            Self::Result => "calls.result",
            Self::Select => "calls.select",
            Self::Expected => "calls.expected",
            Self::Accept => "calls.accept",
            Self::Match => "calls.match",
            Self::Infer => "calls.infer",
        }
    }
}

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
    fn profile<T>(&self, query: ProfileQuery, run: impl FnOnce() -> T) -> T {
        match self.world.profile() {
            Some(profile) => profile.time_semantic_query(query.name(), run),
            None => run(),
        }
    }

    /// One overload set per value alternative; intersections combine headers.
    pub fn headers(&self, value: &NodeValue, kind: InvokeKind) -> Known<Vec<Vec<Header>>> {
        self.profile(ProfileQuery::Headers, || {
            let key = (value.clone(), kind);
            if let Some(answer) = self.semantic.calls.headers.get(&key) {
                return answer;
            }
            let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
            self.semantic.calls.headers.insert(self.semantic.epoch, key.clone(), Known::Unknown);
            let answer = headers::headers(value, kind, &self.semantic.values());
            self.semantic.calls.headers.insert(self.semantic.epoch, key, answer.clone());
            answer
        })
    }

    pub fn selection(&self, call: Leaf) -> Known<Option<Selection>> {
        let Known::Yes(call) = self.semantic.leaf(call) else { return Known::Unknown };
        select::selection(call, &self.semantic.values())
    }

    pub fn argument_sites(&self, formal_binding: Leaf) -> Known<Option<Vec<Leaf>>> {
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        let Known::Yes(formal_binding) = self.semantic.leaf(formal_binding) else { return Known::Unknown };
        select::argument_sites(formal_binding, &self.semantic.values())
    }

    fn candidates(&self, call: Leaf) -> select::Answer {
        self.profile(ProfileQuery::Select, || {
            let Known::Yes(call) = self.semantic.leaf(call) else { return Known::Unknown };
            if let Some(answer) = self.semantic.calls.candidates.get(&call) {
                return answer;
            }
            if !self.question.budget.step() { return Known::Unknown }
            let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
            self.semantic.calls.candidates.insert(self.semantic.epoch, call, Known::Unknown);
            let answer = select::candidates(call, &self.semantic.values());
            self.semantic.calls.candidates.insert(self.semantic.epoch, call, answer.clone());
            answer
        })
    }

    /// Whether every value `actual` may be is one `expected` allows,
    /// memoised per question; a pending comparison (a cycle) is unknown.
    pub fn accepts(&self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        self.profile(ProfileQuery::Accept, || {
            let key = (expected.clone(), actual.clone());
            if let Some(known) = self.semantic.calls.acceptance.get(&key) {
                return known;
            }
            let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
            self.semantic.calls.acceptance.insert(self.semantic.epoch, key.clone(), Known::Unknown);
            let answer = accepts::accepts(expected, actual, &self.semantic.values());
            self.semantic.calls.acceptance.insert(self.semantic.epoch, key, answer);
            answer
        })
    }

    pub fn match_value(&self, at: Leaf, pattern: &Value, actual: &NodeValue, slots: &[Leaf]) -> Known<Match> {
        self.profile(ProfileQuery::Match, || {
            let values = self.semantic.values();
            let Known::Yes(mut run) = accepts::Run::new(&values, slots, &[]) else { return Known::Unknown };
            run.match_value(at, pattern, actual, slots)
        })
    }

    pub fn constrained_arguments(&self, owner: Leaf, through: Option<Leaf>, formals: &[Leaf], arguments: Vec<Option<NodeValue>>, locked: &[bool]) -> Known<Option<Vec<Option<NodeValue>>>> {
        infer::constrained_arguments(owner, through, formals, arguments, locked, &Values { semantic: self.semantic })
    }

    pub fn result(&self, at: Leaf) -> Known<Option<NodeValue>> {
        self.profile(ProfileQuery::Result, || self.value(at, ValueQuestion::Result))
    }

    pub fn expected(&self, at: Leaf) -> Known<Option<NodeValue>> {
        self.profile(ProfileQuery::Expected, || self.value(at, ValueQuestion::Expected))
    }

    pub fn parameters(&self, header: Header) -> Known<Vec<Leaf>> {
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        let values = self.semantic.values();
        let Some((site, signature)) = signature(header, &values) else { return Known::Unknown };
        values.follow(site, &signature.parameters)
    }

    pub fn receiver(&self, header: Header) -> Known<Option<Leaf>> {
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        let values = self.semantic.values();
        let Some((site, signature)) = signature(header, &values) else { return Known::Unknown };
        let Some(path) = &signature.receiver else { return Known::Unknown };
        match values.follow(site, path) {
            Known::Yes(receivers) if receivers.is_empty() => Known::Yes(None),
            Known::Yes(receivers) if receivers.len() == 1 => Known::Yes(Some(receivers[0])),
            _ => Known::Unknown,
        }
    }

    pub fn header_result(&self, header: Header) -> Known<NodeValue> {
        self.profile(ProfileQuery::Result, || {
            let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
            result::header_result(header, &self.semantic.values())
        })
    }

    pub fn header_result_sites(&self, header: Header) -> Known<Vec<Leaf>> {
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        let values = self.semantic.values();
        let Some((site, signature)) = signature(header, &values) else { return Known::Unknown };
        values.sites(site, &signature.result)
    }

    pub fn inferred(&self, at: Leaf) -> Known<Option<NodeValue>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        self.semantic.values().bound_argument(at)
    }

    fn value(&self, at: Leaf, question: ValueQuestion) -> Known<Option<NodeValue>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        let key = (at, question);
        if let Some(answer) = self.semantic.calls.values.get(&key) {
            return answer;
        }
        if !self.question.budget.step() {
            return Known::Unknown;
        }
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        self.semantic.calls.values.insert(self.semantic.epoch, key, Known::Unknown);
        let answer = match question {
            ValueQuestion::Result => result::result(at, &self.semantic.values()),
            ValueQuestion::Expected => expected::expected(at, &self.semantic.values()),
        };
        self.semantic.calls.values.insert(self.semantic.epoch, key, answer.clone());
        answer
    }
}

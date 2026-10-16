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

/// Whether a parameter or argument site is a rest/spread: its shape, which
/// an open (`any`) value does not leave undecided.
fn rest(at: Leaf, values: &Values<'_>) -> Known<bool> {
    values.tables.vocabulary.traits.iter().position(|name| name == "rest")
        .map_or(Known::Yes(false), |id| values.declared_fits(at, id as u16))
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
    /// The value a call passes as its receiver; `None` leaves declared
    /// receivers unchecked.
    pub receiver: Option<Value>,
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
    pub flags: Vec<Conditional<Box<str>>>,
    pub variances: Vec<Conditional<Variance>>,
    pub infer_last: Vec<Option<u32>>,
    pub exact_first: Vec<Option<u32>>,
}

/// A value question's answer: `Yes(None)` when there is none to ask about.
type ValueAnswer = Known<Option<NodeValue>>;
type HeaderAnswer = Known<Vec<Vec<Header>>>;

#[derive(Default)]
pub(crate) struct State {
    pub(super) acceptance: Memo<(NodeValue, NodeValue), Known<bool>>,
    /// Comparisons in progress, outermost first, without incidental frames
    /// ([`Calls::accepts`]).
    accepting: std::cell::RefCell<Vec<(NodeValue, NodeValue)>>,
    /// The outermost in-progress comparison assumed to hold so far.
    assumed: std::cell::Cell<Option<usize>>,
    headers: Memo<(NodeValue, InvokeKind), HeaderAnswer>,
    candidates: Memo<Leaf, select::Answer>,
    values: Memo<(Leaf, ValueQuestion), ValueAnswer>,
}

impl State {
    pub(super) fn finish(&self, epoch: Epoch, failed: bool, cut: u64) -> u64 {
        use super::memo::Settled;
        self.acceptance.finish(epoch, failed, cut, Settled::settled) + self.headers.finish(epoch, failed, cut, |_| false)
            + self.candidates.finish(epoch, failed, cut, |_| false) + self.values.finish(epoch, failed, cut, |_| false)
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
            let memo = &self.semantic.calls.headers;
            let values = self.semantic.values();
            if memo.get(&key).is_some() { return values.memoized(memo, key, || Known::Unknown) }
            let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
            values.memoized(memo, key, || headers::headers(value, kind, &values))
        })
    }

    pub fn selection(&self, call: Leaf) -> Known<Option<Selection>> {
        let Known::Yes(call) = self.semantic.leaf(call) else { return Known::Unknown };
        select::selection(call, &self.semantic.values())
    }

    /// Whether a signature's parameter gathers the remaining arguments.
    pub fn is_rest(&self, parameter: Leaf) -> Known<bool> {
        rest(parameter, &self.semantic.values())
    }

    pub fn argument_sites(&self, formal_binding: Leaf) -> Known<Option<Vec<Leaf>>> {
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        let Known::Yes(formal_binding) = self.semantic.leaf(formal_binding) else { return Known::Unknown };
        select::argument_sites(formal_binding, &self.semantic.values())
    }

    fn candidates(&self, call: Leaf) -> select::Answer {
        self.profile(ProfileQuery::Select, || {
            let Known::Yes(call) = self.semantic.leaf(call) else { return Known::Unknown };
            let memo = &self.semantic.calls.candidates;
            let values = self.semantic.values();
            if memo.get(&call).is_some() { return values.memoized(memo, call, || Known::Unknown) }
            if !self.question.budget.step() { return Known::Unknown }
            let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
            values.memoized(memo, call, || select::candidates(call, &values))
        })
    }

    /// Whether every value `actual` may be is one `expected` allows,
    /// memoised per question. Relations are coinductive (recursive types
    /// like `class C { next: C }`): a comparison met again while in
    /// progress is assumed to hold, and answers resting on an outer
    /// comparison's assumption are kept only once that one is decided.
    pub fn accepts(&self, expected: &NodeValue, actual: &NodeValue) -> Known<bool> {
        self.profile(ProfileQuery::Accept, || {
            let state = &self.semantic.calls;
            let key = (expected.clone(), actual.clone());
            // Recursion through members rebinds frames each round; frames of
            // non-generic declarations bind nothing, so the comparison is
            // recognised without them. Generic instantiations keep their
            // frames (`Box<Box<number>>` vs `Box<Box<string>>` is compared).
            let unbound = (self.without_incidental_frames(expected, false), self.without_incidental_frames(actual, false));
            let assumed = state.accepting.borrow().iter().position(|pending| *pending == unbound);
            if let Some(depth) = assumed {
                state.assumed.set(Some(state.assumed.get().map_or(depth, |outer| outer.min(depth))));
                return Known::Yes(true);
            }
            if let Some(known) = state.acceptance.get(&key) {
                return known;
            }
            let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
            let depth = state.accepting.borrow().len();
            state.accepting.borrow_mut().push(unbound);
            let outer = state.assumed.replace(None);
            let values = self.semantic.values();
            let mark = values.cycle_mark();
            let answer = accepts::accepts(expected, actual, &values);
            state.accepting.borrow_mut().pop();
            let rests_on = state.assumed.get().filter(|&at| at < depth);
            state.assumed.set(match (outer, rests_on) {
                (Some(outer), Some(inner)) => Some(outer.min(inner)),
                (outer, inner) => outer.or(inner),
            });
            let provisional = answer == Known::Yes(true) && rests_on.is_some()
                || answer == Known::Unknown && values.cycled_since(mark);
            if !provisional {
                state.acceptance.insert(self.semantic.epoch, key, answer);
            }
            answer
        })
    }

    /// `value` with the frames of non-generic declarations dropped: they
    /// bind no arguments, so only their identity differs between rounds.
    /// `views`: a frame selecting view classes keeps them, re-rooted (a value
    /// that must keep reading the same view, unlike comparison keys).
    pub(crate) fn without_incidental_frames(&self, value: &NodeValue, views: bool) -> NodeValue {
        let NodeValue::Alternatives(rows) = value else { return value.clone() };
        // Binding unless this frame and every frame it reads through (its
        // parents, its reference's frame) belongs to a declaration with no
        // type parameters and binds no evaluated arguments: a non-generic
        // class inside a generic function still sees the function's.
        let owner_generic = |owner: super::question::NodeRef| {
            let Some(node) = self.semantic.world.tree(owner.file).and_then(|tree| tree.nodes.get(owner.node as usize)) else { return true };
            let Some(kind) = self.semantic.tables.kinds.get(node.matcher as usize) else { return true };
            kind.generics.iter().any(|(path, _)| match path.as_slice() {
                [label] => node.children.iter().any(|edge| edge.label == *label),
                _ => true,
            })
        };
        let generic = |start| {
            let mut pending = vec![start];
            let mut seen = Vec::new();
            while let Some(id) = pending.pop() {
                if id == super::question::FrameId::ROOT || seen.contains(&id) { continue }
                if seen.len() >= 32 { return true }
                seen.push(id);
                let Some(frame) = self.question.get(id) else { return true };
                if frame.args.is_some_and(|args| !self.question.binds_nothing(args)) || owner_generic(frame.owner) { return true }
                pending.push(frame.parent);
                if let Some(through) = frame.through { pending.push(through.frame) }
            }
            false
        };
        let rooted = |frame| match self.question.get(frame) {
            Some(frame) if views && frame.classes != 0 => self.question.bind_definition(super::question::FrameId::ROOT, frame.owner, None, None, frame.classes),
            _ => super::question::FrameId::ROOT,
        };
        NodeValue::Alternatives(rows.iter().map(|row| row.iter().map(|&leaf| {
            if leaf.frame == super::question::FrameId::ROOT || generic(leaf.frame) { leaf } else { Leaf { frame: rooted(leaf.frame), ..leaf } }
        }).collect()).collect())
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
        let memo = &self.semantic.calls.values;
        let values = self.semantic.values();
        if memo.get(&key).is_some() { return values.memoized(memo, key, || Known::Unknown) }
        if !self.question.budget.step() { return Known::Unknown }
        let Some(_nest) = self.question.budget.nest() else { return Known::Unknown };
        values.memoized(memo, key, || match question {
            ValueQuestion::Result => result::result(at, &self.semantic.values()),
            ValueQuestion::Expected => expected::expected(at, &self.semantic.values()),
        })
    }
}

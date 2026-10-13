use std::sync::Arc;
use super::Run;

use crate::grammar::syntax::ParseError;

#[derive(Clone)]
pub(super) enum Expected {
    Static(&'static str),
    Label(Arc<str>),
}

impl Expected {
    fn text(&self) -> &str {
        match self {
            Self::Static(text) => text,
            Self::Label(text) => text,
        }
    }
}

impl PartialEq for Expected {
    fn eq(&self, other: &Self) -> bool { self.text() == other.text() }
}

impl From<&'static str> for Expected {
    fn from(text: &'static str) -> Self { Self::Static(text) }
}

impl From<String> for Expected {
    fn from(text: String) -> Self { Self::Label(text.into()) }
}

impl From<Arc<str>> for Expected {
    fn from(text: Arc<str>) -> Self { Self::Label(text) }
}

#[derive(Clone)]
pub(super) enum Labels {
    One(Expected),
    Many(Arc<Vec<Expected>>),
}

impl Labels {
    fn values(&self) -> &[Expected] {
        match self {
            Self::One(label) => std::slice::from_ref(label),
            Self::Many(labels) => labels,
        }
    }

    fn merge(&mut self, other: &Self) {
        if other.values().iter().all(|label| self.values().contains(label)) { return; }
        if let Self::One(label) = self {
            *self = Self::Many(Arc::new(vec![label.clone()]));
        }
        let Self::Many(labels) = self else { unreachable!() };
        let labels = Arc::make_mut(labels);
        for label in other.values() {
            if !labels.contains(label) { labels.push(label.clone()); }
        }
    }
}

#[derive(Clone)]
pub(super) struct Failure {
    pub(super) offset: u32,
    pub(super) expected: Labels,
    pub(super) committed: bool,
    pub(super) fatal: bool,
}

impl Failure {
    pub fn public(self) -> ParseError {
        ParseError { offset: self.offset, expected: self.expected.values().iter().map(|label| label.text().to_owned()).collect(), committed: self.committed }
    }
}

pub(super) fn error(pos: u32, expected: impl Into<Expected>) -> Failure {
    Failure { offset: pos, expected: Labels::One(expected.into()), committed: false, fatal: false }
}

pub(super) fn merge_error(left: Failure, mut right: Failure) -> Failure {
    if left.fatal { return left; }
    if right.fatal { return right; }
    if left.offset > right.offset { return left; }
    if right.offset > left.offset { return right; }
    right.expected.merge(&left.expected);
    right
}

pub(super) fn resource(pos: u32) -> Failure {
    let mut failure = error(pos, "parser nesting limit");
    failure.committed = true;
    failure.fatal = true;
    failure
}

#[derive(Clone, Copy)]
pub(super) struct ChoiceFailures {
    mark: usize,
    pub(super) first: Option<usize>,
    last: Option<usize>,
}

pub(super) struct Alternative {
    pub(super) next: Option<usize>,
    pub(super) to: usize,
    pub(super) failure: Failure,
}

impl ChoiceFailures {
    pub(super) fn new(run: &Run<'_>) -> Self {
        Self { mark: run.scratch.alternatives.len(), first: None, last: None }
    }

    pub(super) fn clear(self, run: &mut Run<'_>) {
        run.scratch.alternatives.truncate(self.mark);
    }

    pub(super) fn push(&mut self, run: &mut Run<'_>, to: usize, failure: Failure) {
        let id = run.scratch.alternatives.len();
        run.scratch.alternatives.push(Alternative { next: None, to, failure });
        if let Some(last) = self.last { run.scratch.alternatives[last].next = Some(id); }
        self.first = self.first.or(Some(id));
        self.last = Some(id);
    }

}

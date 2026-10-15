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

impl From<&Arc<str>> for Expected {
    fn from(text: &Arc<str>) -> Self { Self::Label(text.clone()) }
}

/// A failed match: where it failed and how it binds enclosing choices.
/// Match failures leave what was expected in the run's [`Farthest`], so
/// failing (constant while parsing valid input) allocates nothing; only
/// failures outside matching carry their own label.
#[derive(Clone)]
pub(super) struct Failure {
    pub(super) offset: u32,
    pub(super) committed: bool,
    pub(super) fatal: bool,
    own: Option<Expected>,
}

/// The farthest offset any match failed at and every label expected there:
/// what a failed parse reports.
#[derive(Default)]
pub(super) struct Farthest {
    offset: u32,
    labels: Vec<Expected>,
}

impl Farthest {
    fn record<L: Into<Expected>>(&mut self, offset: u32, label: L) {
        if !self.labels.is_empty() && offset < self.offset { return; }
        let label = label.into();
        if self.labels.is_empty() || offset > self.offset {
            self.offset = offset;
            self.labels.clear();
            self.labels.push(label);
        } else if !self.labels.contains(&label) {
            self.labels.push(label);
        }
    }

    pub(super) fn clear(&mut self) {
        self.offset = 0;
        self.labels.clear();
    }
}

impl Failure {
    /// Whether this failure reports the farthest match failure, which only a
    /// diagnosing run records.
    pub(super) fn from_matching(&self) -> bool { self.own.is_none() }
}

impl Run<'_> {
    /// A match failure at `pos`, expecting `label` there.
    pub(super) fn fail<L: Into<Expected>>(&mut self, pos: u32, label: L) -> Failure {
        if self.diagnose { self.scratch.farthest.record(pos, label); }
        Failure { offset: pos, committed: false, fatal: false, own: None }
    }

    /// The public error for the failure a parse ended with.
    pub(super) fn public(&self, failure: Failure) -> ParseError {
        let farthest = &self.scratch.farthest;
        let (offset, expected) = match &failure.own {
            Some(label) => (failure.offset, vec![label.text().to_owned()]),
            None => {
                let mut expected: Vec<String> = farthest.labels.iter().map(|label| label.text().to_owned()).collect();
                expected.sort_unstable();
                (farthest.offset.max(failure.offset), expected)
            }
        };
        ParseError { offset, expected, committed: failure.committed }
    }
}

/// A failure outside matching (setup, registration, tree building, the
/// nesting limit), reporting its own label rather than the farthest match.
pub(super) fn error(pos: u32, expected: impl Into<Expected>) -> Failure {
    Failure { offset: pos, committed: false, fatal: false, own: Some(expected.into()) }
}

/// Which of two failures an enclosing choice reports: fatal first, then
/// the farther one (the right one on ties).
pub(super) fn merge_error(left: Failure, right: Failure) -> Failure {
    if left.fatal { return left; }
    if right.fatal { return right; }
    if left.offset > right.offset { return left; }
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

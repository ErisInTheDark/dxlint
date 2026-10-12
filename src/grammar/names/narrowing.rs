//! Narrowing: `propagates`, `truthy`, `excludes` / `includes` / `has`
//! (docs/design/type-layers.md "Narrowing"). Owner: Iris.

use super::effects::{Event, Execution, StepId};
use super::question::{Known, Leaf, NodeValue, QueryContext};
use super::semantic::Semantic;
use super::values::Key;

pub struct Narrowing<'a> {
    #[expect(dead_code, reason = "refine is a stub until guards run at runtime")]
    pub(crate) semantic: &'a Semantic<'a>,
}

impl Narrowing<'_> {
    pub fn refine(&self, _at: Leaf, base: NodeValue) -> NodeValue {
        base
    }
}

/// A narrowed place: a root binding and the member path below it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "places are built once runtime narrowing lands"))]
struct Place {
    root: Leaf,
    path: Vec<Key>,
}

impl Place {
    /// Whether writing `self` makes a guard on `guarded` stale.
    #[cfg_attr(not(test), expect(dead_code, reason = "used once runtime narrowing lands"))]
    fn invalidates(&self, guarded: &Place) -> bool {
        self.root == guarded.root && guarded.path.starts_with(&self.path)
    }
}

/// Whether every path from the entry to `at` passes one of the guard
/// `entries` with no `writes` event after it (a fresh guard resets).
pub fn guarded(
    execution: &Execution,
    entries: &[StepId],
    at: StepId,
    writes: impl Fn(&Event) -> bool,
    question: &QueryContext,
) -> Known<bool> {
    let budget = &question.budget;
    match execution.reaches(execution.entry(), at, &[], budget) {
        Known::Yes(true) => {}
        other => return other,
    }
    match execution.reaches(execution.entry(), at, entries, budget) {
        Known::Yes(false) => {}
        Known::Yes(true) => return Known::Yes(false),
        Known::Unknown => return Known::Unknown,
    }
    for &entry in entries {
        match execution.reaches(execution.entry(), entry, &[], budget) {
            Known::Yes(true) => {}
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
        }
        match execution.reaches_through(entry, at, entries, &writes, budget) {
            Known::Yes(false) => {}
            Known::Yes(true) => return Known::Yes(false),
            Known::Unknown => return Known::Unknown,
        }
    }
    Known::Yes(true)
}

#[cfg(test)]
mod tests;

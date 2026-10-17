use super::{ExpectedAt, kind};
use crate::grammar::names::question::{Known, Leaf, NodeValue};
use crate::grammar::names::values::{Key, Values};

pub(super) fn expected(at: Leaf, values: &Values<'_>) -> Known<Option<NodeValue>> {
    match declared(at, at, true, values) {
        Known::Yes(None) => {}
        answer => return answer,
    }
    let parent = match values.semantic.parent(at) {
        Known::Yes(Some(parent)) => parent,
        Known::Yes(None) => return Known::Yes(None),
        Known::Unknown => return Known::Unknown,
    };
    let Some(kind) = kind(parent, values) else { return Known::Unknown };
    match declared(parent, at, false, values) {
        Known::Yes(None) => {}
        answer => return answer,
    }
    if kind.calls.invokes.is_empty() {
        // An argument may sit one level down (a tagged template's
        // substitutions live in its template): the grandparent invokes.
        let grandparent = match values.semantic.parent(parent) {
            Known::Yes(Some(grandparent)) => grandparent,
            Known::Yes(None) => return Known::Yes(None),
            Known::Unknown => return Known::Unknown,
        };
        // A grandparent of no named kind invokes nothing.
        let Some(outer) = super::kind(grandparent, values) else { return Known::Yes(None) };
        if outer.calls.invokes.is_empty() { return Known::Yes(None) }
        return argument_expected(grandparent, outer, at, values);
    }
    argument_expected(parent, kind, at, values)
}

/// The expected value of `at` as an argument of the invocation `parent`
/// (its parameter in the selected signature); none when it isn't one.
fn argument_expected(parent: Leaf, kind: &crate::grammar::names::KindNames, at: Leaf, values: &Values<'_>) -> Known<Option<NodeValue>> {
    // Only arguments get an expected type from the selected signature; the
    // callee must not ask for that selection (an IIFE would ask itself).
    let mut argument = false;
    for invoke in &kind.calls.invokes {
        if !values.question.budget.step() { return Known::Unknown }
        match invoke.when.map_or(Known::Yes(true), |when| values.world.condition(parent, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => {}
        }
        let Known::Yes(sites) = values.follow(parent, &invoke.item.arguments) else { return Known::Unknown };
        argument |= sites.contains(&at);
    }
    if !argument { return Known::Yes(None) }
    let selected = match values.calls().selection(parent) {
        Known::Yes(Some(selected)) if !selected.uncertain => selected,
        Known::Yes(None) => return Known::Yes(None),
        _ => return Known::Unknown,
    };
    let mut answer = None;
    let mut matched = None;
    for chosen in selected.choices {
        let mut candidate = None;
        for (formal, actuals) in chosen.mapping {
            if !values.question.budget.step() { return Known::Unknown }
            let Some(position) = actuals.iter().position(|&actual| actual == at) else { continue };
            if candidate.is_some() { return Known::Unknown }
            let rest = values.tables.vocabulary.traits.iter().position(|name| name == "rest")
                .map_or(Known::Yes(false), |id| values.fits(formal, id as u16));
            let expected = match rest {
                Known::Yes(false) => values.values(formal),
                Known::Yes(true) => values.member(&values.values(formal), &Key::Ordinal(position as u32)),
                Known::Unknown => return Known::Unknown,
            };
            candidate = Some(expected);
        }
        let present = candidate.is_some();
        if matched.is_some_and(|previous| previous != present) { return Known::Unknown }
        matched = Some(present);
        match candidate {
            Some(value) => {
                if !agree(&mut answer, value, values) { return Known::Unknown }
            }
            None => {}
        }
    }
    Known::Yes(answer)
}

fn declared(owner: Leaf, at: Leaf, own: bool, values: &Values<'_>) -> Known<Option<NodeValue>> {
    let Some(kind) = kind(owner, values) else { return Known::Unknown };
    let mut answer = None;
    for expected in &kind.calls.expected {
        if own != (expected.item.field.as_slice() == [super::super::IT]) { continue }
        if !values.question.budget.step() { return Known::Unknown }
        match expected.when.map_or(Known::Yes(true), |when| values.world.condition(owner, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => {}
        }
        let Known::Yes(sites) = values.follow(owner, &expected.item.field) else { return Known::Unknown };
        if !sites.contains(&at) { continue }
        let subject = match expected.item.at {
            ExpectedAt::Owner => owner,
            ExpectedAt::Subject => at,
        };
        match values.evaluate_optional(subject, &expected.item.value) {
            Known::Yes(Some(value)) => {
                if !agree(&mut answer, value, values) { return Known::Unknown }
            }
            Known::Yes(None) => {}
            Known::Unknown => return Known::Unknown,
        }
    }
    Known::Yes(answer)
}

fn agree(answer: &mut Option<NodeValue>, value: NodeValue, values: &Values<'_>) -> bool {
    if value == NodeValue::Unknown { return false }
    if let Some(previous) = answer {
        previous == &value || values.calls().accepts(previous, &value) == Known::Yes(true)
            && values.calls().accepts(&value, previous) == Known::Yes(true)
    } else {
        *answer = Some(value);
        true
    }
}

use super::{ExpectedAt, kind};
use crate::grammar::names::question::{Known, Leaf, NodeValue};
use crate::grammar::names::values::{Key, Values};

pub(super) fn expected(at: Leaf, values: &Values<'_>) -> Known<Option<NodeValue>> {
    let parent = match values.semantic.parent(at) {
        Known::Yes(parent) => parent,
        Known::Unknown => return Known::Unknown,
    };
    // A parameter's context comes from its function, whatever frame the
    // asking read sees the parameter in.
    if let Some(parent) = parent {
        match parameter_expected(parent, at, values) {
            Known::Yes(None) => {}
            answer => return answer,
        }
    }
    match declared(at, at, true, values) {
        Known::Yes(None) => {}
        answer => return answer,
    }
    let Some(parent) = parent else { return Known::Yes(None) };
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
    // Asked while the call infers from this very argument: TypeScript types
    // an argument that isn't context-sensitive (no callback) without a
    // contextual type then (`new Set([1, 2])`: `number[]`). A callback
    // waits for the call instead (the second pass).
    if values.calls().selecting(parent) {
        let callable = values.tables.vocabulary.traits.iter().position(|name| name == "callable")
            .map_or(Known::Yes(false), |id| values.declared_fits(at, id as u16));
        if callable == Known::Yes(false) { return Known::Yes(None) }
    }
    // The first pass's: an argument it couldn't relate (a callback) is
    // typed by it, then related again ([`super::select::refined`]).
    let selected = match values.calls().context(parent) {
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

/// A parameter's counterpart in its function's expected callable, by
/// position (a rest formal gives each later parameter its element). None
/// when `at` is no parameter of `parent`, `parent` expects no callable, or
/// that callable has several signatures (TypeScript then types nothing).
fn parameter_expected(parent: Leaf, at: Leaf, values: &Values<'_>) -> Known<Option<NodeValue>> {
    // The parameter's position in `parent`'s own signature, read from syntax
    // (its value would ask its parameters' values: a cycle).
    let Some(kind) = kind(parent, values) else { return Known::Yes(None) };
    let mut position = None;
    for signature in &kind.calls.signatures {
        if !values.question.budget.step() { return Known::Unknown }
        // Syntax first: only a parameter asks the signature's condition.
        let Known::Yes(parameters) = values.follow(parent, &signature.item.parameters) else { return Known::Unknown };
        let Some(found) = parameters.iter().position(|parameter| parameter.target == at.target) else { continue };
        match signature.when.map_or(Known::Yes(true), |when| values.world.condition(parent, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => {}
        }
        if position.is_some_and(|previous| previous != found) { return Known::Unknown }
        position = Some(found);
    }
    let Some(position) = position else { return Known::Yes(None) };
    let calls = values.calls();
    // A read in the function's body sees its parameters through the
    // function's own frame; its context belongs to where it is written.
    // Frames binding no arguments (a definition's own, a reference's) are
    // dropped up to one that binds arguments.
    let mut written = parent;
    while written.frame != crate::grammar::names::question::FrameId::ROOT {
        let Some(frame) = values.question.get(written.frame) else { break };
        if frame.args.is_some() { break }
        written.frame = frame.parent;
    }
    let rest_trait = values.tables.vocabulary.traits.iter().position(|name| name == "rest");
    let is_rest = |leaf: Leaf| rest_trait.map_or(Known::Yes(false), |id| values.fits(leaf, id as u16));
    let own_rest = match is_rest(at) {
        Known::Yes(rest) => rest,
        Known::Unknown => return Known::Unknown,
    };
    let expected = match calls.expected(written) {
        Known::Yes(Some(expected)) => expected,
        Known::Yes(None) => return Known::Yes(None),
        Known::Unknown => return Known::Unknown,
    };
    let Known::Yes(groups) = calls.headers(&expected, super::InvokeKind::Call) else { return Known::Unknown };
    let mut headers = groups.iter().flatten();
    let (Some(&header), None) = (headers.next(), headers.next()) else { return Known::Yes(None) };
    let Known::Yes(formals) = calls.parameters(header) else { return Known::Unknown };
    for (index, &formal) in formals.iter().enumerate() {
        if !values.question.budget.step() { return Known::Unknown }
        match is_rest(formal) {
            // A rest parameter takes a rest formal's whole array; positional
            // formals before it would make a tuple (not built here).
            Known::Yes(true) if own_rest => {
                return Known::Yes((index == position).then(|| values.values(formal)));
            }
            _ if own_rest => {}
            Known::Yes(true) if index <= position => {
                let element = (position - index) as u32;
                return Known::Yes(Some(values.member(&values.values(formal), &Key::Ordinal(element))));
            }
            Known::Unknown => return Known::Unknown,
            _ if index == position => return Known::Yes(Some(values.values(formal))),
            _ => {}
        }
    }
    Known::Yes(None)
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

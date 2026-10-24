use super::{kind, node, Header, ProfileQuery, Value};
use crate::grammar::names::question::{Known, Leaf, NodeValue};
use crate::grammar::names::values::Values;

/// One instantiation of a signature at a call.
pub(super) struct Instantiated {
    pub(super) header: Header,
    /// Whether every argument fits its instantiated parameter.
    pub(super) fits: Known<bool>,
    /// An argument could not be related yet (a callback typed by this very
    /// call) while a formal is left to it: the [`Pass::Result`] pass, once
    /// it is typed, infers that formal.
    pub(super) pending: bool,
}

/// TypeScript's two inference passes over a call.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    /// Types the arguments: a formal left to a callback stays an open
    /// parameter, the callback's contextual type.
    Context,
    /// Types the result once callbacks are typed: what stays unbound is
    /// unknown.
    Result,
}

/// The header with its formals inferred from the call; `None` when an
/// argument cannot fit it. `check`: whether the arguments' fit is asked
/// ([`Instantiated::fits`], else unknown): a lone overload's is not.
pub(super) fn instantiate(header: Header, call: Leaf, mapping: &[(Leaf, Vec<Leaf>)], explicit: &[NodeValue], pass: Pass, check: bool, values: &Values<'_>) -> Known<Option<Instantiated>> {
    values.calls().profile(ProfileQuery::Infer, || {
        let Known::Yes(site) = values.semantic.leaf(header.site) else { return Known::Unknown };
        let header = Header { site, ..header };
        let Some(owner) = node(header.site, values) else { return Known::Unknown };
        let Known::Yes(classes) = values.question.owner_classes(header.site.frame, owner) else { return Known::Unknown };
        let Known::Yes(formals) = formals(header.site, values) else { return Known::Unknown };
        if !formals.is_empty() && crate::levers::on(crate::levers::Lever::NoInference) { return Known::Unknown }
        let mut contributions = Vec::new();
        for (position, formal) in formals.iter().enumerate() {
            let supplied = match explicit.get(position) {
                Some(value) => {
                    let Known::Yes(value) = copy_value(value, values) else { return Known::Unknown };
                    Some(value)
                }
                None => match values.bound_argument(*formal) {
                    Known::Yes(value) => value,
                    Known::Unknown => Some(NodeValue::Unknown),
                },
            };
            contributions.push(supplied);
        }
        let locked: Vec<_> = contributions.iter().map(Option::is_some).collect();
        let Known::Yes(mut run) = super::accepts::Run::new(values, &formals, &contributions) else { return Known::Unknown };
        // An argument that could not be related (a callback typed by this
        // call) may still infer what is unbound: defaults never stand in for
        // it, the second pass (`Instantiated::pending`) decides.
        let mut deferred = false;
        if contributions.iter().any(Option::is_none) {
            for (parameter, actuals) in mapping {
                if !values.question.budget.step() { return Known::Unknown }
                let expected = run.recipe(*parameter, &Value::Current);
                if let Some(packed) = packed_rest(*parameter, actuals, &expected, &formals, call, values) {
                    run.deferring(false);
                    let _ = run.relate_or_skip(&expected, &packed);
                    continue;
                }
                for &actual in actuals {
                    if !values.question.budget.step() { return Known::Unknown }
                    let actual = values.values(actual);
                    // A callback may wait on this very call (its parameters
                    // typed by it): what it cannot relate yet contributes
                    // nothing, the other arguments decide and the second pass
                    // relates it again. Any other argument's unknown parts
                    // leave what they reach unknown.
                    let callback = callable(&actual, values);
                    run.deferring(callback);
                    deferred |= run.relate_or_skip(&expected, &actual) == Known::Unknown && callback;
                }
            }
        }
        let Known::Yes(contributions) = run.arguments() else { return Known::Unknown };
        let pending = deferred && contributions.iter().any(Option::is_none);
        let mut slots = Vec::new();
        let mut defaulted = Vec::new();
        for (&formal, contribution) in formals.iter().zip(contributions) {
            if !values.question.budget.step() { return Known::Unknown }
            let mut from_default = false;
            let value = if let Some(value) = contribution { value } else {
                let Known::Yes(prefix) = copy_slots(&slots, values) else { return Known::Unknown };
                let frame = values.question.bind_slots(header.site.frame, owner, Some(call), prefix.into_iter().map(Some).collect(), classes);
                let formal = Leaf { frame, ..formal };
                // Waiting on a callback in the first pass: an open parameter
                // (itself and its bound) that types the callback
                // ([`Pass::Context`]). Built directly: its own value would ask
                // this call's selection for what it infers, a cycle.
                if pass == Pass::Context && pending {
                    let bound = match values.constraint(formal) {
                        Known::Yes(bound) => bound,
                        Known::Unknown => return Known::Unknown,
                    };
                    slots.push(values.combine([NodeValue::leaf(formal)].into_iter().chain(bound), true));
                    defaulted.push(false);
                    continue;
                }
                // Left unbound by the arguments: its default, as in
                // TypeScript, instantiated by the formals before it. A call
                // with a contextual type leaves it to that context instead.
                let value = match values.default_argument(formal) {
                    Known::Yes(Some(value)) if !pending && values.calls().expected(call) == Known::Yes(None) => {
                        from_default = true;
                        value
                    }
                    Known::Yes(_) => values.values(formal),
                    Known::Unknown => return Known::Unknown,
                };
                // Unbound: unknown.
                if matches!(&value, NodeValue::Alternatives(arms) if arms.iter().any(|arm| arm.contains(&formal))) {
                    NodeValue::Unknown
                } else {
                    value
                }
            };
            slots.push(value);
            defaulted.push(from_default);
        }
        // A default already satisfies its bound (TypeScript checks that at
        // the declaration): only the other slots are checked.
        let (checked, defaults): (Vec<_>, Vec<_>) = slots.into_iter().zip(defaulted)
            .map(|(slot, from_default)| if from_default { (None, Some(slot)) } else { (Some(slot), None) }).unzip();
        let checked = match constrained_arguments(header.site, Some(call), &formals, checked, &locked, values) {
            Known::Yes(Some(slots)) => slots,
            Known::Yes(None) => return Known::Yes(None),
            Known::Unknown => return Known::Unknown,
        };
        let slots = checked.into_iter().zip(defaults).map(|(slot, default)| default.or(slot)).collect();
        // Nothing to instantiate: the header's own frame, so a non-generic
        // signature has one result for every call site, as in TypeScript.
        let frame = if formals.is_empty() { header.site.frame }
            else { values.question.bind_slots(header.site.frame, owner, Some(call), slots, classes) };
        let mut fits = if check { Known::Yes(true) } else { Known::Unknown };
        for (parameter, actuals) in if check { mapping } else { &[] } {
            let parameter = Leaf { frame, ..*parameter };
            let Known::Yes(is_rest) = super::rest(parameter, values) else { return Known::Unknown };
            for (offset, &actual) in actuals.iter().enumerate() {
                let expected = if is_rest {
                    values.member(&values.values(parameter), &crate::grammar::names::values::Key::Ordinal(offset as u32))
                } else {
                    values.values(parameter)
                };
                match values.calls().accepts(&expected, &values.values(actual)) {
                    Known::Yes(false) => return Known::Yes(None),
                    Known::Unknown => fits = Known::Unknown,
                    Known::Yes(true) => {}
                }
            }
        }
        Known::Yes(Some(Instantiated { header: Header { site: Leaf { frame, ..header.site }, ..header }, fits, pending }))
    })
}

/// Whether every alternative of `value` is callable (a function argument).
fn callable(value: &NodeValue, values: &Values<'_>) -> bool {
    let Some(id) = values.tables.vocabulary.traits.iter().position(|name| name == "callable") else { return false };
    matches!(value, NodeValue::Alternatives(rows) if !rows.is_empty()
        && rows.iter().all(|row| row.iter().any(|&leaf| values.fits(leaf, id as u16) == Known::Yes(true))))
}

/// The arguments of a rest parameter typed by a bare formal
/// (`...args: A`), packed into one tuple as TypeScript infers `A`; `None`
/// for any other parameter, whose arguments relate one by one.
fn packed_rest(parameter: Leaf, actuals: &[Leaf], expected: &NodeValue, formals: &[Leaf], call: Leaf, values: &Values<'_>) -> Option<NodeValue> {
    let NodeValue::Alternatives(rows) = expected else { return None };
    let [row] = rows.as_slice() else { return None };
    let [leaf] = row.as_slice() else { return None };
    if !formals.iter().any(|formal| formal.target == leaf.target) { return None }
    if super::rest(parameter, values) != Known::Yes(true) { return None }
    let Known::Yes(Some(shape)) = values.pack_shape(parameter) else { return None };
    let elements = actuals.iter().map(|&site| crate::grammar::names::values::products::ProductElement {
        site, value: values.values(site), optional: false, repeated: false, values: false,
    }).collect();
    Some(values.product(shape, call, elements, false))
}

pub(super) fn constrained_arguments(at: Leaf, through: Option<Leaf>, formals: &[Leaf], mut arguments: Vec<Option<NodeValue>>, locked: &[bool], values: &Values<'_>) -> Known<Option<Vec<Option<NodeValue>>>> {
    if formals.len() != arguments.len() || locked.len() != arguments.len() { return Known::Unknown }
    if arguments.is_empty() { return Known::Yes(Some(arguments)) }
    let Some(owner) = node(at, values) else { return Known::Unknown };
    let Known::Yes(classes) = values.question.owner_classes(at.frame, owner) else { return Known::Unknown };
    let frame = values.question.bind_slots(at.frame, owner, through, arguments.clone(), classes);
    let mut changed = false;
    for (position, formal) in formals.iter().enumerate() {
        let Some(argument) = &arguments[position] else { continue };
        let Known::Yes(bound) = values.constraint(Leaf { frame, ..*formal }) else { return Known::Unknown };
        let Some(bound) = bound else { continue };
        if dynamic_fits(&bound, argument, values) { continue }
        match values.calls().accepts(&bound, argument) {
            Known::Yes(true) => {}
            Known::Unknown if locked[position] => return Known::Unknown,
            Known::Yes(false) if locked[position] => return Known::Yes(None),
            // An inferred argument its constraint doesn't (provably) accept
            // falls back to the constraint, as TypeScript does: the call
            // keeps its declared contract (`takes<T extends E>(x: T)` with
            // `takes(1)` checks `1` against `E`). Whether the argument fits
            // is decided against that contract afterwards.
            Known::Unknown | Known::Yes(false) => { arguments[position] = Some(bound); changed = true; }
        }
    }
    if changed {
        let frame = values.question.bind_slots(at.frame, owner, through, arguments.clone(), classes);
        for (position, formal) in formals.iter().enumerate() {
            let Some(argument) = &arguments[position] else { continue };
            let Known::Yes(bound) = values.constraint(Leaf { frame, ..*formal }) else { return Known::Unknown };
            let Some(bound) = bound else { continue };
            if dynamic_fits(&bound, argument, values) { continue }
            match values.calls().accepts(&bound, argument) {
                Known::Yes(true) => {}
                Known::Yes(false) if locked[position] => return Known::Yes(None),
                _ => return Known::Unknown,
            }
        }
    }
    Known::Yes(Some(arguments))
}

/// Whether `argument` is wholly dynamic (`any`) and `bound` isn't `never`:
/// `any` satisfies every other constraint and stays the argument.
fn dynamic_fits(bound: &NodeValue, argument: &NodeValue, values: &Values<'_>) -> bool {
    *bound != NodeValue::never() && matches!(argument, NodeValue::Alternatives(arms) if !arms.is_empty()
        && arms.iter().all(|arm| !arm.is_empty() && arm.iter().all(|&leaf| values.dynamic(leaf) == Known::Yes(true))))
}

fn copy_slots(slots: &[NodeValue], values: &Values<'_>) -> Known<Vec<NodeValue>> {
    let mut copied = Vec::new();
    for slot in slots {
        let Known::Yes(slot) = copy_value(slot, values) else { return Known::Unknown };
        copied.push(slot);
    }
    Known::Yes(copied)
}

fn copy_value(value: &NodeValue, values: &Values<'_>) -> Known<NodeValue> {
    if !values.question.budget.step() { return Known::Unknown }
    if let NodeValue::Alternatives(arms) = value {
        for arm in arms {
            if !values.question.budget.step() { return Known::Unknown }
            for _ in arm {
                if !values.question.budget.step() { return Known::Unknown }
            }
        }
    }
    Known::Yes(value.clone())
}

pub(super) fn formals(at: Leaf, values: &Values<'_>) -> Known<Vec<Leaf>> {
    let Known::Yes(at) = values.semantic.leaf(at) else { return Known::Unknown };
    let Some(kind) = kind(at, values) else { return Known::Unknown };
    let mut formals = None;
    for (path, when) in &kind.generics {
        match when.map_or(Known::Yes(true), |when| values.world.condition(at, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => {}
        }
        if formals.is_some() { return Known::Unknown }
        let Known::Yes(found) = values.follow(at, path) else { return Known::Unknown };
        formals = Some(found);
    }
    Known::Yes(formals.unwrap_or_default())
}

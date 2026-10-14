use super::{kind, node, Header, ProfileQuery, Value};
use crate::grammar::names::question::{Known, Leaf, NodeValue};
use crate::grammar::names::values::Values;

/// The header with its formals inferred from the call, and whether every
/// argument then fits its instantiated parameter; `None` when one cannot.
pub(super) fn instantiate(header: Header, call: Leaf, mapping: &[(Leaf, Vec<Leaf>)], explicit: &[NodeValue], values: &Values<'_>) -> Known<Option<(Header, Known<bool>)>> {
    values.calls().profile(ProfileQuery::Infer, || {
        let Known::Yes(site) = values.semantic.leaf(header.site) else { return Known::Unknown };
        let header = Header { site, ..header };
        let Some(owner) = node(header.site, values) else { return Known::Unknown };
        let Known::Yes(classes) = values.question.owner_classes(header.site.frame, owner) else { return Known::Unknown };
        let Known::Yes(formals) = formals(header.site, values) else { return Known::Unknown };
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
        if contributions.iter().any(Option::is_none) {
            for (parameter, actuals) in mapping {
                for &actual in actuals {
                    if !values.question.budget.step() { return Known::Unknown }
                    let expected = run.recipe(*parameter, &Value::Current);
                    let actual = values.values(actual);
                    if run.relate(&expected, &actual) == Known::Unknown { return Known::Unknown }
                }
            }
        }
        let Known::Yes(contributions) = run.arguments() else { return Known::Unknown };
        let mut slots = Vec::new();
        for (&formal, contribution) in formals.iter().zip(contributions) {
            if !values.question.budget.step() { return Known::Unknown }
            let value = if let Some(value) = contribution { value } else {
                let Known::Yes(prefix) = copy_slots(&slots, values) else { return Known::Unknown };
                let frame = values.question.bind_slots(header.site.frame, owner, Some(call), prefix.into_iter().map(Some).collect(), classes);
                let formal = Leaf { frame, ..formal };
                let value = values.values(formal);
                if matches!(&value, NodeValue::Alternatives(arms) if arms.iter().any(|arm| arm.contains(&formal))) {
                    NodeValue::Unknown
                } else {
                    value
                }
            };
            slots.push(value);
        }
        let slots = slots.into_iter().map(Some).collect();
        let slots = match constrained_arguments(header.site, Some(call), &formals, slots, &locked, values) {
            Known::Yes(Some(slots)) => slots,
            Known::Yes(None) => return Known::Yes(None),
            Known::Unknown => return Known::Unknown,
        };
        let frame = values.question.bind_slots(header.site.frame, owner, Some(call), slots, classes);
        let mut fits = Known::Yes(true);
        for (parameter, actuals) in mapping {
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
        Known::Yes(Some((Header { site: Leaf { frame, ..header.site }, ..header }, fits)))
    })
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
        match values.calls().accepts(&bound, argument) {
            Known::Yes(true) => {}
            Known::Unknown => return Known::Unknown,
            Known::Yes(false) if locked[position] => return Known::Yes(None),
            Known::Yes(false) => { arguments[position] = Some(bound); changed = true; }
        }
    }
    if changed {
        let frame = values.question.bind_slots(at.frame, owner, through, arguments.clone(), classes);
        for (position, formal) in formals.iter().enumerate() {
            let Some(argument) = &arguments[position] else { continue };
            let Known::Yes(bound) = values.constraint(Leaf { frame, ..*formal }) else { return Known::Unknown };
            let Some(bound) = bound else { continue };
            match values.calls().accepts(&bound, argument) {
                Known::Yes(true) => {}
                Known::Yes(false) if locked[position] => return Known::Yes(None),
                _ => return Known::Unknown,
            }
        }
    }
    Known::Yes(Some(arguments))
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

use super::{kind, rest, Header, InvokeKind};
use crate::config::ast::calls::Variance;
use crate::grammar::names::question::{Known, Leaf, NodeValue};
use crate::grammar::names::values::Values;

pub(super) fn accepts(expected: &NodeValue, actual: &NodeValue, run: &mut super::accepts::Run<'_, '_>) -> Known<Option<bool>> {
    let values = run.values;
    let mut found = false;
    let mut unknown = false;
    for (name, invoke) in [("callable", InvokeKind::Call), ("constructible", InvokeKind::Construct)] {
        let bit = values.tables.vocabulary.trait_bit(name);
        if bit == 0 { continue }
        match values.could(expected, bit) {
            Known::Yes(false) => continue,
            Known::Unknown => { unknown = true; continue }
            Known::Yes(true) => found = true,
        }
        let Known::Yes(expected) = values.calls().headers(expected, invoke) else { return Known::Unknown };
        let Known::Yes(actual) = values.calls().headers(actual, invoke) else { return Known::Unknown };
        for required in expected.into_iter().flatten() {
            for alternatives in &actual {
                let mut choices = alternatives.as_slice();
                if run.mode() == super::accepts::Mode::Infer {
                    let mut last = false;
                    for &available in alternatives {
                        match infer_last(available.site, values) {
                            Known::Yes(true) => last = true,
                            Known::Unknown => return Known::Unknown,
                            Known::Yes(false) => {}
                        }
                    }
                    if last { choices = choices.last().map_or(&[], std::slice::from_ref) }
                }
                match run.choose(choices, |run, &available| header(required, available, run)) {
                    Known::Yes(false) => return Known::Yes(Some(false)),
                    Known::Unknown => unknown = true,
                    Known::Yes(true) => {}
                }
            }
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(found.then_some(true)) }
}

fn infer_last(at: Leaf, values: &Values<'_>) -> Known<bool> {
    let Some(kind) = kind(at, values) else { return Known::Unknown };
    let mut unknown = false;
    for &when in &kind.calls.infer_last {
        if !values.question.budget.step() { return Known::Unknown }
        match when.map_or(Known::Yes(true), |when| values.world.condition(at, when, values.semantic)) {
            Known::Yes(true) => return Known::Yes(true),
            Known::Unknown => unknown = true,
            Known::Yes(false) => {}
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(false) }
}

fn header(expected: Header, actual: Header, run: &mut super::accepts::Run<'_, '_>) -> Known<bool> {
    let values = run.values;
    let calls = values.calls();
    let Known::Yes(expected_parameters) = calls.parameters(expected) else { return Known::Unknown };
    let Known::Yes(actual_parameters) = calls.parameters(actual) else { return Known::Unknown };
    let Known::Yes(bivariant) = bivariant(expected.site, values) else { return Known::Unknown };
    let expected_source = expected.site;
    let actual_source = actual.site;
    let mut unknown = false;
    match (calls.receiver(expected), calls.receiver(actual)) {
        (Known::Yes(None), Known::Yes(None)) => {}
        (Known::Yes(Some(expected)), Known::Yes(Some(actual))) => {
            let wanted = run.recipe(actual, &super::Value::Current);
            let provided = run.recipe(expected, &super::Value::Current);
            match run.relate(&wanted, &provided) {
                Known::Yes(false) => return Known::Yes(false),
                Known::Unknown => unknown = true,
                Known::Yes(true) => {}
            }
        }
        _ => unknown = true,
    }
    for position in 0..actual_parameters.len().max(expected_parameters.len()) {
        if !values.question.budget.step() { return Known::Unknown }
        let actual = actual_parameters.get(position).copied();
        let expected = expected_parameters.get(position).copied();
        let Known::Yes(expected_rest) = expected.map_or(Known::Yes(false), |site| rest(site, values)) else { return Known::Unknown };
        let Known::Yes(actual_rest) = actual.map_or(Known::Yes(false), |site| rest(site, values)) else { return Known::Unknown };
        if expected_rest || actual_rest {
            let (provided, wanted) = if expected_rest {
                if position + 1 != expected_parameters.len() { return Known::Unknown }
                let Some(expected) = expected else { return Known::Unknown };
                (run.recipe(expected, &super::Value::Current), pack(actual_parameters.get(position..).unwrap_or_default(), actual_source, expected, run))
            } else {
                if position + 1 != actual_parameters.len() { return Known::Unknown }
                let Some(actual) = actual else { return Known::Unknown };
                (pack(expected_parameters.get(position..).unwrap_or_default(), expected_source, actual, run), run.recipe(actual, &super::Value::Current))
            };
            match parameter(&provided, &wanted, bivariant, run) {
                Known::Yes(false) => return Known::Yes(false),
                Known::Unknown => unknown = true,
                Known::Yes(true) => {}
            }
            break;
        }
        let Some(actual) = actual else { continue };
        let optional = values.optional(actual);
        let Some(expected) = expected else {
            match optional {
                Known::Yes(true) => continue,
                Known::Yes(false) => return Known::Yes(false),
                Known::Unknown => { unknown = true; continue }
            }
        };
        match values.optional(expected) {
            Known::Yes(true) if optional == Known::Yes(false) => return Known::Yes(false),
            Known::Unknown => unknown = true,
            Known::Yes(true) if optional == Known::Unknown => unknown = true,
            _ => {}
        }
        let wanted = run.recipe(actual, &super::Value::Current);
        let provided = run.recipe(expected, &super::Value::Current);
        match parameter(&provided, &wanted, bivariant, run) {
            Known::Yes(false) => return Known::Yes(false),
            Known::Unknown => unknown = true,
            Known::Yes(true) => {}
        }
    }
    match discards(expected.site, values) {
        Known::Yes(true) => {}
        Known::Unknown => unknown = true,
        Known::Yes(false) => {
            let Some((expected_site, expected_signature)) = super::signature(expected, values) else { return Known::Unknown };
            let Some((actual_site, actual_signature)) = super::signature(actual, values) else { return Known::Unknown };
            let expected = run.recipe(expected_site, &expected_signature.result);
            let actual = run.recipe(actual_site, &actual_signature.result);
            match run.relate(&expected, &actual) {
                Known::Yes(false) => return Known::Yes(false),
                Known::Unknown => unknown = true,
                Known::Yes(true) => {}
            }
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(true) }
}

fn parameter(provided: &NodeValue, wanted: &NodeValue, bivariant: bool, run: &mut super::accepts::Run<'_, '_>) -> Known<bool> {
    let prefer_covariant = bivariant && run.capturing();
    let (first, second) = if prefer_covariant { (provided, wanted) } else { (wanted, provided) };
    let mut accepted = run.branch(|run| run.relate(first, second));
    if bivariant && accepted != Known::Yes(true) {
        let reverse = run.branch(|run| run.relate(second, first));
        if reverse == Known::Yes(true) || reverse == Known::Unknown { accepted = reverse }
    }
    accepted
}

fn pack(parameters: &[Leaf], source: Leaf, rest_site: Leaf, run: &mut super::accepts::Run<'_, '_>) -> NodeValue {
    let values = run.values;
    let Known::Yes(Some(shape)) = values.pack_shape(rest_site) else { return NodeValue::Unknown };
    let mut elements = Vec::new();
    for &site in parameters {
        let Known::Yes(optional) = values.optional(site) else { return NodeValue::Unknown };
        let Known::Yes(repeated) = rest(site, values) else { return NodeValue::Unknown };
        elements.push(crate::grammar::names::values::products::ProductElement {
            site, value: run.recipe(site, &super::Value::Current), optional, repeated, values: false,
        });
    }
    values.product(shape, source, elements, false)
}

fn bivariant(at: Leaf, values: &Values<'_>) -> Known<bool> {
    let Some(kind) = kind(at, values) else { return Known::Unknown };
    let mut answer = None;
    for variance in &kind.calls.variances {
        if !values.question.budget.step() { return Known::Unknown }
        match variance.when.map_or(Known::Yes(true), |when| values.world.condition(at, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => {}
        }
        let current = variance.item == Variance::Bivariant;
        if answer.is_some_and(|answer| answer != current) { return Known::Unknown }
        answer = Some(current);
    }
    Known::Yes(answer.unwrap_or(false))
}

fn discards(at: Leaf, values: &Values<'_>) -> Known<bool> {
    let Some(kind) = kind(at, values) else { return Known::Unknown };
    let mut unknown = false;
    for &when in &kind.calls.discards {
        if !values.question.budget.step() { return Known::Unknown }
        match when.map_or(Known::Yes(true), |when| values.world.condition(at, when, values.semantic)) {
            Known::Yes(true) => return Known::Yes(true),
            Known::Unknown => unknown = true,
            Known::Yes(false) => {}
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(false) }
}

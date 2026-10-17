use super::{kind, Header, InvokeKind};
use crate::fx_hash::FxHashSet;
use crate::grammar::names::question::{Known, Leaf, NodeValue};
use crate::grammar::names::values::Values;

pub(super) fn result(at: Leaf, values: &Values<'_>) -> Known<Option<NodeValue>> {
    let Some(kind) = kind(at, values) else { return Known::Unknown };
    if !kind.calls.invokes.is_empty() {
        match super::select::outputs(at, values) {
            Known::Unknown => return Known::Unknown,
            Known::Yes(Some(result)) => return Known::Yes(Some(result)),
            Known::Yes(None) => {}
        }
    }
    let mut results = Vec::new();
    let mut found = false;
    for (index, signature) in kind.calls.signatures.iter().enumerate() {
        if !values.question.budget.step() { return Known::Unknown }
        match signature.when.map_or(Known::Yes(true), |when| values.world.condition(at, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => {}
        }
        let header = Header { site: at, signature: index as u32, delegate: None };
        let Known::Yes(result) = header_result(header, values) else { return Known::Unknown };
        if !append(&mut results, result, values) { return Known::Unknown }
        found = true;
    }
    if found { return Known::Yes(Some(NodeValue::Alternatives(results))) }
    let mut seen = FxHashSet::default();
    for source in &kind.calls.sources {
        if !values.question.budget.step() { return Known::Unknown }
        match source.when.map_or(Known::Yes(true), |when| values.world.condition(at, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => {}
        }
        let source_value = values.evaluate(at, &source.item);
        for (name, invoke) in [("callable", InvokeKind::Call), ("constructible", InvokeKind::Construct)] {
            if values.tables.vocabulary.trait_bit(name) == 0 { continue }
            let Known::Yes(sets) = values.calls().headers(&source_value, invoke) else { return Known::Unknown };
            for header in sets.into_iter().flatten() {
                if !seen.insert(header) { continue }
                let Known::Yes(result) = header_result(header, values) else { return Known::Unknown };
                if !append(&mut results, result, values) { return Known::Unknown }
                found = true;
            }
        }
    }
    Known::Yes(found.then_some(NodeValue::Alternatives(results)))
}

pub(super) fn header_result(header: Header, values: &Values<'_>) -> Known<NodeValue> {
    let result = match header.delegate {
        Some((owner, index)) => {
            let Some(kind) = kind(owner, values) else { return Known::Unknown };
            let Some(Some(result)) = kind.calls.source_results.get(index as usize) else { return Known::Unknown };
            values.evaluate(owner, result)
        }
        None => {
            let Some((site, signature)) = super::signature(header, values) else { return Known::Unknown };
            values.evaluate(site, &signature.result)
        }
    };
    match result {
        NodeValue::Unknown => Known::Unknown,
        result => Known::Yes(result),
    }
}

fn append(results: &mut Vec<Vec<Leaf>>, value: NodeValue, values: &Values<'_>) -> bool {
    let NodeValue::Alternatives(arms) = value else { return false };
    for arm in arms {
        if !values.question.budget.step() { return false }
        results.push(arm);
    }
    true
}

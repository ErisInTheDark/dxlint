use super::{expanded, kind, Header, InvokeKind};
use crate::fx_hash::FxHashSet;
use crate::grammar::names::question::{Known, NodeValue};
use crate::grammar::names::values::Values;

pub(super) fn headers(value: &NodeValue, invoke: InvokeKind, values: &Values<'_>) -> Known<Vec<Vec<Header>>> {
    let trait_name = match invoke { InvokeKind::Call => "callable", InvokeKind::Construct => "constructible" };
    let Some(trait_id) = values.tables.vocabulary.traits.iter().position(|name| name == trait_name) else {
        return Known::Unknown;
    };
    let NodeValue::Alternatives(arms) = expanded(value, values) else { return Known::Unknown };
    let mut sets = Vec::new();
    for arm in arms {
        let mut pending: Vec<_> = arm.into_iter().rev().collect();
        let mut set = Vec::new();
        let mut seen = FxHashSet::default();
        while let Some(site) = pending.pop() {
            if !values.question.budget.step() { return Known::Unknown }
            match values.fits(site, trait_id as u16) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            let Some(kind) = kind(site, values) else { return Known::Unknown };
            if kind.calls.signatures.is_empty() && kind.calls.sources.is_empty() {
                return Known::Unknown;
            }
            for (signature, descriptor) in kind.calls.signatures.iter().enumerate() {
                if !values.question.budget.step() { return Known::Unknown }
                match descriptor.when.map_or(Known::Yes(true), |when| values.world.condition(site, when, values.semantic)) {
                    Known::Yes(false) => continue,
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(true) => {}
                }
                let header = Header { site, signature: signature as u32 };
                if seen.insert(header) { set.push(header) }
            }
            let mut sources = Vec::new();
            for source in &kind.calls.sources {
                if !values.question.budget.step() { return Known::Unknown }
                match source.when.map_or(Known::Yes(true), |when| values.world.condition(site, when, values.semantic)) {
                    Known::Yes(false) => continue,
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(true) => {}
                }
                let NodeValue::Alternatives(arms) = values.evaluate(site, &source.item) else { return Known::Unknown };
                // A source descriptor collects declarations, not runtime choices.
                sources.extend(arms.into_iter().flatten());
            }
            pending.extend(sources.into_iter().rev());
        }
        sets.push(set);
    }
    Known::Yes(sets)
}

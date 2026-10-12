use super::{expanded, kind};
use crate::grammar::names::question::{Known, Leaf, NodeValue};
use crate::grammar::names::values::Values;

pub(super) fn accepts(expected: &NodeValue, actual: &NodeValue, values: &Values<'_>) -> Known<bool> {
    if matches!(actual, NodeValue::Alternatives(arms) if arms.is_empty()) {
        return Known::Yes(true);
    }
    let (NodeValue::Alternatives(expected), NodeValue::Alternatives(actual)) =
        (expanded(expected, values), expanded(actual, values)) else { return Known::Unknown };
    let mut unknown = false;
    for actual in &actual {
        let mut accepted = false;
        let mut uncertain = false;
        for expected in &expected {
            match alternative(expected, actual, values) {
                Known::Yes(true) => { accepted = true; break }
                Known::Yes(false) => {}
                Known::Unknown => uncertain = true,
            }
        }
        if !accepted {
            if !uncertain { return Known::Yes(false) }
            unknown = true;
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(true) }
}

fn alternative(expected: &[Leaf], actual: &[Leaf], values: &Values<'_>) -> Known<bool> {
    if !values.question.budget.step() { return Known::Unknown }
    let expected_value = NodeValue::Alternatives(vec![expected.to_vec()]);
    let actual_value = NodeValue::Alternatives(vec![actual.to_vec()]);
    let mut described = false;
    let mut unknown = false;
    let categories = values.tables.exclusive.iter().fold(0, |all, group| all | group);
    for trait_id in 0..64 {
        let bit = 1 << trait_id;
        if categories & bit == 0 { continue }
        match values.always(&expected_value, bit) {
            Known::Yes(false) => {}
            Known::Unknown => unknown = true,
            Known::Yes(true) => {
                described = true;
                match values.always(&actual_value, bit) {
                    Known::Yes(false) => return Known::Yes(false),
                    Known::Unknown => unknown = true,
                    Known::Yes(true) => {}
                }
            }
        }
    }
    for &leaf in expected {
        let Some(kind) = kind(leaf, values) else { unknown = true; continue };
        // Other descriptors remain obligations even after category agreement.
        // Their owner queries must settle them before this can prove acceptance.
        if !kind.calls.identities.is_empty() || !kind.calls.signatures.is_empty()
            || !kind.calls.sources.is_empty() || kind.member_defaults != 0
            || !kind.member_bases.is_empty() || !kind.member_links.is_empty() {
            unknown = true;
        }
    }
    if unknown || !described { Known::Unknown } else { Known::Yes(true) }
}

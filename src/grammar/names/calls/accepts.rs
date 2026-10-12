use crate::grammar::names::question::{Known, NodeValue};
use crate::grammar::names::values::Values;

pub(super) fn accepts(_expected: &NodeValue, _actual: &NodeValue, _values: &Values<'_>) -> Known<bool> {
    Known::Unknown
}

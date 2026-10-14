//! Type syntax without its parentheses.

use super::*;

pub(super) fn bare(file: &NativeFile<'_>, mut node: u32) -> u32 {
    while file.syntax.is(node, Kind::ParenType) {
        let Some(inner) = file.syntax.get(node, Field::Type) else { break };
        node = inner;
    }
    node
}

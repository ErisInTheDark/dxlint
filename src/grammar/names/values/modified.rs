//! `unwraps modified`: a value read as its source, with every member's
//! requiredness and declaration flags changed (mapped types over a source's
//! own keys: `Readonly<T>`, `Partial<T>`). Only the member edges change; the
//! members' values, the source's traits and carriers stay the source's.

use super::indexed::MemberRequest;
use super::{Known, Leaf, Member, MemberOrigin, MemberSource, NodeValue, Target, Values};
use crate::grammar::names::question::{Modified, ModifiedId};

impl Values<'_> {
    /// `value` with each node leaf read through the edits. Products (tuples,
    /// arrays) are rebuilt with their own element optionality and readonly
    /// flag edited; trait-only leaves (primitives) have no members to edit.
    pub(crate) fn modify(&self, value: NodeValue, required: Option<bool>, readonly: Option<bool>, flags: &[(Box<str>, bool)]) -> NodeValue {
        if required.is_none() && readonly.is_none() && flags.is_empty() { return value }
        let NodeValue::Alternatives(rows) = value else { return value };
        NodeValue::Alternatives(rows.into_iter().map(|row| row.into_iter().map(|source| match source.target {
            Target::Traits(_) => source,
            Target::Product(id) => match self.question.product(id) {
                Some(mut product) => {
                    product.readonly = readonly.unwrap_or(product.readonly);
                    if let Some(required) = required {
                        for element in product.elements.iter_mut().filter(|element| !element.repeated && !element.values) {
                            element.optional = !required;
                        }
                    }
                    Leaf { target: Target::Product(self.question.intern_product(product)), frame: source.frame }
                }
                None => source,
            },
            // Edits of an already modified value apply after its own.
            Target::Modified(inner) => match self.question.modified(inner) {
                Some(inner) => self.modified_leaf(Modified {
                    source: inner.source,
                    required: required.or(inner.required),
                    flags: inner.flags.iter().filter(|(name, _)| !flags.iter().any(|(outer, _)| outer == name))
                        .chain(flags).cloned().collect(),
                }),
                None => source,
            },
            _ => self.modified_leaf(Modified { source, required, flags: flags.into() }),
        }).collect()).collect())
    }

    fn modified_leaf(&self, modified: Modified) -> Leaf {
        let frame = modified.source.frame;
        Leaf { target: Target::Modified(self.question.intern_modified(modified)), frame }
    }

    /// The modified value a leaf reads through, if it is one.
    pub(super) fn modified_source(&self, at: Leaf) -> Option<Leaf> {
        let Target::Modified(id) = at.target else { return None };
        self.question.modified(id).map(|modified| modified.source)
    }

    /// The source's members with the edits applied. Index signatures keep
    /// their requiredness (a lookup that may miss stays one).
    pub(super) fn modified_members(&self, id: ModifiedId, request: MemberRequest<'_>) -> Known<Vec<Member>> {
        let Some(modified) = self.question.modified(id) else { return Known::Unknown };
        let Known::Yes(source) = self.semantic.leaf(modified.source) else { return Known::Unknown };
        let Known::Yes(members) = self.direct_members(source, request) else { return Known::Unknown };
        Known::Yes(members.into_iter().map(|mut member| {
            let indexed = member.sources.iter().all(|source| matches!(source.origin, MemberOrigin::Indexer(_)));
            if let (Some(required), false) = (modified.required, indexed) { member.required = Known::Yes(required) }
            if !modified.flags.is_empty() {
                for source in &mut member.sources { source.edits = Some(id) }
            }
            member
        }).collect())
    }

    /// The declaration flags a member source reports: its site's, with the
    /// edits of the modified value it was read through.
    pub fn source_flags(&self, source: &MemberSource) -> Known<Vec<Box<str>>> {
        let edits = source.edits.and_then(|id| self.question.modified(id)).map(|modified| modified.flags);
        let Known::Yes(mut flags) = self.flags(source.site) else { return Known::Unknown };
        for (name, add) in edits.iter().flat_map(|edits| edits.iter()) {
            let present = flags.iter().position(|flag| flag == name);
            match (present, add) {
                (None, true) => flags.push(name.clone()),
                (Some(index), false) => { flags.remove(index); }
                _ => {}
            }
        }
        Known::Yes(flags)
    }
}

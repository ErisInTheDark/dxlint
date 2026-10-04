//! A typed view of a native tree: node kinds and fields instead of the
//! grammar's matcher and label ids.

use super::kinds::{Field, Kind, ALL_FIELDS, ALL_KINDS};
use crate::grammar::{NativeFront, Tree};

pub struct Syntax<'t> {
    pub tree: &'t Tree,
    pub src: &'t str,
    base: u32,
    /// Label id -> field.
    fields: Vec<Option<Field>>,
}

impl<'t> Syntax<'t> {
    pub fn new(tree: &'t Tree, src: &'t str, front: &NativeFront) -> Self {
        let max = front.labels.iter().copied().max().map_or(0, |m| m as usize + 1);
        let mut fields = vec![None; max];
        for (f, &label) in ALL_FIELDS.iter().zip(&front.labels) {
            fields[label as usize] = Some(*f);
        }
        Syntax { tree, src, base: front.base, fields }
    }

    pub fn root(&self) -> u32 {
        self.tree.root
    }

    pub fn kind(&self, id: u32) -> Option<Kind> {
        let m = self.tree.node(id).matcher;
        m.checked_sub(self.base).and_then(|k| ALL_KINDS.get(k as usize)).copied()
    }

    pub fn is(&self, id: u32, kind: Kind) -> bool {
        self.kind(id) == Some(kind)
    }

    pub fn text(&self, id: u32) -> &'t str {
        let n = self.tree.node(id);
        &self.src[n.start as usize..n.end as usize]
    }

    /// Children with their fields, in document order; token children (see
    /// `tokens::attach`) are not syntax and are left out.
    pub fn kids(&self, id: u32) -> impl Iterator<Item = (Option<Field>, u32)> + '_ {
        self.tree
            .node(id)
            .children
            .iter()
            .map(|e| (self.fields.get(e.label as usize).copied().flatten(), e.node))
            .filter(|(f, _)| *f != Some(Field::Token))
    }

    pub fn get(&self, id: u32, field: Field) -> Option<u32> {
        self.kids(id).find(|(f, _)| *f == Some(field)).map(|(_, n)| n)
    }

    pub fn all(&self, id: u32, field: Field) -> impl Iterator<Item = u32> + '_ {
        self.kids(id).filter(move |(f, _)| *f == Some(field)).map(|(_, n)| n)
    }

    /// The field `id` has in its parent.
    pub fn field(&self, id: u32) -> Option<Field> {
        let label = self.tree.label(id)?;
        self.fields.get(label as usize).copied().flatten()
    }

    pub fn parent(&self, id: u32) -> Option<u32> {
        self.tree.parent(id)
    }

    /// Text of a labelled text child (`op`, `kind`, `modifier`).
    pub fn word(&self, id: u32, field: Field) -> Option<&'t str> {
        self.get(id, field).map(|n| self.text(n))
    }

    pub fn has_word(&self, id: u32, field: Field, word: &str) -> bool {
        self.all(id, field).any(|n| self.text(n) == word)
    }

    /// `id` without the parentheses around it (ESTree has no parentheses,
    /// so eslint's checks look through them).
    pub fn unparen(&self, mut id: u32) -> u32 {
        while self.is(id, Kind::Paren) {
            match self.get(id, Field::Expr) {
                Some(e) => id = e,
                None => break,
            }
        }
        id
    }
}

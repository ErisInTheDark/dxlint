use super::Tree;
use crate::fx_hash::FxHashMap;
use crate::grammar::{ROOT, TEXT};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

type Key = (u32, Reverse<u32>, u32);

#[derive(Clone, Copy)]
struct Bounds {
    enter: usize,
    end: usize,
    first_start: u32,
    last_start: u32,
    fallback: bool,
}

pub(super) struct Index {
    bounds: Vec<Option<Bounds>>,
    kinds: FxHashMap<u32, Vec<u32>>,
    fields: FxHashMap<(u32, u16), Vec<u32>>,
}

impl Index {
    pub(super) fn new(tree: &Tree) -> Self {
        let mut bounds = vec![None; tree.nodes.len()];
        let mut seen = vec![false; tree.nodes.len()];
        let mut owners = vec![None; tree.nodes.len()];
        for (enter, &id) in tree.order.iter().enumerate() {
            let node = tree.node(id);
            let fallback = node.children.iter().fold(false, |fallback, edge| {
                let duplicate = std::mem::replace(&mut seen[edge.node as usize], true);
                fallback || duplicate || tree.parent(edge.node) != Some(id) || tree.node(edge.node).matcher == ROOT
            });
            bounds[id as usize] = Some(Bounds {
                enter, end: enter + 1, first_start: node.start, last_start: node.start, fallback,
            });
            owners[id as usize] = tree.parent(id).and_then(|parent| {
                if tree.node(parent).matcher == TEXT { owners[parent as usize] } else { Some(parent) }
            });
        }
        for &id in tree.order.iter().rev() {
            let mut own = bounds[id as usize].expect("reachable node");
            for edge in &tree.node(id).children {
                if tree.parent(edge.node) != Some(id) { continue; }
                let child = bounds[edge.node as usize].expect("reachable child");
                own.end = own.end.max(child.end);
                own.first_start = own.first_start.min(child.first_start);
                own.last_start = own.last_start.max(child.last_start);
                own.fallback |= child.fallback;
            }
            bounds[id as usize] = Some(own);
        }
        let mut order = tree.order.clone();
        order.sort_unstable_by_key(|&id| key(tree, id));
        let mut kinds: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
        let mut fields: FxHashMap<(u32, u16), Vec<u32>> = FxHashMap::default();
        for id in order {
            kinds.entry(tree.node(id).matcher).or_default().push(id);
            if let (Some(owner), Some(label)) = (owners[id as usize], tree.label(id)) {
                fields.entry((tree.node(owner).matcher, label)).or_default().push(id);
            }
        }
        Self { bounds, kinds, fields }
    }

    pub(super) fn iter<'a>(&'a self, tree: &'a Tree, node: u32, kinds: &[u32], field: Option<u16>) -> Option<Iter<'a>> {
        let bounds = self.bounds[node as usize]?;
        if bounds.fallback { return None; }
        let mut cursors: Vec<_> = kinds.iter().filter_map(|kind| {
            let items = match field {
                Some(label) => self.fields.get(&(*kind, label)),
                None => self.kinds.get(kind),
            }?;
            let from = items.partition_point(|&id| tree.node(id).start < bounds.first_start);
            let to = items.partition_point(|&id| tree.node(id).start <= bounds.last_start);
            Some(Cursor { items: &items[from..to], index: self, bounds })
        }).collect();
        if cursors.len() <= 1 {
            return Some(Iter::Single(cursors.pop().unwrap_or(Cursor { items: &[], index: self, bounds })));
        }
        let mut heads = BinaryHeap::new();
        for (source, cursor) in cursors.iter_mut().enumerate() {
            if let Some(id) = cursor.next() { heads.push(Reverse((key(tree, id), source))); }
        }
        Some(Iter::Merged { tree, cursors, heads })
    }
}

fn key(tree: &Tree, id: u32) -> Key {
    let node = tree.node(id);
    (node.start, Reverse(node.end), id)
}

pub(crate) struct Cursor<'a> {
    items: &'a [u32],
    index: &'a Index,
    bounds: Bounds,
}

impl Iterator for Cursor<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        while let Some((&id, rest)) = self.items.split_first() {
            self.items = rest;
            let entry = self.index.bounds[id as usize].expect("indexed node").enter;
            if self.bounds.enter < entry && entry < self.bounds.end { return Some(id); }
        }
        None
    }
}

pub(crate) enum Iter<'a> {
    Single(Cursor<'a>),
    Merged {
        tree: &'a Tree,
        cursors: Vec<Cursor<'a>>,
        heads: BinaryHeap<Reverse<(Key, usize)>>,
    },
}

impl Iterator for Iter<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        match self {
            Self::Single(cursor) => cursor.next(),
            Self::Merged { tree, cursors, heads } => {
                let Reverse(((.., id), source)) = heads.pop()?;
                if let Some(next) = cursors[source].next() { heads.push(Reverse((key(tree, next), source))); }
                Some(id)
            }
        }
    }
}

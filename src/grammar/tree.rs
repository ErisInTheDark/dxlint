//! The finished layer tree of one file: nodes reachable from the root, with
//! parents and field labels resolved.

use super::parse::{Edge, Node, Parser, Syntax, ROOT, TEXT};
use super::Grammar;
use crate::pattern::compile::NO_LABEL;
use crate::fx_hash::FxHashMap;
use std::sync::OnceLock;

mod descendants;

const NONE: u32 = u32::MAX;

/// [`Tree::field_one`]: how many nodes a field holds, the node when one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldOne {
    None,
    One(u32),
    Many,
}

pub struct Tree {
    pub nodes: Vec<Syntax>,
    /// `edges[first[id]..first[id + 1]]` are node `id`'s children, sorted by start.
    first: Vec<u32>,
    edges: Vec<Edge>,
    parent: Vec<u32>,
    label: Vec<u16>,
    field_indices: OnceLock<Vec<usize>>,
    descendants: OnceLock<descendants::Index>,
    module_index: OnceLock<super::names::modules::index::Index>,
    /// Reachable nodes in document order (pre-order).
    pub order: Vec<u32>,
    pub root: u32,
    /// `(node, start, end)` of nodes' insides (their `*`), sorted by node.
    holes: Vec<(u32, u32, u32)>,
    /// Nesting went past the parser's depth limit somewhere.
    pub too_deep: bool,
    /// `(root node, layer)` of each named layer's tree in this file.
    pub layer_roots: Vec<(u32, u16)>,
    /// Nodes whose children overlap (`@dec export default class`: the
    /// `default` keyword lies inside the decorated class), sorted. Lookups
    /// by position search all their children.
    overlapping: Vec<u32>,
}

impl Tree {
    pub fn parse(g: &Grammar, lang: usize, text: &str) -> Tree {
        Tree::parse_with(g, lang, text, None, None)
    }

    /// Like [`Tree::parse`], searching only overlays flagged in `overlays`.
    pub fn parse_with(g: &Grammar, lang: usize, text: &str, overlays: Option<&[bool]>, profile: Option<&crate::engine::Profile>) -> Tree {
        let mut p = Parser::new(g, lang, text);
        p.enabled_overlays = overlays;
        p.profile = profile;
        let root = p.parse();
        let too_deep = p.too_deep;
        let mut tree = Tree::from_nodes(std::mem::take(&mut p.nodes), std::mem::take(&mut p.holes), root, too_deep);
        tree.layer_roots = std::mem::take(&mut p.layer_roots);
        tree
    }

    /// A tree from a native front end's nodes (already using this grammar's
    /// matcher and label ids).
    pub fn from_native(nodes: Vec<Node>, root: u32) -> Tree {
        Tree::from_nodes(nodes, Vec::new(), root, false)
    }

    /// A front end's tree (in this grammar's ids) with the pattern layers
    /// that live inside it: one tree per file. `layers` (by matcher) limits
    /// which overlays are searched and, under each node, whether its child
    /// layers are built at all (all of them when any is wanted, since they
    /// compete for the same text).
    pub fn graft(g: &Grammar, lang: usize, text: &str, mut nodes: Vec<Node>, root: u32, layers: Option<&[bool]>, child_layers: bool, profile: Option<&crate::engine::Profile>) -> Tree {
        let mut p = Parser::new(g, lang, text);
        p.enabled_overlays = layers;
        p.masked_children = child_layers;
        p.profile = profile;
        let holes = p.graft(&mut nodes, root);
        let too_deep = p.too_deep;
        let layer_roots = std::mem::take(&mut p.layer_roots);
        drop(p);
        let mut tree = Tree::from_nodes(nodes, holes, root, too_deep);
        tree.layer_roots = layer_roots;
        tree
    }

    /// The named layer (index into [`Grammar::layers`]) a node is in, or
    /// `None` for the default layer.
    pub fn layer_of(&self, mut id: u32) -> Option<u16> {
        if self.layer_roots.is_empty() {
            return None;
        }
        loop {
            if let Some(&(_, l)) = self.layer_roots.iter().find(|&&(r, _)| r == id) {
                return Some(l);
            }
            id = self.parent(id)?;
        }
    }

    /// The root node of a named layer's tree, when the file has one.
    pub fn layer_root(&self, l: u16) -> Option<u32> {
        self.layer_roots.iter().find(|&&(_, x)| x == l).map(|&(r, _)| r)
    }

    fn from_nodes(built: Vec<Node>, holes: Vec<(u32, u32, u32)>, root: u32, too_deep: bool) -> Tree {
        let nodes: Vec<Syntax> = built.iter().map(|n| Syntax { matcher: n.matcher, start: n.start, end: n.end }).collect();
        let mut first = Vec::with_capacity(built.len() + 1);
        let mut edges = Vec::with_capacity(built.iter().map(|n| n.children.len()).sum());
        let mut overlapping = Vec::new();
        for (id, node) in built.into_iter().enumerate() {
            first.push(edges.len() as u32);
            let from = edges.len();
            edges.extend(node.children);
            let children = &mut edges[from..];
            if !children.windows(2).all(|pair| nodes[pair[0].node as usize].start <= nodes[pair[1].node as usize].start) {
                children.sort_by_key(|e| nodes[e.node as usize].start);
            }
            let mut reach = 0;
            for e in children.iter() {
                let Syntax { start, end, .. } = nodes[e.node as usize];
                if start < reach && start < end {
                    overlapping.push(id as u32);
                    break;
                }
                reach = reach.max(end);
            }
        }
        first.push(edges.len() as u32);
        let n = nodes.len();
        let mut parent = vec![NONE; n];
        let mut label = vec![NO_LABEL; n];
        let mut order = Vec::new();
        let mut stack = vec![root];
        // Parent slots mark reachability; reserve the root until the walk ends.
        parent[root as usize] = root;
        // Memoised nodes can be shared by several would-be parents; the
        // first one reached in document order owns them.
        while let Some(id) = stack.pop() {
            order.push(id);
            for edge in edges[first[id as usize] as usize..first[id as usize + 1] as usize].iter().rev() {
                let c = edge.node as usize;
                if parent[c] == NONE {
                    parent[c] = id;
                    label[c] = edge.label;
                    stack.push(edge.node);
                }
            }
        }
        parent[root as usize] = NONE;
        Tree { nodes, first, edges, parent, label, field_indices: OnceLock::new(), descendants: OnceLock::new(), module_index: OnceLock::new(), order, root, holes, too_deep, layer_roots: Vec::new(), overlapping }
    }

    /// The node's children in document order.
    pub fn children(&self, id: u32) -> &[Edge] {
        &self.edges[self.first[id as usize] as usize..self.first[id as usize + 1] as usize]
    }

    /// Rebuilds the tree after `change` edits its nodes (tests shape trees by hand).
    #[cfg(test)]
    pub(crate) fn edit(&mut self, change: impl FnOnce(&mut Vec<Node>)) {
        let mut nodes = self.built();
        change(&mut nodes);
        let layer_roots = std::mem::take(&mut self.layer_roots);
        *self = Tree::from_nodes(nodes, std::mem::take(&mut self.holes), self.root, self.too_deep);
        self.layer_roots = layer_roots;
    }

    /// This tree of an edited text, renumbered so every node `previous`
    /// (the tree of the text before) also has keeps its id there: same
    /// matcher, field label and span, positions after the edit shifted.
    /// The edit replaced bytes `start..old_end` of the old text with
    /// `start..new_end` of the new one. Other nodes take the ids left free,
    /// then new ones; ids no node takes stay as empty text nodes nothing
    /// reaches.
    pub fn renumbered_after(self, previous: &Tree, (start, old_end, new_end): (u32, u32, u32)) -> Tree {
        let moved = |at: u32| if at <= start { Some(at) } else if at >= old_end { Some(at - old_end + new_end) } else { None };
        let key = |tree: &Tree, id: u32, at: (u32, u32)| (tree.nodes[id as usize].matcher, tree.label[id as usize], at.0, at.1);
        let mut kept: FxHashMap<(u32, u16, u32, u32), std::collections::VecDeque<u32>> = FxHashMap::default();
        for &id in &previous.order {
            let node = previous.nodes[id as usize];
            if let (Some(s), Some(e)) = (moved(node.start), moved(node.end)) {
                kept.entry(key(previous, id, (s, e))).or_default().push_back(id);
            }
        }
        let n = self.nodes.len();
        let mut to = vec![NONE; n];
        let mut taken = vec![false; previous.nodes.len()];
        for &id in &self.order {
            let node = self.nodes[id as usize];
            if let Some(old) = kept.get_mut(&key(&self, id, (node.start, node.end))).and_then(|ids| ids.pop_front()) {
                to[id as usize] = old;
                taken[old as usize] = true;
            }
        }
        let mut free = (0..previous.nodes.len() as u32).filter(|&id| !taken[id as usize]);
        let mut next = previous.nodes.len() as u32;
        for slot in to.iter_mut().filter(|slot| **slot == NONE) {
            *slot = free.next().unwrap_or_else(|| { next += 1; next - 1 });
        }
        drop(free);
        let total = (next as usize).max(to.iter().map(|&id| id as usize + 1).max().unwrap_or(0));
        let mut nodes = vec![Node { matcher: TEXT, start: 0, end: 0, children: Vec::new() }; total];
        for id in 0..n as u32 {
            let Syntax { matcher, start, end } = self.nodes[id as usize];
            let children = self.children(id).iter().map(|edge| Edge { label: edge.label, node: to[edge.node as usize] }).collect();
            nodes[to[id as usize] as usize] = Node { matcher, start, end, children };
        }
        let mut holes: Vec<(u32, u32, u32)> = self.holes.iter().map(|&(node, s, e)| (to[node as usize], s, e)).collect();
        holes.sort_unstable();
        let mut tree = Tree::from_nodes(nodes, holes, to[self.root as usize], self.too_deep);
        tree.layer_roots = self.layer_roots.iter().map(|&(root, layer)| (to[root as usize], layer)).collect();
        tree
    }

    /// The tree's nodes as building nodes, for tests that derive trees from it.
    #[cfg(test)]
    pub(crate) fn built(&self) -> Vec<Node> {
        (0..self.nodes.len() as u32).map(|id| {
            let Syntax { matcher, start, end } = self.nodes[id as usize];
            Node { matcher, start, end, children: self.children(id).to_vec() }
        }).collect()
    }

    pub(crate) fn descendant_nodes(&self, node: u32, kinds: &[u32], field: Option<u16>) -> Option<impl Iterator<Item = u32> + '_> {
        self.descendants.get_or_init(|| descendants::Index::new(self)).iter(self, node, kinds, field)
    }

    pub fn module_index(&self, tables: &super::names::Names, text: &str, key: &super::names::NameKey<'_>, profile: Option<&crate::engine::Profile>) -> &super::names::modules::index::Index {
        self.module_index.get_or_init(|| {
            let build = || super::names::modules::index::Index::build(tables, self, text, key);
            match profile {
                Some(profile) => profile.time_module_index(build),
                None => build(),
            }
        })
    }

    pub fn semantic_node(&self, node: u32) -> Option<u32> {
        self.semantic_node_with(node, &mut || true)
    }

    pub(crate) fn semantic_node_with(&self, mut node: u32, step: &mut impl FnMut() -> bool) -> Option<u32> {
        for _ in 0..=self.nodes.len() {
            self.nodes.get(node as usize)?;
            let Some(child) = self.capture_child(node) else { return Some(node) };
            if !step() { return None }
            node = child;
        }
        None
    }

    pub(crate) fn capture_child(&self, node: u32) -> Option<u32> {
        let syntax = self.nodes.get(node as usize)?;
        match self.children(node) {
            [child] if syntax.matcher == TEXT && child.label == NO_LABEL => Some(child.node),
            _ => None,
        }
    }

    pub(crate) fn capture_owner_with(&self, node: u32, step: &mut impl FnMut() -> bool) -> Option<u32> {
        let mut node = self.semantic_node_with(node, step)?;
        for _ in 0..=self.nodes.len() {
            let Some(parent) = self.parent(node) else { return Some(node) };
            if self.capture_child(parent) != Some(node) { return Some(node) }
            if !step() { return None }
            node = parent;
        }
        None
    }

    pub fn semantic_follow(&self, node: u32, path: &[u16]) -> Option<Vec<u32>> {
        self.semantic_follow_with(node, path, &mut || true)
    }

    pub(crate) fn semantic_follow_with(&self, node: u32, path: &[u16], step: &mut impl FnMut() -> bool) -> Option<Vec<u32>> {
        let mut nodes = vec![self.semantic_node_with(node, step)?];
        for &label in path {
            if label == super::names::IT { continue }
            let mut next = Vec::new();
            for node in nodes {
                if label == super::names::PARENT {
                    let owner = self.capture_owner_with(node, step)?;
                    if let Some(parent) = self.parent(owner) {
                        next.push(self.semantic_node_with(parent, step)?);
                    }
                } else if label < super::names::ATTR_STEP {
                    for child in self.fields(node, label) {
                        next.push(self.semantic_node_with(child, step)?);
                    }
                }
            }
            nodes = next;
        }
        Some(nodes)
    }

    /// Where a node's inside (its `*`) starts and ends, if its pattern has one.
    pub fn hole(&self, id: u32) -> Option<(u32, u32)> {
        let i = self.holes.binary_search_by_key(&id, |h| h.0).ok()?;
        Some((self.holes[i].1, self.holes[i].2))
    }

    pub fn node(&self, id: u32) -> &Syntax {
        &self.nodes[id as usize]
    }

    pub fn parent(&self, id: u32) -> Option<u32> {
        Some(self.parent[id as usize]).filter(|&p| p != NONE)
    }

    /// The field label this node has inside its parent.
    pub fn label(&self, id: u32) -> Option<u16> {
        Some(self.label[id as usize]).filter(|&l| l != NO_LABEL)
    }

    /// The adjacent child in the same field, using the first occurrence
    /// when memoised nodes appear in several child edges.
    pub fn field_sibling(&self, id: u32, after: bool) -> Option<u32> {
        let parent = self.parent(id)?;
        let label = self.label(id)?;
        let mut same = self.children(parent).iter().filter(|edge| edge.label == label);
        let mut previous = None;
        while let Some(edge) = same.next() {
            if edge.node == id {
                return if after { same.next().map(|edge| edge.node) } else { previous };
            }
            previous = Some(edge.node);
        }
        None
    }

    /// The node's ordinal within its parent's children of the same field.
    pub fn field_index(&self, id: u32) -> Option<usize> {
        self.field_index_with(id, || {})
    }

    fn field_index_with(&self, id: u32, mut inspected: impl FnMut()) -> Option<usize> {
        self.parent(id)?;
        if self.label(id).is_none() {
            return Some(0);
        }
        let indices = self.field_indices.get_or_init(|| {
            let mut indices = vec![usize::MAX; self.nodes.len()];
            let mut counts = FxHashMap::<u16, usize>::default();
            for &parent in &self.order {
                counts.clear();
                for edge in self.children(parent) {
                    inspected();
                    let child = edge.node as usize;
                    // A shared node belongs to its resolved parent; duplicate edges
                    // stop the old scan at the first occurrence, using its resolved label.
                    if self.parent[child] == parent && indices[child] == usize::MAX {
                        indices[child] = counts.get(&self.label[child]).copied().unwrap_or(0);
                    }
                    *counts.entry(edge.label).or_default() += 1;
                }
            }
            indices
        });
        Some(indices[id as usize])
    }

    pub fn is_root(&self, id: u32) -> bool {
        self.nodes[id as usize].matcher == ROOT
    }

    pub fn is_text(&self, id: u32) -> bool {
        self.nodes[id as usize].matcher == TEXT
    }

    /// Fields of `id` labelled `label`, in document order. Looks through
    /// text captures (`sig:(name:ident ...)`) but not into child matchers,
    /// which own their own fields.
    pub fn fields(&self, id: u32, label: u16) -> Vec<u32> {
        let mut out = Vec::new();
        self.visit_fields(id, label, &mut |node| {
            out.push(node);
            true
        });
        out
    }

    /// [`Self::fields`] without building the list: `visit` sees each in
    /// order until it returns false. False when it stopped early.
    pub fn visit_fields(&self, id: u32, label: u16, visit: &mut impl FnMut(u32) -> bool) -> bool {
        for e in self.children(id) {
            let go_on = if e.label == label {
                visit(e.node)
            } else if self.nodes[e.node as usize].matcher == TEXT {
                self.visit_fields(e.node, label, visit)
            } else {
                true
            };
            if !go_on {
                return false;
            }
        }
        true
    }

    /// A field read: no node, one, or several (then see [`Self::fields`]).
    pub fn field_one(&self, id: u32, label: u16) -> FieldOne {
        let mut found = FieldOne::None;
        self.visit_fields(id, label, &mut |node| match found {
            FieldOne::None => {
                found = FieldOne::One(node);
                true
            }
            _ => {
                found = FieldOne::Many;
                false
            }
        });
        found
    }

    /// The matcher node a labelled node is a field of (skipping text
    /// captures in between).
    pub fn owner(&self, id: u32) -> Option<u32> {
        let mut p = self.parent(id)?;
        while self.nodes[p as usize].matcher == TEXT {
            p = self.parent(p)?;
        }
        Some(p)
    }

    /// The deepest layer node (not labelled text) whose span contains `pos`,
    /// ignoring `skip` (the node asking) and its descendants, in the default
    /// layer.
    pub fn innermost_at(&self, g: &Grammar, pos: u32, skip: u32) -> Option<u32> {
        let mut id = self.innermost_with_text(g, pos, skip);
        // Labelled text inside a layer belongs to that layer.
        while self.nodes[id as usize].matcher == TEXT {
            id = self.parent(id)?;
        }
        Some(id)
    }

    /// As [`Tree::innermost_at`], but labelled text (a native node's `?.`,
    /// a field of text) counts as a node.
    pub fn innermost_with_text(&self, g: &Grammar, pos: u32, skip: u32) -> u32 {
        let mut id = self.root;
        'descend: loop {
            let children = self.children(id);
            // Children are sorted by start and mostly disjoint: the container
            // can only be a late child starting at or before `pos`. Where
            // they overlap, an earlier one may hold `pos` past a later one.
            let overlap = self.overlapping.binary_search(&id).is_ok();
            let upto = children.partition_point(|e| self.nodes[e.node as usize].start <= pos);
            for e in children[..upto].iter().rev() {
                let n = &self.nodes[e.node as usize];
                // Named layers (their roots, their nodes) are other trees.
                let other = n.matcher == ROOT || n.matcher != TEXT && g.matchers[n.matcher as usize].layer.is_some();
                if e.node == skip || other {
                    continue;
                }
                if pos < n.end {
                    id = e.node;
                    continue 'descend;
                }
                if !overlap {
                    break;
                }
            }
            break;
        }
        id
    }

    /// The closest node of matcher `m` (or a specialisation of it), starting
    /// at `id` itself.
    pub fn ancestor(&self, g: &Grammar, mut id: u32, m: u32) -> Option<u32> {
        loop {
            let here = self.nodes[id as usize].matcher;
            if here != ROOT && here != TEXT && g.is_a(here, m) {
                return Some(id);
            }
            id = self.parent(id)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Edge;

    #[test]
    fn shared_nodes_keep_first_parent_labels_and_stable_document_order() {
        let edge = |node, label| Edge { node, label };
        let node = |start, end, children| Node { matcher: TEXT, start, end, children };
        let nodes = vec![
            node(2, 3, Vec::new()),
            node(0, 3, vec![edge(0, 20)]),
            node(4, 7, vec![edge(0, 21)]),
            node(0, 0, Vec::new()),
            node(8, 9, Vec::new()),
            Node { matcher: ROOT, start: 0, end: 10, children: vec![edge(2, 12), edge(3, 11), edge(1, 10)] },
        ];
        let tree = Tree::from_native(nodes, 5);
        assert_eq!(tree.order, [5, 3, 1, 0, 2]);
        assert_eq!(tree.parent(0), Some(1));
        assert_eq!(tree.label(0), Some(20));
        assert_eq!(tree.parent(5), None);
        assert_eq!(tree.label(5), None);
        assert_eq!(tree.parent(4), None);
        assert_eq!(tree.label(4), None);
        assert_eq!(tree.fields(5, 10), [1]);
        assert_eq!(tree.fields(1, 20), [0]);
    }

    #[test]
    fn field_indices_match_sibling_scans_with_shared_and_unlabelled_nodes() {
        let edge = |node, label| Edge { node, label };
        let node = |start, end, children| Node { matcher: TEXT, start, end, children };
        let tree = Tree::from_native(vec![
            node(2, 3, vec![]),
            node(0, 4, vec![edge(0, 2), edge(3, 2), edge(0, 3)]),
            node(5, 8, vec![edge(0, 7), edge(4, 7), edge(5, NO_LABEL), edge(5, 9)]),
            node(0, 1, vec![]),
            node(6, 7, vec![]),
            node(7, 8, vec![]),
            node(10, 11, vec![]),
            Node { matcher: ROOT, start: 0, end: 12, children: vec![edge(2, 11), edge(1, 10)] },
        ], 7);
        for id in 0..tree.nodes.len() as u32 {
            let expected = tree.parent(id).map(|parent| tree.children(parent).iter()
                .take_while(|edge| edge.node != id).filter(|edge| Some(edge.label) == tree.label(id)).count());
            assert_eq!(tree.field_index(id), expected, "shared node {id}");
            let siblings: Vec<_> = tree.parent(id).into_iter().flat_map(|parent| tree.children(parent).iter())
                .filter(|edge| Some(edge.label) == tree.label(id)).map(|edge| edge.node).collect();
            let at = siblings.iter().position(|&node| node == id);
            assert_eq!(tree.field_sibling(id, false), at.and_then(|at| at.checked_sub(1)).map(|at| siblings[at]));
            assert_eq!(tree.field_sibling(id, true), at.and_then(|at| siblings.get(at + 1).copied()));
        }
        for count in 0..64 {
            let mut nodes: Vec<_> = (0..count).map(|i| node(i, i + 1, vec![])).collect();
            nodes.push(Node { matcher: ROOT, start: 0, end: count, children: (0..count).rev()
                .map(|i| edge(i, if i % 3 == 0 { NO_LABEL } else { (i % 3) as u16 })).collect() });
            let tree = Tree::from_native(nodes, count);
            for id in 0..=count {
                let expected = tree.parent(id).map(|parent| tree.children(parent).iter()
                    .take_while(|edge| edge.node != id).filter(|edge| Some(edge.label) == tree.label(id)).count());
                assert_eq!(tree.field_index(id), expected, "width {count}, node {id}");
            }
        }
    }

    #[test]
    fn reading_each_field_index_does_not_rescan_wide_sibling_prefixes() {
        let count = 4096;
        let mut nodes: Vec<_> = (0..count).map(|i| Node { matcher: TEXT, start: i, end: i + 1, children: vec![] }).collect();
        nodes.push(Node { matcher: ROOT, start: 0, end: count, children: (0..count).map(|i| Edge { node: i, label: 1 }).collect() });
        let tree = Tree::from_native(nodes, count);
        let mut inspected = 0;
        for id in 0..count {
            assert_eq!(tree.field_index_with(id, || inspected += 1), Some(id as usize));
        }
        assert!(inspected <= count as usize * 2, "reading {count} siblings inspected {inspected} edges");
    }
}

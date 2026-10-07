//! The finished layer tree of one file: nodes reachable from the root, with
//! parents and field labels resolved.

use super::parse::{Node, Parser, ROOT, TEXT};
use super::Grammar;
use crate::pattern::compile::NO_LABEL;
use crate::fx_hash::FxHashMap;
use std::sync::OnceLock;

const NONE: u32 = u32::MAX;

pub struct Tree {
    pub nodes: Vec<Node>,
    parent: Vec<u32>,
    label: Vec<u16>,
    field_indices: OnceLock<Vec<usize>>,
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
    /// that live inside it: one tree per file.
    pub fn graft(g: &Grammar, lang: usize, text: &str, mut nodes: Vec<Node>, root: u32, overlays: Option<&[bool]>, profile: Option<&crate::engine::Profile>) -> Tree {
        let mut p = Parser::new(g, lang, text);
        p.enabled_overlays = overlays;
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

    fn from_nodes(mut nodes: Vec<Node>, holes: Vec<(u32, u32, u32)>, root: u32, too_deep: bool) -> Tree {
        let spans: Vec<_> = nodes.iter().map(|n| (n.start, n.end)).collect();
        let mut overlapping = Vec::new();
        for (id, node) in nodes.iter_mut().enumerate() {
            if !node.children.windows(2).all(|pair| spans[pair[0].node as usize].0 <= spans[pair[1].node as usize].0) {
                node.children.sort_by_key(|e| spans[e.node as usize].0);
            }
            let mut reach = 0;
            for e in &node.children {
                let (s, e) = spans[e.node as usize];
                if s < reach && s < e {
                    overlapping.push(id as u32);
                    break;
                }
                reach = reach.max(e);
            }
        }
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
            for edge in nodes[id as usize].children.iter().rev() {
                let c = edge.node as usize;
                if parent[c] == NONE {
                    parent[c] = id;
                    label[c] = edge.label;
                    stack.push(edge.node);
                }
            }
        }
        parent[root as usize] = NONE;
        Tree { nodes, parent, label, field_indices: OnceLock::new(), order, root, holes, too_deep, layer_roots: Vec::new(), overlapping }
    }

    /// Where a node's inside (its `*`) starts and ends, if its pattern has one.
    pub fn hole(&self, id: u32) -> Option<(u32, u32)> {
        let i = self.holes.binary_search_by_key(&id, |h| h.0).ok()?;
        Some((self.holes[i].1, self.holes[i].2))
    }

    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn parent(&self, id: u32) -> Option<u32> {
        Some(self.parent[id as usize]).filter(|&p| p != NONE)
    }

    /// The field label this node has inside its parent.
    pub fn label(&self, id: u32) -> Option<u16> {
        Some(self.label[id as usize]).filter(|&l| l != NO_LABEL)
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
                for edge in &self.node(parent).children {
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
        self.collect_fields(id, label, &mut out);
        out
    }

    fn collect_fields(&self, id: u32, label: u16, out: &mut Vec<u32>) {
        for e in &self.nodes[id as usize].children {
            if e.label == label {
                out.push(e.node);
            } else if self.nodes[e.node as usize].matcher == TEXT {
                self.collect_fields(e.node, label, out);
            }
        }
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
            let children = &self.nodes[id as usize].children;
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
    use std::path::Path;

    use crate::engine::tests::native_linter;

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
            let expected = tree.parent(id).map(|parent| tree.node(parent).children.iter()
                .take_while(|edge| edge.node != id).filter(|edge| Some(edge.label) == tree.label(id)).count());
            assert_eq!(tree.field_index(id), expected, "shared node {id}");
        }
        for count in 0..64 {
            let mut nodes: Vec<_> = (0..count).map(|i| node(i, i + 1, vec![])).collect();
            nodes.push(Node { matcher: ROOT, start: 0, end: count, children: (0..count).rev()
                .map(|i| edge(i, if i % 3 == 0 { NO_LABEL } else { (i % 3) as u16 })).collect() });
            let tree = Tree::from_native(nodes, count);
            for id in 0..=count {
                let expected = tree.parent(id).map(|parent| tree.node(parent).children.iter()
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

    #[test]
    fn positions_find_the_child_holding_them_where_children_overlap() {
        // The export's `default` lies inside its decorated class: the
        // method's line is still found in the class.
        let l = native_linter(r#"in ts_line rule ind info "{indent_level}" { indent ts_class.member }"#);
        let src = "@Bar()\nexport default class Foo {\n  method() {}\n}\n";
        let levels: Vec<String> = l.lint(Path::new("/root/a.ts"), src).into_iter().map(|d| d.message).collect();
        assert_eq!(levels, ["0", "0", "1", "0"]);
    }
}

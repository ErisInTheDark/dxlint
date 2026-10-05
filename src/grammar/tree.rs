//! The finished layer tree of one file: nodes reachable from the root, with
//! parents and field labels resolved.

use super::parse::{Node, Parser, ROOT, TEXT};
use super::Grammar;
use crate::pattern::compile::NO_LABEL;

const NONE: u32 = u32::MAX;

pub struct Tree {
    pub nodes: Vec<Node>,
    parent: Vec<u32>,
    label: Vec<u16>,
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
        let starts: Vec<u32> = nodes.iter().map(|n| n.start).collect();
        let ends: Vec<u32> = nodes.iter().map(|n| n.end).collect();
        let mut overlapping = Vec::new();
        for (id, node) in nodes.iter_mut().enumerate() {
            node.children.sort_by_key(|e| starts[e.node as usize]);
            let mut reach = 0;
            for e in &node.children {
                let (s, e) = (starts[e.node as usize], ends[e.node as usize]);
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
        let mut reached = vec![false; n];
        let mut order = Vec::new();
        let mut stack = vec![root];
        reached[root as usize] = true;
        // Memoised nodes can be shared by several would-be parents; the
        // first one reached in document order owns them.
        while let Some(id) = stack.pop() {
            order.push(id);
            for edge in nodes[id as usize].children.iter().rev() {
                let c = edge.node as usize;
                if !reached[c] {
                    reached[c] = true;
                    parent[c] = id;
                    label[c] = edge.label;
                    stack.push(edge.node);
                }
            }
        }
        Tree { nodes, parent, label, order, root, holes, too_deep, layer_roots: Vec::new(), overlapping }
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
    use std::path::Path;

    use crate::engine::tests::native_linter;

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

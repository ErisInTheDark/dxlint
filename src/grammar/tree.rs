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
    /// Nesting went past the parser's depth limit somewhere.
    pub too_deep: bool,
}

impl Tree {
    pub fn parse(g: &Grammar, lang: usize, text: &str) -> Tree {
        Tree::parse_with(g, lang, text, None)
    }

    /// Like [`Tree::parse`], searching only overlays flagged in `overlays`.
    pub fn parse_with(g: &Grammar, lang: usize, text: &str, overlays: Option<&[bool]>) -> Tree {
        let mut p = Parser::new(g, lang, text);
        p.enabled_overlays = overlays;
        let root = p.parse();
        let too_deep = p.too_deep;
        Tree::from_nodes(std::mem::take(&mut p.nodes), root, too_deep)
    }

    fn from_nodes(mut nodes: Vec<Node>, root: u32, too_deep: bool) -> Tree {
        let starts: Vec<u32> = nodes.iter().map(|n| n.start).collect();
        for node in &mut nodes {
            node.children.sort_by_key(|e| starts[e.node as usize]);
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
        Tree { nodes, parent, label, order, root, too_deep }
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

    /// The closest node of matcher `m`, starting at `id` itself.
    pub fn ancestor(&self, mut id: u32, m: u32) -> Option<u32> {
        loop {
            if self.nodes[id as usize].matcher == m {
                return Some(id);
            }
            id = self.parent(id)?;
        }
    }
}

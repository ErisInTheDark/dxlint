use super::NodeSel;
use crate::grammar::{Tree, ROOT};

impl NodeSel {
    pub(crate) fn descendants<'a>(&self, tree: &'a Tree, node: u32) -> impl Iterator<Item = u32> + 'a {
        let indexed = (!self.kinds.is_empty()).then(|| tree.descendant_nodes(node, &self.kinds, self.field)).flatten();
        let fallback = (indexed.is_none() && !self.kinds.is_empty()).then(|| self.walk(tree, node));
        indexed.into_iter().flatten().chain(fallback.into_iter().flatten())
    }

    fn walk(&self, tree: &Tree, node: u32) -> Vec<u32> {
        let mut found = Vec::new();
        let mut stack: Vec<u32> = tree.node(node).children.iter().map(|edge| edge.node).collect();
        while let Some(id) = stack.pop() {
            let child = tree.node(id);
            if child.matcher == ROOT { continue; }
            if self.matches(tree, id) { found.push((child.start, std::cmp::Reverse(child.end), id)); }
            stack.extend(child.children.iter().map(|edge| edge.node));
        }
        found.sort_unstable();
        found.into_iter().map(|(_, _, id)| id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::{Edge, Node, TEXT};

    #[test]
    fn indexed_descendants_preserve_order_fields_and_structural_membership() {
        let linter = crate::engine::tests::linter("match base \"b\"\nmatch specialised like base \"s\"\nmatch other \"o\"");
        let grammar = &linter.grammar;
        let base = grammar.matcher("base").unwrap();
        let specialised = grammar.matcher("specialised").unwrap();
        let other = grammar.matcher("other").unwrap();
        let edge = |node, label| Edge { node, label };
        let node = |matcher, start, end, children| Node { matcher, start, end, children };
        let tree = Tree::from_native(vec![
            node(TEXT, 0, 1, vec![]),
            node(specialised, 0, 4, vec![edge(0, 3)]),
            node(base, 0, 4, vec![]),
            node(other, 0, 10, vec![edge(2, 4), edge(1, 4), edge(6, 4)]),
            node(base, 2, 3, vec![]),
            node(ROOT, 0, 10, vec![edge(3, 1), edge(4, 1)]),
            node(base, 30, 31, vec![]),
            node(base, 0, 1, vec![]),
        ], 5);
        let selectors = [
            NodeSel::new(grammar, &[base], None),
            NodeSel::new(grammar, &[base, other, specialised], None),
            NodeSel::new(grammar, &[base], Some(3)),
            NodeSel::new(grammar, &[other], Some(4)),
        ];
        for selector in &selectors {
            for id in 0..tree.nodes.len() as u32 {
                assert_eq!(selector.descendants(&tree, id).collect::<Vec<_>>(), selector.walk(&tree, id));
            }
        }
        assert_eq!(selectors[0].descendants(&tree, 3).collect::<Vec<_>>(), [1, 2, 6]);
        assert_eq!(selectors[2].descendants(&tree, 3).collect::<Vec<_>>(), [0]);
        assert_eq!(selectors[1].descendants(&tree, 5).collect::<Vec<_>>(), [3, 1, 2, 4, 6]);
    }

    #[test]
    fn shared_edges_and_nested_files_keep_walk_semantics() {
        let linter = crate::engine::tests::linter("match item \"i\"");
        let item = linter.grammar.matcher("item").unwrap();
        let edge = |node, label| Edge { node, label };
        let node = |matcher, start, end, children| Node { matcher, start, end, children };
        let tree = Tree::from_native(vec![
            node(item, 1, 2, vec![]),
            node(item, 0, 3, vec![edge(0, 1), edge(0, 2)]),
            node(item, 4, 6, vec![edge(0, 3)]),
            node(ROOT, 7, 9, vec![edge(4, 1)]),
            node(item, 7, 8, vec![]),
            node(ROOT, 0, 10, vec![edge(1, 1), edge(2, 1), edge(3, 1)]),
        ], 5);
        let selector = NodeSel::new(&linter.grammar, &[item], None);
        for id in 0..tree.nodes.len() as u32 {
            assert_eq!(selector.descendants(&tree, id).collect::<Vec<_>>(), selector.walk(&tree, id));
        }
        assert_eq!(selector.descendants(&tree, 1).collect::<Vec<_>>(), [0, 0]);
        assert_eq!(selector.descendants(&tree, 2).collect::<Vec<_>>(), [0]);
        assert_eq!(selector.descendants(&tree, 3).collect::<Vec<_>>(), [4]);
        assert!(!selector.descendants(&tree, 5).any(|id| id == 4));
    }
}

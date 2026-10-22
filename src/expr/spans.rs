//! File-local interval queries over a selector's sorted syntax nodes.

use super::{Ctx, NodeSel};
use crate::engine::Query;

pub(super) struct LayerSpans {
    nodes: Vec<(u32, u32, u32)>,
    // Each midpoint stores its balanced subtree's furthest effective end.
    max_end: Vec<u32>,
}

impl LayerSpans {
    pub(super) fn new(mut nodes: Vec<(u32, u32, u32)>) -> Self {
        nodes.sort_unstable();
        fn index(nodes: &[(u32, u32, u32)], max_end: &mut [u32]) -> u32 {
            if nodes.is_empty() {
                return 0;
            }
            let mid = nodes.len() / 2;
            let (left, rest) = max_end.split_at_mut(mid);
            let (own, right) = rest.split_at_mut(1);
            let (start, end, _) = nodes[mid];
            own[0] = end.max(start + 1).max(index(&nodes[..mid], left)).max(index(&nodes[mid + 1..], right));
            own[0]
        }
        let mut max_end = vec![0; nodes.len()];
        index(&nodes, &mut max_end);
        Self { nodes, max_end }
    }

    fn candidates(&self, prefix: usize, threshold: u32, inclusive: bool, reverse: bool, mut visit: impl FnMut((u32, u32, u32))) {
        struct Query {
            prefix: usize,
            threshold: u32,
            inclusive: bool,
            reverse: bool,
        }
        fn search(
            spans: &LayerSpans, lo: usize, hi: usize, query: &Query, visit: &mut impl FnMut((u32, u32, u32)),
        ) {
            if lo >= hi || lo >= query.prefix {
                return;
            }
            let mid = lo + (hi - lo) / 2;
            let far = spans.max_end[mid];
            if far < query.threshold || !query.inclusive && far == query.threshold {
                return;
            }
            if query.reverse {
                search(spans, mid + 1, hi, query, visit);
                if mid < query.prefix {
                    visit(spans.nodes[mid]);
                }
                search(spans, lo, mid, query, visit);
            } else {
                search(spans, lo, mid, query, visit);
                if mid < query.prefix {
                    visit(spans.nodes[mid]);
                }
                search(spans, mid + 1, hi, query, visit);
            }
        }
        search(self, 0, self.nodes.len(), &Query { prefix, threshold, inclusive, reverse }, &mut visit);
    }

    pub(super) fn overlapping(&self, start: u32, end: u32, current: u32) -> Vec<u32> {
        let end = end.max(start.saturating_add(1));
        let prefix = self.nodes.partition_point(|&(s, _, _)| s < end);
        let mut found = Vec::new();
        self.candidates(prefix, start, false, false, |(s, e, id)| {
            if e.max(s + 1) > start && id != current {
                found.push(id);
            }
        });
        found
    }

    pub(super) fn containing(&self, start: u32, end: u32, current: u32, is_ancestor: impl Fn(u32, u32) -> bool) -> Option<u32> {
        let prefix = self.nodes.partition_point(|&(s, _, _)| s <= start);
        let mut best: Option<(u32, u32, u32)> = None;
        self.candidates(prefix, end, true, true, |(s, e, id)| {
            let better = best.is_none_or(|(bs, be, b)| e - s < be - bs || e - s == be - bs && is_ancestor(b, id));
            if e >= end && id != current && better {
                best = Some((s, e, id));
            }
        });
        best.map(|(_, _, id)| id)
    }
}

impl<'a> Ctx<'a> {
    fn layer_spans(&self, sel: &NodeSel) -> std::rc::Rc<LayerSpans> {
        if let Some(spans) = self.memo.spans.borrow().get(sel) {
            return spans.clone();
        }
        let mut nodes = Vec::new();
        let mut stack = vec![self.tree.root];
        while let Some(id) = stack.pop() {
            let node = self.tree.node(id);
            if sel.matches(self.tree, id) {
                nodes.push((node.start, node.end, id));
            }
            stack.extend(self.tree.children(id).iter().map(|edge| edge.node));
        }
        let spans = std::rc::Rc::new(LayerSpans::new(nodes));
        self.memo.spans.borrow_mut().insert(sel.clone(), spans.clone());
        spans
    }

    /// One node query, timed under `--profile`.
    fn query<T>(&self, query: Query, run: impl FnOnce() -> T, found: impl FnOnce(&T) -> usize) -> T {
        match self.profile {
            Some(profile) => profile.time_query(query, run, found),
            None => run(),
        }
    }

    /// `containing(sel)` on this node: the innermost selected node holding it.
    pub(crate) fn containing_id(&self, sel: &NodeSel) -> Option<u32> {
        self.query(Query::Containing, || {
            let spans = self.layer_spans(sel);
            let node = self.tree.node(self.node);
            spans.containing(node.start, node.end, self.node, |a, b| self.is_ancestor(a, b))
        }, |found| usize::from(found.is_some()))
    }

    /// `overlapping(sel, start, end)` from this node, as clamped offsets
    /// (`None`: this node's span).
    pub(crate) fn overlapping_ids(&self, sel: &NodeSel, range: Option<(f64, f64)>) -> Vec<u32> {
        let (start, end) = match range {
            Some((start, end)) => {
                let len = self.source.len() as f64;
                (start.clamp(0.0, len) as u32, end.clamp(0.0, len) as u32)
            }
            None => {
                let node = self.tree.node(self.node);
                (node.start, node.end)
            }
        };
        self.query(Query::Overlapping, || self.layer_spans(sel).overlapping(start, end, self.node), Vec::len)
    }

    /// `descendants(sel)` under this node.
    pub(crate) fn descendant_ids<'s>(&'s self, node: u32, sel: &NodeSel) -> impl Iterator<Item = u32> + 's {
        crate::engine::Profile::iter_query(self.profile, Query::Descendants, || sel.descendants(self.tree, node))
    }

}

#[cfg(test)]
mod tests {
    use super::LayerSpans;

    #[test]
    fn interval_queries_match_sorted_scan() {
        let nodes = [(0, 40, 1), (2, 4, 2), (4, 4, 3), (5, 8, 4), (5, 8, 5), (9, 10, 6), (11, 30, 7), (15, 16, 8)];
        for len in 0..=nodes.len() {
            let spans = LayerSpans::new(nodes[..len].to_vec());
            let mut sorted = nodes[..len].to_vec();
            sorted.sort_unstable();
            for start in 0..42 {
                for end in 0..42 {
                    for current in [1, 3, 5, 8, u32::MAX] {
                        let normalized_end = end.max(start + 1);
                        let expected: Vec<_> = sorted.iter().filter(|&&(s, e, id)| s < normalized_end && e.max(s + 1) > start && id != current)
                            .map(|&(_, _, id)| id).collect();
                        assert_eq!(spans.overlapping(start, end, current), expected, "overlap {start}..{end}, current {current}");
                        let expected = sorted.iter().rev().filter(|&&(s, e, id)| s <= start && e >= end && id != current)
                            .min_by_key(|&&(s, e, _)| e - s).map(|&(_, _, id)| id);
                        assert_eq!(spans.containing(start, end, current, |_, _| false), expected, "container {start}..{end}, current {current}");
                    }
                }
            }
        }
    }

    #[test]
    fn containment_keeps_deeper_ancestor_and_unrelated_tie_order() {
        let spans = LayerSpans::new(vec![(0, 20, 9), (0, 20, 4), (0, 20, 6), (2, 19, 3)]);
        assert_eq!(spans.containing(1, 20, u32::MAX, |a, b| a == 9 && b == 4), Some(4));
        assert_eq!(spans.containing(1, 20, u32::MAX, |_, _| false), Some(9));
        assert_eq!(spans.containing(3, 18, u32::MAX, |_, _| false), Some(3));
        assert_eq!(spans.containing(3, 18, 3, |_, _| false), Some(9));
    }

    #[test]
    fn long_container_does_not_rescan_expired_siblings() {
        let mut nodes = vec![(0, 100_000, 0)];
        nodes.extend((1..4096).map(|i| (i * 2, i * 2 + 1, i)));
        let spans = LayerSpans::new(nodes);
        for (threshold, inclusive) in [(9000, false), (9001, true)] {
            let mut examined = 0;
            spans.candidates(spans.nodes.len(), threshold, inclusive, true, |_| examined += 1);
            assert!(examined < 64, "one surviving container examined {examined} candidate spans");
        }
    }
}

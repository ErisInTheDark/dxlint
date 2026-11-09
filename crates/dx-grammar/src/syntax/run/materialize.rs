use super::{error, Failure, Run, Values};
use crate::syntax::Registration;
use crate::Node;
use crate::Edge;

#[derive(Default)]
pub(super) struct Storage {
    /// Draft id -> its index in `reached`, once reached.
    slot: Vec<Option<u32>>,
    /// Reached drafts in discovery order, with their children's range in `kids`.
    reached: Vec<(u32, u32)>,
    /// Every reached draft's children: (label, draft id), in order.
    kids: Vec<(u16, u32)>,
    /// Draft id -> its node id, once built.
    node: Vec<Option<u32>>,
    /// The post-order walk: (draft id, its next child), open drafts marked.
    stack: Vec<(u32, u32)>,
    open: Vec<bool>,
    pending: Vec<u32>,
    fields: Vec<(u16, Values)>,
    values: Vec<u32>,
}

impl Storage {
    pub(super) fn clear(&mut self) {
        self.slot.clear();
        self.reached.clear();
        self.kids.clear();
        self.node.clear();
        self.stack.clear();
        self.open.clear();
        self.pending.clear();
        self.fields.clear();
        self.values.clear();
    }
}

/// The tree under `root`, numbered as the engine expects of every parser:
/// each node after the nodes under it, children in order (post-order).
pub(super) fn build(run: &mut Run<'_>, root: u32, registration: &Registration) -> Result<(Vec<Node>, u32), Failure> {
    let scratch = &mut run.scratch;
    let storage = &mut scratch.materialize;
    storage.clear();
    let drafts = scratch.drafts.len();
    storage.slot.resize(drafts, None);
    storage.node.resize(drafts, None);
    storage.open.resize(drafts, false);
    // Every reached draft's children, labelled.
    storage.pending.push(root);
    while let Some(id) = storage.pending.pop() {
        if storage.slot[id as usize].is_some() { continue }
        storage.slot[id as usize] = Some(storage.reached.len() as u32);
        let draft = &scratch.drafts[id as usize];
        let first = storage.kids.len() as u32;
        if let Some(kind) = draft.kind {
            let schema = &run.program.node_fields[kind as usize];
            scratch.fields.write_ordered(draft.fields, &mut storage.fields);
            for (field, values) in &storage.fields {
                let local = schema.iter().position(|candidate| candidate == field)
                    .ok_or_else(|| error(draft.start, format!("field `{}` on `{}`", run.program.fields[*field as usize], run.parser.nodes()[kind as usize].name)))?;
                let label = registration.labels[kind as usize][local];
                scratch.values.write_ordered(*values, &mut storage.values);
                for &value in &storage.values {
                    storage.kids.push((label, value));
                    storage.pending.push(value);
                }
            }
        }
        storage.reached.push((first, storage.kids.len() as u32 - first));
    }
    let mut nodes = Vec::with_capacity(storage.reached.len());
    storage.stack.push((root, 0));
    storage.open[root as usize] = true;
    while let Some(&(id, next)) = storage.stack.last() {
        let (first, count) = storage.reached[storage.slot[id as usize].expect("reached draft") as usize];
        if next < count {
            storage.stack.last_mut().expect("open draft").1 += 1;
            let child = storage.kids[(first + next) as usize].1;
            if storage.node[child as usize].is_none() && !storage.open[child as usize] {
                storage.open[child as usize] = true;
                storage.stack.push((child, 0));
            }
            continue;
        }
        storage.stack.pop();
        let draft = &scratch.drafts[id as usize];
        let children = storage.kids[first as usize..(first + count) as usize].iter()
            .map(|&(label, child)| Edge { label, node: storage.node[child as usize].expect("children built first") })
            .collect();
        storage.node[id as usize] = Some(nodes.len() as u32);
        nodes.push(Node {
            matcher: draft.kind.map_or(registration.text_matcher, |kind| registration.matchers[kind as usize]),
            start: draft.start, end: draft.end, children,
        });
    }
    Ok((nodes, storage.node[root as usize].expect("root built")))
}

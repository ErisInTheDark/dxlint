use super::{error, Failure, Run, Values};
use crate::grammar::syntax::Registration;
use crate::grammar::Node;
use crate::grammar::Edge;

#[derive(Default)]
pub(super) struct Storage {
    map: Vec<Option<u32>>,
    pending: Vec<u32>,
    order: Vec<u32>,
    fields: Vec<(u16, Values)>,
    values: Vec<u32>,
}

impl Storage {
    pub(super) fn clear(&mut self) {
        self.map.clear();
        self.pending.clear();
        self.order.clear();
        self.fields.clear();
        self.values.clear();
    }
}

pub(super) fn build(run: &mut Run<'_>, root: u32, registration: &Registration) -> Result<(Vec<Node>, u32), Failure> {
    let scratch = &mut run.scratch;
    let storage = &mut scratch.materialize;
    storage.clear();
    storage.map.resize(scratch.drafts.len(), None);
    storage.pending.push(root);
    let mut nodes = Vec::new();
    while let Some(id) = storage.pending.pop() {
        if storage.map[id as usize].is_some() { continue; }
        let draft = &scratch.drafts[id as usize];
        storage.map[id as usize] = Some(nodes.len() as u32);
        storage.order.push(id);
        nodes.push(Node {
            matcher: draft.kind.map_or(registration.text_matcher, |kind| registration.matchers[kind as usize]),
            start: draft.start, end: draft.end, children: Vec::new(),
        });
        scratch.fields.write_ordered(draft.fields, &mut storage.fields);
        for (_, values) in &storage.fields {
            storage.pending.extend(scratch.values.reversed(*values));
        }
    }
    for id in &storage.order {
        let draft = &scratch.drafts[*id as usize];
        let Some(kind) = draft.kind else { continue; };
        let schema = &run.program.node_fields[kind as usize];
        scratch.fields.write_ordered(draft.fields, &mut storage.fields);
        let children = &mut nodes[storage.map[*id as usize].expect("reachable draft") as usize].children;
        children.reserve_exact(storage.fields.iter().map(|(_, values)| values.len()).sum());
        for (field, values) in &storage.fields {
            let local = schema.iter().position(|candidate| candidate == field)
                .ok_or_else(|| error(draft.start, format!("field `{}` on `{}`", run.program.fields[*field as usize], run.parser.nodes()[kind as usize].name)))?;
            scratch.values.write_ordered(*values, &mut storage.values);
            for value in &storage.values {
                children.push(Edge {
                    label: registration.labels[kind as usize][local],
                    node: storage.map[*value as usize].expect("reachable child"),
                });
            }
        }
    }
    Ok((nodes, storage.map[root as usize].expect("reachable root")))
}

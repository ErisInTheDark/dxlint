use super::{After, Catch, Event, Exit, Flow, NodeRef, Phase, Point, Step, StepId};
use crate::grammar::names::question::{Known, Leaf, QueryContext, Target};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Returned,
    Thrown,
    Broken,
    Continued,
}

#[derive(Clone, Debug)]
pub struct Effect {
    pub event: Event,
    pub control: Option<Control>,
    pub target: Option<NodeRef>,
    pub possible: bool,
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub flow: Flow,
    pub effects: Vec<Effect>,
    pub catches: Vec<Catch>,
    pub after: Vec<After>,
}

pub trait Source {
    fn shape(&self, node: Leaf, question: &QueryContext) -> Known<Shape>;
    fn children(&self, node: Leaf, question: &QueryContext) -> Known<Vec<Leaf>>;
    fn path(&self, node: Leaf, path: &[u16], question: &QueryContext) -> Known<Vec<Leaf>>;
    fn truth(&self, node: Leaf, question: &QueryContext) -> Known<Option<bool>>;
}

#[derive(Clone, Debug)]
struct Pending {
    at: StepId,
    control: Control,
    exit: Exit,
}

#[derive(Default, Debug)]
struct Fragment {
    entry: Option<StepId>,
    normal: Vec<StepId>,
    pending: Vec<Pending>,
}

type Slot = usize;

enum Task {
    Node { node: Leaf, out: Slot },
    FinishNode { node: Leaf, enter: StepId, body: Slot, shape: Shape, out: Slot },
    Flow { node: Leaf, flow: Flow, catches: Vec<Catch>, out: Slot },
    Sequence { node: NodeRef, parts: Vec<Slot>, out: Slot },
    Alternative { node: NodeRef, parts: Vec<Slot>, optional: bool, out: Slot },
    Field { node: NodeRef, path: Vec<u16>, body: Slot, out: Slot },
    Repeat { node: NodeRef, body: Slot, test: Slot, next: Slot, truth: Option<bool>, first: bool, out: Slot },
    Catch { source: Slot, handlers: Vec<(Vec<u16>, Slot)>, out: Slot },
    After { node: Leaf, source: Slot, path: Vec<u16>, out: Slot },
    FinishAfter { source: Fragment, normal: Slot, pending: Vec<(Pending, Slot)>, out: Slot },
}

struct Builder<'a, S> {
    source: &'a S,
    question: &'a QueryContext,
    steps: Vec<Step>,
    edges: Vec<Vec<StepId>>,
    slots: Vec<Option<Fragment>>,
    tasks: Vec<Task>,
}

impl<S: Source> Builder<'_, S> {
    fn slot(&mut self) -> Slot {
        let id = self.slots.len();
        self.slots.push(None);
        id
    }

    fn step(&mut self, node: NodeRef, phase: Phase, event: Option<Event>) -> StepId {
        let id = self.steps.len() as StepId;
        let point = match event {
            Some(event) => Point::Event(event),
            None => match phase { Phase::Enter => Point::Enter, Phase::Leave => Point::Leave },
        };
        self.steps.push(Step { node, point });
        self.edges.push(Vec::new());
        id
    }

    fn connect(&mut self, from: &[StepId], to: StepId) {
        for &from in from {
            if !self.edges[from as usize].contains(&to) {
                self.edges[from as usize].push(to);
            }
        }
    }

    fn take(&mut self, slot: Slot) -> Fragment {
        self.slots[slot].take().expect("finished execution fragment")
    }

    fn point(&mut self, node: NodeRef, point: Point) -> StepId {
        let id = self.steps.len() as StepId;
        self.steps.push(Step { node, point });
        self.edges.push(Vec::new());
        id
    }

    fn nodes(&mut self, owner: NodeRef, nodes: Vec<Leaf>, out: Slot) -> Option<()> {
        let mut parts = Vec::new();
        for _ in &nodes {
            if !self.question.budget.step() { return None }
            parts.push(self.slot());
        }
        self.tasks.push(Task::Sequence { node: owner, parts: parts.clone(), out });
        for (node, out) in nodes.into_iter().zip(parts).rev() {
            self.tasks.push(Task::Node { node, out });
        }
        Some(())
    }

    fn flows(&mut self, node: Leaf, flows: Vec<Flow>, catches: &[Catch]) -> Option<Vec<Slot>> {
        let mut parts = Vec::new();
        for _ in &flows {
            if !self.question.budget.step() { return None }
            parts.push(self.slot());
        }
        for (flow, out) in flows.into_iter().zip(&parts).rev() {
            self.tasks.push(Task::Flow { node, flow, catches: catches.to_vec(), out: *out });
        }
        Some(parts)
    }

    fn effects(&mut self, node: NodeRef, fragment: &mut Fragment, effects: &[Effect], phase: Phase) -> Option<()> {
        for effect in effects.iter().filter(|effect| effect.event.phase == phase) {
            if !self.question.budget.step() { return None }
            let at = self.point(node, Point::Event(effect.event));
            self.connect(&fragment.normal, at);
            if let Some(control) = effect.control {
                let super::EventKind::Effect(kind) = effect.event.kind else { return None };
                fragment.pending.push(Pending {
                    at, control,
                    exit: Exit { effect: kind, site: effect.event.site, target: effect.target, value: effect.event.value },
                });
                if !effect.possible {
                    fragment.normal.clear();
                    continue;
                }
            }
            fragment.normal = vec![at];
        }
        Some(())
    }

    fn run(&mut self) -> Option<()> {
        while let Some(task) = self.tasks.pop() {
            if !self.question.budget.step() { return None }
            match task {
                Task::Node { node, out } => {
                    let Target::Node(id) = node.target else { return None };
                    let shape = self.source.shape(node, self.question).known()?;
                    if shape.after.len() > 1 { return None }
                    let enter = self.step(id, Phase::Enter, None);
                    let body = self.slot();
                    self.tasks.push(Task::FinishNode { node, enter, body, shape: shape.clone(), out });
                    let flow_out = match shape.after.first() {
                        Some(after) => {
                            let source = self.slot();
                            self.tasks.push(Task::After { node, source, path: after.finalizer.clone(), out: body });
                            source
                        }
                        None => body,
                    };
                    self.tasks.push(Task::Flow { node, flow: shape.flow, catches: shape.catches, out: flow_out });
                }
                Task::FinishNode { node, enter, body, shape, out } => {
                    let Target::Node(id) = node.target else { return None };
                    let body = self.take(body);
                    let mut fragment = Fragment { entry: Some(enter), normal: vec![enter], pending: Vec::new() };
                    self.effects(id, &mut fragment, &shape.effects, Phase::Enter)?;
                    if let Some(entry) = body.entry { self.connect(&fragment.normal, entry); }
                    fragment.normal = body.normal;
                    fragment.pending.extend(body.pending);
                    self.effects(id, &mut fragment, &shape.effects, Phase::Leave)?;
                    let leave = self.step(id, Phase::Leave, None);
                    self.connect(&fragment.normal, leave);
                    fragment.pending.retain(|pending| {
                        if pending.control == Control::Broken && pending.exit.target == Some(id) {
                            self.edges[pending.at as usize].push(leave);
                            false
                        } else {
                            true
                        }
                    });
                    fragment.normal = vec![leave];
                    self.slots[out] = Some(fragment);
                }
                Task::Flow { node, flow, catches, out } => {
                    let Target::Node(id) = node.target else { return None };
                    match flow {
                        Flow::Children => self.nodes(id, self.source.children(node, self.question).known()?, out)?,
                        Flow::Nothing => {
                            let at = self.point(id, Point::Join);
                            self.slots[out] = Some(Fragment { entry: Some(at), normal: vec![at], pending: Vec::new() });
                        }
                        Flow::Field(path) => {
                            let nodes = self.source.path(node, &path, self.question).known()?;
                            let body = self.slot();
                            self.tasks.push(Task::Field { node: id, path: path.clone(), body, out });
                            let matching: Vec<_> = catches.iter().filter(|catch| catch.source == path).collect();
                            if matching.is_empty() {
                                self.nodes(id, nodes, body)?;
                            } else {
                                let source = self.slot();
                                let mut handlers = Vec::new();
                                let mut effects = crate::fx_hash::FxHashSet::default();
                                for catch in &matching {
                                    for &effect in &catch.effects {
                                        if !effects.insert(effect) { return None }
                                    }
                                    if !self.question.budget.step() { return None }
                                    handlers.push((catch.effects.clone(), self.slot()));
                                }
                                self.tasks.push(Task::Catch { source, handlers: handlers.clone(), out: body });
                                for (catch, (_, out)) in matching.into_iter().zip(handlers).rev() {
                                    self.tasks.push(Task::Flow { node, flow: Flow::Field(catch.handler.clone()), catches: Vec::new(), out });
                                }
                                self.nodes(id, nodes, source)?;
                            }
                        }
                        Flow::Then(flows) => {
                            let at = self.tasks.len();
                            let parts = self.flows(node, flows, &catches)?;
                            self.tasks.insert(at, Task::Sequence { node: id, parts, out });
                        }
                        Flow::Or(flows) => {
                            let at = self.tasks.len();
                            let parts = self.flows(node, flows, &catches)?;
                            self.tasks.insert(at, Task::Alternative { node: id, parts, optional: false, out });
                        }
                        Flow::Maybe(flow) => {
                            let at = self.tasks.len();
                            let parts = self.flows(node, vec![*flow], &catches)?;
                            self.tasks.insert(at, Task::Alternative { node: id, parts, optional: true, out });
                        }
                        Flow::Repeat { body, test, next, first } => {
                            let (test_flow, truth) = match test {
                                Some(path) => {
                                    let nodes = self.source.path(node, &path, self.question).known()?;
                                    let truth = self.source.truth(nodes.last().copied().unwrap_or(node), self.question).known()?;
                                    (Flow::Field(path), truth)
                                }
                                None => (Flow::Nothing, None),
                            };
                            let at = self.tasks.len();
                            let parts = self.flows(node, vec![*body, test_flow, next.map_or(Flow::Nothing, |next| *next)], &catches)?;
                            self.tasks.insert(at, Task::Repeat { node: id, body: parts[0], test: parts[1], next: parts[2], truth, first, out });
                        }
                        Flow::Select { .. } => return None,
                    }
                }
                Task::Sequence { node, parts, out } => {
                    let entry = self.point(node, Point::Join);
                    let mut fragment = Fragment { entry: Some(entry), normal: vec![entry], pending: Vec::new() };
                    for part in parts {
                        let part = self.take(part);
                        if let Some(entry) = part.entry { self.connect(&fragment.normal, entry); }
                        fragment.normal = part.normal;
                        fragment.pending.extend(part.pending);
                    }
                    self.slots[out] = Some(fragment);
                }
                Task::Alternative { node, parts, optional, out } => {
                    let entry = self.point(node, Point::Join);
                    let mut fragment = Fragment { entry: Some(entry), normal: if optional { vec![entry] } else { Vec::new() }, pending: Vec::new() };
                    for part in parts {
                        let part = self.take(part);
                        if let Some(branch) = part.entry { self.connect(&[entry], branch); }
                        fragment.normal.extend(part.normal);
                        fragment.pending.extend(part.pending);
                    }
                    self.slots[out] = Some(fragment);
                }
                Task::Field { node, path, body, out } => {
                    let mut fragment = self.take(body);
                    let entry = self.point(node, Point::Field(path));
                    if let Some(body) = fragment.entry { self.connect(&[entry], body); }
                    fragment.entry = Some(entry);
                    self.slots[out] = Some(fragment);
                }
                Task::Repeat { node, body, test, next, truth, first, out } => {
                    let mut body = self.take(body);
                    let mut test = self.take(test);
                    let mut next = self.take(next);
                    let entry = self.point(node, Point::Join);
                    let end = self.point(node, Point::Join);
                    let body_entry = body.entry?;
                    let test_entry = test.entry?;
                    let next_entry = next.entry?;
                    self.connect(&[entry], if first { body_entry } else { test_entry });
                    if truth != Some(false) { self.connect(&test.normal, body_entry); }
                    if truth != Some(true) { self.connect(&test.normal, end); }
                    self.connect(&body.normal, next_entry);
                    self.connect(&next.normal, test_entry);
                    let mut pending = Vec::new();
                    for exit in body.pending.drain(..).chain(test.pending.drain(..)).chain(next.pending.drain(..)) {
                        if exit.exit.target == Some(node) && matches!(exit.control, Control::Broken | Control::Continued) {
                            self.connect(&[exit.at], if exit.control == Control::Broken { end } else { next_entry });
                        } else {
                            pending.push(exit);
                        }
                    }
                    self.slots[out] = Some(Fragment { entry: Some(entry), normal: vec![end], pending });
                }
                Task::Catch { source, handlers, out } => {
                    let mut source = self.take(source);
                    let handlers: Vec<_> = handlers.into_iter().map(|(effects, slot)| (effects, self.take(slot))).collect();
                    let mut pending = Vec::new();
                    for exit in source.pending {
                        match handlers.iter().find(|(effects, _)| effects.contains(&exit.exit.effect)) {
                            Some((_, handler)) => self.connect(&[exit.at], handler.entry?),
                            None => pending.push(exit),
                        }
                    }
                    for (_, handler) in handlers {
                        source.normal.extend(handler.normal);
                        pending.extend(handler.pending);
                    }
                    source.pending = pending;
                    self.slots[out] = Some(source);
                }
                Task::After { node, source, path, out } => {
                    let mut source = self.take(source);
                    let normal = self.slot();
                    let mut pending = Vec::new();
                    for exit in source.pending.drain(..) {
                        if !self.question.budget.step() { return None }
                        pending.push((exit, self.slot()));
                    }
                    let branches: Vec<_> = std::iter::once(normal).chain(pending.iter().map(|(_, slot)| *slot)).collect();
                    self.tasks.push(Task::FinishAfter { source, normal, pending, out });
                    for out in branches.into_iter().rev() {
                        self.tasks.push(Task::Flow { node, flow: Flow::Field(path.clone()), catches: Vec::new(), out });
                    }
                }
                Task::FinishAfter { mut source, normal, pending, out } => {
                    let normal = self.take(normal);
                    self.connect(&source.normal, normal.entry?);
                    source.normal = normal.normal;
                    source.pending = normal.pending;
                    for (exit, slot) in pending {
                        let finalizer = self.take(slot);
                        self.connect(&[exit.at], finalizer.entry?);
                        for at in finalizer.normal {
                            source.pending.push(Pending { at, control: exit.control, exit: exit.exit });
                        }
                        source.pending.extend(finalizer.pending);
                    }
                    self.slots[out] = Some(source);
                }
            }
        }
        Some(())
    }
}

pub fn execution(source: &impl Source, entry: Leaf, question: &QueryContext) -> Known<super::Execution> {
    let mut builder = Builder { source, question, steps: Vec::new(), edges: Vec::new(), slots: Vec::new(), tasks: Vec::new() };
    let out = builder.slot();
    builder.tasks.push(Task::Node { node: entry, out });
    if builder.run().is_none() { return Known::Unknown }
    let fragment = builder.take(out);
    let Some(entry) = fragment.entry else { return Known::Unknown };
    Known::Yes(super::Execution::new(entry, builder.steps, builder.edges))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::names::effects::EventKind;
    use crate::grammar::names::question::{Budget, FrameId, Target};

    struct Node {
        shape: Shape,
        children: Vec<u32>,
        truth: Option<bool>,
    }

    struct Language(Vec<Node>);

    fn leaf(node: u32) -> Leaf {
        Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT }
    }

    fn index(leaf: Leaf) -> usize {
        let Target::Node(node) = leaf.target else { panic!("syntax fixture node") };
        node.node as usize
    }

    fn node(flow: Flow, children: &[u32]) -> Node {
        Node { shape: Shape { flow, effects: Vec::new(), catches: Vec::new(), after: Vec::new() }, children: children.to_vec(), truth: None }
    }

    fn returned() -> Node {
        let mut node = node(Flow::Nothing, &[]);
        node.shape.effects.push(Effect {
            event: Event {
                kind: EventKind::Effect(0),
                site: NodeRef::here(1),
                target: None,
                value: None,
                phase: Phase::Leave,
            },
            control: Some(Control::Returned),
            target: Some(NodeRef::here(0)),
            possible: false,
        });
        node
    }

    impl Source for Language {
        fn shape(&self, node: Leaf, _: &QueryContext) -> Known<Shape> {
            self.0.get(index(node)).map_or(Known::Unknown, |node| Known::Yes(node.shape.clone()))
        }

        fn children(&self, node: Leaf, _: &QueryContext) -> Known<Vec<Leaf>> {
            Known::Yes(self.0[index(node)].children.iter().copied().map(leaf).collect())
        }

        fn path(&self, node: Leaf, path: &[u16], _: &QueryContext) -> Known<Vec<Leaf>> {
            let mut nodes = vec![node];
            for &field in path {
                nodes = nodes.into_iter().filter_map(|node| self.0[index(node)].children.get(field as usize).copied().map(leaf)).collect();
            }
            Known::Yes(nodes)
        }

        fn truth(&self, node: Leaf, _: &QueryContext) -> Known<Option<bool>> {
            Known::Yes(self.0[index(node)].truth)
        }
    }

    fn built(language: &Language) -> super::super::Execution {
        match execution(language, leaf(0), &QueryContext::default()) {
            Known::Yes(graph) => graph,
            Known::Unknown => panic!("execution must be known within the shared budget"),
        }
    }

    #[test]
    fn control_exit_stops_the_following_statement() {
        let language = Language(vec![node(Flow::Children, &[1, 2]), returned(), node(Flow::Nothing, &[])]);
        let graph = built(&language);
        assert_eq!(graph.reachable(NodeRef::here(1), &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reachable(NodeRef::here(2), &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn an_alternative_normal_arm_keeps_the_following_statement_alive() {
        let language = Language(vec![
            node(Flow::Then(vec![Flow::Or(vec![Flow::Field(vec![0]), Flow::Field(vec![1])]), Flow::Field(vec![2])]), &[1, 2, 3]),
            returned(),
            node(Flow::Nothing, &[]),
            node(Flow::Nothing, &[]),
        ]);
        let graph = built(&language);
        for id in [1, 2, 3] {
            assert_eq!(graph.reachable(NodeRef::here(id), &Budget::default()), Known::Yes(true));
        }
    }

    #[test]
    fn self_referencing_execution_uses_the_common_budget_without_recursion() {
        let language = Language(vec![node(Flow::Children, &[0])]);
        assert!(matches!(execution(&language, leaf(0), &QueryContext::default()), Known::Unknown));
    }

    fn empty_branch(first: Flow, second: Flow) {
        let language = Language(vec![
            node(Flow::Then(vec![Flow::Or(vec![first, second]), Flow::Field(vec![1])]), &[1, 2]),
            returned(),
            node(Flow::Nothing, &[]),
        ]);
        let graph = built(&language);
        let seed = *graph.field_entries(NodeRef::here(0), &[9]).first().expect("empty branch entry");
        let use_site = graph.sites(NodeRef::here(2), Phase::Enter).next().expect("following statement");
        assert_eq!(graph.reaches(seed, use_site, &[], &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reaches(graph.entry(), use_site, &[seed], &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn absent_alternate_remains_a_guard_seed_after_the_other_arm_exits() {
        empty_branch(Flow::Field(vec![0]), Flow::Field(vec![9]));
    }

    #[test]
    fn empty_consequent_remains_a_guard_seed_after_the_other_arm_exits() {
        empty_branch(Flow::Field(vec![9]), Flow::Field(vec![0]));
    }

    #[test]
    fn false_guard_keeps_the_zero_round_path_without_entering_the_body() {
        let mut guard = node(Flow::Nothing, &[]);
        guard.truth = Some(false);
        let language = Language(vec![
            node(Flow::Then(vec![
                Flow::Repeat { body: Box::new(Flow::Field(vec![0])), test: Some(vec![1]), next: None, first: false },
                Flow::Field(vec![2]),
            ]), &[1, 2, 3]),
            returned(),
            guard,
            node(Flow::Nothing, &[]),
        ]);
        let graph = built(&language);
        assert_eq!(graph.reachable(NodeRef::here(1), &Budget::default()), Known::Yes(false));
        assert_eq!(graph.reachable(NodeRef::here(2), &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reachable(NodeRef::here(3), &Budget::default()), Known::Yes(true));
    }

    fn control_node(id: u32, control: Control) -> Node {
        let mut node = returned();
        node.shape.effects[0].event.site = NodeRef::here(id);
        node.shape.effects[0].event.kind = super::super::EventKind::Effect(if control == Control::Thrown { 1 } else { 0 });
        node.shape.effects[0].control = Some(control);
        node.shape.effects[0].target = if control == Control::Thrown { None } else { Some(NodeRef::here(0)) };
        node
    }

    fn nested_finalizer(original: Control, finalizer: Option<Control>) -> Language {
        let mut outer = node(Flow::Field(vec![0]), &[2, 3]);
        outer.shape.catches.push(Catch { handler: vec![1], source: vec![0], effects: vec![1] });
        let mut inner = node(Flow::Field(vec![0]), &[4, 5]);
        inner.shape.after.push(After { finalizer: vec![1], sources: vec![vec![0]] });
        Language(vec![
            node(Flow::Children, &[1, 8]),
            outer,
            inner,
            node(Flow::Nothing, &[]),
            node(Flow::Children, &[6]),
            node(Flow::Children, &[7]),
            control_node(6, original),
            finalizer.map_or_else(|| node(Flow::Nothing, &[]), |control| control_node(7, control)),
            node(Flow::Nothing, &[]),
        ])
    }

    #[test]
    fn throwing_finalizer_overrides_return_and_reaches_outer_catch() {
        let graph = built(&nested_finalizer(Control::Returned, Some(Control::Thrown)));
        assert_eq!(graph.reachable(NodeRef::here(7), &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reachable(NodeRef::here(3), &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reachable(NodeRef::here(8), &Budget::default()), Known::Yes(true));
    }

    #[test]
    fn returning_finalizer_overrides_throw_without_reaching_outer_catch() {
        let graph = built(&nested_finalizer(Control::Thrown, Some(Control::Returned)));
        assert_eq!(graph.reachable(NodeRef::here(7), &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reachable(NodeRef::here(3), &Budget::default()), Known::Yes(false));
        assert_eq!(graph.reachable(NodeRef::here(8), &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn normally_finishing_finalizer_resumes_the_original_return() {
        let graph = built(&nested_finalizer(Control::Returned, None));
        assert_eq!(graph.reachable(NodeRef::here(7), &Budget::default()), Known::Yes(true));
        assert_eq!(graph.reachable(NodeRef::here(3), &Budget::default()), Known::Yes(false));
        assert_eq!(graph.reachable(NodeRef::here(8), &Budget::default()), Known::Yes(false));
    }

    #[test]
    fn optional_throw_preserves_normal_completion() {
        let mut possible = control_node(1, Control::Thrown);
        possible.shape.effects[0].possible = true;
        let graph = built(&Language(vec![node(Flow::Children, &[1, 2]), possible, node(Flow::Nothing, &[])]));
        assert_eq!(graph.reachable(NodeRef::here(2), &Budget::default()), Known::Yes(true));
    }

    #[test]
    fn finalizer_normal_completion_does_not_mix_pending_and_normal_continuations() {
        let mut guarded = node(Flow::Field(vec![0]), &[2, 3]);
        guarded.shape.after.push(After { finalizer: vec![1], sources: vec![vec![0]] });
        let graph = built(&Language(vec![
            node(Flow::Children, &[1, 6]),
            guarded,
            node(Flow::Or(vec![Flow::Field(vec![0]), Flow::Field(vec![1])]), &[4, 5]),
            node(Flow::Nothing, &[]),
            control_node(4, Control::Returned),
            node(Flow::Nothing, &[]),
            node(Flow::Nothing, &[]),
        ]));
        let returned = graph.sites(NodeRef::here(4), Phase::Enter).next().expect("return entry");
        let normal = graph.sites(NodeRef::here(5), Phase::Enter).next().expect("normal arm entry");
        let tail = graph.sites(NodeRef::here(6), Phase::Enter).next().expect("following statement");
        assert_eq!(graph.reaches(returned, tail, &[], &Budget::default()), Known::Yes(false));
        assert_eq!(graph.reaches(normal, tail, &[], &Budget::default()), Known::Yes(true));
    }
}

use std::cell::RefCell;
use std::rc::Rc;

use crate::fx_hash::FxHashMap;
use crate::fx_hash::FxHashSet;

use super::{EffectId, Execution};
use crate::grammar::names::question::{Known, Leaf, NodeRef, Target};
use crate::grammar::names::semantic::Semantic;

#[derive(Default)]
pub(crate) struct State {
    /// Graphs by entry and whether they carry read events.
    graphs: RefCell<FxHashMap<(Leaf, bool), Known<Rc<Execution>>>>,
    /// Each body's distinct graphs, shared by the frames that build alike.
    variants: RefCell<FxHashMap<(Target, bool), Vec<Variant>>>,
    reads: RefCell<FxHashMap<Leaf, Known<Rc<Reads>>>>,
    entry_owners: RefCell<FxHashMap<Leaf, Leaf>>,
    /// Per file, every write's target in tree order (for `unwritten`).
    file_writes: RefCell<FxHashMap<u32, Known<Rc<[FileWrite]>>>>,
    /// Per file, those writes resolved to the bindings they replace.
    written_bindings: RefCell<FxHashMap<u32, Known<Rc<WrittenBindings>>>>,
    /// Graphs and read catalogues being built: an ask meanwhile is a cycle.
    building: RefCell<FxHashSet<(Leaf, Option<bool>)>>,
}

/// One built graph of a body, with the conditions its build decided (by
/// target, all in the build's frame): another frame where they answer
/// alike builds the same graph. `None` when a condition sat in another
/// frame, so the build can't be replayed by its conditions.
struct Variant {
    graph: Rc<Execution>,
    decided: Option<Rc<[(Target, u32, bool)]>>,
}

/// One write found walking a file: its target, or a node whose writes
/// can't be read (nothing after it is looked at).
#[derive(Clone, Copy)]
enum FileWrite {
    Target(NodeRef),
    Unreadable,
}

#[derive(Default)]
struct WrittenBindings {
    first: FxHashMap<NodeRef, usize>,
    unknown: Option<usize>,
}

pub(super) type Reads = FxHashMap<NodeRef, FxHashSet<(NodeRef, NodeRef)>>;

pub struct Effects<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredEffect {
    pub site: Leaf,
    pub target: Option<NodeRef>,
    pub value: Option<Leaf>,
    pub possible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutedEffect {
    pub effect: EffectId,
    pub site: Leaf,
    pub target: Option<NodeRef>,
    pub value: Option<Leaf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creation {
    pub entry: Leaf,
    pub site: Leaf,
}

impl Effects<'_> {
    pub fn has_reads(&self, at: Leaf) -> Known<bool> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Unknown };
        let runtime = super::source::Runtime::new(self.semantic);
        let Some(Some(binding)) = runtime.binding(at, node).known() else { return Known::Unknown };
        let Some(reads) = self.reads(at).known() else { return Known::Unknown };
        Known::Yes(reads.get(&binding).is_some_and(|reads| !reads.is_empty()))
    }

    pub fn value_used(&self, at: Leaf) -> Known<bool> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(entry) = self.entry_for(at).known() else { return Known::Unknown };
        let Some(graph) = self.execution(entry).known() else { return Known::Unknown };
        if graph.write_steps(node).is_empty() { return Known::Unknown }
        let Some(reads) = self.reads(at).known() else { return Known::Unknown };
        graph.value_used(node, &reads, &self.semantic.question.budget)
    }

    fn reads(&self, at: Leaf) -> Known<Rc<Reads>> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let root = Leaf { target: Target::Node(NodeRef { node: tree.root, ..node }), ..at };
        let Some(root) = self.semantic.leaf(root).known() else { return Known::Unknown };
        let effects = &self.semantic.effects;
        let kept = effects.reads.borrow().get(&root).cloned();
        if let Some(reads) = kept {
            if effects.building.borrow().contains(&(root, None)) { self.semantic.values().note_cycle_read() }
            return reads;
        }
        let Some(effect) = self.semantic.tables.vocabulary.effect("read") else { return Known::Unknown };
        effects.reads.borrow_mut().insert(root, Known::Unknown);
        effects.building.borrow_mut().insert((root, None));
        let mark = self.semantic.values().cycle_mark();
        // A fact kept for every later question of the file, on an allowance
        // for the file's size.
        let allowance = u32::try_from(tree.nodes.len()).unwrap_or(u32::MAX).saturating_mul(Self::BUILD_STEPS_PER_NODE)
            .max(crate::grammar::names::question::Budget::DEFAULT);
        let collect = || self.semantic.question.budget.sized(allowance, || self.collect_reads(at, node, tree, effect))
            .unwrap_or(Known::Unknown);
        let reads = match self.semantic.world.profile() {
            Some(profile) => profile.time_effect_build(crate::engine::EffectBuild::ReadCatalogue, false, collect, |reads, _| matches!(reads, Known::Yes(_))),
            None => collect(),
        };
        effects.building.borrow_mut().remove(&(root, None));
        if matches!(reads, Known::Unknown) && self.semantic.values().cycled_since(mark) {
            effects.reads.borrow_mut().remove(&root);
        } else {
            effects.reads.borrow_mut().insert(root, reads.clone());
        }
        reads
    }

    fn collect_reads(&self, at: Leaf, node: NodeRef, tree: &crate::grammar::Tree, effect: EffectId) -> Known<Rc<Reads>> {
        let runtime = super::source::Runtime::new(self.semantic);
        let mut reads = Reads::default();
        let mut visited = FxHashSet::default();
        for id in 0..tree.nodes.len() {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let Some(kind) = self.semantic.tables.kinds.get(tree.nodes[id].matcher as usize) else { continue };
            if !kind.unwraps.iter().any(|item| item.effect == Some(effect)) { continue }
            let site = Leaf { target: Target::Node(NodeRef { node: id as u32, ..node }), ..at };
            let Some(site) = self.semantic.leaf(site).known() else { return Known::Unknown };
            if !visited.insert(site) { continue }
            let Some(declared) = runtime.declarations(site, Some(effect)).known() else { return Known::Unknown };
            for emission in declared {
                let Some(target) = runtime.read(site, &emission).known() else { return Known::Unknown };
                if let Some(binding) = target.binding {
                    let Target::Node(owner) = site.target else { return Known::Unknown };
                    reads.entry(binding).or_default().insert((owner, target.value));
                }
            }
        }
        Known::Yes(Rc::new(reads))
    }

    pub fn entries(&self, owner: Leaf) -> Known<Vec<Leaf>> {
        let Some(owner) = self.semantic.leaf(owner).known() else { return Known::Unknown };
        let Target::Node(node) = owner.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.semantic.tables.kinds.get(syntax.matcher as usize) else { return Known::Yes(Vec::new()) };
        if kind.execution.entries.is_empty() { return Known::Yes(Vec::new()) }
        let runtime = super::source::Runtime::new(self.semantic);
        let Some(items) = runtime.enabled(owner, &kind.execution.entries).known() else { return Known::Unknown };
        let mut entries = Vec::new();
        for item in items {
            let Some(projected) = self.semantic.follow(owner, &item.item).known() else { return Known::Unknown };
            for entry in projected {
                if !self.semantic.question.budget.step() { return Known::Unknown }
                if !entries.contains(&entry) { entries.push(entry) }
            }
        }
        Known::Yes(entries)
    }

    /// Whether no write in `binding`'s file (other than its declaration)
    /// replaces it; `declaration` names the binding a write target replaces
    /// (`None`: none). Each file's writes are resolved once
    /// ([`Effects::written_bindings`]): closure reads ask per binding, and
    /// resolving every write per read made big files quadratic.
    pub fn unwritten(&self, binding: Leaf, declaration: &impl Fn(Leaf) -> Known<Option<NodeRef>>) -> Known<bool> {
        let Some(binding) = self.semantic.leaf(binding).known() else { return Known::Unknown };
        if binding.frame != super::super::question::FrameId::ROOT { return Known::Unknown }
        let Target::Node(node) = binding.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        let Some(names) = self.semantic.world.names(node.file) else { return Known::Unknown };
        if !names.declared.contains_key(&node.node) { return Known::Unknown }
        let Known::Yes(written) = self.written_bindings(binding, tree.nodes.len(), declaration) else { return Known::Unknown };
        match (written.first.get(&node), written.unknown) {
            (Some(&first), unknown) if unknown.is_none_or(|unknown| first < unknown) => Known::Yes(false),
            (_, Some(_)) => Known::Unknown,
            _ => Known::Yes(true),
        }
    }

    /// Per file (root frame): each binding's first replacing write, in tree
    /// order, and the first write whose binding can't be told.
    fn written_bindings(&self, at: Leaf, nodes: usize, declaration: &impl Fn(Leaf) -> Known<Option<NodeRef>>) -> Known<Rc<WrittenBindings>> {
        let Target::Node(file) = at.target else { return Known::Unknown };
        let Known::Yes(writes) = self.file_writes(at, nodes) else { return Known::Unknown };
        // Once per file, kept for every later question: a fact on an
        // allowance for the file's writes.
        let budget = &self.semantic.question.budget;
        let allowance = u32::try_from(writes.len()).unwrap_or(u32::MAX).saturating_mul(Self::FILE_WALK_STEPS_PER_NODE)
            .max(super::super::question::Budget::per_question());
        self.file_fact(&self.semantic.effects.written_bindings, file.file, allowance, || {
            let mut written = WrittenBindings::default();
            for (index, &write) in writes.iter().enumerate() {
                if !budget.step() { return None }
                let FileWrite::Target(target) = write else { written.unknown = Some(index); break };
                match declaration(Leaf { target: Target::Node(target), ..at }) {
                    // A declaration's own write doesn't replace it.
                    Known::Yes(Some(binding)) => if binding != target { written.first.entry(binding).or_insert(index); },
                    Known::Yes(None) => {}
                    Known::Unknown => { written.unknown = Some(index); break }
                }
            }
            Some(Rc::new(written))
        })
    }

    /// A once-per-file fact kept in `memo` by file
    /// ([`super::super::question::Budget::sized`]): unknown answers too,
    /// unless they read something undecided.
    fn file_fact<V: Clone>(&self, memo: &RefCell<FxHashMap<u32, Known<V>>>, file: u32, allowance: u32, compute: impl FnOnce() -> Option<V>) -> Known<V> {
        let values = self.semantic.values();
        if let Some(answer) = memo.borrow().get(&file).cloned() { return answer }
        let mark = values.cycle_mark();
        let answer = self.semantic.question.budget.sized(allowance, compute).ok().flatten().map_or(Known::Unknown, Known::Yes);
        if matches!(answer, Known::Yes(_)) || !values.cycled_since(mark) {
            memo.borrow_mut().insert(file, answer.clone());
        }
        answer
    }

    /// Steps per node a whole-file write walk may spend: a few per node
    /// for its write items and their conditions.
    const FILE_WALK_STEPS_PER_NODE: u32 = 64;
    /// Steps per node of its file a graph build may spend.
    const BUILD_STEPS_PER_NODE: u32 = 64;

    /// Every write in `at`'s file (root frame), in tree order, walked once
    /// per file: each closure read's unwritten proof asks about one binding,
    /// and walking the whole file per read made big files quadratic.
    fn file_writes(&self, at: Leaf, nodes: usize) -> Known<Rc<[FileWrite]>> {
        let Target::Node(file) = at.target else { return Known::Unknown };
        let runtime = super::source::Runtime::new(self.semantic);
        let budget = &self.semantic.question.budget;
        // A whole-file walk, kept for every later question: it gets an
        // allowance for the file's size rather than what is left of the
        // asking question's budget (which cut big files' walks short in
        // every question that asked).
        let allowance = u32::try_from(nodes).unwrap_or(u32::MAX).saturating_mul(Self::FILE_WALK_STEPS_PER_NODE)
            .max(super::super::question::Budget::per_question());
        self.file_fact(&self.semantic.effects.file_writes, file.file, allowance, || {
            let mut visited = FxHashSet::default();
            let mut writes = Vec::new();
            'nodes: for id in 0..nodes {
                if !budget.step() { return None }
                let node = Leaf { target: Target::Node(NodeRef { node: id as u32, ..file }), ..at };
                let Some(node) = self.semantic.leaf(node).known() else { writes.push(FileWrite::Unreadable); break };
                if !visited.insert(node) { continue }
                let Some(found) = runtime.writes(node, None).known() else { writes.push(FileWrite::Unreadable); break };
                for (_, _, write) in found {
                    match write.event.target {
                        Some(target) => writes.push(FileWrite::Target(target.value)),
                        None => { writes.push(FileWrite::Unreadable); break 'nodes }
                    }
                }
            }
            Some(Rc::<[FileWrite]>::from(writes))
        })
    }

    pub fn enclosing(&self, entry: Leaf) -> Known<Option<Creation>> {
        let Some(entry) = self.semantic.leaf(entry).known() else { return Known::Unknown };
        if entry.frame != super::super::question::FrameId::ROOT { return Known::Unknown }
        let Target::Node(node) = entry.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        if tree.nodes.get(node.node as usize).is_none() { return Known::Unknown }
        if self.entry_for(entry) != Known::Yes(entry) { return Known::Unknown }
        let mut owner = Some(node.node);
        while let Some(id) = owner {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let site = Leaf { target: Target::Node(NodeRef { node: id, ..node }), ..entry };
            let Some(site) = self.semantic.leaf(site).known() else { return Known::Unknown };
            if site.target != Target::Node(NodeRef { node: id, ..node }) { owner = tree.parent(id); continue }
            let Some(entries) = self.entries(site).known() else { return Known::Unknown };
            if entries.contains(&entry) {
                if site == entry { return Known::Unknown }
                let Some(outer) = self.entry_for(site).known() else { return Known::Unknown };
                return Known::Yes(Some(Creation { entry: outer, site }));
            }
            owner = tree.parent(id);
        }
        let Some(root) = self.semantic.leaf(Leaf { target: Target::Node(NodeRef { node: tree.root, ..node }), ..entry }).known() else { return Known::Unknown };
        if entry == root { Known::Yes(None) } else { Known::Unknown }
    }

    pub fn entry_for(&self, at: Leaf) -> Known<Leaf> {
        let find = || self.find_entry(at);
        match self.semantic.world.profile() {
            Some(profile) => profile.time_effect_entry(find),
            None => find(),
        }
    }

    fn find_entry(&self, at: Leaf) -> Known<Leaf> {
        let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
        if let Some(&entry) = self.semantic.effects.entry_owners.borrow().get(&at) { return Known::Yes(entry) }
        let entry = self.search_entry(at);
        if let Known::Yes(entry) = entry { self.semantic.effects.entry_owners.borrow_mut().insert(at, entry); }
        entry
    }

    fn search_entry(&self, at: Leaf) -> Known<Leaf> {
        let Target::Node(node) = at.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        if tree.nodes.get(node.node as usize).is_none() { return Known::Unknown }
        let mut ancestry = Vec::new();
        let mut owner = Some(node.node);
        while let Some(id) = owner {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let owner_leaf = Leaf { target: Target::Node(NodeRef { node: id, ..node }), ..at };
            let Some(owner_leaf) = self.semantic.leaf(owner_leaf).known() else { return Known::Unknown };
            if owner_leaf.target != Target::Node(NodeRef { node: id, ..node }) { owner = tree.parent(id); continue }
            ancestry.push(id);
            let Some(entries) = self.entries(owner_leaf).known() else { return Known::Unknown };
            let closest = entries.into_iter().filter_map(|entry| {
                let Target::Node(entry_node) = entry.target else { return None };
                ancestry.iter().position(|&ancestor| ancestor == entry_node.node).map(|distance| (distance, entry))
            }).min_by_key(|&(distance, _)| distance);
            if let Some((_, entry)) = closest {
                return Known::Yes(entry);
            }
            owner = tree.parent(id);
        }
        self.semantic.leaf(Leaf { target: Target::Node(NodeRef { node: tree.root, ..node }), ..at })
    }

    /// `entry`'s flow graph, read events included.
    pub fn execution(&self, entry: Leaf) -> Known<Rc<Execution>> {
        self.graph(entry, true)
    }

    /// `entry`'s flow graph for narrowing, which reads no read events: built
    /// without them unless a full graph already exists (then that one, so
    /// one entry's narrowing always sees the same steps).
    pub fn narrowing_execution(&self, entry: Leaf) -> Known<Rc<Execution>> {
        let Some(entry) = self.semantic.leaf(entry).known() else { return Known::Unknown };
        if Self::force_reads() { return self.graph(entry, true) }
        let full = self.semantic.effects.graphs.borrow().get(&(entry, true)).cloned();
        if let Some(full @ Known::Yes(_)) = full {
            let mut graphs = self.semantic.effects.graphs.borrow_mut();
            return graphs.entry((entry, false)).or_insert(full).clone();
        }
        self.graph(entry, false)
    }

    /// `DXLINT_GRAPH_PER_FRAME=1`: every frame builds its own graph (A/B ruler).
    fn per_frame() -> bool {
        static PER_FRAME: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *PER_FRAME.get_or_init(|| std::env::var_os("DXLINT_GRAPH_PER_FRAME").is_some())
    }

    /// `DXLINT_GRAPH_READS=1`: narrowing builds full graphs (A/B ruler).
    fn force_reads() -> bool {
        static FORCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FORCE.get_or_init(|| std::env::var_os("DXLINT_GRAPH_READS").is_some())
    }

    fn graph(&self, entry: Leaf, reads: bool) -> Known<Rc<Execution>> {
        let Some(entry) = self.semantic.leaf(entry).known() else { return Known::Unknown };
        let state = &self.semantic.effects.graphs;
        let kept = state.borrow().get(&(entry, reads)).cloned();
        if let Some(graph) = kept {
            if self.semantic.effects.building.borrow().contains(&(entry, Some(reads))) { self.semantic.values().note_cycle_read() }
            return graph;
        }
        // A body built in another frame builds alike when the conditions its
        // build decided answer alike here (graphs read written values
        // through `Execution::owner`, so the rest doesn't depend on frames).
        if !Self::per_frame() {
            if let Some(graph) = self.replayed(entry, reads) {
                if let Some(profile) = self.semantic.world.profile() { crate::engine::Profile::add(&profile.effect_graph_frame_reuses, 1) }
                state.borrow_mut().insert((entry, reads), Known::Yes(graph.clone()));
                return Known::Yes(graph);
            }
        }
        state.borrow_mut().insert((entry, reads), Known::Unknown);
        self.semantic.effects.building.borrow_mut().insert((entry, Some(reads)));
        let mark = self.semantic.values().cycle_mark();
        let source = if reads { super::source::Runtime::new(self.semantic) } else { super::source::Runtime::without_reads(self.semantic) };
        // Graphs are kept for every later question: a build runs on an
        // allowance for its file's size, not on what the asking question
        // has left (big bodies were cut and rebuilt question after question,
        // and a small per-question budget could never build them).
        let nodes = match entry.target {
            Target::Node(node) => self.semantic.world.tree(node.file).map_or(0, |tree| tree.nodes.len()),
            _ => 0,
        };
        let allowance = u32::try_from(nodes).unwrap_or(u32::MAX).saturating_mul(Self::BUILD_STEPS_PER_NODE)
            .max(crate::grammar::names::question::Budget::DEFAULT);
        let build = || match self.semantic.question.budget.sized(allowance, || super::build::execution_sized(&source, entry, self.semantic.question).0) {
            Ok(graph) => graph,
            Err(cut) => {
                if cut == crate::grammar::names::question::Cut::RanOut { source.ran_out() }
                Known::Unknown
            }
        };
        let graph = match self.semantic.world.profile() {
            Some(profile) => {
                profile.time_effect_build(crate::engine::EffectBuild::Graph, false, build, |graph, elapsed| {
                    if matches!(graph, Known::Yes(_)) { return true }
                    if let Target::Node(node) = entry.target {
                        if let Some(path) = self.semantic.world.path(node.file) {
                            profile.record_effect_failure(path, node.node, source.failure(), elapsed);
                        }
                    }
                    false
                })
            }
            None => build(),
        };
        let graph = match graph {
            Known::Yes(graph) => {
                let decided = self.replayable(entry, source.decided());
                Known::Yes(self.shared(entry, reads, graph.built_in(entry.frame), decided))
            }
            Known::Unknown => Known::Unknown,
        };
        self.semantic.effects.building.borrow_mut().remove(&(entry, Some(reads)));
        if matches!(graph, Known::Unknown) && self.semantic.values().cycled_since(mark) {
            state.borrow_mut().remove(&(entry, reads));
        } else {
            state.borrow_mut().insert((entry, reads), graph.clone());
        }
        graph
    }

    /// `graph`, or the same graph another frame's build of the body already
    /// made: a body asked about in hundreds of frames mostly builds alike.
    fn shared(&self, entry: Leaf, reads: bool, graph: Execution, decided: Option<Rc<[(Target, u32, bool)]>>) -> Rc<Execution> {
        let mut variants = self.semantic.effects.variants.borrow_mut();
        let variants = variants.entry((entry.target, reads)).or_default();
        if let Some(profile) = self.semantic.world.profile() {
            if !variants.is_empty() { crate::engine::Profile::add(&profile.effect_graph_frame_rebuilds, 1) }
        }
        if let Some(same) = variants.iter().find(|variant| variant.graph.same_as(&graph)) {
            if let Some(profile) = self.semantic.world.profile() { crate::engine::Profile::add(&profile.effect_graph_frame_duplicates, 1) }
            return same.graph.clone();
        }
        let graph = Rc::new(graph);
        variants.push(Variant { graph: graph.clone(), decided });
        graph
    }

    /// A build's decided conditions by target, when each sat in the
    /// entry's own frame (so another entry frame can ask them again).
    fn replayable(&self, entry: Leaf, decided: Vec<(Leaf, u32, bool)>) -> Option<Rc<[(Target, u32, bool)]>> {
        decided.into_iter().map(|(at, condition, holds)| {
            let here = self.semantic.leaf(Leaf { target: at.target, frame: entry.frame }).known()?;
            (here == at).then_some((at.target, condition, holds))
        }).collect()
    }

    /// A graph another frame built for `entry`'s body whose decided
    /// conditions answer alike in `entry`'s frame.
    fn replayed(&self, entry: Leaf, reads: bool) -> Option<Rc<Execution>> {
        let built: Vec<_> = self.semantic.effects.variants.borrow().get(&(entry.target, reads))?.iter()
            .filter_map(|variant| Some((variant.graph.clone(), variant.decided.clone()?))).collect();
        'variants: for (graph, decided) in built {
            for &(target, condition, holds) in decided.iter() {
                let at = self.semantic.leaf(Leaf { target, frame: entry.frame }).known()?;
                match self.semantic.world.condition(at, condition, self.semantic) {
                    Known::Yes(answer) if answer == holds => {}
                    Known::Yes(_) => continue 'variants,
                    Known::Unknown => return None,
                }
            }
            return Some(graph);
        }
        None
    }

    pub fn normal(&self, entry: Leaf) -> Known<bool> {
        match self.execution(entry) {
            Known::Yes(graph) => graph.normal(&self.semantic.question.budget),
            Known::Unknown => Known::Unknown,
        }
    }

    pub fn declared(&self, entry: Leaf, effect: EffectId) -> Known<Vec<DeclaredEffect>> {
        let Some(entry) = self.semantic.leaf(entry).known() else { return Known::Unknown };
        let Target::Node(node) = entry.target else { return Known::Unknown };
        let Some(tree) = self.semantic.world.tree(node.file) else { return Known::Unknown };
        if tree.nodes.get(node.node as usize).is_none() { return Known::Unknown }
        let runtime = super::source::Runtime::new(self.semantic);
        let mut pending = vec![node.node];
        let mut declared = Vec::new();
        let mut unique = crate::fx_hash::FxHashSet::default();
        let mut visited = FxHashSet::default();
        while let Some(id) = pending.pop() {
            if !self.semantic.question.budget.step() { return Known::Unknown }
            let at = Leaf { target: Target::Node(NodeRef { node: id, ..node }), ..entry };
            let Some(at) = self.semantic.leaf(at).known() else { return Known::Unknown };
            if !visited.insert(at) { continue }
            if at != entry {
                match self.entry_for(at) {
                    Known::Yes(owner) if owner != entry => continue,
                    Known::Yes(_) => {}
                    Known::Unknown => return Known::Unknown,
                }
            }
            let Some(effects) = runtime.declarations(at, Some(effect)).known() else { return Known::Unknown };
            for emission in effects {
                let record = emission.declared;
                if unique.insert((record.site, record.target, record.value, record.possible)) { declared.push(record) }
            }
            let Some(writes) = runtime.writes(at, Some(effect)).known() else { return Known::Unknown };
            for (_, _, write) in writes {
                let value = match write.event.written {
                    Some(written) => {
                        let Some(_nested) = self.semantic.question.budget.nest() else { return Known::Unknown };
                        let Some(sites) = self.semantic.values().sites(written.owner, &written.recipe).known() else { return Known::Unknown };
                        if sites.len() > 1 { return Known::Unknown }
                        sites.first().copied()
                    }
                    None => None,
                };
                let record = DeclaredEffect {
                    site: at, target: write.event.target.map(|target| target.value), value, possible: write.possible,
                };
                if unique.insert((record.site, record.target, record.value, record.possible)) { declared.push(record) }
            }
            let Target::Node(at) = at.target else { return Known::Unknown };
            pending.extend(tree.children(at.node).iter().rev().map(|edge| edge.node));
        }
        Known::Yes(declared)
    }

    pub fn exits(&self, entry: Leaf) -> Known<Vec<ExecutedEffect>> {
        let Some(entry) = self.semantic.leaf(entry).known() else { return Known::Unknown };
        let Some(graph) = self.execution(entry).known() else { return Known::Unknown };
        let Some(exits) = graph.exits(&self.semantic.question.budget).known() else { return Known::Unknown };
        Known::Yes(exits.into_iter().map(|exit| ExecutedEffect {
            effect: exit.effect,
            site: Leaf { target: Target::Node(exit.site), ..entry },
            target: exit.target,
            value: exit.value.map(|value| Leaf { target: Target::Node(value), ..entry }),
        }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ast::Config;
    use crate::grammar::{Edge, Grammar, Node, Tree};
    use crate::grammar::names::{KindNames, Names, Resolved};
    use crate::grammar::names::effects::{Conditional, Flow};
    use crate::grammar::names::question::{FrameId, QueryContext, ScopeRef, Target, World};

    struct TestWorld {
        grammar: Grammar,
        tree: Tree,
        names: Resolved,
        profile: Option<crate::engine::Profile>,
        interrupt: std::cell::Cell<bool>,
        conditions: std::cell::Cell<u32>,
        unknown_condition: std::cell::Cell<bool>,
    }

    impl TestWorld {
        fn new() -> Self {
            Self {
                grammar: Grammar::build(&Config::default(), &mut Vec::new()),
                tree: Tree::from_native(vec![
                    Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 1, node: 1 }] },
                    Node { matcher: 1, start: 0, end: 0, children: vec![Edge { label: 2, node: 2 }] },
                    Node { matcher: 2, start: 0, end: 0, children: vec![Edge { label: 3, node: 3 }] },
                    Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
                ], 0),
                names: Resolved::default(),
                profile: None,
                interrupt: std::cell::Cell::new(false),
                conditions: std::cell::Cell::new(0),
                unknown_condition: std::cell::Cell::new(false),
            }
        }
    }

    impl World for TestWorld {
        fn profile(&self) -> Option<&crate::engine::Profile> { self.profile.as_ref() }
        fn path(&self, file: u32) -> Option<&std::path::Path> {
            (file == 0).then(|| std::path::Path::new("effects.test"))
        }
        fn grammar(&self) -> &Grammar { &self.grammar }
        fn tree(&self, file: u32) -> Option<&Tree> { (file == 0).then_some(&self.tree) }
        fn text(&self, file: u32) -> Option<&str> { (file == 0).then_some("") }
        fn names(&self, file: u32) -> Option<&Resolved> { (file == 0).then_some(&self.names) }
        fn index(&self, _: u32) -> Option<&crate::grammar::names::ScopeIndex> { None }
        fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> {
            std::borrow::Cow::Borrowed(text)
        }
        fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Yes(None) }
        fn condition(&self, at: Leaf, _: u32, semantic: &Semantic<'_>) -> Known<bool> {
            self.conditions.set(self.conditions.get() + 1);
            if self.interrupt.get() {
                while semantic.question.budget.step() {}
                return Known::Unknown;
            }
            if self.unknown_condition.get() { return Known::Unknown }
            Known::Yes(at.frame != FrameId::ROOT)
        }
        fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
            Known::Unknown
        }
        fn operand(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
            Known::Unknown
        }
    }

    fn leaf(node: u32) -> Leaf {
        Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT }
    }

    fn tables() -> Names {
        Names { kinds: vec![KindNames::default(); 4], ..Names::default() }
    }

    #[test]
    fn write_origins_preserve_declaration_order_with_possible_throws() {
        for write_before_throw in [false, true] {
            let mut world = TestWorld::new();
            world.tree = Tree::from_native(vec![
                Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 1, node: 1 }, Edge { label: 4, node: 3 }] },
                Node { matcher: 1, start: 0, end: 0, children: vec![Edge { label: 2, node: 2 }] },
                Node { matcher: 2, start: 0, end: 0, children: Vec::new() },
                Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
            ], 0);
            let mut tables = tables();
            tables.effect_roles = vec![super::super::EffectRole::Control(super::super::Control::Thrown),
                super::super::EffectRole::Event { writes: true }];
            tables.kinds[0].execution.runs.push(conditional(Flow::Field(vec![1]), None));
            tables.kinds[0].execution.catches.push(conditional(super::super::Catch {
                handler: vec![4], source: vec![1], effects: vec![0],
            }, None));
            tables.kinds[1].execution.runs.push(conditional(Flow::Nothing, None));
            tables.kinds[1].unwraps.push(crate::grammar::names::Unwrapping {
                effect: Some(0), possible: true, item: u32::from(write_before_throw), ..Default::default()
            });
            tables.kinds[1].effects.push(crate::grammar::names::Effect {
                item_id: Some(u32::from(!write_before_throw)),
                effect: 1, path: vec![2], when: None, value: None, possible: false,
            });
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            let graph = semantic.effects().execution(leaf(0)).known().expect("execution");
            let handler = graph.sites(NodeRef::here(3), super::super::Phase::Enter).next().expect("handler entry");
            assert_eq!(graph.reaches_through(graph.entry(), handler, &[],
                |event| event.kind == super::super::EventKind::Write, &question.budget), Known::Yes(write_before_throw));
        }
    }

    fn conditional<T>(item: T, when: Option<u32>) -> Conditional<T> {
        Conditional {
            item, when, item_id: 0,
            span: crate::config::source::Span::new(crate::config::source::FileId(0), 0, 1),
        }
    }

    fn read_world(external: bool) -> (TestWorld, Names) {
        let mut world = TestWorld::new();
        world.tree = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 0, children: vec![
                Edge { label: 1, node: 1 }, Edge { label: 2, node: 2 },
            ] },
            Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
            Node { matcher: crate::grammar::parse::TEXT, start: 0, end: 0, children: vec![
                Edge { label: crate::pattern::compile::NO_LABEL, node: 4 },
            ] },
            Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 2, start: 0, end: 0, children: vec![Edge { label: 3, node: 3 }] },
        ], 0);
        world.names.declaration.insert(1, 1);
        world.names.declaration.insert(3, 4);
        world.names.merged.insert(4, 1);
        let mut tables = tables();
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["assigned".into(), "read".into()], ..Default::default()
        });
        tables.effect_roles = super::super::compile::effect_roles(&tables.vocabulary);
        tables.kinds[1].effects.push(crate::grammar::names::Effect {
            effect: 0, path: Vec::new(), when: None, value: None, possible: false, item_id: None,
        });
        tables.kinds[2].execution.runs.push(conditional(Flow::Nothing, None));
        tables.kinds[2].unwraps.push(crate::grammar::names::Unwrapping {
            effect: Some(1), value: Some(crate::grammar::names::Value::Path(vec![3])), ..Default::default()
        });
        if external {
            tables.kinds[0].execution.runs.push(conditional(Flow::Field(vec![1]), None));
            tables.kinds[0].execution.entries.push(conditional(vec![2], None));
        }
        (world, tables)
    }

    #[test]
    fn declared_read_targets_need_not_be_executed_syntax_and_other_entries_still_count() {
        for external in [false, true] {
            let (world, tables) = read_world(external);
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            let effects = semantic.effects();
            assert_eq!(effects.has_reads(leaf(1)), Known::Yes(true));
            assert_eq!(effects.value_used(leaf(1)), Known::Yes(true));
            assert_eq!(effects.value_used(leaf(1)), Known::Yes(true));
            let graph = effects.execution(leaf(0)).known().expect("outer execution");
            assert_eq!(graph.reachable(NodeRef::here(3), &question.budget), Known::Yes(false));
        }
    }

    #[test]
    fn a_graph_that_ran_out_stays_unknown_and_completed_effects_stay_kept() {
        use crate::engine::Profile;
        use crate::grammar::names::semantic::Session;

        let (mut world, mut tables) = read_world(true);
        world.profile = Some(Profile::default());
        tables.kinds[2].execution.runs.push(conditional(Flow::Field(vec![3]), Some(0)));
        let session = Session::default();
        assert_eq!(session.run(&world, &tables, |semantic| semantic.effects().value_used(leaf(1))), Known::Yes(true));
        let profile = world.profile.as_ref().expect("profile");
        assert_eq!(Profile::get(&profile.effect_graph_builds), 1);
        assert_eq!(Profile::get(&profile.effect_read_catalogue_builds), 1);

        world.interrupt.set(true);
        assert!(matches!(session.run(&world, &tables, |semantic| semantic.effects().execution(leaf(4))), Known::Unknown));
        world.interrupt.set(false);
        assert_eq!(session.run(&world, &tables, |semantic| semantic.effects().value_used(leaf(1))), Known::Yes(true));
        assert_eq!(Profile::get(&profile.effect_graph_builds), 2);
        assert_eq!(Profile::get(&profile.effect_read_catalogue_builds), 1);
        // The graph's build ran out of its own allowance: that is its answer.
        assert!(matches!(session.run(&world, &tables, |semantic| semantic.effects().execution(leaf(4))), Known::Unknown));
        assert_eq!(Profile::get(&profile.effect_graph_builds), 2);
    }

    #[test]
    fn failed_graph_profiles_distinguish_limits_shapes_conditions_and_dependencies() {
        use super::super::build::Failure;

        for failure in [Failure::Budget, Failure::Shape, Failure::Condition, Failure::Dependency] {
            let mut world = TestWorld::new();
            world.profile = Some(crate::engine::Profile::default());
            let mut tables = tables();
            match failure {
                Failure::Budget | Failure::Condition => {
                    tables.kinds[0].execution.runs.push(conditional(Flow::Nothing, Some(0)));
                    world.interrupt.set(failure == Failure::Budget);
                    world.unknown_condition.set(failure == Failure::Condition);
                }
                Failure::Shape => {
                    tables.kinds[0].execution.runs.push(conditional(Flow::Nothing, None));
                    let mut conflict = conditional(Flow::Children, None);
                    conflict.item_id = 1;
                    tables.kinds[0].execution.runs.push(conflict);
                }
                Failure::Dependency => {
                    tables.kinds[0].execution.runs.push(conditional(Flow::Repeat {
                        body: Box::new(Flow::Nothing), test: None, truth: Some(0), next: None, first: false,
                    }, None));
                }
            }
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            assert!(matches!(semantic.effects().execution(leaf(0)), Known::Unknown));
            let rows = world.profile.as_ref().expect("profile").effect_failures();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].2, failure);
            assert_eq!(rows[0].3.calls, 1);
        }
    }

    #[test]
    fn entry_lookup_reuses_completed_answers_without_reusing_interrupted_or_other_frames() {
        use crate::grammar::names::semantic::Session;

        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[1].execution.entries.push(conditional(vec![2], Some(0)));
        let session = Session::default();
        let lookup = |frame| session.run(&world, &tables, |semantic| {
            let frame = if frame {
                semantic.question.bind(FrameId::ROOT, NodeRef::here(3), Some(leaf(1)), None)
            } else { FrameId::ROOT };
            semantic.effects().entry_for(Leaf { frame, ..leaf(3) })
        });

        assert_eq!(lookup(false), Known::Yes(leaf(0)));
        let completed_conditions = world.conditions.get();
        assert_eq!(lookup(false), Known::Yes(leaf(0)));
        assert_eq!(world.conditions.get(), completed_conditions);

        world.interrupt.set(true);
        assert_eq!(lookup(true), Known::Unknown);
        world.interrupt.set(false);
        let after_cut = world.conditions.get();
        assert_eq!(lookup(false), Known::Yes(leaf(0)));
        assert_eq!(world.conditions.get(), after_cut);
        let Known::Yes(entry) = lookup(true) else { panic!("interrupted entry must retry") };
        assert_eq!(entry.target, leaf(2).target);
        assert_ne!(entry.frame, FrameId::ROOT);
        assert!(world.conditions.get() > after_cut);
        let completed_conditions = world.conditions.get();
        assert_eq!(lookup(true), Known::Yes(entry));
        assert_eq!(world.conditions.get(), completed_conditions);
    }

    #[test]
    fn capture_wrapped_entries_keep_execution_and_creation_identity() {
        let mut world = TestWorld::new();
        world.tree = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 1, node: 1 }] },
            Node { matcher: crate::grammar::parse::TEXT, start: 0, end: 0, children: vec![
                Edge { label: crate::pattern::compile::NO_LABEL, node: 2 },
            ] },
            Node { matcher: 1, start: 0, end: 0, children: vec![Edge { label: 2, node: 3 }] },
            Node { matcher: crate::grammar::parse::TEXT, start: 0, end: 0, children: vec![
                Edge { label: crate::pattern::compile::NO_LABEL, node: 4 },
            ] },
            Node { matcher: 2, start: 0, end: 0, children: vec![Edge { label: 3, node: 5 }] },
            Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
        ], 0);
        let mut tables = tables();
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["returned".into()], ..Default::default()
        });
        tables.effect_roles = super::super::compile::effect_roles(&tables.vocabulary);
        tables.kinds[1].execution.runs.push(conditional(Flow::Nothing, None));
        tables.kinds[1].execution.entries.push(conditional(vec![2], None));
        tables.kinds[3].unwraps.push(crate::grammar::names::Unwrapping {
            effect: Some(0), ..Default::default()
        });
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let effects = semantic.effects();
        assert_eq!(effects.entries(leaf(1)), Known::Yes(vec![leaf(4)]));
        assert_eq!(effects.entry_for(leaf(5)), Known::Yes(leaf(4)));
        assert_eq!(effects.enclosing(leaf(3)), Known::Yes(Some(Creation { entry: leaf(0), site: leaf(2) })));
        assert_eq!(effects.normal(leaf(3)), Known::Yes(false));
        assert_eq!(effects.exits(leaf(3)), Known::Yes(vec![
            ExecutedEffect { effect: 0, site: leaf(5), target: None, value: None },
        ]));
        assert_eq!(effects.execution(leaf(3)).known().expect("body execution")
            .reachable(NodeRef::here(5), &question.budget), Known::Yes(true));
    }

    #[test]
    fn enclosing_tracks_the_compiled_creation_path_not_the_body_parent() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[1].execution.entries.push(conditional(vec![2, 3], None));
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.effects().enclosing(leaf(3)), Known::Yes(Some(Creation {
            entry: leaf(0), site: leaf(1),
        })));
        assert_eq!(semantic.effects().enclosing(leaf(0)), Known::Yes(None));
    }

    #[test]
    fn enclosing_does_not_invent_a_capture_from_the_callers_frame() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[1].execution.entries.push(conditional(vec![2, 3], None));
        let question = QueryContext::default();
        let frame = question.bind(FrameId::ROOT, NodeRef::here(3), None, None);
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.effects().enclosing(Leaf { frame, ..leaf(3) }), Known::Unknown);
    }

    #[test]
    fn nearest_separate_entry_does_not_run_in_its_declaration() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[1].execution.runs.push(conditional(Flow::Nothing, None));
        tables.kinds[1].execution.entries.push(conditional(vec![2], None));
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let effects = semantic.effects();
        assert_eq!(effects.entry_for(leaf(3)), Known::Yes(leaf(2)));
        assert_eq!(effects.entry_for(leaf(1)), Known::Yes(leaf(0)));
        let outer = effects.execution(leaf(0)).known().expect("outer execution must be known");
        assert_eq!(outer.reachable(NodeRef::here(3), &question.budget), Known::Yes(false));
        let body = effects.execution(leaf(2)).known().expect("separate execution must be known");
        assert_eq!(body.reachable(NodeRef::here(3), &question.budget), Known::Yes(true));
    }

    #[test]
    fn conditional_execution_memos_keep_instantiation_frames_distinct() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[0].execution.runs.push(conditional(Flow::Nothing, Some(0)));
        let question = QueryContext::default();
        let framed = Leaf {
            frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None),
            ..leaf(0)
        };
        let semantic = Semantic::new(&world, &tables, &question);
        for (entry, reachable) in [(leaf(0), true), (framed, false), (leaf(0), true)] {
            let graph = semantic.effects().execution(entry).known().expect("execution must be known");
            assert_eq!(graph.reachable(NodeRef::here(3), &question.budget), Known::Yes(reachable));
        }
    }

    #[test]
    fn active_conditional_shapes_report_a_defect_instead_of_choosing_one() {
        let world = TestWorld::new();
        let mut tables = tables();
        let first = conditional(Flow::Nothing, Some(0));
        let mut second = conditional(Flow::Children, Some(0));
        second.item_id = 1;
        tables.kinds[0].execution.runs.extend([first, second]);
        let question = QueryContext::default();
        let entry = Leaf {
            frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None),
            ..leaf(0)
        };
        let semantic = Semantic::new(&world, &tables, &question);
        assert!(matches!(semantic.effects().execution(entry), Known::Unknown));
        assert_eq!(semantic.defects().len(), 1);
    }

    #[test]
    fn declared_effects_include_dead_sites_but_exclude_separate_entries() {
        let mut world = TestWorld::new();
        world.tree = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 0, children: vec![
                Edge { label: 1, node: 1 }, Edge { label: 2, node: 2 }, Edge { label: 4, node: 4 },
            ] },
            Node { matcher: 1, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 2, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 4, start: 0, end: 0, children: vec![Edge { label: 5, node: 5 }] },
            Node { matcher: 5, start: 0, end: 0, children: Vec::new() },
        ], 0);
        let mut tables = Names { kinds: vec![KindNames::default(); 6], ..Names::default() };
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["returned".into()], ..Default::default()
        });
        tables.effect_roles = super::super::compile::effect_roles(&tables.vocabulary);
        tables.kinds[4].execution.runs.push(conditional(Flow::Nothing, None));
        tables.kinds[4].execution.entries.push(conditional(vec![5], None));
        for kind in [1, 2, 5] {
            tables.kinds[kind].unwraps.push(crate::grammar::names::Unwrapping {
                effect: Some(0), ..Default::default()
            });
        }
        tables.kinds[2].unwraps[0].possible = true;
        let question = QueryContext::default();
        let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None);
        let entry = Leaf { frame, ..leaf(0) };
        let at = |node| Leaf { frame, ..leaf(node) };
        let semantic = Semantic::new(&world, &tables, &question);
        let effects = semantic.effects();
        assert_eq!(effects.declared(entry, 0), Known::Yes(vec![
            DeclaredEffect { site: at(1), target: None, value: None, possible: false },
            DeclaredEffect { site: at(2), target: None, value: None, possible: true },
        ]));
        assert_eq!(effects.exits(entry), Known::Yes(vec![
            ExecutedEffect { effect: 0, site: at(1), target: None, value: None },
        ]));
        assert_eq!(effects.normal(entry), Known::Yes(false));
        assert_eq!(effects.declared(at(5), 0), Known::Yes(vec![
            DeclaredEffect { site: at(5), target: None, value: None, possible: false },
        ]));
    }

    #[test]
    fn active_conditional_finalizers_report_a_defect() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.kinds[0].execution.runs.push(conditional(Flow::Field(vec![1]), None));
        let first = conditional(super::super::After { finalizer: vec![2], sources: vec![vec![1]] }, Some(0));
        let mut second = conditional(super::super::After { finalizer: vec![3], sources: vec![vec![1]] }, Some(0));
        second.item_id = 1;
        tables.kinds[0].execution.after.extend([first, second]);
        let question = QueryContext::default();
        let entry = Leaf {
            frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None),
            ..leaf(0)
        };
        let semantic = Semantic::new(&world, &tables, &question);
        assert!(matches!(semantic.effects().execution(entry), Known::Unknown));
        assert_eq!(semantic.defects().len(), 1);
    }

    #[test]
    fn nearest_effect_targets_use_field_limited_scope_membership() {
        let mut world = TestWorld::new();
        world.tree = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 1, node: 1 }] },
            Node { matcher: 1, start: 0, end: 0, children: vec![Edge { label: 2, node: 2 }, Edge { label: 3, node: 3 }] },
            Node { matcher: 2, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
        ], 0);
        let mut tables = tables();
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["returned".into()], scope_classes: vec!["function".into()], ..Default::default()
        });
        tables.effect_roles = super::super::compile::effect_roles(&tables.vocabulary);
        for (kind, within) in [(0, Vec::new()), (1, vec![2])] {
            tables.kinds[kind].scopes.push(crate::grammar::names::OpenScope { classes: 1, within, when: None });
        }
        tables.kinds[1].execution.runs.push(conditional(Flow::Field(vec![3]), None));
        tables.kinds[1].execution.entries.push(conditional(vec![2], None));
        for kind in [2, 3] {
            tables.kinds[kind].unwraps.push(crate::grammar::names::Unwrapping { effect: Some(0), nearest: 1, ..Default::default() });
        }
        world.names = tables.resolve(&world.tree, "", &std::borrow::Cow::Borrowed, &|_, _| true, &|_, _, _| Err("no operand".into()));
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        assert_eq!(semantic.effects().declared(leaf(2), 0), Known::Yes(vec![
            DeclaredEffect { site: leaf(2), target: Some(NodeRef::here(1)), value: None, possible: false },
        ]));
        assert_eq!(semantic.effects().declared(leaf(0), 0), Known::Yes(vec![
            DeclaredEffect { site: leaf(3), target: Some(NodeRef::here(0)), value: None, possible: false },
        ]));
    }

    #[test]
    fn possible_target_writes_preserve_a_path_to_the_following_use() {
        let mut world = TestWorld::new();
        world.tree = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 0, children: vec![Edge { label: 1, node: 1 }, Edge { label: 3, node: 3 }] },
            Node { matcher: 1, start: 0, end: 0, children: vec![Edge { label: 2, node: 2 }] },
            Node { matcher: 2, start: 0, end: 0, children: Vec::new() },
            Node { matcher: 3, start: 0, end: 0, children: Vec::new() },
        ], 0);
        let mut tables = tables();
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["assigned".into()], ..Default::default()
        });
        tables.effect_roles = super::super::compile::effect_roles(&tables.vocabulary);
        tables.kinds[1].effects.push(crate::grammar::names::Effect {
            item_id: Some(0),
            effect: 0, path: vec![2], when: None, value: None, possible: true,
        });
        tables.kinds[1].unwraps.push(crate::grammar::names::Unwrapping {
            effect: Some(0), possible: true, value: Some(crate::grammar::names::Value::Path(vec![2])),
            ..Default::default()
        });
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        let graph = semantic.effects().execution(leaf(0)).known().expect("possible write execution must be known");
        let writes: Vec<_> = graph.steps().filter_map(|(id, step)| {
            step.event().filter(|event| event.kind == super::super::EventKind::Write).map(|_| id)
        }).collect();
        assert_eq!(writes.len(), 1);
        let use_site = graph.sites(NodeRef::here(3), super::super::Phase::Enter).next().expect("following use");
        assert_eq!(graph.reaches(graph.entry(), use_site, &writes, &question.budget), Known::Yes(true));
        assert_eq!(graph.reaches_through(graph.entry(), use_site, &[], |event| {
            event.kind == super::super::EventKind::Write
        }, &question.budget), Known::Yes(true));
    }

    #[test]
    fn assignment_events_keep_written_recipes_in_their_owner_frame() {
        let world = TestWorld::new();
        let mut tables = tables();
        tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
            effects: vec!["assigned".into()], ..Default::default()
        });
        tables.effect_roles = super::super::compile::effect_roles(&tables.vocabulary);
        tables.kinds[0].effects.push(crate::grammar::names::Effect {
            item_id: Some(0),
            effect: 0, path: vec![1], when: None, possible: false,
            value: Some(std::sync::Arc::new(crate::grammar::names::Value::Path(vec![1, 2]))),
        });
        let question = QueryContext::default();
        let framed = Leaf {
            frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None),
            ..leaf(0)
        };
        let semantic = Semantic::new(&world, &tables, &question);
        for entry in [leaf(0), framed] {
            let graph = semantic.effects().execution(entry).known().expect("assignment execution must be known");
            let writes: Vec<_> = graph.steps().filter_map(|(_, step)| step.event())
                .filter(|event| event.kind == super::super::EventKind::Write).collect();
            assert_eq!(writes.len(), 1);
            assert_eq!(writes[0].target.expect("write target").value, NodeRef::here(1));
            assert_eq!(writes[0].phase, super::super::Phase::Leave);
            let written = writes[0].written.as_ref().expect("written value recipe");
            let owner = graph.owner(written, entry.frame);
            assert_eq!(owner, entry);
            assert_eq!(semantic.values().sites(owner, &written.recipe), Known::Yes(vec![
                Leaf { frame: entry.frame, ..leaf(2) },
            ]));
        }
    }

    #[test]
    fn proof_effects_do_not_enter_execution_or_declared_events() {
        for targets in [false, true] {
            let world = TestWorld::new();
            let mut tables = tables();
            tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
                effects: vec!["truthy".into()], ..Default::default()
            });
            tables.effect_roles = super::super::compile::effect_roles(&tables.vocabulary);
            tables.kinds[0].unwraps.push(crate::grammar::names::Unwrapping {
                effect: Some(0),
                value: Some(crate::grammar::names::Value::Excludes { traits: 1, from: None }),
                ..Default::default()
            });
            if targets {
                tables.kinds[0].effects.push(crate::grammar::names::Effect {
                    item_id: Some(0),
                    effect: 0, path: vec![1], when: None, value: None, possible: false,
                });
            }
            let question = QueryContext::default();
            let semantic = Semantic::new(&world, &tables, &question);
            assert_eq!(semantic.effects().normal(leaf(0)), Known::Yes(true));
            assert_eq!(semantic.effects().declared(leaf(0), 0), Known::Yes(Vec::new()));
            assert_eq!(semantic.effects().exits(leaf(0)), Known::Yes(Vec::new()));
        }
    }

    #[test]
    fn conflicting_conditional_terminators_report_their_item() {
        for unavailable_payload in [false, true] {
            let world = TestWorld::new();
            let mut tables = tables();
            tables.vocabulary = std::sync::Arc::new(crate::grammar::names::Vocabulary {
                effects: vec!["returned".into(), "thrown".into()], ..Default::default()
            });
            tables.effect_roles = super::super::compile::effect_roles(&tables.vocabulary);
            for effect in [0, 1] {
                tables.kinds[0].unwraps.push(crate::grammar::names::Unwrapping {
                    effect: Some(effect), item: effect as u32, when: Some(0),
                    value: unavailable_payload.then(|| crate::grammar::names::Value::Path(vec![crate::grammar::names::ATTR_STEP])),
                    ..Default::default()
                });
            }
            let question = QueryContext::default();
            let entry = Leaf {
                frame: question.bind(FrameId::ROOT, NodeRef::here(0), Some(leaf(1)), None),
                ..leaf(0)
            };
            let semantic = Semantic::new(&world, &tables, &question);
            assert!(matches!(semantic.effects().execution(entry), Known::Unknown));
            assert_eq!(semantic.defects().len(), 1);
            assert_eq!(semantic.defects()[0].item, 1);
        }
    }
}

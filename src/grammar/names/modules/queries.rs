use super::super::question::{FrameId, Known, Leaf, NodeRef, QueryContext, ScopeRef, Target};
use super::super::semantic::Semantic;
use super::compile::Action;
use super::index::PublicationKind;
use super::{Entry, Memo, Modules, SharedQuestion, Source};
use crate::expr::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

mod preparation;
use preparation::{Batch, PreparedItem};
pub use preparation::ClassAnswers;

type Answer = Known<Option<Vec<Target>>>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Request {
    Definition(NodeRef, u64),
    Global(u32, Box<str>, u64),
}

#[derive(Default)]
pub(crate) struct State {
    answers: RefCell<BTreeMap<Request, Answer>>,
    star_filters: RefCell<BTreeMap<(ScopeRef, Box<str>), Known<bool>>>,
    publications: RefCell<BTreeMap<ScopeRef, bool>>,
    graph: Memo,
}

/// Module queries timed in `--profile` (inclusive, nested calls overlap).
pub(crate) const PROFILE_QUERIES: &[&str] = &[
    "modules.definition", "modules.global", "modules.publication", "modules.condition", "modules.operand", "modules.targets",
    "modules.entries", "modules.prepare", "modules.scopes", "modules.roots", "modules.resolve", "modules.resolve.unknown",
    "modules.scopes.catalog", "modules.scopes.named", "modules.scopes.augmentations",
];

pub struct Queries<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
    batch: Option<&'a Batch>,
}

impl<'a> Queries<'a> {
    pub(crate) fn new(semantic: &'a Semantic<'a>) -> Self {
        Self { semantic, batch: None }
    }

    pub fn binding_has_class(&self, at: NodeRef, key: &str, class: u64) -> Known<bool> {
        let present = || -> Result<bool, ()> {
            self.spend()?;
            let world = self.semantic.world;
            let names = world.names(at.file).ok_or(())?;
            let tree = world.tree(at.file).ok_or(())?;
            let source = world.text(at.file).ok_or(())?;
            if !names.failures.is_empty() { return Err(()) }
            let key = world.name_key(at.file, key);
            // An import whose target lacks the class leaves it to the next
            // declaration (`import { T }` of a type beside `const T`).
            let mut skipped = Vec::new();
            while let Some(declaration) = names.visible_except(tree, source, &|text| world.name_key(at.file, text), at.node, &key, class, &skipped) {
                self.spend()?;
                match self.definition(NodeRef { file: at.file, node: declaration }, class).known().ok_or(())? {
                    Some(targets) if targets.is_empty() => skipped.push(declaration),
                    Some(_) | None => return Ok(true),
                }
            }
            Ok(!self.global(at.file, &key, class).known().ok_or(())?.is_empty())
        };
        present().map_or(Known::Unknown, Known::Yes)
    }

    pub fn export_has_class(&self, scope: ScopeRef, name: &str, class: u64) -> Known<bool> {
        match self.namespace_members(scope, name, class) {
            Known::Yes(Some(targets)) => Known::Yes(!targets.is_empty()),
            _ => Known::Unknown,
        }
    }

    pub fn publication_names(&self, scope: ScopeRef) -> Known<Option<Vec<Box<str>>>> {
        match self.is_publication(scope) {
            Ok(true) => match self.export_names(scope) {
                Known::Yes(names) => Known::Yes(Some(names)),
                Known::Unknown => Known::Unknown,
            },
            Ok(false) => Known::Yes(None),
            Err(()) => Known::Unknown,
        }
    }

    pub fn export_names(&self, scope: ScopeRef) -> Known<Vec<Box<str>>> {
        enum Visit { Enter(ScopeRef), Exit(ScopeRef) }
        let collect = || -> Result<Vec<Box<str>>, ()> {
            let mut active = BTreeSet::new();
            let mut complete = BTreeSet::new();
            let mut names = BTreeSet::new();
            let mut visits = vec![Visit::Enter(scope)];
            while let Some(visit) = visits.pop() {
                self.spend()?;
                let scope = match visit {
                    Visit::Enter(scope) => scope,
                    Visit::Exit(scope) => { active.remove(&scope); complete.insert(scope); continue; }
                };
                if active.contains(&scope) { return Err(()) }
                if complete.contains(&scope) { continue }
                active.insert(scope);
                visits.push(Visit::Exit(scope));
                let scopes = self.semantic.world.module_scopes(scope, self.semantic).known().ok_or(())?;
                for scope in scopes {
                    for PreparedItem { node, item, target: prepared } in self.items(scope, PublicationKind::Exports)? {
                        let Action::Exports { target, module, alias, .. } = item.action else { continue };
                        if !self.holds(node, item.when)? { continue }
                        if let Some(target) = target {
                            if let Some(alias) = alias {
                                let alias = self.operand(node, alias)?;
                                names.insert(text(&alias).ok_or(())?.into());
                            } else {
                                let target = self.target_value(node, target, prepared)?;
                                if let Value::Str(name) = &target {
                                    names.insert((&**name).into());
                                } else {
                                    let world = self.semantic.world;
                                    let tree = world.tree(node.file).ok_or(())?;
                                    let source = world.text(node.file).ok_or(())?;
                                    for name in self.target_nodes(node.file, &target)? {
                                        let syntax = tree.nodes.get(name as usize).ok_or(())?;
                                        let text = source.get(syntax.start as usize..syntax.end as usize).ok_or(())?;
                                        names.insert(world.name_key(node.file, text).into_owned().into());
                                    }
                                }
                            }
                        } else {
                            let source = self.operand(node, module.ok_or(())?)?;
                            let source = self.resolve(node, text(&source).ok_or(())?).known().ok_or(())?.ok_or(())?;
                            visits.push(Visit::Enter(source));
                        }
                    }
                }
            }
            Ok(names.into_iter().collect())
        };
        collect().map_or(Known::Unknown, Known::Yes)
    }

    fn resolve(&self, at: NodeRef, specifier: &str) -> Known<Option<ScopeRef>> {
        let answer = self.profiled("modules.resolve", || self.semantic.world.module(leaf(at), specifier, self.semantic));
        // Unknown resolutions are never memoised: count them apart.
        if answer == Known::Unknown { self.profiled("modules.resolve.unknown", || ()) }
        answer
    }

    fn profiled<T>(&self, query: &'static str, run: impl FnOnce() -> T) -> T {
        let _owner = crate::alloc::heap::Owner::enter(crate::alloc::heap::MODULES);
        match self.semantic.world.profile() {
            Some(profile) => profile.time_semantic_query(query, run),
            None => run(),
        }
    }

    pub fn definition(&self, name: NodeRef, class: u64) -> Answer {
        self.profiled("modules.definition", || self.definition_uncounted(name, class))
    }

    fn definition_uncounted(&self, name: NodeRef, class: u64) -> Answer {
        /// How far above an undeclared name a `resolves` item may sit
        /// (specifier, clause, statement).
        const UNDECLARED_RESOLVE_DEPTH: usize = 3;
        self.cached(Request::Definition(name, class), || {
            let world = self.semantic.world;
            let names = world.names(name.file).ok_or(())?;
            let tree = world.tree(name.file).ok_or(())?;
            let declaration = names.declaration_in(name.node, class);
            // A name nothing declares can still resolve through its own
            // syntax: a re-export (`export { x } from "m"`) binds no local
            // name but names the module's member. Look a few levels up for a
            // `resolves` item binding it.
            let owner = match names.declared.get(&declaration) {
                Some(&(_, owner, _, _)) => owner,
                None if declaration == name.node => {
                    let mut owner = declaration;
                    for _ in 0..UNDECLARED_RESOLVE_DEPTH {
                        let Some(parent) = tree.parent(owner) else { break };
                        owner = parent;
                    }
                    owner
                }
                None => return Ok(None),
            };
            let mut at = declaration;
            let mut handled = false;
            let mut targets = Vec::new();
            loop {
                self.spend()?;
                let node = NodeRef { file: name.file, node: at };
                let kind = self.semantic.tables.kinds.get(tree.nodes.get(at as usize).ok_or(())?.matcher as usize).ok_or(())?;
                for item in &kind.modules.items {
                    let Action::Resolves { classes, asks, binding, module, imported } = &item.action else { continue };
                    if !self.holds(node, item.when)? { continue }
                    let bindings = self.operand(node, *binding)?;
                    if !self.target_nodes(node.file, &bindings)?.contains(&declaration) { continue }
                    handled = true;
                    if classes & class == 0 { continue }
                    let source = self.operand(node, *module)?;
                    let Some(source) = text(&source) else { continue };
                    let Some(scope) = self.resolve(node, source).known().ok_or(())? else { continue };
                    let imported = imported.map(|operand| self.operand(node, operand)).transpose()?;
                    let key = if let Some(imported) = imported.as_ref().and_then(text) {
                        imported.to_owned()
                    } else {
                        let node = tree.nodes.get(declaration as usize).ok_or(())?;
                        let source = world.text(name.file).ok_or(())?.get(node.start as usize..node.end as usize).ok_or(())?;
                        world.name_key(name.file, source).into_owned()
                    };
                    if key == "*" {
                        targets.push(Target::Scope(scope));
                    } else {
                        targets.extend(self.ask(scope, &key, self.asks(*asks, class))?);
                    }
                }
                if at == owner { break }
                let Some(parent) = tree.parent(at) else { break };
                at = parent;
            }
            if !handled && name.node != declaration {
                let target = Target::Node(NodeRef { file: name.file, node: declaration });
                if self.merges(target, class)? {
                    let syntax = tree.nodes.get(declaration as usize).ok_or(())?;
                    let text = world.text(name.file).ok_or(())?.get(syntax.start as usize..syntax.end as usize).ok_or(())?;
                    let key = world.name_key(name.file, text);
                    let mut scope = names.scope_of(declaration).ok_or(())?;
                    loop {
                        let current = ScopeRef { file: name.file, scope };
                        if self.is_publication(current)? {
                            let published = self.global_scope(current, &key, class).known().ok_or(())?.ok_or(())?;
                            if published.contains(&target) {
                                targets = self.global(name.file, &key, class).known().ok_or(())?;
                                handled = true;
                            }
                            break;
                        }
                        let Some(parent) = names.scopes.get(scope as usize).ok_or(())?.parent else { break };
                        scope = parent;
                    }
                }
            }
            targets.sort_unstable();
            targets.dedup();
            Ok(handled.then_some(targets))
        })
    }

    pub fn namespace_members(&self, scope: ScopeRef, key: &str, class: u64) -> Answer {
        match self.is_publication(scope) {
            Ok(true) => {}
            Ok(false) => return Known::Yes(None),
            Err(()) => return Known::Unknown,
        }
        self.publication(scope, key, class)
    }

    pub fn global(&self, importer: u32, name: &str, class: u64) -> Known<Vec<Target>> {
        self.profiled("modules.global", || self.semantic.world.shared_modules(
            SharedQuestion::Global { name, class },
            &mut || self.global_uncounted(importer, name, class),
        ))
    }

    fn global_uncounted(&self, importer: u32, name: &str, class: u64) -> Known<Vec<Target>> {
        match self.cached(Request::Global(importer, name.into(), class), || {
            let scopes = self.global_scopes(importer, name)?;
            let world = self.semantic.world;
            let mut targets = Vec::new();
            for scope in scopes {
                // Roots whose file cannot publish the name contribute nothing;
                // skip them before any per-scope publication work.
                if !world.module_index(scope.file, self.semantic.tables).ok_or(())?.mentions(name, class, PublicationKind::Globals) { continue }
                for PreparedItem { node, item, target: prepared } in self.indexed_items(scope, Some((name, class)), PublicationKind::Globals, true)? {
                    let Action::Globals { first: true, classes, target: Some(target), module: None, asks, .. } = item.action else { continue };
                    if classes != 0 && classes & class == 0 || !self.holds(node, item.when)? { continue }
                    let value = self.target_value(node, target, prepared)?;
                    let found = self.named_targets(node, &value, None, name, class, asks)?.unwrap_or_default();
                    if !found.is_empty() { return Ok(Some(found)) }
                }
                let Some(found) = self.global_scope(scope, name, class).known().ok_or(())? else { return Err(()) };
                targets.extend(found);
            }
            targets.sort_unstable();
            targets.dedup();
            Ok(Some(targets))
        }) {
            Known::Yes(Some(targets)) => Known::Yes(targets),
            _ => Known::Unknown,
        }
    }

    fn cached(&self, request: Request, run: impl FnOnce() -> Result<Option<Vec<Target>>, ()>) -> Answer {
        let revision = self.sync_revision();
        if let Some(answer) = self.semantic.modules.answers.borrow().get(&request) { return answer.clone() }
        self.semantic.modules.answers.borrow_mut().insert(request.clone(), Known::Unknown);
        let mut answer = if self.spend().is_ok() { run().map_or(Known::Unknown, Known::Yes) } else { Known::Unknown };
        if self.semantic.world.revision() != revision {
            self.sync_revision();
            return Known::Unknown;
        }
        if self.semantic.question.budget.limited() { answer = Known::Unknown; }
        match &answer {
            Known::Yes(_) => { self.semantic.modules.answers.borrow_mut().insert(request, answer.clone()); }
            Known::Unknown => { self.semantic.modules.answers.borrow_mut().remove(&request); }
        }
        answer
    }

    fn sync_revision(&self) -> u64 {
        let revision = self.semantic.world.revision();
        if self.semantic.modules.graph.sync_revision(revision) {
            self.semantic.modules.answers.borrow_mut().clear();
            self.semantic.modules.star_filters.borrow_mut().clear();
            self.semantic.modules.publications.borrow_mut().clear();
        }
        revision
    }

    fn asks(&self, asks: u64, class: u64) -> u64 {
        if asks == 0 { return class }
        let count = self.semantic.tables.vocabulary.declaration_classes.len();
        if asks == u64::MAX && count != 0 && count < 64 { (1 << count) - 1 } else { asks }
    }

    fn ask(&self, scope: ScopeRef, name: &str, classes: u64) -> Result<Vec<Target>, ()> {
        let mut targets = Vec::new();
        for bit in (0..64).map(|bit| 1 << bit).filter(|bit| classes & bit != 0) {
            let Some(found) = self.publication(scope, name, bit).known().ok_or(())? else { return Err(()) };
            targets.extend(found);
        }
        targets.sort_unstable();
        targets.dedup();
        Ok(targets)
    }

    fn publication(&self, scope: ScopeRef, name: &str, class: u64) -> Answer {
        self.profiled("modules.publication", || self.publication_uncounted(scope, name, class))
    }

    fn publication_uncounted(&self, scope: ScopeRef, name: &str, class: u64) -> Answer {
        self.sync_revision();
        let question = SharedQuestion::Publication { kind: PublicationKind::Exports, scope, name, class };
        match self.semantic.world.shared_modules(question, &mut || {
            (Modules { source: self }).publication(scope, name, class, self.semantic.question, &self.semantic.modules.graph)
        }) {
            Known::Yes(targets) => Known::Yes(Some(targets)),
            Known::Unknown => Known::Unknown,
        }
    }

    fn global_scope(&self, scope: ScopeRef, name: &str, class: u64) -> Answer {
        self.sync_revision();
        let question = SharedQuestion::Publication { kind: PublicationKind::Globals, scope, name, class };
        match self.semantic.world.shared_modules(question, &mut || {
            (Modules { source: &GlobalSource(self) }).publication(scope, name, class, self.semantic.question, &self.semantic.modules.graph)
        }) {
            Known::Yes(targets) => Known::Yes(Some(targets)),
            Known::Unknown => Known::Unknown,
        }
    }

    fn items(&self, scope: ScopeRef, publication: PublicationKind) -> Result<Vec<PreparedItem>, ()> {
        self.indexed_items(scope, None, publication, false)
    }

    fn named_items(&self, scope: ScopeRef, name: &str, class: u64, publication: PublicationKind) -> Result<Vec<PreparedItem>, ()> {
        self.indexed_items(scope, Some((name, class)), publication, false)
    }

    fn is_publication(&self, scope: ScopeRef) -> Result<bool, ()> {
        let revision = self.sync_revision();
        if let Some(&published) = self.semantic.modules.publications.borrow().get(&scope) { return Ok(published) }
        let world = self.semantic.world;
        let info = world.names(scope.file).ok_or(())?.scopes.get(scope.scope as usize).ok_or(())?;
        let tree = world.tree(scope.file).ok_or(())?;
        let kind = self.semantic.tables.kinds.get(tree.nodes.get(info.node as usize).ok_or(())?.matcher as usize).ok_or(())?;
        let mut published = false;
        for item in &kind.modules.items {
            let marks = matches!(item.action, Action::Uses(_))
                || matches!(item.action, Action::Scope { classes, .. } if classes & info.classes != 0);
            if marks && self.holds(NodeRef { file: scope.file, node: info.node }, item.when)? {
                published = true;
                break;
            }
        }
        if self.semantic.world.revision() != revision { self.sync_revision(); return Err(()) }
        if self.semantic.question.budget.limited() { return Err(()) }
        self.semantic.modules.publications.borrow_mut().insert(scope, published);
        Ok(published)
    }

    fn named_targets(&self, at: NodeRef, value: &Value<'_>, alias: Option<u32>, name: &str, class: u64, asks: u64) -> Result<Option<Vec<Target>>, ()> {
        let alias = alias.map(|operand| self.operand(at, operand)).transpose()?;
        let world = self.semantic.world;
        let names = world.names(at.file).ok_or(())?;
        let mut targets = Vec::new();
        let mut matched = false;
        for node in self.target_nodes(at.file, value)? {
            self.spend()?;
            let tree = world.tree(at.file).ok_or(())?;
            let syntax = tree.nodes.get(node as usize).ok_or(())?;
            let source = world.text(at.file).ok_or(())?.get(syntax.start as usize..syntax.end as usize).ok_or(())?;
            let key = alias.as_ref().and_then(text).map_or_else(|| world.name_key(at.file, source), std::borrow::Cow::Borrowed);
            if key != name { continue }
            let mut declaration = names.declaration_in(node, class);
            // An import whose target lacks the class leaves it to the next
            // declaration (`import type { A }` beside `const A`).
            let mut skipped = Vec::new();
            loop {
                let reference = NodeRef { file: at.file, node: declaration };
                let Some(&(_, _, classes, _)) = names.declared.get(&declaration) else {
                    matched = true;
                    targets.push(Target::Node(reference));
                    break;
                };
                if classes & class == 0 { break }
                matched = true;
                match self.definition(reference, self.asks(asks, class)) {
                    Known::Yes(Some(found)) if found.is_empty() => {
                        skipped.push(declaration);
                        let text = world.text(at.file).ok_or(())?;
                        let local = world.name_key(at.file, source);
                        match names.visible_except(tree, text, &|t| world.name_key(at.file, t), node, &local, class, &skipped) {
                            Some(next) => {
                                self.spend()?;
                                declaration = next;
                            }
                            None => break,
                        }
                    }
                    Known::Yes(Some(found)) => {
                        targets.extend(found);
                        break;
                    }
                    Known::Yes(None) => {
                        targets.push(Target::Node(reference));
                        break;
                    }
                    Known::Unknown => return Err(()),
                }
            }
        }
        targets.sort_unstable();
        targets.dedup();
        Ok(matched.then_some(targets))
    }

    fn target_nodes(&self, file: u32, value: &Value<'_>) -> Result<Vec<u32>, ()> {
        self.profiled("modules.targets", || self.target_nodes_uncounted(file, value))
    }

    fn target_nodes_uncounted(&self, file: u32, value: &Value<'_>) -> Result<Vec<u32>, ()> {
        let world = self.semantic.world;
        let tree = world.tree(file).ok_or(())?;
        let mut targets = Vec::new();
        for node in nodes(value) {
            self.spend()?;
            targets.extend(self.semantic.tables.targets_of(tree, node, &|node, condition| {
                world.condition(leaf(NodeRef { file, node }), condition, self.semantic)
            }).known().ok_or(())?);
        }
        targets.sort_unstable();
        targets.dedup();
        Ok(targets)
    }
}

fn leaf(node: NodeRef) -> Leaf { Leaf { target: Target::Node(node), frame: FrameId::ROOT } }

fn text<'a>(value: &'a Value<'_>) -> Option<&'a str> {
    match value { Value::Str(text) | Value::Node(_, text) => Some(text), _ => None }
}

fn nodes(value: &Value<'_>) -> Vec<u32> {
    let mut pending = vec![value];
    let mut nodes = Vec::new();
    while let Some(value) = pending.pop() {
        match value {
            Value::Node(node, _) => nodes.push(*node),
            Value::List(values) => pending.extend(values.iter().rev()),
            _ => {}
        }
    }
    nodes
}

struct GlobalSource<'a, 's>(&'a Queries<'s>);

impl Queries<'_> {
    fn merges(&self, target: Target, class: u64) -> Result<bool, ()> {
        let node = match target {
            Target::Node(mut node) => {
                let names = self.semantic.world.names(node.file).ok_or(())?;
                if let Some((_, owner, _, _)) = names.declared.get(&node.node) { node.node = *owner; }
                node
            }
            Target::Scope(scope) => {
                let names = self.semantic.world.names(scope.file).ok_or(())?;
                let info = names.scopes.get(scope.scope as usize).ok_or(())?;
                NodeRef { file: scope.file, node: info.node }
            }
            _ => return Ok(false),
        };
        let tree = self.semantic.world.tree(node.file).ok_or(())?;
        let matcher = tree.nodes.get(node.node as usize).ok_or(())?.matcher;
        let Some(kind) = self.semantic.tables.kinds.get(matcher as usize) else { return Ok(false) };
        for item in &kind.modules.items {
            if let Action::Merges { classes } = item.action {
                if classes & class != 0 && self.holds(node, item.when)? { return Ok(true) }
            }
        }
        Ok(false)
    }

    fn entries_for(&self, scope: ScopeRef, name: &str, class: u64, q: &QueryContext, publications: PublicationKind) -> Known<Vec<Entry>> {
        self.profiled("modules.entries", || self.entries_uncounted(scope, name, class, q, publications))
    }

    fn entries_uncounted(&self, scope: ScopeRef, name: &str, class: u64, q: &QueryContext, publications: PublicationKind) -> Known<Vec<Entry>> {
        let entries = || -> Result<Vec<Entry>, ()> {
            let mut scopes = vec![scope];
            let mut expand = matches!(publications, PublicationKind::Exports);
            let mut direct = Vec::new();
            let mut stars = Vec::new();
            while let Some(current) = scopes.pop() {
                for PreparedItem { node, item, target: prepared } in self.named_items(current, name, class, publications)? {
                    let (classes, asks, target, module, alias, first) = match (publications, item.action) {
                        (PublicationKind::Exports, Action::Exports { classes, asks, target, module, alias }) => (classes, asks, target, module, alias, false),
                        (PublicationKind::Globals, Action::Globals { first, classes, asks, target, module, .. }) => (classes, asks, target, module, None, first),
                        _ => continue,
                    };
                    if classes != 0 && classes & class == 0 || !self.holds(node, item.when)? { continue }
                    if target.is_none() {
                        match publications {
                            PublicationKind::Exports => stars.push((node, asks, module)),
                            PublicationKind::Globals => {
                                let source = self.operand(node, module.ok_or(())?)?;
                                let source = self.resolve(node, text(&source).ok_or(())?).known().ok_or(())?.ok_or(())?;
                                direct.push(Entry::Reexport {
                                    name: name.into(), classes: class, asks: self.asks(asks, class),
                                    source: Known::Yes(source), imported: name.into(),
                                });
                            }
                        }
                        continue;
                    }
                    let target = target.ok_or(())?;
                    if let Some(module) = module {
                        let imported = self.target_value(node, target, prepared)?;
                        let imported = text(&imported).ok_or(())?;
                        let alias = alias.map(|alias| self.operand(node, alias)).transpose()?;
                        let key = alias.as_ref().and_then(text).unwrap_or(imported);
                        if key != name { continue }
                        let source = self.operand(node, module)?;
                        let source = text(&source).ok_or(())?;
                        let source = self.resolve(node, source).known().ok_or(())?.ok_or(())?;
                        direct.push(if imported == "*" {
                            Entry::Namespace { name: name.into(), classes: class, source: Known::Yes(source) }
                        } else {
                            Entry::Reexport { name: name.into(), classes: class, asks: self.asks(asks, class), source: Known::Yes(source), imported: imported.into() }
                        });
                    } else {
                        let value = self.target_value(node, target, prepared)?;
                        if let Some(targets) = self.named_targets(node, &value, alias, name, class, asks)? {
                            if first && !targets.is_empty() {
                                return Ok(vec![Entry::Local { name: name.into(), classes: class, targets }]);
                            }
                            direct.push(Entry::Local { name: name.into(), classes: class, targets });
                        }
                    }
                }
                if scopes.is_empty() && expand {
                    expand = false;
                    let mut available = false;
                    let mut merge = false;
                    for entry in &direct {
                        match entry {
                            Entry::Local { targets, .. } => {
                                available |= !targets.is_empty();
                                for target in targets { merge |= self.merges(*target, class)?; }
                            }
                            Entry::Reexport { .. } | Entry::Namespace { .. } => available = true,
                            Entry::Star { .. } => {}
                        }
                    }
                    if !available || merge {
                        scopes = self.profiled("modules.scopes", || self.semantic.world.module_scopes(scope, self.semantic)).known().ok_or(())?
                            .into_iter().filter(|other| *other != scope).rev().collect();
                    }
                }
            }
            if !direct.is_empty() { return Ok(direct) }
            if !stars.is_empty() && self.star_excludes(scope, name, q).known().ok_or(())? {
                return Ok(Vec::new());
            }
            for (node, asks, module) in stars {
                let module = self.operand(node, module.ok_or(())?)?;
                let source = self.resolve(node, text(&module).ok_or(())?).known().ok_or(())?.ok_or(())?;
                direct.push(Entry::Star { classes: class, asks: self.asks(asks, class), source: Known::Yes(source) });
            }
            Ok(direct)
        };
        entries().map_or(Known::Unknown, Known::Yes)
    }
}

impl Source for Queries<'_> {
    fn revision(&self) -> u64 { self.semantic.world.revision() }
    fn entries(&self, scope: ScopeRef, name: &str, class: u64, q: &QueryContext) -> Known<Vec<Entry>> {
        self.entries_for(scope, name, class, q, PublicationKind::Exports)
    }
    fn star_excludes(&self, scope: ScopeRef, name: &str, _: &QueryContext) -> Known<bool> {
        let revision = self.sync_revision();
        let key = (scope, name.into());
        if let Some(answer) = self.semantic.modules.star_filters.borrow().get(&key) { return *answer }
        self.semantic.modules.star_filters.borrow_mut().insert(key.clone(), Known::Unknown);
        let mut answer = if self.spend().is_ok() {
            self.semantic.world.module_star_excludes(scope, name, self.semantic)
        } else {
            Known::Unknown
        };
        if self.semantic.world.revision() != revision { self.sync_revision(); return Known::Unknown }
        if self.semantic.question.budget.limited() { answer = Known::Unknown; }
        match answer {
            Known::Yes(_) => { self.semantic.modules.star_filters.borrow_mut().insert(key, answer); }
            Known::Unknown => { self.semantic.modules.star_filters.borrow_mut().remove(&key); }
        }
        answer
    }
}

impl Source for GlobalSource<'_, '_> {
    fn publication_kind(&self) -> PublicationKind { PublicationKind::Globals }
    fn revision(&self) -> u64 { self.0.semantic.world.revision() }
    fn entries(&self, scope: ScopeRef, name: &str, class: u64, q: &QueryContext) -> Known<Vec<Entry>> {
        self.0.entries_for(scope, name, class, q, PublicationKind::Globals)
    }

    fn star_excludes(&self, _: ScopeRef, _: &str, _: &QueryContext) -> Known<bool> { Known::Yes(false) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::compile::{Action, Conditional};
    use super::super::super::question::{Leaf, QueryContext, World};
    use super::super::super::{KindNames, Names, Resolved, ScopeIndex, ScopeInfo};
    use crate::config::ast::Config;
    use crate::grammar::{Edge, Grammar, Node, Tree};

    struct Files {
        grammar: Grammar,
        trees: Vec<Tree>,
        names: Vec<Resolved>,
        star_filter: Known<bool>,
        star_queries: std::cell::Cell<usize>,
        module_queries: std::cell::Cell<usize>,
        global_queries: std::cell::Cell<usize>,
        merged_scopes: Known<Vec<ScopeRef>>,
        merge_queries: std::cell::Cell<usize>,
        condition: Known<bool>,
        condition_queries: std::cell::Cell<usize>,
        revision: std::cell::Cell<u64>,
        cut: std::cell::Cell<bool>,
        drift: std::cell::Cell<bool>,
        operands: crate::fx_hash::FxHashMap<(NodeRef, u32), Known<crate::expr::Value<'static>>>,
    }

    impl Files {
        fn new() -> Self {
            let trees = (0..2).map(|_| Tree::from_native(vec![
                Node { matcher: 0, start: 0, end: 3, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 2 }] },
                Node { matcher: 1, start: 0, end: 1, children: Vec::new() },
                Node { matcher: 2, start: 2, end: 3, children: Vec::new() },
            ], 0)).collect();
            let names = (0..2).map(|_| {
                let mut names = Resolved::default();
                names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
                names.declared.insert(1, (0, 1, 1, true));
                names.declared.insert(2, (0, 2, 1, true));
                names.scope_of = vec![0; 3];
                names
            }).collect();
            Self {
                grammar: Grammar::build(&Config::default(), &mut Vec::new()), trees, names,
                star_filter: Known::Yes(false),
                star_queries: std::cell::Cell::new(0),
                module_queries: std::cell::Cell::new(0),
                global_queries: std::cell::Cell::new(0),
                merged_scopes: Known::Yes(vec![ScopeRef { file: 0, scope: 0 }]),
                merge_queries: std::cell::Cell::new(0),
                condition: Known::Yes(true),
                condition_queries: std::cell::Cell::new(0),
                revision: Default::default(),
                cut: Default::default(),
                drift: Default::default(),
                operands: Default::default(),
            }
        }
    }

    impl World for Files {
        fn grammar(&self) -> &Grammar { &self.grammar }
        fn revision(&self) -> u64 { self.revision.get() }
        fn tree(&self, file: u32) -> Option<&Tree> { self.trees.get(file as usize) }
        fn text(&self, _: u32) -> Option<&str> { Some("X P") }
        fn names(&self, file: u32) -> Option<&Resolved> { self.names.get(file as usize) }
        fn index(&self, file: u32) -> Option<&ScopeIndex> {
            Some(self.names(file)?.index(self.tree(file)?, self.text(file)?, &|text| text.into()))
        }
        fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> { text.into() }
        fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> {
            self.module_queries.set(self.module_queries.get() + 1);
            Known::Yes(Some(ScopeRef { file: 1, scope: 0 }))
        }
        fn module_star_excludes(&self, _: ScopeRef, _: &str, semantic: &Semantic<'_>) -> Known<bool> {
            self.star_queries.set(self.star_queries.get() + 1);
            if self.cut.get() { while semantic.question.budget.step() {} }
            self.star_filter
        }
        fn module_scopes(&self, scope: ScopeRef, _: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
            self.merge_queries.set(self.merge_queries.get() + 1);
            match &self.merged_scopes {
                Known::Yes(scopes) if scopes.len() == 1 => Known::Yes(vec![scope]),
                answer => answer.clone(),
            }
        }
        fn global_scopes(&self, _: u32, _: &str, _: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
            self.global_queries.set(self.global_queries.get() + 1);
            Known::Yes(vec![ScopeRef { file: 0, scope: 0 }, ScopeRef { file: 1, scope: 0 }])
        }
        fn condition(&self, _: Leaf, _: u32, semantic: &Semantic<'_>) -> Known<bool> {
            self.condition_queries.set(self.condition_queries.get() + 1);
            if self.drift.replace(false) { self.revision.set(self.revision.get() + 1); }
            if self.cut.get() { while semantic.question.budget.step() {} }
            self.condition
        }
        fn operand(&self, at: Leaf, operand: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
            let Target::Node(node) = at.target else { return Known::Unknown };
            if let Some(answer) = self.operands.get(&(node, operand)) { return answer.clone() }
            if operand == 1 { return Known::Yes(crate::expr::Value::Str("other".to_string().into())) }
            if operand == 2 { return Known::Yes(crate::expr::Value::Str("X".to_string().into())) }
            Known::Yes(crate::expr::Value::Node(node.node, "X".to_string().into()))
        }
        fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> { Known::Yes(crate::expr::Value::Null) }
    }

    fn tables(first: bool) -> Names {
        let mut kinds = vec![KindNames::default(); 3];
        kinds[0].modules.items.push(Conditional { action: Action::Uses("test".into()), when: None });
        kinds[1].modules.items.extend([
            Conditional { action: Action::Exports { classes: 1, asks: 0, target: Some(0), module: None, alias: None }, when: None },
            Conditional { action: Action::Globals { first, classes: 1, asks: 0, target: Some(0), module: None, named: None }, when: None },
        ]);
        Names { kinds, ..Names::default() }
    }

    fn target(file: u32) -> Target { Target::Node(NodeRef { file, node: 1 }) }

    #[test]
    fn bulk_classes_share_publication_conditions_without_losing_unknown_meanings() {
        for publication_classes in [0, 3] {
            let mut files = Files::new();
            files.names[0].declared.get_mut(&1).unwrap().2 = 3;
            files.merged_scopes = Known::Unknown;
            let mut tables = tables(false);
            for item in &mut tables.kinds[1].modules.items {
                if let Action::Exports { classes, .. } = &mut item.action {
                    *classes = publication_classes;
                    item.when = Some(0);
                }
            }
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            assert_eq!(semantic.modules().export_classes(ScopeRef { file: 0, scope: 0 }, "X", 7),
                ClassAnswers { known: 3, present: 3 });
            assert_eq!(files.condition_queries.get(), 1);
        }
    }

    #[test]
    fn bulk_binding_fallback_loads_global_scopes_once_for_all_classes() {
        let mut files = Files::new();
        files.names[0].declared.get_mut(&1).unwrap().2 = 3;
        files.names[1].declared.clear();
        files.trees[1].nodes[1].matcher = 2;
        let mut tables = tables(false);
        for item in &mut tables.kinds[1].modules.items {
            if let Action::Globals { classes, .. } = &mut item.action { *classes = 3; }
        }
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        assert_eq!(semantic.modules().binding_classes(NodeRef { file: 1, node: 0 }, "X", 7),
            ClassAnswers { known: 7, present: 3 });
        assert_eq!(files.global_queries.get(), 1);
    }

    #[test]
    fn bulk_classes_do_not_combine_answers_across_world_revisions() {
        let mut files = Files::new();
        files.names[0].declared.get_mut(&1).unwrap().2 = 3;
        files.drift.set(true);
        let mut tables = tables(false);
        for item in &mut tables.kinds[1].modules.items {
            if let Action::Exports { classes, .. } = &mut item.action {
                *classes = 3;
                item.when = Some(0);
            }
        }
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        assert_eq!(semantic.modules().export_classes(ScopeRef { file: 0, scope: 0 }, "X", 3),
            ClassAnswers::default());
        assert_eq!(semantic.modules().export_classes(ScopeRef { file: 0, scope: 0 }, "X", 3),
            ClassAnswers { known: 3, present: 3 });
    }

    #[test]
    fn scope_opening_declarations_publish_their_names_outside_but_keep_members_inside() {
        let mut files = Files::new();
        files.trees[0] = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 3, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 2 }] },
            Node { matcher: 1, start: 0, end: 3, children: vec![Edge { label: 1, node: 3 }, Edge { label: 0, node: 4 }] },
            Node { matcher: 3, start: 2, end: 3, children: Vec::new() },
            Node { matcher: 3, start: 0, end: 1, children: Vec::new() },
            Node { matcher: 2, start: 2, end: 3, children: Vec::new() },
        ], 0);
        let names = &mut files.names[0];
        names.scopes.push(ScopeInfo { node: 1, classes: 2, parent: Some(0) });
        names.declared.remove(&1);
        names.declared.insert(3, (0, 1, 1, true));
        names.declared.insert(4, (1, 4, 1, true));
        names.scope_of = vec![0, 1, 0, 1, 1];
        let mut tables = tables(false);
        tables.kinds.push(KindNames::default());
        tables.kinds[1].modules.items.push(Conditional {
            action: Action::Scope { classes: 2, name: 2, module: None }, when: None,
        });
        let own = tables.operand(0, 1, 0);
        tables.operands[own as usize].expr.set(crate::expr::CExpr::OnField(
            1, Box::new(crate::expr::CExpr::Builtin(crate::expr::Builtin::Node)),
        )).unwrap();
        let member = tables.operand(0, 2, 0);
        tables.operands[member as usize].expr.set(crate::expr::CExpr::Builtin(crate::expr::Builtin::Node)).unwrap();
        tables.kinds[2].modules.items.push(Conditional {
            action: Action::Exports { classes: 1, asks: 0, target: Some(member), module: None, alias: None }, when: None,
        });
        files.operands.insert((NodeRef { file: 0, node: 1 }, own),
            Known::Yes(crate::expr::Value::Node(3, "X".into())));
        files.operands.insert((NodeRef { file: 0, node: 4 }, member),
            Known::Yes(crate::expr::Value::Node(4, "P".into())));
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        let root = ScopeRef { file: 0, scope: 0 };
        let nested = ScopeRef { file: 0, scope: 1 };
        assert_eq!(semantic.modules().namespace_members(root, "X", 1),
            Known::Yes(Some(vec![Target::Node(NodeRef { file: 0, node: 3 })])));
        assert_eq!(semantic.modules().namespace_members(nested, "P", 1),
            Known::Yes(Some(vec![Target::Node(NodeRef { file: 0, node: 4 })])));
        assert_eq!(semantic.modules().namespace_members(root, "P", 1), Known::Yes(Some(Vec::new())));
        assert_eq!(semantic.modules().namespace_members(nested, "X", 1), Known::Yes(Some(Vec::new())));
        assert_eq!(semantic.modules().export_names(root), Known::Yes(vec!["X".into()]));
        assert_eq!(semantic.modules().export_names(nested), Known::Yes(vec!["P".into()]));
        assert_eq!(semantic.modules().global_scope(root, "X", 1),
            Known::Yes(Some(vec![Target::Node(NodeRef { file: 0, node: 3 })])));
    }

    #[test]
    fn local_references_expand_only_proven_global_merges() {
        for (global, merges, condition, class, expected) in [
            (true, true, Known::Yes(true), 1, Known::Yes(Some(vec![target(0), target(1)]))),
            (false, true, Known::Yes(true), 1, Known::Yes(None)),
            (true, false, Known::Yes(true), 1, Known::Yes(None)),
            (true, true, Known::Yes(false), 1, Known::Yes(None)),
            (true, true, Known::Unknown, 1, Known::Unknown),
            (true, true, Known::Yes(true), 2, Known::Yes(None)),
        ] {
            let mut files = Files::new();
            let mut nodes = files.trees[0].nodes.clone();
            nodes.push(Node { matcher: 2, start: 0, end: 1, children: Vec::new() });
            nodes[0].children.push(Edge { label: 0, node: 3 });
            files.trees[0] = Tree::from_native(nodes, 0);
            files.names[0].scope_of.push(0);
            files.names[0].declaration.insert(3, 1);
            files.condition = condition;
            let mut tables = tables(false);
            for item in &mut tables.kinds[1].modules.items {
                if matches!(item.action, Action::Globals { .. }) { item.when = Some(0); }
            }
            if !global {
                tables.kinds[1].modules.items.retain(|item| !matches!(item.action, Action::Globals { .. }));
            }
            if merges {
                tables.kinds[1].modules.items.push(Conditional { action: Action::Merges { classes: 1 }, when: None });
            }
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            assert_eq!(semantic.modules().definition(NodeRef { file: 0, node: 1 }, class), Known::Yes(None));
            assert_eq!(semantic.modules().definition(NodeRef { file: 0, node: 3 }, class), expected);
        }
    }

    #[test]
    fn publication_classification_reuses_known_answers_but_retries_unknowns() {
        for condition in [Known::Yes(true), Known::Yes(false), Known::Unknown] {
            let mut files = Files::new();
            files.condition = condition;
            let mut tables = tables(false);
            tables.kinds[0].modules.items[0].when = Some(0);
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            let expected = condition.known().ok_or(());
            for _ in 0..3 {
                assert_eq!(semantic.modules().is_publication(ScopeRef { file: 0, scope: 0 }), expected);
            }
            assert_eq!(files.condition_queries.get(), if condition == Known::Unknown { 3 } else { 1 });
        }
    }

    #[test]
    fn completed_publication_classification_survives_session_budget_cuts() {
        let files = Files::new();
        let mut tables = tables(false);
        tables.kinds[0].modules.items[0].when = Some(0);
        let session = super::super::super::semantic::Session::default();
        assert_eq!(session.run(&files, &tables, |semantic| {
            assert_eq!(semantic.modules().is_publication(ScopeRef { file: 0, scope: 0 }), Ok(true));
            while semantic.question.budget.step() {}
            Known::<bool>::Unknown
        }), Known::Unknown);
        assert_eq!(session.run(&files, &tables, |semantic| {
            semantic.modules().is_publication(ScopeRef { file: 0, scope: 0 }).map_or(Known::Unknown, Known::Yes)
        }), Known::Yes(true));
        assert_eq!(files.condition_queries.get(), 1);
    }

    #[test]
    fn limited_conditions_and_star_filters_do_not_publish_known_answers() {
        let mut answers = Vec::new();
        for star_filter in [false, true] {
            let mut files = Files::new();
            files.condition = Known::Yes(false);
            files.cut.set(true);
            let mut tables = tables(false);
            tables.kinds[0].modules.items[0].when = Some(0);
            let session = super::super::super::semantic::Session::default();
            let query = |semantic: &Semantic<'_>| {
                let scope = ScopeRef { file: 0, scope: 0 };
                if star_filter { semantic.modules().star_excludes(scope, "X", semantic.question) }
                else { semantic.modules().is_publication(scope).map_or(Known::Unknown, Known::Yes) }
            };
            let cut = session.run(&files, &tables, query);
            files.cut.set(false);
            let retry = session.run(&files, &tables, query);
            let calls = if star_filter { files.star_queries.get() } else { files.condition_queries.get() };
            answers.push((cut, retry, calls));
        }
        assert_eq!(answers, vec![(Known::Unknown, Known::Yes(false), 2); 2]);
    }

    #[test]
    fn module_answers_survive_session_limits_but_follow_world_revisions() {
        let mut files = Files::new();
        let mut tables = tables(false);
        for item in &mut tables.kinds[1].modules.items {
            item.when = Some(0);
        }
        let session = super::super::super::semantic::Session::default();
        let expected = Known::Yes(vec![target(0), target(1)]);
        assert_eq!(session.run(&files, &tables, |semantic| {
            let answer = semantic.modules().global(0, "X", 1);
            while semantic.question.budget.step() {}
            answer
        }), expected);
        assert_eq!(session.run(&files, &tables, |semantic| semantic.modules().global(0, "X", 1)), expected);
        assert_eq!(files.condition_queries.get(), 2);
        files.condition = Known::Yes(false);
        files.revision.set(1);
        assert_eq!(session.run(&files, &tables, |semantic| semantic.modules().global(0, "X", 1)), Known::Yes(Vec::new()));
    }

    #[test]
    fn publication_questions_do_not_spend_their_budget_on_unrelated_syntax() {
        let mut answers = Vec::new();
        for unrelated_globals in [None, Some(false), Some(true)] {
            let mut files = Files::new();
            let mut nodes = files.trees[0].nodes.clone();
            for id in 3..1003 {
                nodes.push(Node { matcher: if id % 2 == 0 { 1 } else { 2 }, start: 2, end: 3, children: Vec::new() });
                nodes[0].children.push(Edge { label: 0, node: id });
            }
            files.trees[0] = Tree::from_native(nodes, 0);
            files.names[0].scope_of.resize(1003, 0);
            let mut tables = tables(false);
            let operand = tables.operand(0, 1, 0);
            tables.operands[operand as usize].expr.set(crate::expr::CExpr::Builtin(crate::expr::Builtin::Node)).unwrap();
            if let Some(globals) = unrelated_globals {
                let computed = tables.operand(0, 2, 0);
                tables.operands[computed as usize].expr.set(crate::expr::CExpr::Attr(0)).unwrap();
                let action = if globals {
                    Action::Globals { first: false, classes: 1, asks: 0, target: Some(computed), module: None, named: None }
                } else {
                    Action::Exports { classes: 1, asks: 0, target: Some(computed), module: None, alias: None }
                };
                tables.kinds[2].modules.items.push(Conditional { action, when: None });
            }
            let mut question = QueryContext::default();
            question.budget = super::super::super::question::Budget::new(128);
            let semantic = Semantic::new(&files, &tables, &question);
            answers.push((
                (unrelated_globals != Some(true)).then(|| semantic.modules().global(0, "X", 1)),
                (unrelated_globals != Some(false)).then(|| semantic.modules().namespace_members(ScopeRef { file: 0, scope: 0 }, "X", 1)),
            ));
        }
        assert_eq!(answers, [
            (Some(Known::Yes(vec![target(0), target(1)])), Some(Known::Yes(Some(vec![target(0)])))),
            (Some(Known::Yes(vec![target(0), target(1)])), None),
            (None, Some(Known::Yes(Some(vec![target(0)])))),
        ]);
    }

    #[test]
    fn publication_collection_ignores_raw_text_nodes() {
        let mut files = Files::new();
        files.trees[0] = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 3, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 2 }] },
            Node { matcher: 1, start: 0, end: 1, children: Vec::new() },
            Node { matcher: crate::grammar::parse::TEXT, start: 2, end: 3, children: Vec::new() },
        ], 0);
        files.names[0].declared.remove(&2);
        let tables = tables(false);
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        assert_eq!(semantic.modules().namespace_members(ScopeRef { file: 0, scope: 0 }, "X", 1), Known::Yes(Some(vec![target(0)])));
        assert_eq!(semantic.modules().export_names(ScopeRef { file: 0, scope: 0 }), Known::Yes(vec!["X".into()]));
        assert_eq!(semantic.modules().global(0, "X", 1), Known::Yes(vec![target(0), target(1)]));
    }

    #[test]
    fn name_class_questions_keep_independent_meanings_and_incomplete_answers() {
        let mut files = Files::new();
        files.names[0].declared.get_mut(&2).unwrap().2 = 2;
        files.names[0].keys.insert(2, Some("X".into()));
        files.merged_scopes = Known::Unknown;
        let tables = tables(false);
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        let name = NodeRef { file: 0, node: 1 };
        assert_eq!(semantic.modules().binding_has_class(name, "X", 1), Known::Yes(true));
        assert_eq!(semantic.modules().binding_has_class(name, "X", 2), Known::Yes(true));
        assert_eq!(semantic.modules().binding_has_class(name, "X", 4), Known::Yes(false));
        assert_eq!(semantic.modules().binding_classes(name, "X", 7), ClassAnswers { known: 7, present: 3 });
        let scope = ScopeRef { file: 0, scope: 0 };
        assert_eq!(semantic.modules().export_has_class(scope, "X", 1), Known::Yes(true));
        assert_eq!(semantic.modules().export_has_class(scope, "X", 2), Known::Unknown);
        assert_eq!(semantic.modules().export_classes(scope, "X", 3), ClassAnswers { known: 1, present: 1 });
        drop(semantic);
        files.names[0].failures.push((0, "unreadable declaration operand".into()));
        let semantic = Semantic::new(&files, &tables, &question);
        assert_eq!(semantic.modules().binding_has_class(name, "X", 4), Known::Unknown);
    }

    #[test]
    fn export_inventory_excludes_private_names_and_rejects_incomplete_or_cyclic_graphs() {
        let scope = ScopeRef { file: 0, scope: 0 };
        for (cyclic, scopes, expected) in [
            (false, Known::Yes(vec![scope]), Known::Yes(vec!["X".into()])),
            (false, Known::Unknown, Known::Unknown),
            (true, Known::Yes(vec![scope]), Known::Unknown),
        ] {
            let mut files = Files::new();
            files.merged_scopes = scopes;
            let mut tables = tables(false);
            if cyclic {
                tables.kinds[2].modules.items.push(Conditional {
                    action: Action::Exports { classes: 1, asks: 0, target: None, module: Some(1), alias: None },
                    when: None,
                });
            }
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            assert_eq!(semantic.modules().export_names(scope), expected);
        }
    }

    #[test]
    fn direct_publications_only_need_augmentation_proof_when_their_declarations_merge() {
        let scope = ScopeRef { file: 0, scope: 0 };
        for (name, merges, condition, scopes, expected, searches) in [
            ("X", false, Known::Yes(true), Known::Unknown, Known::Yes(Some(vec![target(0)])), 0),
            ("P", false, Known::Yes(true), Known::Unknown, Known::Unknown, 1),
            ("X", true, Known::Yes(true), Known::Unknown, Known::Unknown, 1),
            ("X", true, Known::Yes(false), Known::Unknown, Known::Yes(Some(vec![target(0)])), 0),
            ("X", true, Known::Unknown, Known::Yes(vec![scope]), Known::Unknown, 0),
            ("X", true, Known::Yes(true), Known::Yes(vec![scope, ScopeRef { file: 1, scope: 0 }]), Known::Yes(Some(vec![target(0), target(1)])), 1),
        ] {
            let mut files = Files::new();
            files.merged_scopes = scopes;
            files.condition = condition;
            let mut tables = tables(false);
            if merges {
                tables.kinds[1].modules.items.push(Conditional {
                    action: Action::Merges { classes: 1 }, when: Some(0),
                });
            }
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            assert_eq!(semantic.modules().namespace_members(scope, name, 1), expected);
            assert_eq!(files.merge_queries.get(), searches);
        }
    }

    #[test]
    fn star_filters_precede_source_loading_and_direct_publications_skip_them() {
        for (filter, direct, expected, loads, filters) in [
            (Known::Yes(true), false, Known::Yes(Some(Vec::new())), 0, 1),
            (Known::Unknown, false, Known::Unknown, 0, 2),
            (Known::Yes(false), false, Known::Yes(Some(vec![target(1)])), 1, 1),
            (Known::Unknown, true, Known::Yes(Some(vec![target(0)])), 0, 0),
        ] {
            let mut files = Files::new();
            files.star_filter = filter;
            let mut tables = tables(false);
            if !direct {
                tables.kinds[1].modules.items.clear();
                files.trees[1].nodes[1].matcher = 2;
                tables.kinds[2].modules.items.push(Conditional {
                    action: Action::Exports { classes: 1, asks: 0, target: Some(0), module: None, alias: None },
                    when: None,
                });
            }
            tables.kinds[1].modules.items.push(Conditional {
                action: Action::Exports { classes: 1, asks: 0, target: None, module: Some(1), alias: None },
                when: None,
            });
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            let scope = ScopeRef { file: 0, scope: 0 };
            assert_eq!(semantic.modules().namespace_members(scope, "X", 1), expected);
            if filters != 0 {
                assert_eq!(semantic.modules().star_excludes(scope, "X", &question), filter);
            }
            assert_eq!(files.module_queries.get(), loads);
            assert_eq!(files.star_queries.get(), filters);
        }
    }

    #[test]
    fn global_links_keep_requested_names_and_classes_and_cycles_unknown() {
        for (name, class, cycle, expected) in [
            ("X", 2, false, Known::Yes(vec![target(1)])),
            ("P", 2, false, Known::Yes(Vec::new())),
            ("X", 4, false, Known::Yes(Vec::new())),
            ("X", 2, true, Known::Unknown),
        ] {
            let mut files = Files::new();
            files.trees[0].nodes[1].matcher = 3;
            let mut tables = tables(false);
            tables.kinds.push(KindNames::default());
            tables.kinds[3].modules.items.push(Conditional {
                action: Action::Globals { first: false, classes: 2, asks: 1, target: None, module: Some(1), named: None },
                when: None,
            });
            if cycle {
                tables.kinds[1].modules.items.clear();
                tables.kinds[1].modules.items.push(Conditional {
                    action: Action::Globals { first: false, classes: 1, asks: 1, target: None, module: Some(1), named: None },
                    when: None,
                });
            }
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            assert_eq!(semantic.modules().global(0, name, class), expected);
        }
    }

    #[test]
    fn publication_scopes_do_not_expose_private_lexical_declarations() {
        let files = Files::new();
        let tables = tables(false);
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        let scope = ScopeRef { file: 0, scope: 0 };
        assert_eq!(semantic.modules().namespace_members(scope, "X", 1), Known::Yes(Some(vec![target(0)])));
        assert_eq!(semantic.modules().namespace_members(scope, "P", 1), Known::Yes(Some(Vec::new())));
    }

    #[test]
    fn publications_keep_distinct_bindings_with_a_shared_declaring_owner() {
        let mut files = Files::new();
        files.names[0].declared.get_mut(&1).unwrap().1 = 0;
        files.names[0].declared.get_mut(&2).unwrap().1 = 0;
        let mut tables = tables(false);
        tables.kinds[2].modules.items = tables.kinds[1].modules.items.clone();
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        let scope = ScopeRef { file: 0, scope: 0 };
        assert_eq!(semantic.modules().namespace_members(scope, "X", 1), Known::Yes(Some(vec![target(0)])));
        assert_eq!(semantic.modules().namespace_members(scope, "P", 1), Known::Yes(Some(vec![Target::Node(NodeRef { file: 0, node: 2 })])));
    }

    #[test]
    fn lexical_body_scopes_are_transparent_to_publications_not_publication_targets() {
        let mut files = Files::new();
        files.trees[0] = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 3, children: vec![Edge { label: 0, node: 3 }] },
            Node { matcher: 1, start: 0, end: 1, children: Vec::new() },
            Node { matcher: 2, start: 2, end: 3, children: Vec::new() },
            Node { matcher: 2, start: 0, end: 3, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 2 }] },
        ], 0);
        files.names[0].scopes.push(ScopeInfo { node: 3, classes: 2, parent: Some(0) });
        files.names[0].scope_of = vec![0, 1, 1, 1];
        files.names[0].declared.get_mut(&1).unwrap().0 = 1;
        files.names[0].declared.get_mut(&2).unwrap().0 = 1;
        let tables = tables(false);
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        assert_eq!(semantic.modules().namespace_members(ScopeRef { file: 0, scope: 0 }, "X", 1), Known::Yes(Some(vec![target(0)])));
        assert_eq!(semantic.modules().namespace_members(ScopeRef { file: 0, scope: 1 }, "X", 1), Known::Yes(None));
    }

    #[test]
    fn ordinary_globals_merge_but_first_globals_choose_one_real_declaration() {
        for (first, expected) in [(false, vec![target(0), target(1)]), (true, vec![target(0)])] {
            let files = Files::new();
            let tables = tables(first);
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            assert_eq!(semantic.modules().global(0, "X", 1), Known::Yes(expected));
            assert_eq!(semantic.modules().global(0, "Missing", 1), Known::Yes(Vec::new()));
        }
        let mut files = Files::new();
        let mut nodes = files.trees[0].nodes.clone();
        for id in 3..503 {
            nodes.push(Node { matcher: 3, start: 2, end: 3, children: Vec::new() });
            nodes[0].children.push(Edge { label: 0, node: id });
        }
        files.trees[0] = Tree::from_native(nodes, 0);
        files.names[0].scope_of.resize(503, 0);
        let mut tables = tables(true);
        let computed = tables.operand(0, 3, 0);
        tables.operands[computed as usize].expr.set(crate::expr::CExpr::Attr(0)).unwrap();
        let mut ordinary = KindNames::default();
        ordinary.modules.items.push(Conditional {
            action: Action::Globals { first: false, classes: 1, asks: 0, target: Some(computed), module: None, named: None },
            when: None,
        });
        tables.kinds.push(ordinary);
        let mut question = QueryContext::default();
        question.budget = super::super::super::question::Budget::new(128);
        let semantic = Semantic::new(&files, &tables, &question);
        assert_eq!(semantic.modules().global(0, "X", 1), Known::Yes(vec![target(0)]));
    }

    #[test]
    fn import_alias_definition_accepts_uses_and_declarations_without_exposing_source_classes() {
        let mut files = Files::new();
        files.trees[0].nodes[1].matcher = 3;
        files.names[0].declared.get_mut(&1).unwrap().2 = 2;
        files.names[0].declared.remove(&2);
        files.names[0].declaration.insert(2, 1);
        let mut tables = tables(false);
        let mut import = KindNames::default();
        import.modules.items.push(Conditional {
            action: Action::Resolves { classes: 2, asks: 1, binding: 0, module: 1, imported: Some(2) },
            when: None,
        });
        tables.kinds.push(import);
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        for node in [1, 2] {
            assert_eq!(semantic.modules().definition(NodeRef { file: 0, node }, 2), Known::Yes(Some(vec![target(1)])));
            assert_eq!(semantic.modules().definition(NodeRef { file: 0, node }, 1), Known::Yes(Some(Vec::new())));
        }
        assert_eq!(semantic.modules().definition(NodeRef { file: 1, node: 1 }, 1), Known::Yes(None));
    }
}

use super::super::question::{FrameId, Known, Leaf, NodeRef, QueryContext, ScopeRef, Target};
use super::super::semantic::Semantic;
use super::compile::{Action, Conditional};
use super::{Entry, Modules, Source};
use crate::expr::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;

type Answer = Known<Option<Vec<Target>>>;
type Items = BTreeMap<u32, Vec<(NodeRef, Conditional)>>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Request {
    Definition(NodeRef, u64),
    Publication(ScopeRef, Box<str>, u64),
    Global(u32, Box<str>, u64),
}

#[derive(Default)]
pub(crate) struct State {
    answers: RefCell<BTreeMap<Request, Answer>>,
    items: RefCell<BTreeMap<u32, Items>>,
}

pub struct Queries<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
}

impl Queries<'_> {
    pub fn definition(&self, name: NodeRef, class: u64) -> Answer {
        self.cached(Request::Definition(name, class), || {
            let world = self.semantic.world;
            let names = world.names(name.file).ok_or(())?;
            let tree = world.tree(name.file).ok_or(())?;
            let declaration = names.declaration.get(&name.node).copied().unwrap_or(name.node);
            let Some(&(_, owner, _, _)) = names.declared.get(&declaration) else { return Ok(None) };
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
                    let Some(scope) = world.module(leaf(node), source, self.semantic).known().ok_or(())? else { continue };
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
        match self.cached(Request::Global(importer, name.into(), class), || {
            let scopes = self.semantic.world.global_scopes(importer, self.semantic).known().ok_or(())?;
            let mut targets = Vec::new();
            for scope in scopes {
                for (node, item) in self.items(scope)? {
                    let Action::Globals { first, classes, target: Some(target), module: None, asks } = item.action else { continue };
                    if classes != 0 && classes & class == 0 || !self.holds(node, item.when)? { continue }
                    let found = self.named_targets(node, target, None, name, class, asks)?.unwrap_or_default();
                    if first && !found.is_empty() { return Ok(Some(found)) }
                    targets.extend(found);
                }
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
        if let Some(answer) = self.semantic.modules.answers.borrow().get(&request) { return answer.clone() }
        self.semantic.modules.answers.borrow_mut().insert(request.clone(), Known::Unknown);
        let answer = if self.spend().is_ok() { run().map_or(Known::Unknown, Known::Yes) } else { Known::Unknown };
        self.semantic.modules.answers.borrow_mut().insert(request, answer.clone());
        answer
    }

    fn spend(&self) -> Result<(), ()> {
        self.semantic.question.budget.step().then_some(()).ok_or(())
    }

    fn holds(&self, node: NodeRef, condition: Option<u32>) -> Result<bool, ()> {
        self.spend()?;
        condition.map_or(Ok(true), |condition| self.semantic.world.condition(leaf(node), condition, self.semantic).known().ok_or(()))
    }

    fn operand(&self, node: NodeRef, operand: u32) -> Result<Value<'static>, ()> {
        self.spend()?;
        self.semantic.world.operand(leaf(node), operand, self.semantic).known().ok_or(())
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
        self.cached(Request::Publication(scope, name.into(), class), || {
            Modules { source: self }.publication(scope, name, class, self.semantic.question).known().map(Some).ok_or(())
        })
    }

    fn items(&self, scope: ScopeRef) -> Result<Vec<(NodeRef, Conditional)>, ()> {
        let file = scope.file;
        if let Some(items) = self.semantic.modules.items.borrow().get(&scope.file) {
            return Ok(items.get(&scope.scope).cloned().unwrap_or_default());
        }
        let world = self.semantic.world;
        let tree = world.tree(scope.file).ok_or(())?;
        let names = world.names(scope.file).ok_or(())?;
        let opened = names.scopes.iter().enumerate().map(|(scope, info)| (info.node, scope as u32)).collect::<crate::fx_hash::FxHashMap<_, _>>();
        let mut containing = vec![None; tree.nodes.len()];
        let mut items = Items::new();
        let publication = (0..names.scopes.len()).map(|scope| self.is_publication(ScopeRef { file, scope: scope as u32 })).collect::<Result<Vec<_>, _>>()?;
        for &id in &tree.order {
            self.spend()?;
            let scope = opened.get(&id).copied().or_else(|| tree.parent(id).and_then(|parent| containing[parent as usize]));
            containing[id as usize] = scope;
            let Some(mut scope) = scope else { continue };
            while !publication[scope as usize] {
                let Some(parent) = names.scopes[scope as usize].parent else { break };
                scope = parent;
            }
            if !publication[scope as usize] { continue }
            let kind = self.semantic.tables.kinds.get(tree.nodes[id as usize].matcher as usize).ok_or(())?;
            if kind.modules.items.is_empty() { continue }
            items.entry(scope).or_default().extend(kind.modules.items.iter().cloned().map(|item| (NodeRef { file, node: id }, item)));
        }
        let result = items.get(&scope.scope).cloned().unwrap_or_default();
        self.semantic.modules.items.borrow_mut().insert(scope.file, items);
        Ok(result)
    }

    fn is_publication(&self, scope: ScopeRef) -> Result<bool, ()> {
        let world = self.semantic.world;
        let info = world.names(scope.file).ok_or(())?.scopes.get(scope.scope as usize).ok_or(())?;
        let tree = world.tree(scope.file).ok_or(())?;
        let kind = self.semantic.tables.kinds.get(tree.nodes.get(info.node as usize).ok_or(())?.matcher as usize).ok_or(())?;
        for item in &kind.modules.items {
            let marks = matches!(item.action, Action::Uses(_))
                || matches!(item.action, Action::Scope { classes, .. } if classes & info.classes != 0);
            if marks && self.holds(NodeRef { file: scope.file, node: info.node }, item.when)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn named_targets(&self, at: NodeRef, operand: u32, alias: Option<u32>, name: &str, class: u64, asks: u64) -> Result<Option<Vec<Target>>, ()> {
        let value = self.operand(at, operand)?;
        let alias = alias.map(|operand| self.operand(at, operand)).transpose()?;
        let world = self.semantic.world;
        let names = world.names(at.file).ok_or(())?;
        let mut targets = Vec::new();
        let mut matched = false;
        for node in self.target_nodes(at.file, &value)? {
            self.spend()?;
            let tree = world.tree(at.file).ok_or(())?;
            let syntax = tree.nodes.get(node as usize).ok_or(())?;
            let source = world.text(at.file).ok_or(())?.get(syntax.start as usize..syntax.end as usize).ok_or(())?;
            let key = alias.as_ref().and_then(text).map_or_else(|| world.name_key(at.file, source), std::borrow::Cow::Borrowed);
            if key != name { continue }
            let declaration = names.declaration.get(&node).copied().unwrap_or(node);
            let reference = NodeRef { file: at.file, node: declaration };
            if let Some(&(_, owner, classes, _)) = names.declared.get(&declaration) {
                if classes & class == 0 { continue }
                matched = true;
                match self.definition(reference, self.asks(asks, class)) {
                    Known::Yes(Some(found)) => targets.extend(found),
                    Known::Yes(None) => targets.push(Target::Node(NodeRef { file: at.file, node: owner })),
                    Known::Unknown => return Err(()),
                }
            } else {
                matched = true;
                targets.push(Target::Node(reference));
            }
        }
        targets.sort_unstable();
        targets.dedup();
        Ok(matched.then_some(targets))
    }

    fn target_nodes(&self, file: u32, value: &Value<'_>) -> Result<Vec<u32>, ()> {
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

impl Source for Queries<'_> {
    fn entries(&self, scope: ScopeRef, name: &str, class: u64, _: &QueryContext) -> Known<Vec<Entry>> {
        let entries = || -> Result<Vec<Entry>, ()> {
            let scopes = self.semantic.world.module_scopes(scope, self.semantic).known().ok_or(())?;
            let mut direct = Vec::new();
            let mut stars = Vec::new();
            for scope in scopes {
                for (node, item) in self.items(scope)? {
                    let Action::Exports { classes, asks, target, module, alias } = item.action else { continue };
                    if classes != 0 && classes & class == 0 || !self.holds(node, item.when)? { continue }
                    if target.is_none() { stars.push((node, asks, module)); continue }
                    let target = target.ok_or(())?;
                    if let Some(module) = module {
                        let imported = self.operand(node, target)?;
                        let imported = text(&imported).ok_or(())?;
                        let alias = alias.map(|alias| self.operand(node, alias)).transpose()?;
                        let key = alias.as_ref().and_then(text).unwrap_or(imported);
                        if key != name { continue }
                        let source = self.operand(node, module)?;
                        let source = text(&source).ok_or(())?;
                        let source = self.semantic.world.module(leaf(node), source, self.semantic).known().ok_or(())?.ok_or(())?;
                        direct.push(if imported == "*" {
                            Entry::Namespace { name: name.into(), classes: class, source: Known::Yes(source) }
                        } else {
                            Entry::Reexport { name: name.into(), classes: class, asks: self.asks(asks, class), source: Known::Yes(source), imported: imported.into() }
                        });
                    } else {
                        if let Some(targets) = self.named_targets(node, target, alias, name, class, asks)? {
                            direct.push(Entry::Local { name: name.into(), classes: class, targets });
                        }
                    }
                }
            }
            if !direct.is_empty() { return Ok(direct) }
            for (node, asks, module) in stars {
                let module = self.operand(node, module.ok_or(())?)?;
                let source = self.semantic.world.module(leaf(node), text(&module).ok_or(())?, self.semantic).known().ok_or(())?.ok_or(())?;
                direct.push(Entry::Star { classes: class, asks: self.asks(asks, class), source: Known::Yes(source) });
            }
            Ok(direct)
        };
        entries().map_or(Known::Unknown, Known::Yes)
    }

    fn star_excludes(&self, _: ScopeRef, _: &str) -> bool { false }
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
                names
            }).collect();
            Self { grammar: Grammar::build(&Config::default(), &mut Vec::new()), trees, names }
        }
    }

    impl World for Files {
        fn grammar(&self) -> &Grammar { &self.grammar }
        fn tree(&self, file: u32) -> Option<&Tree> { self.trees.get(file as usize) }
        fn text(&self, _: u32) -> Option<&str> { Some("X P") }
        fn names(&self, file: u32) -> Option<&Resolved> { self.names.get(file as usize) }
        fn index(&self, file: u32) -> Option<&ScopeIndex> {
            Some(self.names(file)?.index(self.tree(file)?, self.text(file)?, &|text| text.into()))
        }
        fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> { text.into() }
        fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Yes(Some(ScopeRef { file: 1, scope: 0 })) }
        fn global_scopes(&self, _: u32, _: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
            Known::Yes(vec![ScopeRef { file: 0, scope: 0 }, ScopeRef { file: 1, scope: 0 }])
        }
        fn condition(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<bool> { Known::Yes(true) }
        fn operand(&self, at: Leaf, operand: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
            let Target::Node(node) = at.target else { return Known::Unknown };
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
            Conditional { action: Action::Globals { first, classes: 1, asks: 0, target: Some(0), module: None }, when: None },
        ]);
        Names { kinds, ..Names::default() }
    }

    fn target(file: u32) -> Target { Target::Node(NodeRef { file, node: 1 }) }

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
    fn lexical_body_scopes_are_transparent_to_publications_not_publication_targets() {
        let mut files = Files::new();
        files.trees[0] = Tree::from_native(vec![
            Node { matcher: 0, start: 0, end: 3, children: vec![Edge { label: 0, node: 3 }] },
            Node { matcher: 1, start: 0, end: 1, children: Vec::new() },
            Node { matcher: 2, start: 2, end: 3, children: Vec::new() },
            Node { matcher: 2, start: 0, end: 3, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 2 }] },
        ], 0);
        files.names[0].scopes.push(ScopeInfo { node: 3, classes: 2, parent: Some(0) });
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
        let files = Files::new();
        for (first, expected) in [(false, vec![target(0), target(1)]), (true, vec![target(0)])] {
            let tables = tables(first);
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            assert_eq!(semantic.modules().global(0, "X", 1), Known::Yes(expected));
            assert_eq!(semantic.modules().global(0, "Missing", 1), Known::Yes(Vec::new()));
        }
    }

    #[test]
    fn import_alias_definition_accepts_uses_and_declarations_without_exposing_source_classes() {
        let mut files = Files::new();
        files.trees[0].nodes[1].matcher = 3;
        files.names[0].declared.get_mut(&1).unwrap().2 = 2;
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

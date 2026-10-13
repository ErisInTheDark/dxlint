use crate::expr::{Builtin, CExpr, Value};
use crate::fx_hash::FxHashMap;
use crate::grammar::{Tree, names::{NameKey, Names}};

use super::compile::{Action, Conditional};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PublicationKind {
    Exports,
    Globals,
}

#[derive(Default)]
pub struct Index {
    scopes: FxHashMap<(u32, PublicationKind), Scope>,
    plans: Vec<Plan>,
    /// Union of class masks any scope of the file publishes, per
    /// publication kind: for each static name, and for computed names.
    mentioned: FxHashMap<Box<str>, [u64; PublicationKind::COUNT]>,
    computed: [u64; PublicationKind::COUNT],
}

impl PublicationKind {
    const COUNT: usize = 2;
}

#[derive(Default)]
struct Scope {
    all: Vec<usize>,
    named: FxHashMap<Box<str>, FxHashMap<u64, Vec<usize>>>,
    computed: FxHashMap<u64, Vec<usize>>,
}

pub struct Plan {
    pub node: u32,
    pub item: Conditional,
}

impl Index {
    pub fn build(tables: &Names, tree: &Tree, text: &str, key: &NameKey<'_>) -> Self {
        let mut index = Self::default();
        for &node in &tree.order {
            let Some(kind) = tables.kinds.get(tree.node(node).matcher as usize) else { continue };
            if kind.modules.items.is_empty() { continue }
            let mut scopes = Vec::new();
            let mut ancestor = Some(node);
            while let Some(owner) = ancestor {
                if tables.kinds.get(tree.node(owner).matcher as usize).is_some_and(|kind| {
                    kind.modules.items.iter().any(|item| matches!(item.action, Action::Uses(_) | Action::Scope { .. }))
                }) {
                    scopes.push(owner);
                }
                ancestor = tree.parent(owner);
            }
            for item in &kind.modules.items {
                let (publication, classes) = match item.action {
                    Action::Exports { classes, .. } => (PublicationKind::Exports, classes),
                    Action::Globals { classes, .. } => (PublicationKind::Globals, classes),
                    _ => continue,
                };
                let id = index.plans.len();
                let names = publication_names(tables, tree, text, key, node, &item.action);
                index.plans.push(Plan { node, item: item.clone() });
                let mask = if classes == 0 { u64::MAX } else { classes };
                if !scopes.is_empty() {
                    match &names {
                        Some(names) => for name in names {
                            index.mentioned.entry(name.clone()).or_default()[publication as usize] |= mask;
                        },
                        None => index.computed[publication as usize] |= mask,
                    }
                }
                for &owner in &scopes {
                    let scope = index.scopes.entry((owner, publication)).or_default();
                    scope.all.push(id);
                    match &names {
                        Some(names) => for name in names {
                            scope.named.entry(name.clone()).or_default().entry(classes).or_default().push(id);
                        },
                        None => scope.computed.entry(classes).or_default().push(id),
                    }
                }
            }
        }
        index
    }

    /// Whether any scope of the file could publish `name` in `class`: when
    /// not, every owner's plans for it are empty, without resolving names.
    pub fn mentions(&self, name: &str, class: u64, publication: PublicationKind) -> bool {
        let named = self.mentioned.get(name).map_or(0, |masks| masks[publication as usize]);
        (named | self.computed[publication as usize]) & class != 0
    }

    pub fn plans(&self, owner: u32, name: Option<(&str, u64)>, publication: PublicationKind) -> Vec<&Plan> {
        let Some(scope) = self.scopes.get(&(owner, publication)) else { return Vec::new() };
        let ids = match name {
            None => return scope.all.iter().map(|&id| &self.plans[id]).collect(),
            Some((name, class)) => {
                let matching = |classes: &FxHashMap<u64, Vec<usize>>| {
                    classes.iter().filter(|(mask, _)| class != 0 && (**mask == 0 || **mask & class != 0))
                        .flat_map(|(_, ids)| ids.iter().copied()).collect::<Vec<_>>()
                };
                let mut ids = scope.named.get(name).map(matching).unwrap_or_default();
                ids.extend(matching(&scope.computed));
                ids.sort_unstable();
                ids
            }
        };
        ids.into_iter().map(|id| &self.plans[id]).collect()
    }
}

fn publication_names(tables: &Names, tree: &Tree, text: &str, key: &NameKey<'_>, node: u32, action: &Action) -> Option<Vec<Box<str>>> {
    let expression = |id: u32| tables.operands.get(id as usize)?.expr.get();
    if let Action::Globals { named: Some(named), .. } = *action {
        return static_names(expression(named)?);
    }
    let (target, module, alias) = match *action {
        Action::Exports { target, module, alias, .. } => (target?, module, alias),
        Action::Globals { target, module, .. } => (target?, module, None),
        _ => return None,
    };
    if let Some(alias) = alias {
        return match expression(alias)? {
            CExpr::Const(Value::Str(name)) => Some(vec![(&**name).into()]),
            _ => None,
        };
    }
    if module.is_some() {
        return match expression(target)? {
            CExpr::Const(Value::Str(name)) => Some(vec![(&**name).into()]),
            _ => None,
        };
    }
    let mut names = Vec::new();
    for node in syntax_nodes(expression(target)?, tree, node)? {
        let mut targets = Vec::new();
        possible_targets(tables, tree, node, &mut Vec::new(), &mut targets)?;
        for target in targets {
            let syntax = tree.node(target);
            names.push(key(text.get(syntax.start as usize..syntax.end as usize)?).into());
        }
    }
    names.sort_unstable();
    names.dedup();
    Some(names)
}

/// A constant list of text names (`named [...]`); anything else is unknown.
fn static_names(expression: &CExpr) -> Option<Vec<Box<str>>> {
    let text = |value: &Value<'_>| match value { Value::Str(name) => Some(Box::<str>::from(&**name)), _ => None };
    let mut names: Vec<Box<str>> = match expression {
        CExpr::Const(Value::List(values)) => values.iter().map(text).collect::<Option<_>>()?,
        CExpr::List(expressions) => expressions.iter().map(|expression| match expression {
            CExpr::Const(value) => text(value),
            _ => None,
        }).collect::<Option<_>>()?,
        _ => return None,
    };
    names.sort_unstable();
    names.dedup();
    Some(names)
}

fn syntax_nodes(expression: &CExpr, tree: &Tree, node: u32) -> Option<Vec<u32>> {
    match expression {
        CExpr::Builtin(Builtin::Node) => Some(vec![node]),
        CExpr::OnField(label, inner) => {
            let mut nodes = Vec::new();
            for child in tree.fields(node, *label) {
                nodes.extend(syntax_nodes(inner, tree, child)?);
            }
            Some(nodes)
        }
        CExpr::OnParent(inner) => tree.parent(node).map_or_else(|| Some(Vec::new()), |parent| syntax_nodes(inner, tree, parent)),
        CExpr::List(expressions) => {
            let mut nodes = Vec::new();
            for expression in expressions {
                nodes.extend(syntax_nodes(expression, tree, node)?);
            }
            Some(nodes)
        }
        CExpr::Member(base, name, None) if &**name == "nodes" => syntax_nodes(base, tree, node),
        CExpr::Const(Value::Null) => Some(Vec::new()),
        _ => None,
    }
}

fn possible_targets(tables: &Names, tree: &Tree, node: u32, active: &mut Vec<u32>, targets: &mut Vec<u32>) -> Option<()> {
    let node = tree.semantic_node(node)?;
    if active.len() >= super::super::question::Budget::DEPTH as usize || active.contains(&node) { return None }
    let paths = tables.kinds.get(tree.node(node).matcher as usize).map_or(&[][..], |kind| &kind.targets);
    if paths.is_empty() || paths.iter().all(|(_, when)| when.is_some()) {
        targets.push(node);
    }
    active.push(node);
    for (alternatives, _) in paths {
        for path in alternatives {
            for child in tree.semantic_follow(node, path)? {
                possible_targets(tables, tree, child, active, targets)?;
            }
        }
    }
    active.pop();
    Some(())
}

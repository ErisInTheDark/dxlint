use dx_ir::builtins::Builtin;
use dx_ir::expr::CExpr;
use dx_ir::value::Value;
use dx_core::fx_hash::FxHashMap;
use dx_tree::Tree;
use crate::{NameKey, Names};

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
    /// Another file's index: lookups are recorded ([`dx_core::parts`]).
    pub owner: Option<std::sync::Arc<dx_core::parts::Owner>>,
}

impl PublicationKind {
    const COUNT: usize = 2;
}

#[derive(Default)]
struct Scope {
    all: Vec<usize>,
    named: FxHashMap<Box<str>, FxHashMap<u64, Vec<usize>>>,
    computed: FxHashMap<u64, Vec<usize>>,
    /// Global publications that only pull in another file's: they name
    /// nothing themselves, and which files they reach is one program fact,
    /// not a question per name.
    links: Vec<usize>,
}

pub struct Plan {
    pub node: u32,
    pub item: Conditional,
}

/// The tree's module index, built once and kept for the tree's life.
pub fn module_index<'t>(
    tree: &'t Tree,
    tables: &Names,
    text: &str,
    key: &NameKey<'_>,
    profile: Option<&dx_core::profile::Profile>,
) -> &'t Index {
    if tree.regions().is_some() {
        dx_tree::footprint::enter_atom(dx_tree::footprint::Atom::whole(dx_tree::footprint::tables::MODULE))
    }
    tree.cached(|| dx_tree::footprint::quiet(|| {
        let build = || Index::build(tables, tree, text, key);
        let mut index = match profile {
            Some(profile) => profile.time_module_index(build),
            None => build(),
        };
        index.owner = tree.owner.clone();
        index
    }))
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
                let link = matches!(item.action, Action::Globals { target: None, module: Some(_), asks: 0, .. });
                if !scopes.is_empty() && !link {
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
                    if link {
                        scope.links.push(id);
                        continue;
                    }
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
        if let Some(owner) = &self.owner { dx_core::parts::note(owner, dx_core::parts::Read::Name(dx_core::parts::name_hash(name))) }
        let named = self.mentioned.get(name).map_or(0, |masks| masks[publication as usize]);
        (named | self.computed[publication as usize]) & class != 0
    }

    pub fn plans(&self, owner: u32, name: Option<(&str, u64)>, publication: PublicationKind) -> Vec<&Plan> {
        if let Some(file) = &self.owner {
            dx_core::parts::note(file, name.map_or(dx_core::parts::Read::Whole, |(name, _)| dx_core::parts::Read::Name(dx_core::parts::name_hash(name))));
        }
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

    /// Whether any scope of the file links to other files' global publications.
    pub fn has_links(&self) -> bool {
        self.scopes.iter().any(|((_, publication), scope)| *publication == PublicationKind::Globals && !scope.links.is_empty())
    }

    /// `owner`'s links to other files' global publications. Who reads through
    /// them records the names it looks up in each file they reach.
    pub fn links(&self, owner: u32) -> Vec<&Plan> {
        self.scopes.get(&(owner, PublicationKind::Globals))
            .map_or_else(Vec::new, |scope| scope.links.iter().map(|&id| &self.plans[id]).collect())
    }

    /// What `owner`'s scope may publish: each name (`None`: computed) with
    /// its plan. Unrecorded: for describing the file itself.
    pub fn published(&self, owner: u32) -> Vec<(Option<&str>, &Plan)> {
        let mut out = Vec::new();
        for publication in [PublicationKind::Exports, PublicationKind::Globals] {
            let Some(scope) = self.scopes.get(&(owner, publication)) else { continue };
            for (name, classes) in &scope.named {
                out.extend(classes.values().flatten().map(|&id| (Some(&**name), &self.plans[id])));
            }
            out.extend(scope.computed.values().flatten().map(|&id| (None, &self.plans[id])));
            out.extend(scope.links.iter().map(|&id| (None, &self.plans[id])));
        }
        out
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

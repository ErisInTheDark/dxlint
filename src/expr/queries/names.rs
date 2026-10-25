use crate::expr::Value;
use crate::grammar::names::{ReferenceLookup, Resolved, ScopeInfo};
use crate::grammar::names::question::{Known, Leaf, NodeRef, ScopeRef, Target};
use crate::grammar::names::semantic::Semantic;

use super::QueryFunction;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ClassKind {
    Scope,
    Declaration,
}

#[derive(Default)]
pub(crate) struct Memo {
    classes: std::cell::RefCell<crate::fx_hash::FxHashMap<(u32, ClassKind, u64), Value<'static>>>,
    scopes: std::cell::RefCell<crate::fx_hash::FxHashMap<ScopeRef, Value<'static>>>,
    ancestors: std::cell::RefCell<crate::fx_hash::FxHashMap<(ScopeRef, Option<u32>), Value<'static>>>,
    bindings: std::cell::RefCell<crate::fx_hash::FxHashMap<NodeRef, Value<'static>>>,
}

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "name_scopes", arity: 2, call: scopes },
    QueryFunction { name: "name_scope_declarations", arity: 3, call: declarations },
    QueryFunction { name: "name_role_classes", arity: 2, call: role_classes },
    QueryFunction { name: "name_binding", arity: 1, call: binding },
    QueryFunction { name: "name_definitions", arity: 2, call: definitions },
];

/// `name_definitions(name, class)`: the declarations a name of `class`
/// finally stands for, through imports and re-exports, as `{file, node}`
/// sites; `[]` when it names nothing of that class, null when unknown.
fn definitions(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(context) = Context::read(semantic, at) else { return Known::Unknown };
    let Some(Value::Str(class)) = args.first() else { return Known::Unknown };
    let class = semantic.tables.vocabulary.declaration_class(class);
    if class == 0 { return Known::Unknown }
    // A reference stands for its local declaration first; that one (an
    // import specifier) may stand for another module's.
    let names = context.names;
    let declaration = if names.declared.contains_key(&context.node.node) { context.node.node }
        else if let Some(&declaration) = names.declaration.get(&context.node.node) { declaration }
        else { return Known::Yes(Value::Null) };
    // Of merged declarations (`interface X` + `namespace X`), the one of `class`.
    let declaration = names.declaration_in(names.merged.get(&declaration).copied().unwrap_or(declaration), class);
    let node = NodeRef { file: context.node.file, node: declaration };
    let declares = names.declared.get(&declaration).is_some_and(|&(_, _, classes, _)| classes & class != 0);
    match semantic.modules().definition(node, class) {
        // Nothing further: the declaration itself, when it declares `class`.
        Known::Yes(targets) if declares && targets.as_ref().is_none_or(Vec::is_empty) => Known::Yes(Value::list(vec![Value::record(vec![
            ("file".into(), Value::Num(f64::from(node.file))),
            ("node".into(), Value::Num(f64::from(node.node))),
        ])])),
        Known::Yes(Some(targets)) => Known::Yes(Value::list(targets.into_iter().filter_map(|target| {
            // A scope target (a namespace, an enum) stands for its owner.
            let node = match target {
                Target::Node(node) => node,
                Target::Scope(scope) => NodeRef { file: scope.file, node: semantic.world.names(scope.file)?.scopes.get(scope.scope as usize)?.node },
                _ => return None,
            };
            Some(Value::record(vec![
                ("file".into(), Value::Num(f64::from(node.file))),
                ("node".into(), Value::Num(f64::from(node.node))),
            ]))
        }).collect())),
        Known::Yes(None) => Known::Yes(Value::list(Vec::new())),
        Known::Unknown => Known::Unknown,
    }
}

enum Role {
    Declaration,
    Reference,
    Lexical,
}

impl Role {
    fn read(value: Option<&Value<'static>>) -> Option<Self> {
        match value? {
            Value::Str(role) => match &**role {
                "declaration" => Some(Self::Declaration),
                "reference" => Some(Self::Reference),
                "lexical" => Some(Self::Lexical),
                _ => None,
            },
            _ => None,
        }
    }

    fn scope(&self, names: &Resolved, node: u32) -> Option<u32> {
        match self {
            Self::Declaration => names.declared.get(&node).map(|&(scope, ..)| scope),
            Self::Reference => names.asked.get(&node).map(|lookup| lookup.scope),
            Self::Lexical => names.scope_of(node),
        }
    }
}

struct Context<'a, 's> {
    semantic: &'a Semantic<'s>,
    node: NodeRef,
    names: &'s Resolved,
}

impl<'a, 's> Context<'a, 's> {
    fn read(semantic: &'a Semantic<'s>, at: Leaf) -> Option<Self> {
        let Known::Yes(at) = semantic.leaf(at) else { return None };
        let Target::Node(node) = at.target else { return None };
        semantic.world.tree(node.file)?.nodes.get(node.node as usize)?;
        let names = semantic.world.names(node.file)?;
        if !names.failures.is_empty() { return None }
        Some(Self { semantic, node, names })
    }

    fn node(&self, node: u32) -> Option<Value<'static>> {
        self.semantic.world.tree(self.node.file)?.nodes.get(node as usize)?;
        Some(Value::Node(node, "".into()))
    }

    fn classes(&self, mask: u64, kind: ClassKind) -> Option<Value<'static>> {
        let key = (self.node.file, kind, mask);
        let memo = &self.semantic.name_queries.classes;
        if let Some(classes) = memo.borrow().get(&key) { return Some(classes.clone()) }
        let vocabulary = match kind {
            ClassKind::Scope => &self.names.vocabulary.scope_classes,
            ClassKind::Declaration => &self.names.vocabulary.declaration_classes,
        };
        let mut classes = Vec::new();
        for (bit, name) in vocabulary.iter().enumerate() {
            if !self.semantic.question.budget.step() { return None }
            if mask & (1 << bit) != 0 { classes.push(Value::from(name.clone())); }
        }
        let classes = Value::list(classes);
        memo.borrow_mut().insert(key, classes.clone());
        Some(classes)
    }

    fn scope_id(&self, scope: u32) -> Value<'static> {
        Value::from(format!("{}:{scope}", self.node.file))
    }

    fn read_scope(&self, id: &Value<'static>) -> Option<u32> {
        let Value::Str(id) = id else { return None };
        let (file, scope) = id.split_once(':')?;
        let scope = ScopeRef { file: file.parse().ok()?, scope: scope.parse().ok()? };
        if scope.file != self.node.file { return None }
        self.names.scopes.get(scope.scope as usize)?;
        Some(scope.scope)
    }

    fn scope(&self, id: u32, scope: &ScopeInfo) -> Option<Value<'static>> {
        let key = ScopeRef { file: self.node.file, scope: id };
        let memo = &self.semantic.name_queries.scopes;
        if let Some(scope) = memo.borrow().get(&key) { return Some(scope.clone()) }
        let value = Value::record(vec![
            ("id".into(), self.scope_id(id)),
            ("parent".into(), scope.parent.map_or(Value::Null, |parent| self.scope_id(parent))),
            ("owner".into(), self.node(scope.node)?),
            ("classes".into(), self.classes(scope.classes, ClassKind::Scope)?),
        ]);
        memo.borrow_mut().insert(key, value.clone());
        Some(value)
    }
}

fn scopes(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(context) = Context::read(semantic, at) else { return Known::Unknown };
    let Some(role) = Role::read(args.first()) else { return Known::Unknown };
    let Some(id) = role.scope(context.names, context.node.node) else { return Known::Yes(Value::Null) };
    let lookup = match role {
        Role::Reference => context.names.asked[&context.node.node],
        _ => ReferenceLookup { scope: id, classes: 0, outside: None },
    };
    let key = (ScopeRef { file: context.node.file, scope: id }, lookup.outside);
    let memo = &semantic.name_queries.ancestors;
    if let Some(chain) = memo.borrow().get(&key) { return Known::Yes(chain.clone()) }
    let mut chain = Vec::new();
    for id in context.names.lookup_scopes(lookup) {
        if !semantic.question.budget.step() { return Known::Unknown }
        let Some(scope) = context.names.scopes.get(id as usize) else { return Known::Unknown };
        let Some(value) = context.scope(id, scope) else { return Known::Unknown };
        chain.push(value);
    }
    let chain = Value::list(chain);
    memo.borrow_mut().insert(key, chain.clone());
    Known::Yes(chain)
}

fn declarations(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(context) = Context::read(semantic, at) else { return Known::Unknown };
    let Some(scope) = args.first().and_then(|id| context.read_scope(id)) else { return Known::Unknown };
    let Some(Value::Str(key)) = args.get(1) else { return Known::Unknown };
    let Some(index) = semantic.world.index(context.node.file) else { return Known::Unknown };
    let mut rows = Vec::new();
    for &node in index.declared(scope, key) {
        if !semantic.question.budget.step() { return Known::Unknown }
        let Some(&(declared_scope, owner, classes, hoisted)) = context.names.declared.get(&node) else { return Known::Unknown };
        if declared_scope != scope { return Known::Unknown }
        let Some(node) = context.node(node) else { return Known::Unknown };
        let Some(owner) = context.node(owner) else { return Known::Unknown };
        let Some(classes) = context.classes(classes, ClassKind::Declaration) else { return Known::Unknown };
        rows.push(Value::record(vec![
            ("node".into(), node),
            ("key".into(), Value::from(key.to_string())),
            ("classes".into(), classes),
            ("hoisted".into(), Value::Bool(hoisted)),
            ("owner".into(), owner),
        ]));
    }
    Known::Yes(Value::list(rows))
}

fn role_classes(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(context) = Context::read(semantic, at) else { return Known::Unknown };
    let Some(role) = Role::read(args.first()) else { return Known::Unknown };
    let classes = match role {
        Role::Declaration => context.names.declared.get(&context.node.node).map(|&(_, _, classes, _)| classes),
        Role::Reference => context.names.asked.get(&context.node.node).map(|lookup| lookup.classes),
        Role::Lexical => return Known::Unknown,
    };
    let Some(classes) = classes else { return Known::Yes(Value::Null) };
    context.classes(classes, ClassKind::Declaration).map_or(Known::Unknown, Known::Yes)
}

fn binding(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(context) = Context::read(semantic, at) else { return Known::Unknown };
    let names = context.names;
    let declaration = if names.declared.contains_key(&context.node.node) {
        context.node.node
    } else if let Some(&declaration) = names.declaration.get(&context.node.node) {
        declaration
    } else {
        return Known::Yes(Value::Null);
    };
    let declaration = names.merged.get(&declaration).copied().unwrap_or(declaration);
    if names.implicit.contains_key(&declaration) { return Known::Yes(Value::Null) }
    let cache_key = NodeRef { file: context.node.file, node: declaration };
    let memo = &semantic.name_queries.bindings;
    if let Some(binding) = memo.borrow().get(&cache_key) { return Known::Yes(binding.clone()) }
    let Some(&(scope, ..)) = names.declared.get(&declaration) else { return Known::Unknown };
    let Some(tree) = semantic.world.tree(context.node.file) else { return Known::Unknown };
    let key = match names.keys.get(&declaration) {
        Some(Some(key)) => key.to_string(),
        Some(None) => return Known::Unknown,
        None => {
            let Some(node) = tree.nodes.get(declaration as usize) else { return Known::Unknown };
            let Some(text) = semantic.world.text(context.node.file)
                .and_then(|text| text.get(node.start as usize..node.end as usize)) else { return Known::Unknown };
            semantic.world.name_key(context.node.file, text).into_owned()
        }
    };
    let Some(index) = semantic.world.index(context.node.file) else { return Known::Unknown };
    let mut declarations = Vec::new();
    let mut reference_nodes = Vec::new();
    for &node in index.declared(scope, &key) {
        if !semantic.question.budget.step() { return Known::Unknown }
        if names.merged.get(&node).copied().unwrap_or(node) != declaration { continue }
        for &reference in names.references.get(&node).into_iter().flatten() {
            if !semantic.question.budget.step() { return Known::Unknown }
            if tree.nodes.get(reference as usize).is_none() { return Known::Unknown }
            reference_nodes.push(reference);
        }
        let Some(node) = context.node(node) else { return Known::Unknown };
        declarations.push(node);
    }
    if declarations.is_empty() { return Known::Unknown }
    reference_nodes.sort_unstable_by_key(|&node| (tree.node(node).start, node));
    reference_nodes.dedup();
    let mut references = Vec::new();
    for node in reference_nodes {
        let Some(node) = context.node(node) else { return Known::Unknown };
        references.push(node);
    }
    let Some(scope_info) = names.scopes.get(scope as usize) else { return Known::Unknown };
    let Some(scope) = context.scope(scope, scope_info) else { return Known::Unknown };
    let Some(node) = context.node(declaration) else { return Known::Unknown };
    let value = Value::record(vec![
        ("node".into(), node),
        ("key".into(), Value::from(key)),
        ("scope".into(), scope),
        ("declarations".into(), Value::list(declarations)),
        ("references".into(), Value::list(references)),
    ]);
    memo.borrow_mut().insert(cache_key, value.clone());
    Known::Yes(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::{lint, linter};
    use crate::grammar::{Grammar, Node, Tree};
    use crate::grammar::names::{Names, ScopeIndex};
    use crate::grammar::names::question::{FrameId, QueryContext, World};

    struct ScopeWorld {
        grammar: Grammar,
        tree: Tree,
        names: Resolved,
        available: std::cell::Cell<bool>,
        reads: std::cell::Cell<u32>,
    }

    impl World for ScopeWorld {
        fn grammar(&self) -> &Grammar { &self.grammar }
        fn tree(&self, file: u32) -> Option<&Tree> { (file == 0).then_some(&self.tree) }
        fn text(&self, file: u32) -> Option<&str> { (file == 0).then_some("") }
        fn names(&self, file: u32) -> Option<&Resolved> {
            self.reads.set(self.reads.get() + 1);
            (file == 0 && self.available.get()).then_some(&self.names)
        }
        fn index(&self, file: u32) -> Option<&ScopeIndex> {
            (file == 0).then(|| self.names.index(&self.tree, "", &|text| text.into()))
        }
        fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> { text.into() }
        fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Unknown }
        fn operand(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<Value<'static>> { Known::Unknown }
        fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<Value<'static>> { Known::Unknown }
        fn condition(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<bool> { Known::Unknown }
    }

    #[test]
    fn reference_scope_queries_keep_nested_scopes_and_distinct_lookup_policies() {
        let mut world = ScopeWorld {
            grammar: Grammar::build(&crate::config::ast::Config::default(), &mut Vec::new()),
            tree: Tree::from_native(vec![Node { matcher: 0, start: 0, end: 0, children: Vec::new() }; 3], 0),
            names: Resolved::default(),
            available: std::cell::Cell::new(true),
            reads: std::cell::Cell::new(0),
        };
        for (node, parent) in [(0, None), (1, Some(0)), (1, Some(1)), (2, Some(2))] {
            world.names.scopes.push(ScopeInfo { node, classes: 1, parent });
        }
        std::sync::Arc::get_mut(&mut world.names.vocabulary).unwrap().scope_classes.push("block".into());
        world.names.asked.insert(1, ReferenceLookup { scope: 3, classes: 1, outside: Some(1) });
        world.names.asked.insert(2, ReferenceLookup { scope: 3, classes: 1, outside: None });
        let tables = Names::default();
        let question = QueryContext::default();
        let semantic = Semantic::new(&world, &tables, &question);
        for node in [1, 2, 2, 1] {
            let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
            let Known::Yes(Value::List(chain)) = scopes(&semantic, at, &[Value::from("reference")]) else {
                panic!("known scopes");
            };
            let owners: Vec<u32> = chain.iter().map(|scope| {
                let Value::Record(fields) = scope else { panic!("scope"); };
                let Value::Node(owner, _) = &fields.iter().find(|(key, _)| &**key == "owner").unwrap().1 else {
                    panic!("owner");
                };
                *owner
            }).collect();
            assert_eq!(owners, if node == 1 { vec![2, 0] } else { vec![2, 1, 1, 0] });
        }
    }

    #[test]
    fn lexical_name_queries_do_not_lose_answers_in_bound_frames() {
        let mut world = ScopeWorld {
            grammar: Grammar::build(&crate::config::ast::Config::default(), &mut Vec::new()),
            tree: Tree::from_native(vec![Node { matcher: 0, start: 0, end: 0, children: Vec::new() }; 2], 0),
            names: Resolved::default(),
            available: std::cell::Cell::new(true),
            reads: std::cell::Cell::new(0),
        };
        world.names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
        let vocabulary = std::sync::Arc::get_mut(&mut world.names.vocabulary).unwrap();
        vocabulary.scope_classes.push("lexical".into());
        vocabulary.declaration_classes.push("value".into());
        world.names.asked.insert(0, ReferenceLookup { scope: 0, classes: 1, outside: None });
        world.names.declared.insert(0, (0, 0, 1, true));
        let tables = Names::default();
        let question = QueryContext::default();
        let at = Leaf { target: Target::Node(NodeRef::here(0)), frame: FrameId::ROOT };
        let frame = question.bind(FrameId::ROOT, NodeRef::here(0), Some(at), None);
        let semantic = Semantic::new(&world, &tables, &question);
        let bound = Leaf { frame, ..at };
        for role in ["reference", "declaration"] {
            let args = [Value::from(role)];
            let expected = scopes(&semantic, at, &args);
            assert!(matches!(expected, Known::Yes(_)));
            assert_eq!(scopes(&semantic, bound, &args), expected);
            assert_eq!(role_classes(&semantic, bound, &args), Known::Yes(Value::list(vec![Value::from("value")])));
        }
        let args = [Value::from("0:0"), Value::from("missing")];
        assert_eq!(declarations(&semantic, bound, &args), Known::Yes(Value::list(Vec::new())));
        let absent = Leaf { target: Target::Node(NodeRef::here(1)), frame };
        for role in ["reference", "declaration", "lexical"] {
            assert_eq!(scopes(&semantic, absent, &[Value::from(role)]), Known::Yes(Value::Null));
        }
        for role in ["reference", "declaration"] {
            assert_eq!(role_classes(&semantic, absent, &[Value::from(role)]), Known::Yes(Value::Null));
        }
        assert_eq!(scopes(&semantic, absent, &[Value::from("invalid")]), Known::Unknown);
        assert_eq!(role_classes(&semantic, absent, &[Value::from("lexical")]), Known::Unknown);
        let foreign = Leaf { target: Target::Node(NodeRef { file: 1, node: 0 }), frame };
        assert_eq!(scopes(&semantic, foreign, &[Value::from("reference")]), Known::Unknown);
        let args = [Value::from("reference")];
        let expected_classes = role_classes(&semantic, at, &args);
        let expected_scopes = scopes(&semantic, at, &args);
        while question.budget.step() {}
        let reads = world.reads.get();
        assert_eq!(role_classes(&semantic, bound, &args), expected_classes);
        assert_eq!(scopes(&semantic, bound, &args), expected_scopes);
        assert!(world.reads.get() > reads);
        world.available.set(false);
        assert_eq!(role_classes(&semantic, bound, &args), Known::Unknown);
        assert_eq!(scopes(&semantic, bound, &args), Known::Unknown);
        world.available.set(true);
        let session = crate::grammar::names::semantic::Session::default();
        assert_eq!(session.run(&world, &tables, |semantic| {
            while semantic.question.budget.step() {}
            role_classes(semantic, at, &args)
        }), Known::Unknown);
        assert_eq!(session.run(&world, &tables, |semantic| role_classes(semantic, at, &args)), expected_classes);
        for exhaust in [true, false, false] {
            assert_eq!(session.run(&world, &tables, |semantic| {
                let frame = semantic.question.bind(FrameId::ROOT, NodeRef::here(0), Some(at), None);
                if exhaust { while semantic.question.budget.step() {} }
                role_classes(semantic, Leaf { frame, ..at }, &args)
            }), if exhaust { Known::Unknown } else { expected_classes.clone() });
        }
    }

    #[test]
    fn scope_queries_keep_dual_roles_logical_scope_ids_and_indexed_merged_sites() {
        let linter = linter(r#"
            in .t match document /[\s\S]+/ { scope outer; scope inner }
            in document match declaration name:/x/ /;/ {
                declares value name in nearest outer hoisted
            }
            in document match dual name:/x/ /\?/ {
                scope members
                declares member name in nearest outer
                resolves value name
                let decl_scope { value name.node; name_scopes "declaration"; first }
                let referenced name_scopes(name.node, "reference")
                let member_scope first(referenced)
                let inner { value referenced; slice 1; first }
                let outer last(referenced)
                let sites {
                    value name_scope_declarations(name.node, outer.id, "x")
                    filter (site) contains(site.classes, "value")
                }
                let declared_classes name_role_classes(name.node, "declaration")
                let asked_classes name_role_classes(name.node, "reference")
                rule observed warn "observed"
                rule invalid warn "invalid" {
                    when member_scope.owner != node || inner.owner != outer.owner || inner.id == outer.id
                        || member_scope.parent != inner.id || inner.parent != outer.id || outer.parent != null
                        || decl_scope.id != outer.id || count(sites) != 2
                        || first(sites).node.offset >= last(sites).node.offset
                        || !all(sites, site => site.hoisted && contains(site.classes, "value"))
                        || !contains(declared_classes, "member") || contains(declared_classes, "value")
                        || !contains(asked_classes, "value") || contains(asked_classes, "member")
                        || outer.owner.text != source
                        || name_scope_declarations(name.node, "not a scope", "x") != null
                        || name_scope_declarations(name.node, outer.id, "missing") != []
                }
            }
        "#);
        let reports = lint(&linter, "x;x;x?");
        assert_eq!(reports.len(), 1, "{reports:#?}");
        assert_eq!(reports[0].rule, "observed");
    }

    #[test]
    fn binding_queries_join_class_specific_reference_buckets() {
        let mut world = ScopeWorld {
            grammar: Grammar::build(&crate::config::ast::Config::default(), &mut Vec::new()),
            tree: Tree::from_native((0..5).map(|start| Node {
                matcher: 0, start, end: start + 1, children: Vec::new(),
            }).collect(), 0),
            names: Resolved::default(),
            available: std::cell::Cell::new(true),
            reads: std::cell::Cell::new(0),
        };
        world.names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
        std::sync::Arc::get_mut(&mut world.names.vocabulary).unwrap().scope_classes.push("module".into());
        for (node, classes) in [(0, 1), (1, 2)] {
            world.names.declared.insert(node, (0, node, classes, true));
            world.names.keys.insert(node, Some("x".into()));
        }
        world.names.merged.insert(1, 0);
        world.names.declaration.extend([(2, 0), (3, 1), (4, 1)]);
        world.names.references.extend([(0, vec![2]), (1, vec![3, 4])]);
        let question = QueryContext::default();
        let tables = Names::default();
        let semantic = Semantic::new(&world, &tables, &question);
        for node in 0..5 {
            let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
            let Known::Yes(answer) = binding(&semantic, at, &[]) else { panic!("merged binding exists for {node}") };
            assert_eq!(answer.key("node"), Value::Node(0, "".into()));
            assert_eq!(answer.key("references"), Value::list((2..5).map(|node| Value::Node(node, "".into())).collect()));
            while question.budget.step() {}
        }
        world.available.set(false);
        let at = Leaf { target: Target::Node(NodeRef::here(0)), frame: FrameId::ROOT };
        assert_eq!(binding(&semantic, at, &[]), Known::Unknown);
    }

    #[test]
    fn binding_queries_keep_merged_sites_and_ordered_references() {
        let linter = linter(r#"
            in .t match document /[\s\S]+/ { scope outer }
            in document match declaration name:/x/ /;/ {
                declares value name in nearest outer hoisted
                let binding name_binding(name.node)
                rule observed warn "observed"
                rule invalid-declaration warn "invalid" {
                    when binding.node != first(binding.declarations) || binding.key != "x"
                        || count(binding.declarations) != 2 || count(binding.references) != 2
                        || first(binding.declarations).offset >= last(binding.declarations).offset
                        || first(binding.references).offset >= last(binding.references).offset
                        || binding.node.text != "x" || binding.scope.owner.text != source
                }
            }
            in document match reference name:/x/ /!/ {
                resolves value name
                let binding name_binding(name.node)
                rule invalid-reference warn "invalid" {
                    when binding.node != first(binding.declarations) || count(binding.declarations) != 2
                        || count(binding.references) != 2
                }
            }
        "#);
        let reports = lint(&linter, "x;x;x!x!");
        assert_eq!(reports.len(), 2, "{reports:#?}");
        assert!(reports.iter().all(|report| report.rule == "observed"));
    }
}

//! Value handles: rules walk a value graph themselves (their own recursion,
//! pruning and seen sets) through opaque `{ value_handle }` records. A
//! handle names one interned value of the linted file's question context;
//! foreign or stale handles read as unknown.

use crate::expr::Value;
use crate::grammar::names::question::{Handle, Known, NodeValue};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::values::Key;

use super::values::{lookup_in, member_key, member_parameters_in, project, project_path, project_with, source_value};
use super::QueryFunction;

const FIELD: &str = "value_handle";

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "value_parts", arity: 1, call: |semantic, at, args| parts(semantic, semantic.values().values(at), args) },
    QueryFunction { name: "value_children", arity: 3, call: |semantic, at, args| children(semantic, semantic.values().values(at), args) },
    QueryFunction { name: "value_signatures", arity: 1, call: |semantic, at, args| signatures(semantic, semantic.values().values(at), args) },
    QueryFunction { name: "value_constructors", arity: 1, call: |semantic, at, args| constructors(semantic, semantic.values().values(at), args) },
    QueryFunction { name: "value_bases", arity: 1, call: |semantic, at, args| bases(semantic, semantic.values().values(at), args) },
    QueryFunction { name: "call_selection", arity: 1, call: selection },
    QueryFunction { name: "call_binding", arity: 1, call: |semantic, at, _| binding(semantic, at, None) },
    QueryFunction { name: "call_binding_at", arity: 2, call: |semantic, at, args| {
        let Some(Value::List(names)) = args.first() else { return Known::Yes(Value::Null) };
        let labels = &semantic.world.grammar().labels;
        let path: Option<Vec<u16>> = names.iter().map(|name| labels.get(&name.text())).collect();
        match path {
            Some(path) => binding(semantic, at, Some(&path)),
            None => Known::Yes(Value::Null),
        }
    } },
    QueryFunction { name: "site_text", arity: 2, call: site_text },
    QueryFunction { name: "declaration_origin", arity: 2, call: declaration_origin },
    QueryFunction { name: "declaration_value", arity: 2, call: declaration_value },
    QueryFunction { name: "value_sources", arity: 1, call: |semantic, at, _| sources(semantic, at) },
    QueryFunction { name: "site_sources", arity: 2, call: |semantic, _, args| {
        use crate::grammar::names::question::{FrameId, Leaf, NodeRef, Target};
        match args.first().and_then(site_of) {
            Some((file, node)) => sources(semantic, Leaf { target: Target::Node(NodeRef { file, node }), frame: FrameId::ROOT }),
            None => Known::Yes(Value::Null),
        }
    } },
];

/// A query reading a handle subject instead of a node.
pub struct HandleFunction {
    pub name: &'static str,
    pub call: fn(&Semantic<'_>, NodeValue, &[Value<'static>]) -> Known<Value<'static>>,
}

static HANDLES: &[HandleFunction] = &[
    HandleFunction { name: "value_parts", call: parts },
    HandleFunction { name: "value_children", call: children },
    HandleFunction { name: "value_signatures", call: signatures },
    HandleFunction { name: "value_constructors", call: constructors },
    HandleFunction { name: "value_bases", call: bases },
    HandleFunction { name: "value_constituents", call: |semantic, value, _| project(semantic, value) },
    HandleFunction { name: "value_at", call: project_path },
    HandleFunction { name: "value_lookup", call: |semantic, value, args| match args.first().and_then(member_key) {
        Some(key) => lookup_in(semantic, value, &key),
        None => Known::Unknown,
    } },
    HandleFunction { name: "member_call_parameters", call: |semantic, value, args| match args.first().and_then(member_key) {
        Some(key) => member_parameters_in(semantic, &value, &key),
        None => Known::Unknown,
    } },
    HandleFunction { name: "value_has_member", call: |semantic, value, args| match args.first().and_then(member_key) {
        Some(key) => match semantic.values().has_member(&value, &key) {
            Known::Yes(found) => Known::Yes(Value::Bool(found)),
            Known::Unknown => Known::Unknown,
        },
        None => Known::Unknown,
    } },
    // `value_accepts(allowing, given, _, _)`: handles are already values,
    // so the read modes of the node form do not apply.
    HandleFunction { name: "value_accepts", call: |semantic, value, args| {
        let Some(given) = args.first().and_then(handle_of).and_then(|handle| semantic.question.handle_value(handle)) else { return Known::Unknown };
        match semantic.calls().accepts(&value, &given) {
            Known::Yes(accepted) => Known::Yes(Value::Bool(accepted)),
            Known::Unknown => Known::Unknown,
        }
    } },
];

pub fn handle_function(name: &str) -> Option<&'static HandleFunction> {
    HANDLES.iter().find(|function| function.name == name)
}

/// The handle a rule value carries, if it is one.
pub fn handle_of(value: &Value<'_>) -> Option<Handle> {
    let Value::Record(fields) = value else { return None };
    let [(key, Value::Num(number))] = &fields[..] else { return None };
    if &**key != FIELD { return None }
    Handle::from_number(*number)
}

pub(super) fn handle(semantic: &Semantic<'_>, value: NodeValue) -> Value<'static> {
    let canonical = semantic.calls().without_incidental_frames(&value, true);
    Value::record(vec![(FIELD.into(), Value::Num(semantic.question.handle(canonical).number()))])
}

/// Each alternative as `{ handle, parts }`: the arm's handle (its leaves
/// together) and its constituents, each with its own handle.
fn parts(semantic: &Semantic<'_>, value: NodeValue, _: &[Value<'static>]) -> Known<Value<'static>> {
    let NodeValue::Alternatives(rows) = value else { return Known::Unknown };
    let mut arms = Vec::with_capacity(rows.len());
    for row in rows {
        if row.is_empty() || !semantic.question.budget.step() { return Known::Unknown }
        let arm = NodeValue::Alternatives(vec![row.clone()]);
        let projected = project_with(semantic, arm.clone(), &mut |semantic, argument| Known::Yes(handle(semantic, argument)));
        let Known::Yes(Value::List(projected)) = projected else { return Known::Unknown };
        let Some(Value::List(constituents)) = projected.first() else { return Known::Unknown };
        let parts = constituents.iter().zip(&row).map(|(part, &leaf)| with_handle(part, handle(semantic, NodeValue::leaf(leaf))))
            .collect::<Option<Vec<_>>>();
        let Some(parts) = parts else { return Known::Unknown };
        arms.push(Value::record(vec![("handle".into(), handle(semantic, arm)), ("parts".into(), Value::list(parts))]));
    }
    Known::Yes(Value::list(arms))
}

fn with_handle(part: &Value<'static>, handle: Value<'static>) -> Option<Value<'static>> {
    let Value::Record(fields) = part else { return None };
    let mut fields = fields.to_vec();
    fields.push(("handle".into(), handle));
    Some(Value::record(fields))
}

/// The edges out of one arm: its members (`members` true; only when the
/// arm's members are all known) and the selected carrier arguments of each
/// constituent (`arguments` a list of indexes; `null` is an unknown
/// selection, so unknown).
fn children(semantic: &Semantic<'_>, value: NodeValue, args: &[Value<'static>]) -> Known<Value<'static>> {
    let [Value::Bool(members), arguments] = args else { return Known::Unknown };
    let indexes = match arguments {
        Value::List(indexes) => indexes.iter().map(|index| match index {
            Value::Num(index) if *index >= 0.0 && index.fract() == 0.0 && *index <= f64::from(u32::MAX) => Some(*index as usize),
            _ => None,
        }).collect::<Option<Vec<_>>>(),
        _ => None,
    };
    let Some(indexes) = indexes else { return Known::Unknown };
    let NodeValue::Alternatives(rows) = &value else { return Known::Unknown };
    let [row] = &rows[..] else { return Known::Unknown };
    if row.is_empty() { return Known::Unknown }
    // Generic recursion (`Frozen<T>` holding `Frozen<T>`) binds a new frame
    // per level, so its handles never repeat: past this depth the walk is
    // unknown instead of endless.
    if row.iter().any(|leaf| frame_depth(semantic, leaf.frame) > MAX_FRAME_DEPTH) { return Known::Unknown }
    let values = semantic.values();
    let mut edges = Vec::new();
    if *members {
        // Constituents without members of their own (trait-only parts such
        // as the intersection marker, function types) add none and leave
        // the arm's listing complete.
        let mut owning = Vec::with_capacity(row.len());
        for &leaf in row {
            // Products (tuples) list their own elements, never inherited
            // methods.
            match values.product_info(leaf) {
                Known::Yes(Some(product)) => {
                    for (index, element) in product.elements.into_iter().enumerate() {
                        if !semantic.question.budget.step() { return Known::Unknown }
                        // A rest element (`...string[]`) stands for its items.
                        let item = if element.repeated {
                            values.member(&element.value, &Key::Domain(crate::config::ast::values::KeyDomain::Ordinal))
                        } else { element.value };
                        edges.push(Value::record(vec![
                            ("handle".into(), handle(semantic, item)),
                            ("member".into(), Value::record(vec![
                                ("key".into(), Value::Num(index as f64)),
                                ("optional".into(), Value::Bool(element.optional)),
                                ("repeated".into(), Value::Bool(element.repeated)),
                                ("values".into(), Value::Bool(element.values)),
                                ("sources".into(), Value::list(Vec::new())),
                            ])),
                            ("argument".into(), Value::Null),
                            ("part".into(), Value::Null),
                        ]));
                    }
                    continue;
                }
                Known::Unknown => return Known::Unknown,
                Known::Yes(None) => {}
            }
            match own_member_free(semantic, leaf) {
                Known::Yes(true) => {}
                Known::Yes(false) => owning.push(leaf),
                Known::Unknown => return Known::Unknown,
            }
        }
        let owning = NodeValue::Alternatives(vec![owning]);
        let listed = match &owning {
            NodeValue::Alternatives(rows) if rows[0].is_empty() => Vec::new(),
            _ => {
                if values.closed(&owning) != Known::Yes(true) { return Known::Unknown }
                let Known::Yes(mut listed) = values.members(&owning) else { return Known::Unknown };
                let Some(listed) = listed.pop() else { return Known::Unknown };
                listed
            }
        };
        for member in listed {
            if !semantic.question.budget.step() || member.sources.is_empty() { return Known::Unknown }
            let mut sources = Vec::with_capacity(member.sources.len());
            for source in &member.sources {
                let Known::Yes(source) = source_value(semantic, source) else { return Known::Unknown };
                sources.push(source);
            }
            let child = values.combine(member.sources.iter().map(|source| {
                source.value.as_ref().map_or_else(|| values.values(source.site), |recipe| values.evaluate(source.site, recipe))
            }), true);
            if semantic.question.budget.exhausted() { return Known::Unknown }
            edges.push(Value::record(vec![
                ("handle".into(), handle(semantic, child)),
                ("member".into(), Value::record(vec![
                    ("key".into(), key_value(&member.key)),
                    ("optional".into(), member.required.known().map_or(Value::Null, |required| Value::Bool(!required))),
                    ("sources".into(), Value::list(sources)),
                ])),
                ("argument".into(), Value::Null),
                ("part".into(), Value::Null),
            ]));
        }
    }
    if !indexes.is_empty() {
        for (part, &leaf) in row.iter().enumerate() {
            if !semantic.question.budget.step() { return Known::Unknown }
            let carrier = match values.carrier(leaf) {
                Known::Yes(Some(carrier)) => carrier,
                Known::Yes(None) => continue,
                Known::Unknown => return Known::Unknown,
            };
            for &index in &indexes {
                let Some(argument) = carrier.arguments.get(index) else { continue };
                edges.push(Value::record(vec![
                    ("handle".into(), handle(semantic, argument.clone())),
                    ("member".into(), Value::Null),
                    ("argument".into(), Value::Num(index as f64)),
                    ("part".into(), Value::Num(part as f64)),
                ]));
            }
        }
    }
    Known::Yes(Value::list(edges))
}

/// The call signatures of each union arm, as handles: per header
/// `{parameters, receiver, has_receiver, result}` where `parameters` is a
/// list of handles (null when unreadable), `receiver`/`result` handles (null
/// when absent or unreadable; `has_receiver` false absent, null unknown).
fn signatures(semantic: &Semantic<'_>, value: NodeValue, _: &[Value<'static>]) -> Known<Value<'static>> {
    headers_of(semantic, value, crate::grammar::names::calls::InvokeKind::Call)
}

/// The values one constituent (a part handle, or a value of one leaf)
/// inherits its members from, as handles in declaration order: declared
/// bases (`members ... inherits`), a product's base, a primitive's apparent
/// wrapper. `[]` when it has none; null when unknown or not one constituent.
fn bases(semantic: &Semantic<'_>, value: NodeValue, _: &[Value<'static>]) -> Known<Value<'static>> {
    let NodeValue::Alternatives(rows) = value else { return Known::Unknown };
    let [row] = rows.as_slice() else { return Known::Unknown };
    let [leaf] = row.as_slice() else { return Known::Unknown };
    let Known::Yes(bases) = semantic.values().bases(*leaf) else { return Known::Unknown };
    Known::Yes(Value::list(bases.into_iter().map(|base| handle(semantic, base)).collect()))
}

/// [`signatures`] for `new`.
fn constructors(semantic: &Semantic<'_>, value: NodeValue, _: &[Value<'static>]) -> Known<Value<'static>> {
    headers_of(semantic, value, crate::grammar::names::calls::InvokeKind::Construct)
}

/// The headers a call or `new` node selects: `{selected: [site], uncertain}`
/// (several sites when the choice stays open); null when unknown or not an
/// invocation.
fn selection(semantic: &Semantic<'_>, at: crate::grammar::names::question::Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    match semantic.calls().selection(at) {
        Known::Yes(Some(selection)) => {
            let mut sites = Vec::new();
            for choice in &selection.choices {
                let site = site_value(choice.header.site);
                if !sites.contains(&site) { sites.push(site) }
            }
            Known::Yes(Value::record(vec![
                ("selected".into(), Value::list(sites)),
                ("uncertain".into(), Value::Bool(selection.uncertain)),
            ]))
        }
        Known::Yes(None) => Known::Yes(Value::Null),
        Known::Unknown => Known::Unknown,
    }
}

/// The call's selected signatures in their bound frames (generic
/// parameters instantiated for this call): `{uncertain, choices: [{site,
/// result, parameters: [{site, rest, value, arguments}]}]}`, `result` and `value`
/// as handles, `arguments` the `{file, node}` sites each parameter receives
/// (several for a rest parameter). Null when not an invocation. With a
/// `path` of field names (`call_binding_at`), each choice also has `at`:
/// the value at that path from the signature's node, read in the bound
/// frame (`["return_type", "type"]`: a type guard's instantiated target).
fn binding(semantic: &Semantic<'_>, at: crate::grammar::names::question::Leaf, path: Option<&[u16]>) -> Known<Value<'static>> {
    let selection = match semantic.calls().selection(at) {
        Known::Yes(Some(selection)) => selection,
        Known::Yes(None) => return Known::Yes(Value::Null),
        Known::Unknown => return Known::Unknown,
    };
    let mut choices = Vec::new();
    for choice in &selection.choices {
        let result = match semantic.calls().header_result(choice.header) {
            Known::Yes(result) => handle(semantic, result),
            Known::Unknown => Value::Null,
        };
        let mut parameters = Vec::new();
        for (parameter, actuals) in &choice.mapping {
            if !semantic.question.budget.step() { return Known::Unknown }
            let rest = match semantic.calls().is_rest(*parameter) {
                Known::Yes(rest) => Value::Bool(rest),
                Known::Unknown => Value::Null,
            };
            parameters.push(Value::record(vec![
                ("site".into(), site_value(*parameter)),
                ("rest".into(), rest),
                ("value".into(), handle(semantic, semantic.values().values(*parameter))),
                ("arguments".into(), Value::list(actuals.iter().map(|&actual| site_value(actual)).collect())),
            ]));
        }
        let mut fields = vec![
            ("site".into(), site_value(choice.header.site)),
            ("result".into(), result),
            ("parameters".into(), Value::list(parameters)),
        ];
        if let Some(path) = path {
            let reached = match semantic.values().follow(choice.header.site, path) {
                Known::Yes(leaves) => {
                    let values = semantic.values();
                    let reached: Vec<_> = leaves.into_iter().map(|leaf| values.values(leaf)).collect();
                    handle(semantic, values.combine(reached, false))
                }
                Known::Unknown => Value::Null,
            };
            fields.push(("at".into(), reached));
        }
        choices.push(Value::record(fields));
    }
    Known::Yes(Value::record(vec![
        ("uncertain".into(), Value::Bool(selection.uncertain)),
        ("choices".into(), Value::list(choices)),
    ]))
}

/// A `{file, node}` site's source, in any file of the program: `{text,
/// leading}` where `leading` is the text between the previous sibling (or
/// the parent's start) and the node (its comments). Null when unavailable.
fn site_text(semantic: &Semantic<'_>, _: crate::grammar::names::question::Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some((file, node)) = args.first().and_then(site_of) else { return Known::Yes(Value::Null) };
    let (Some(tree), Some(text)) = (semantic.world.tree(file), semantic.world.text(file)) else { return Known::Unknown };
    let Some(data) = tree.nodes.get(node as usize) else { return Known::Yes(Value::Null) };
    let before = tree.parent(node).map_or(0, |parent| {
        let siblings = &tree.node(parent).children;
        siblings.iter().map(|edge| tree.node(edge.node)).filter(|sibling| sibling.end <= data.start)
            .map(|sibling| sibling.end).max().unwrap_or(tree.node(parent).start)
    });
    let slice = |from: u32, to: u32| text.get(from as usize..to as usize).map(str::to_owned);
    let (Some(own), Some(leading)) = (slice(data.start, data.end), slice(before.min(data.start), data.start)) else { return Known::Yes(Value::Null) };
    Known::Yes(Value::record(vec![("text".into(), Value::from(own)), ("leading".into(), Value::from(leading))]))
}

/// The value a `{file, node}` declaration site unwraps to, as a handle
/// (read at the root frame, in any program file); null when not a site.
fn declaration_value(semantic: &Semantic<'_>, _: crate::grammar::names::question::Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    use crate::grammar::names::question::{FrameId, Leaf, NodeRef, Target};
    let Some((file, node)) = args.first().and_then(site_of) else { return Known::Yes(Value::Null) };
    match semantic.values().values(Leaf { target: Target::Node(NodeRef { file, node }), frame: FrameId::ROOT }) {
        NodeValue::Unknown => Known::Unknown,
        value => Known::Yes(handle(semantic, value)),
    }
}

/// One step of where a value comes from, read off the language's value
/// recipe: `{combine, sites}`. A reference's site is the declaration it
/// resolves to; union and intersection recipes list their parts
/// (`combine` is `union` / `intersection`, else `one`). Sites are `{file,
/// node, kind, name}`, `name` being the declared name when the site
/// declares one. `sites` is empty when the node holds its own value; the
/// whole answer is null when the recipe can't be followed (computed
/// values, products, calls).
fn sources(semantic: &Semantic<'_>, at: crate::grammar::names::question::Leaf) -> Known<Value<'static>> {
    use crate::grammar::names::question::Target;
    use crate::grammar::names::values::compile::Value as Recipe;
    let values = semantic.values();
    let mut recipe = match values.unwrap_recipe(at) {
        Known::Yes(Some(recipe)) => recipe,
        Known::Yes(None) => return Known::Yes(sources_record("one", Vec::new())),
        Known::Unknown => return Known::Unknown,
    };
    while let Recipe::Excludes { from: Some(inner), .. } = recipe { recipe = inner }
    let own = || Known::Yes(Vec::new());
    let (combine, leaves) = match recipe {
        Recipe::Current | Recipe::Excludes { from: None, .. } | Recipe::Traits(_) | Recipe::Text(_) => ("one", own()),
        Recipe::Union(parts) | Recipe::Intersection(parts) => {
            let mut all = Vec::new();
            for part in parts {
                match values.sites(at, part) {
                    Known::Yes(found) => all.extend(found),
                    Known::Unknown => return Known::Yes(Value::Null),
                }
            }
            (if matches!(recipe, Recipe::Union(_)) { "union" } else { "intersection" }, Known::Yes(all))
        }
        Recipe::Spread { intersection, value: None, .. } => (if *intersection { "intersection" } else { "union" }, values.sites(at, recipe)),
        Recipe::Path(_) | Recipe::Either(..) | Recipe::From(..) | Recipe::Scope { .. } => ("one", values.sites(at, recipe)),
        _ => return Known::Yes(Value::Null),
    };
    let Known::Yes(leaves) = leaves else { return Known::Yes(Value::Null) };
    let Known::Yes(at) = semantic.leaf(at) else { return Known::Unknown };
    let mut sites = Vec::new();
    for leaf in leaves {
        if !semantic.question.budget.step() { return Known::Unknown }
        if leaf.target == at.target { continue }
        let Target::Node(node) = leaf.target else { return Known::Yes(Value::Null) };
        let Some(site) = site_record(semantic, node) else { return Known::Unknown };
        if !sites.contains(&site) { sites.push(site) }
    }
    Known::Yes(sources_record(combine, sites))
}

fn sources_record(combine: &str, sites: Vec<Value<'static>>) -> Value<'static> {
    Value::record(vec![("combine".into(), Value::from(combine.to_string())), ("sites".into(), Value::list(sites))])
}

/// `{file, node, kind, name}` for a node of any program file.
fn site_record(semantic: &Semantic<'_>, node: crate::grammar::names::question::NodeRef) -> Option<Value<'static>> {
    let tree = semantic.world.tree(node.file)?;
    let matcher = semantic.world.grammar().matchers.get(tree.nodes.get(node.node as usize)?.matcher as usize)?;
    Some(Value::record(vec![
        ("file".into(), Value::Num(f64::from(node.file))),
        ("node".into(), Value::Num(f64::from(node.node))),
        ("kind".into(), Value::from(matcher.name.clone())),
        ("name".into(), declared_name(semantic, node)),
    ]))
}

/// The name a node declares: itself when it is a declared name, else the
/// first name whose declaring node it is. Null for none or computed names.
fn declared_name(semantic: &Semantic<'_>, node: crate::grammar::names::question::NodeRef) -> Value<'static> {
    let Some(names) = semantic.world.names(node.file) else { return Value::Null };
    let name = if names.declared.contains_key(&node.node) {
        Some(node.node)
    } else {
        names.declared.iter().filter(|(_, &(_, owner, ..))| owner == node.node).map(|(&name, _)| name).min()
    };
    let Some(name) = name else { return Value::Null };
    match names.keys.get(&name) {
        Some(Some(key)) => Value::from(key.to_string()),
        Some(None) => Value::Null,
        None => {
            let text = semantic.world.tree(node.file).and_then(|tree| tree.nodes.get(name as usize))
                .zip(semantic.world.text(node.file))
                .and_then(|(data, text)| text.get(data.start as usize..data.end as usize));
            text.map_or(Value::Null, |text| Value::from(semantic.world.name_key(node.file, text).into_owned()))
        }
    }
}

/// A `{file, node}` site record's numbers.
fn site_of(site: &Value<'static>) -> Option<(u32, u32)> {
    let Value::Record(fields) = site else { return None };
    let number = |name: &str| fields.iter().find(|(key, _)| &**key == name).and_then(|(_, value)| match value {
        Value::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX) => Some(*n as u32),
        _ => None,
    });
    Some((number("file")?, number("node")?))
}

/// Where a `{file, node}` site's file comes from: `{path, kind, package}`
/// with kind `library`, `package` (package named) or `project`.
fn declaration_origin(semantic: &Semantic<'_>, _: crate::grammar::names::question::Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    use crate::grammar::names::modules::resolution::Origin;
    let Some((file, _)) = args.first().and_then(site_of) else { return Known::Yes(Value::Null) };
    let (Some(path), Known::Yes(origin)) = (semantic.world.path(file), semantic.world.origin(file)) else { return Known::Unknown };
    let (kind, package) = match origin {
        Origin::Library => ("library", Value::Null),
        Origin::Package(name) => ("package", Value::from(name.to_string())),
        Origin::Project => ("project", Value::Null),
    };
    Known::Yes(Value::record(vec![
        ("path".into(), Value::from(path.to_string_lossy().replace('\\', "/"))),
        ("kind".into(), Value::from(kind.to_string())),
        ("package".into(), package),
    ]))
}

fn site_value(site:crate::grammar::names::question::Leaf) -> Value<'static> {
    match site.target {
        crate::grammar::names::question::Target::Node(node) => Value::record(vec![
            ("file".into(), Value::Num(f64::from(node.file))),
            ("node".into(), Value::Num(f64::from(node.node))),
        ]),
        _ => Value::Null,
    }
}

fn headers_of(semantic: &Semantic<'_>, value: NodeValue, kind: crate::grammar::names::calls::InvokeKind) -> Known<Value<'static>> {
    let values = semantic.values();
    let calls = semantic.calls();
    let Known::Yes(arms) = calls.headers(&value, kind) else { return Known::Unknown };
    let mut alternatives = Vec::with_capacity(arms.len());
    for arm in arms {
        let mut headers = Vec::with_capacity(arm.len());
        for header in arm {
            if !semantic.question.budget.step() { return Known::Unknown }
            let parameters = match calls.parameters(header) {
                Known::Yes(parameters) => Value::list(parameters.into_iter().map(|parameter| handle(semantic, values.values(parameter))).collect()),
                Known::Unknown => Value::Null,
            };
            let (receiver, has_receiver) = match calls.receiver(header) {
                Known::Yes(Some(receiver)) => (handle(semantic, values.values(receiver)), Value::Bool(true)),
                Known::Yes(None) => (Value::Null, Value::Bool(false)),
                Known::Unknown => (Value::Null, Value::Null),
            };
            let result = calls.header_result(header).known().map_or(Value::Null, |result| handle(semantic, result));
            if semantic.question.budget.exhausted() { return Known::Unknown }
            headers.push(Value::record(vec![
                ("site".into(), site_value(header.site)),
                ("parameters".into(), parameters),
                ("receiver".into(), receiver),
                ("has_receiver".into(), has_receiver),
                ("result".into(), result),
            ]));
        }
        alternatives.push(Value::list(headers));
    }
    Known::Yes(Value::list(alternatives))
}

/// Whether the leaf declares no members of its own: nodeless trait values
/// and node kinds with no member scope, index signature, base or member link
/// (a function type). Apparent members (`Function`'s) are not its own, so
/// this proves nothing absent for lookups.
fn own_member_free(semantic: &Semantic<'_>, at: crate::grammar::names::question::Leaf) -> Known<bool> {
    use crate::grammar::names::question::Target;
    let Known::Yes(at) = semantic.leaf(at) else { return Known::Unknown };
    match at.target {
        // Nodeless: no declarations, so no own members, open or not (the
        // intersection marker is open). Rules read openness from the
        // part's `uncertain_traits`.
        Target::Traits(_) => Known::Yes(true),
        Target::Product(_) | Target::Scope(_) => Known::Yes(false),
        Target::Node(node) => {
            let Some(data) = semantic.world.tree(node.file).and_then(|tree| tree.nodes.get(node.node as usize)) else { return Known::Unknown };
            let Some(kind) = semantic.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
            Known::Yes(kind.member_defaults == 0 && kind.indexed.is_empty() && kind.member_bases.is_empty() && kind.member_links.is_empty())
        }
    }
}

const MAX_FRAME_DEPTH: usize = 24;

/// How many frames `frame` reads through (parents and references), counted
/// up to just past [`MAX_FRAME_DEPTH`].
fn frame_depth(semantic: &Semantic<'_>, frame: crate::grammar::names::question::FrameId) -> usize {
    let mut pending = vec![(frame, 0)];
    let mut reached: crate::fx_hash::FxHashMap<crate::grammar::names::question::FrameId, usize> = Default::default();
    let mut deepest = 0;
    while let Some((id, depth)) = pending.pop() {
        deepest = deepest.max(depth);
        if deepest > MAX_FRAME_DEPTH { break }
        if reached.get(&id).is_some_and(|&seen| seen >= depth) { continue }
        reached.insert(id, depth);
        let Some(frame) = semantic.question.get(id) else { continue };
        pending.push((frame.parent, depth + 1));
        if let Some(through) = frame.through { pending.push((through.frame, depth + 1)) }
    }
    deepest
}

pub(super) fn key_value(key: &Key) -> Value<'static> {
    match key {
        Key::Text(text) => Value::from(text.to_string()),
        Key::Ordinal(index) => Value::Num(f64::from(*index)),
        Key::Symbol(node) => Value::record(vec![("symbol".into(), Value::record(vec![
            ("file".into(), Value::Num(f64::from(node.file))),
            ("node".into(), Value::Num(f64::from(node.node))),
        ]))]),
        Key::Domain(domain) => Value::record(vec![("domain".into(), Value::from(match domain {
            crate::config::ast::values::KeyDomain::Text => "text",
            crate::config::ast::values::KeyDomain::Ordinal => "ordinal",
            crate::config::ast::values::KeyDomain::Symbol => "symbol",
        }))]),
    }
}

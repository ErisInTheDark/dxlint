//! Value handles: rules walk a value graph themselves (their own recursion,
//! pruning and seen sets) through opaque `{ value_handle }` records. A
//! handle names one interned value of the linted file's question context;
//! foreign or stale handles read as unknown.

use crate::expr::Value;
use crate::grammar::names::question::{Handle, Known, NodeValue};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::values::Key;

use super::values::{lookup_in, member_key, project, project_path, project_with, source_value};
use super::QueryFunction;

const FIELD: &str = "value_handle";

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "value_parts", arity: 1, call: |semantic, at, args| parts(semantic, semantic.values().values(at), args) },
    QueryFunction { name: "value_children", arity: 3, call: |semantic, at, args| children(semantic, semantic.values().values(at), args) },
    QueryFunction { name: "value_signatures", arity: 1, call: |semantic, at, args| signatures(semantic, semantic.values().values(at), args) },
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
    HandleFunction { name: "value_constituents", call: |semantic, value, _| project(semantic, value) },
    HandleFunction { name: "value_at", call: project_path },
    HandleFunction { name: "value_lookup", call: |semantic, value, args| match args.first().and_then(member_key) {
        Some(key) => lookup_in(semantic, value, &key),
        None => Known::Unknown,
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

fn handle(semantic: &Semantic<'_>, value: NodeValue) -> Value<'static> {
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
    let values = semantic.values();
    let mut edges = Vec::new();
    if *members {
        // Constituents without members of their own (trait-only parts such
        // as the intersection marker, function types) add none and leave
        // the arm's listing complete.
        let mut owning = Vec::with_capacity(row.len());
        for &leaf in row {
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
                let flags = match values.flags(source.site) {
                    Known::Yes(flags) => Value::list(flags.into_iter().map(|flag| Value::from(String::from(flag))).collect()),
                    Known::Unknown => return Known::Unknown,
                };
                let Value::Record(fields) = source_value(source) else { return Known::Unknown };
                let mut fields = fields.to_vec();
                fields.push(("flags".into(), flags));
                sources.push(Value::record(fields));
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
    let values = semantic.values();
    let calls = semantic.calls();
    let Known::Yes(arms) = calls.headers(&value, crate::grammar::names::calls::InvokeKind::Call) else { return Known::Unknown };
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
        // An open trait value (some traits uncertain: `unknown`-like) may
        // have anything.
        Target::Traits(_) => {
            let values = semantic.values();
            for id in 0..semantic.tables.vocabulary.traits.len() {
                let Ok(id) = u16::try_from(id) else { return Known::Unknown };
                if values.fits(at, id) == Known::Unknown { return Known::Unknown }
            }
            Known::Yes(true)
        }
        Target::Product(_) | Target::Scope(_) => Known::Yes(false),
        Target::Node(node) => {
            let Some(data) = semantic.world.tree(node.file).and_then(|tree| tree.nodes.get(node.node as usize)) else { return Known::Unknown };
            let Some(kind) = semantic.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
            Known::Yes(kind.member_defaults == 0 && kind.indexed.is_empty() && kind.member_bases.is_empty() && kind.member_links.is_empty())
        }
    }
}

fn key_value(key: &Key) -> Value<'static> {
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

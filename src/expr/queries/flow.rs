use crate::expr::Value;
use crate::grammar::names::question::{Budget, FrameId, Known, Leaf, NodeRef, Target};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::effects::{DeclaredEffect, EffectId, EventKind, Execution, Phase, StepId};

use super::QueryFunction;

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "flow_normal", arity: 1, call: normal },
    QueryFunction { name: "flow_declared_effects", arity: 2, call: declared },
    QueryFunction { name: "reachable", arity: 1, call: reachable },
    QueryFunction { name: "flow_entry", arity: 1, call: entry },
    QueryFunction { name: "flow_prior", arity: 2, call: prior },
    QueryFunction { name: "flow_required", arity: 2, call: required },
    QueryFunction { name: "flow_complete_without", arity: 3, call: complete_without },
    QueryFunction { name: "flow_complete_with", arity: 3, call: complete_with },
    QueryFunction { name: "flow_value_used", arity: 1, call: value_used },
    QueryFunction { name: "flow_has_reads", arity: 1, call: has_reads },
    QueryFunction { name: "flow_exits", arity: 2, call: exits },
    QueryFunction { name: "flow_prior_event", arity: 2, call: prior_event },
    QueryFunction { name: "flow_crosses", arity: 5, call: crosses },
];

fn effect_id(semantic: &Semantic<'_>, value: &Value<'static>) -> Option<EffectId> {
    let Value::Str(name) = value else { return None };
    semantic.tables.vocabulary.effect(name)
}

fn effect_steps(graph: &Execution, effect: EffectId, budget: &Budget) -> Known<Vec<StepId>> {
    let mut steps = Vec::new();
    for (step, point) in graph.steps() {
        if !budget.step() { return Known::Unknown }
        if point.event().is_some_and(|event| event.kind == EventKind::Effect(effect)) { steps.push(step) }
    }
    Known::Yes(steps)
}

fn entry(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(node) = local_node(semantic, at) else { return Known::Unknown };
    let Some(entry) = semantic.effects().entry_for(at).known() else { return Known::Unknown };
    node_value(semantic, entry, node.file).map_or(Known::Unknown, Known::Yes)
}

fn prior(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    before(semantic, at, args, false)
}

fn prior_event(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(node) = local_node(semantic, at) else { return Known::Unknown };
    let Some(effect) = args.first().and_then(|value| effect_id(semantic, value)) else { return Known::Unknown };
    let effects = semantic.effects();
    let Some(entry) = effects.entry_for(at).known() else { return Known::Unknown };
    let Some(graph) = effects.execution(entry).known() else { return Known::Unknown };
    graph.prior_event(node, effect, &semantic.question.budget).known()
        .map_or(Known::Unknown, |found| Known::Yes(Value::Bool(found)))
}

fn required(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    before(semantic, at, args, true)
}

fn before(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>], required: bool) -> Known<Value<'static>> {
    let Some(node) = local_node(semantic, at) else { return Known::Unknown };
    let Some(effect) = args.first().and_then(|value| effect_id(semantic, value)) else { return Known::Unknown };
    let effects = semantic.effects();
    let Some(entry) = effects.entry_for(at).known() else { return Known::Unknown };
    let Some(graph) = effects.execution(entry).known() else { return Known::Unknown };
    let budget = &semantic.question.budget;
    let Some(reachable) = graph.reachable(node, budget).known() else { return Known::Unknown };
    if !reachable { return Known::Yes(Value::Bool(false)) }
    let Some(barriers) = effect_steps(&graph, effect, budget).known() else { return Known::Unknown };
    for target in graph.sites(node, Phase::Enter) {
        let found = if required {
            graph.reaches(graph.entry(), target, &barriers, budget)
        } else {
            graph.reaches_through(graph.entry(), target, &[], |event| event.kind == EventKind::Effect(effect), budget)
        };
        let Some(found) = found.known() else { return Known::Unknown };
        if found { return Known::Yes(Value::Bool(!required)) }
    }
    Known::Yes(Value::Bool(required))
}

fn complete_without(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    complete(semantic, at, args, false)
}

fn complete_with(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    complete(semantic, at, args, true)
}

fn complete(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>], with: bool) -> Known<Value<'static>> {
    if local_node(semantic, at).is_none() { return Known::Unknown }
    let Some(effect) = args.first().and_then(|value| effect_id(semantic, value)) else { return Known::Unknown };
    let Some(Value::List(names)) = args.get(1) else { return Known::Unknown };
    let Some(exits) = names.iter().map(|name| effect_id(semantic, name)).collect::<Option<Vec<_>>>() else { return Known::Unknown };
    let Some(graph) = semantic.effects().execution(at).known() else { return Known::Unknown };
    let found = if with {
        graph.completes_through(effect, &exits, &semantic.question.budget)
    } else {
        let Some(barriers) = effect_steps(&graph, effect, &semantic.question.budget).known() else { return Known::Unknown };
        graph.completes_avoiding(&barriers, &exits, &semantic.question.budget)
    };
    found.known().map_or(Known::Unknown, |found| Known::Yes(Value::Bool(found)))
}

fn value_used(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    if local_node(semantic, at).is_none() { return Known::Unknown }
    semantic.effects().value_used(at).known().map_or(Known::Unknown, |used| Known::Yes(Value::Bool(used)))
}

fn has_reads(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    if local_node(semantic, at).is_none() { return Known::Unknown }
    semantic.effects().has_reads(at).known().map_or(Known::Unknown, |reads| Known::Yes(Value::Bool(reads)))
}

fn crosses(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    if local_node(semantic, at).is_none() { return Known::Unknown }
    let Some(graph) = semantic.effects().execution(at).known() else { return Known::Unknown };
    let Some(Value::List(names)) = args.get(2) else { return Known::Unknown };
    let Some(effects) = names.iter().map(|name| effect_id(semantic, name)).collect::<Option<Vec<_>>>() else { return Known::Unknown };
    let Some(starts) = args.first().and_then(|nodes| graph_sites(semantic, at, &graph, nodes, Phase::Enter).known()) else { return Known::Unknown };
    let Some(observations) = args.get(1).and_then(|nodes| graph_sites(semantic, at, &graph, nodes, Phase::Leave).known()) else { return Known::Unknown };
    let Some(barriers) = args.get(3).and_then(|nodes| graph_sites(semantic, at, &graph, nodes, Phase::Enter).known()) else { return Known::Unknown };
    graph.crosses(&starts, &observations, &effects, &barriers, &semantic.question.budget).known()
        .map_or(Known::Unknown, |crossed| Known::Yes(Value::Bool(crossed)))
}

fn graph_sites(semantic: &Semantic<'_>, at: Leaf, graph: &Execution, value: &Value<'static>, phase: Phase) -> Known<Vec<StepId>> {
    let Some(owner) = local_node(semantic, at) else { return Known::Unknown };
    let Value::List(nodes) = value else { return Known::Unknown };
    let mut steps = Vec::new();
    for node in nodes.iter() {
        if !semantic.question.budget.step() { return Known::Unknown }
        let Value::Node(node, _) = node else { return Known::Unknown };
        let node = NodeRef { node: *node, ..owner };
        let Some(node) = local_node(semantic, Leaf { target: Target::Node(node), ..at }) else { return Known::Unknown };
        let before = steps.len();
        for step in graph.sites(node, phase) {
            if !semantic.question.budget.step() { return Known::Unknown }
            steps.push(step);
        }
        if before == steps.len() { return Known::Unknown }
    }
    Known::Yes(steps)
}

fn local_node(semantic: &Semantic<'_>, at: Leaf) -> Option<NodeRef> {
    let at = semantic.leaf(at).known()?;
    if at.frame != FrameId::ROOT { return None; }
    let Target::Node(node) = at.target else { return None };
    semantic.world.tree(node.file)?.nodes.get(node.node as usize)?;
    Some(node)
}

fn node_value(semantic: &Semantic<'_>, at: Leaf, file: u32) -> Option<Value<'static>> {
    let node = local_node(semantic, at).filter(|node| node.file == file)?;
    let syntax = semantic.world.tree(file)?.nodes.get(node.node as usize)?;
    let text = semantic.world.text(file)?.get(syntax.start as usize..syntax.end as usize)?;
    Some(Value::Node(node.node, text.to_owned().into()))
}

fn normal(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    if local_node(semantic, at).is_none() { return Known::Unknown; }
    semantic.effects().normal(at).known().map_or(Known::Unknown, |normal| Known::Yes(Value::Bool(normal)))
}

fn declared(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(node) = local_node(semantic, at) else { return Known::Unknown };
    let Some(Value::Str(name)) = args.first() else { return Known::Unknown };
    let Some(effect) = semantic.tables.vocabulary.effect(name) else { return Known::Unknown };
    let Some(effects) = semantic.effects().declared(at, effect).known() else { return Known::Unknown };
    records(semantic, at, node.file, effects)
}

fn exits(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(node) = local_node(semantic, at) else { return Known::Unknown };
    let Some(effect) = args.first().and_then(|value| effect_id(semantic, value)) else { return Known::Unknown };
    let Some(exits) = semantic.effects().exits(at).known() else { return Known::Unknown };
    records(semantic, at, node.file, exits.into_iter().filter(|exit| exit.effect == effect).map(|exit| DeclaredEffect {
        site: exit.site, target: exit.target, value: exit.value, possible: false,
    }))
}

fn records(semantic: &Semantic<'_>, at: Leaf, file: u32, effects: impl IntoIterator<Item = DeclaredEffect>) -> Known<Value<'static>> {
    let records = effects.into_iter().map(|effect| {
        let site = node_value(semantic, effect.site, file)?;
        let target = match effect.target {
            Some(target) => node_value(semantic, Leaf { target: Target::Node(target), ..at }, file)?,
            None => Value::Null,
        };
        let value = match effect.value {
            Some(value) => node_value(semantic, value, file)?,
            None => Value::Null,
        };
        Some(Value::record(vec![
            ("site".into(), site), ("target".into(), target), ("value".into(), value),
        ]))
    }).collect::<Option<Vec<_>>>();
    records.map_or(Known::Unknown, |records| Known::Yes(Value::list(records)))
}

fn reachable(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(node) = local_node(semantic, at) else { return Known::Unknown };
    let effects = semantic.effects();
    let Some(entry) = effects.entry_for(at).known() else { return Known::Unknown };
    if local_node(semantic, entry).is_none_or(|entry| entry.file != node.file) { return Known::Unknown; }
    let Some(graph) = effects.execution(entry).known() else { return Known::Unknown };
    graph.reachable(node, &semantic.question.budget).known().map_or(Known::Unknown, |reachable| Known::Yes(Value::Bool(reachable)))
}

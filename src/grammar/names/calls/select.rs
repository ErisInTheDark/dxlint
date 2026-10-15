use super::{kind, node, rest, Chosen, Header, InvokeKind, Selection};
use crate::grammar::names::question::{Known, Leaf, NodeRef, NodeValue, Target};
use crate::grammar::names::values::{Key, Values};

#[derive(Clone, Debug)]
pub(super) struct Candidate {
    chosen: Chosen,
    compatible: Known<bool>,
}

pub(super) type Answer = Known<Option<Vec<Vec<Known<Candidate>>>>>;

pub(super) fn candidates(call: Leaf, values: &Values<'_>) -> Answer {
    let Some(kind) = kind(call, values) else { return Known::Unknown };
    for invoke in &kind.calls.invokes {
        match invoke.when.map_or(Known::Yes(true), |when| values.world.condition(call, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => {}
        }
        let Known::Yes(arguments) = values.follow(call, &invoke.item.arguments) else { return Known::Unknown };
        for &argument in &arguments {
            if rest(argument, values) != Known::Yes(false) {
                // Unknown-length spreads cannot become invented finite slots.
                return Known::Unknown;
            }
        }
        let callee = values.evaluate(call, &invoke.item.callee);
        let receiver = invoke.item.receiver.as_ref().map(|receiver| values.evaluate(call, receiver));
        let invoke_kind = if invoke.item.construct { InvokeKind::Construct } else { InvokeKind::Call };
        let Known::Yes(sets) = values.calls().headers(&callee, invoke_kind) else { return Known::Unknown };
        let mut groups = Vec::new();
        for set in sets {
            let mut group = Vec::new();
            for header in set {
                if !values.question.budget.step() { return Known::Unknown }
                let candidate = candidate(header, call, &arguments, receiver.as_ref(), values);
                let proven = matches!(&candidate, Known::Yes(candidate) if candidate.compatible == Known::Yes(true));
                group.push(candidate);
                if proven { break }
            }
            groups.push(group);
        }
        return Known::Yes(Some(groups));
    }
    Known::Yes(None)
}

pub(super) fn selection(call: Leaf, values: &Values<'_>) -> Known<Option<Selection>> {
    let groups = match values.calls().candidates(call) {
        Known::Yes(Some(groups)) => groups,
        Known::Yes(None) => return Known::Yes(None),
        Known::Unknown => return Known::Unknown,
    };
    let mut selected = Selection { choices: Vec::new(), uncertain: false };
    for group in groups {
        if let Some(candidate) = proven(&group) {
            selected.choices.push(candidate.chosen.clone());
            continue;
        }
        let mut open = group.into_iter().filter(|candidate| !matches!(candidate,
            Known::Yes(candidate) if candidate.compatible == Known::Yes(false)));
        let first = open.next();
        // The only candidate not ruled out is the signature the call
        // resolves to, even when an argument (an untyped callback) can't be
        // checked against it yet: TypeScript types such arguments from it.
        let alone = open.next().is_none();
        match first {
            Some(Known::Yes(candidate)) => {
                selected.choices.push(candidate.chosen);
                selected.uncertain |= !alone;
            }
            _ => return Known::Unknown,
        }
    }
    Known::Yes(Some(selected))
}

pub(super) fn outputs(call: Leaf, values: &Values<'_>) -> Known<Option<NodeValue>> {
    let groups = match values.calls().candidates(call) {
        Known::Yes(Some(groups)) => groups,
        Known::Yes(None) => return Known::Yes(None),
        Known::Unknown => return Known::Unknown,
    };
    let mut results = Vec::new();
    for group in groups {
        let result = match proven(&group) {
            Some(candidate) => super::result::header_result(candidate.chosen.header, values),
            None => agreement(&group, values),
        };
        let Known::Yes(NodeValue::Alternatives(arms)) = result else { return Known::Unknown };
        for arm in arms {
            if !values.question.budget.step() { return Known::Unknown }
            results.push(arm);
        }
    }
    Known::Yes(Some(NodeValue::Alternatives(results)))
}

fn proven(group: &[Known<Candidate>]) -> Option<&Candidate> {
    group.iter().find_map(|candidate| match candidate {
        Known::Yes(candidate) if candidate.compatible == Known::Yes(true) => Some(candidate),
        _ => None,
    })
}

fn agreement(group: &[Known<Candidate>], values: &Values<'_>) -> Known<NodeValue> {
    let mut agreed = None;
    for candidate in group {
        let Known::Yes(candidate) = candidate else { return Known::Unknown };
        if candidate.compatible == Known::Yes(false) { continue }
        let Known::Yes(result) = super::result::header_result(candidate.chosen.header, values) else { return Known::Unknown };
        if let Some(previous) = &agreed {
            if previous != &result && (values.calls().accepts(previous, &result) != Known::Yes(true)
                || values.calls().accepts(&result, previous) != Known::Yes(true)) {
                return Known::Unknown;
            }
        } else {
            agreed = Some(result);
        }
    }
    agreed.map_or(Known::Unknown, Known::Yes)
}

fn candidate(header: Header, call: Leaf, arguments: &[Leaf], receiver: Option<&NodeValue>, values: &Values<'_>) -> Known<Candidate> {
    let declaration = header;
    let Some(owner) = node(header.site, values) else { return Known::Unknown };
    let Some(kind) = kind(header.site, values) else { return Known::Unknown };
    let Some(signature) = kind.calls.signatures.get(header.signature as usize) else { return Known::Unknown };
    let Known::Yes(formals) = super::infer::formals(header.site, values) else { return Known::Unknown };
    let explicit = match values.declared_arguments(call) {
        Known::Yes(arguments) => arguments.unwrap_or_default(),
        Known::Unknown => return Known::Unknown,
    };
    if !explicit.is_empty() {
        let mut required = 0;
        for &formal in &formals {
            match values.optional(formal) {
                Known::Yes(false) => required += 1,
                Known::Yes(true) => {}
                Known::Unknown => return Known::Unknown,
            }
        }
        if explicit.len() < required || explicit.len() > formals.len() {
            return Known::Yes(Candidate {
                chosen: Chosen { header, mapping: Vec::new() },
                compatible: Known::Yes(false),
            });
        }
    }
    let Known::Yes(classes) = values.question.owner_classes(header.site.frame, owner) else { return Known::Unknown };
    let frame = values.question.bind_slots(header.site.frame, owner, Some(call), explicit.iter().cloned().map(Some).collect(), classes);
    let header = Header { site: Leaf { frame, ..header.site }, ..header };
    let Known::Yes(parameters) = values.follow(header.site, &signature.item.parameters) else { return Known::Unknown };
    let mut mapping = Vec::new();
    let mut compatible = Known::Yes(true);
    // A declared receiver must accept the one the call passes.
    if let Some(receiver) = receiver {
        let answer = match values.calls().receiver(header) {
            Known::Yes(Some(declared)) => values.calls().accepts(&values.values(declared), receiver),
            Known::Yes(None) => Known::Yes(true),
            Known::Unknown => Known::Unknown,
        };
        compatible = answer;
    }
    let mut has_rest = false;
    let needs_inference = !formals.is_empty() && !arguments.is_empty();
    for (index, parameter) in parameters.iter().copied().enumerate() {
        if !values.question.budget.step() { return Known::Unknown }
        let is_rest = match rest(parameter, values) {
            Known::Yes(rest) => rest,
            Known::Unknown => return Known::Unknown,
        };
        if is_rest && index + 1 != parameters.len() { return Known::Unknown }
        has_rest |= is_rest;
        let actuals = if is_rest {
            arguments.get(index..).unwrap_or_default()
        } else {
            arguments.get(index..index + 1).unwrap_or_default()
        };
        let mut captured = Vec::new();
        for &actual in actuals {
            if !values.question.budget.step() { return Known::Unknown }
            captured.push(actual);
        }
        mapping.push((parameter, captured));
        if actuals.is_empty() && !is_rest {
            match values.optional(parameter) {
                Known::Yes(false) => compatible = Known::Yes(false),
                Known::Unknown if compatible != Known::Yes(false) => compatible = Known::Unknown,
                _ => {}
            }
        }
        // Inferred parameters are checked once instantiated.
        if needs_inference { continue }
        for (offset, &actual) in actuals.iter().enumerate() {
            let expected = if is_rest {
                values.member(&values.values(parameter), &Key::Ordinal(offset as u32))
            } else {
                values.values(parameter)
            };
            let answer = values.calls().accepts(&expected, &values.values(actual));
            match answer {
                Known::Yes(false) => compatible = Known::Yes(false),
                Known::Unknown if compatible != Known::Yes(false) => compatible = Known::Unknown,
                _ => {}
            }
        }
    }
    if !has_rest && arguments.len() > parameters.len() {
        compatible = Known::Yes(false);
    }
    let header = if formals.is_empty() {
        header
    } else {
        let header = match super::infer::instantiate(declaration, call, &mapping, &explicit, values) {
            Known::Yes(Some((header, fits))) => {
                if compatible == Known::Yes(true) { compatible = fits }
                header
            }
            Known::Yes(None) => return Known::Yes(Candidate { chosen: Chosen { header, mapping }, compatible: Known::Yes(false) }),
            Known::Unknown => return Known::Unknown,
        };
        for (parameter, _) in &mut mapping { parameter.frame = header.site.frame }
        header
    };
    Known::Yes(Candidate { chosen: Chosen { header, mapping }, compatible })
}

pub(super) fn argument_sites(binding: Leaf, values: &Values<'_>) -> Known<Option<Vec<Leaf>>> {
    let Target::Node(name) = binding.target else { return Known::Yes(None) };
    let Some(names) = values.world.names(name.file) else { return Known::Unknown };
    let Some(&(_, owner, ..)) = names.declared.get(&name.node) else { return Known::Yes(None) };
    let owner = NodeRef { file: name.file, node: owner };
    let mut frame = binding.frame;
    while let Some(environment) = values.question.get(frame) {
        if !values.question.budget.step() { return Known::Unknown }
        if environment.owner != owner {
            frame = environment.parent;
            continue;
        }
        let Some(call) = environment.through else { return Known::Yes(None) };
        let selected = match values.calls().selection(call) {
            Known::Yes(Some(selected)) if !selected.uncertain => selected,
            Known::Yes(None) => return Known::Yes(None),
            _ => return Known::Unknown,
        };
        let Some(chosen) = selected.choices.iter().find(|chosen| chosen.header.site.frame == frame
            && node(chosen.header.site, values) == Some(owner)) else { return Known::Unknown };
        let mut answer = None;
        for (parameter, actuals) in &chosen.mapping {
            let Known::Yes(roots) = direct_targets(*parameter, values) else { return Known::Unknown };
            for root in roots {
                if !values.question.budget.step() { return Known::Unknown }
                let Known::Yes(terminal) = terminal(root, values) else { return Known::Unknown };
                if !terminal { return Known::Unknown }
                if root.target == binding.target {
                    if answer.is_some() { return Known::Unknown }
                    let mut captured = Vec::new();
                    for &actual in actuals {
                        if !values.question.budget.step() { return Known::Unknown }
                        captured.push(actual);
                    }
                    answer = Some(captured);
                }
            }
        }
        return Known::Yes(answer);
    }
    Known::Yes(None)
}

fn direct_targets(at: Leaf, values: &Values<'_>) -> Known<Vec<Leaf>> {
    let Known::Yes(at) = values.semantic.leaf(at) else { return Known::Unknown };
    let Some(kind) = kind(at, values) else { return Known::Unknown };
    let mut roots = Vec::new();
    let mut forwards = false;
    for (alternatives, when) in &kind.targets {
        match when.map_or(Known::Yes(true), |when| values.world.condition(at, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return Known::Unknown,
            Known::Yes(true) => forwards = true,
        }
        for path in alternatives {
            let Known::Yes(found) = values.follow(at, path) else { return Known::Unknown };
            if !found.is_empty() {
                roots.extend(found);
                break;
            }
        }
    }
    if !forwards { roots.push(at) }
    Known::Yes(roots)
}

fn terminal(at: Leaf, values: &Values<'_>) -> Known<bool> {
    let Known::Yes(at) = values.semantic.leaf(at) else { return Known::Unknown };
    let Some(kind) = kind(at, values) else { return Known::Unknown };
    for (_, when) in &kind.targets {
        match when.map_or(Known::Yes(true), |when| values.world.condition(at, when, values.semantic)) {
            Known::Yes(false) => {}
            Known::Yes(true) => return Known::Yes(false),
            Known::Unknown => return Known::Unknown,
        }
    }
    Known::Yes(true)
}

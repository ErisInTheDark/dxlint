use super::{kind, node, rest, Chosen, Header, InvokeKind, Selection};
use crate::grammar::names::question::{Known, Leaf, NodeRef, NodeValue, Target};
use crate::grammar::names::values::{Key, Values};

#[derive(Clone, Debug)]
pub(super) struct Candidate {
    chosen: Chosen,
    compatible: Known<bool>,
    /// False when the signature's type arguments couldn't be bound (its
    /// result would still name unbound formals).
    instantiated: bool,
    /// The first pass left a formal to a callback this call types.
    pending: Option<Box<Pending>>,
}

/// What a candidate's second inference pass instantiates again.
#[derive(Clone, Debug)]
struct Pending {
    declaration: Header,
    /// Formal leaves in the explicit arguments' frame.
    mapping: Vec<(Leaf, Vec<Leaf>)>,
    explicit: Vec<NodeValue>,
    /// Compatibility decided before instantiation (arity, receiver).
    checked: Known<bool>,
}

pub(super) type Answer = Known<Option<Vec<Vec<Known<Candidate>>>>>;

/// Whether a candidates answer holds no unknown part (kept from a root
/// cut short after it was written).
pub(super) fn settled(answer: &Answer) -> bool {
    match answer {
        Known::Yes(None) => true,
        // A lone overload's compatibility is never asked (unknown by design).
        Known::Yes(Some(groups)) => groups.iter().all(|group| group.iter().all(|candidate| matches!(candidate,
            Known::Yes(candidate) if group.len() == 1 || candidate.compatible != Known::Unknown))),
        Known::Unknown => false,
    }
}

/// The candidates once callbacks are typed (TypeScript's second inference
/// pass): an argument the first pass couldn't relate, a callback whose
/// parameters this call types, is typed by the first pass's selection
/// ([`context`]) and related again, inferring the formals only it can
/// (`map(x => x + 1)`'s `U`). Results read these; argument contexts the
/// first pass, which they themselves type.
pub(super) fn refined(call: Leaf, values: &Values<'_>) -> Answer {
    let groups = match values.calls().candidates(call) {
        Known::Yes(Some(groups)) => groups,
        answer => return answer,
    };
    let mut refined = Vec::with_capacity(groups.len());
    for group in groups {
        let mut candidates = Vec::with_capacity(group.len());
        for candidate in group {
            candidates.push(match candidate {
                Known::Yes(candidate) => second_pass(candidate, call, values),
                Known::Unknown => Known::Unknown,
            });
        }
        refined.push(candidates);
    }
    Known::Yes(Some(refined))
}

fn second_pass(mut candidate: Candidate, call: Leaf, values: &Values<'_>) -> Known<Candidate> {
    let Some(pending) = candidate.pending.take() else { return Known::Yes(candidate) };
    let Pending { declaration, mut mapping, explicit, checked } = *pending;
    // The first pass's open parameters never reach results: what the
    // callbacks still leave unbound is unknown.
    match super::infer::instantiate(declaration, call, &mapping, &explicit, super::infer::Pass::Result, checked == Known::Yes(true), values) {
        Known::Yes(Some(instantiated)) => {
            for (parameter, _) in &mut mapping { parameter.frame = instantiated.header.site.frame }
            let compatible = if checked == Known::Yes(true) { instantiated.fits } else { checked };
            Known::Yes(Candidate { chosen: Chosen { header: instantiated.header, mapping }, compatible, instantiated: true, pending: None })
        }
        Known::Yes(None) => Known::Yes(Candidate { compatible: Known::Yes(false), ..candidate }),
        Known::Unknown => Known::Unknown,
    }
}

/// The first pass's selection: the signature that types the call's
/// arguments (their expected values).
pub(super) fn context(call: Leaf, values: &Values<'_>) -> Known<Option<Selection>> {
    select(values.calls().candidates(call))
}

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
        let Known::Yes(exact_first) = exact_first(kind, call, values) else { return Known::Unknown };
        let mut groups = Vec::new();
        for set in sets {
            // The first overload proven under exact matching wins; failing
            // that, under ordinary acceptance.
            if exact_first && set.len() > 1 {
                let exact = std::cell::Cell::new(false);
                let Known::Yes((group, proven)) = pass(&set, call, &arguments, receiver.as_ref(), Some(&exact), values) else { return Known::Unknown };
                // Exact matching that never met a dynamic argument decided
                // exactly what ordinary acceptance would.
                if proven || !exact.get() {
                    groups.push(group);
                    continue;
                }
            }
            let Known::Yes((group, _)) = pass(&set, call, &arguments, receiver.as_ref(), None, values) else { return Known::Unknown };
            groups.push(group);
        }
        return Known::Yes(Some(groups));
    }
    Known::Yes(None)
}

/// The overloads of one set tried in order up to the first proven one, and
/// whether one was. `exact` matching notes whether a dynamic argument
/// changed (or left unknown) any answer.
fn pass(set: &[Header], call: Leaf, arguments: &[Leaf], receiver: Option<&NodeValue>, exact: Option<&std::cell::Cell<bool>>, values: &Values<'_>) -> Known<(Vec<Known<Candidate>>, bool)> {
    let mut group = Vec::new();
    // A lone overload is the call's contract whether or not its arguments
    // fit (`selection`, `outputs`), so nothing reads its compatibility.
    let decide = set.len() > 1;
    for &header in set {
        if !values.question.budget.step() { return Known::Unknown }
        let candidate = candidate(header, call, arguments, receiver, exact, decide, values);
        let proven = matches!(&candidate, Known::Yes(candidate) if candidate.compatible == Known::Yes(true));
        group.push(candidate);
        if proven { return Known::Yes((group, true)) }
    }
    Known::Yes((group, false))
}

/// Whether the invocation's kind declares `overloads exact first`.
fn exact_first(kind: &crate::grammar::names::KindNames, call: Leaf, values: &Values<'_>) -> Known<bool> {
    let mut unknown = false;
    for &when in &kind.calls.exact_first {
        match when.map_or(Known::Yes(true), |when| values.world.condition(call, when, values.semantic)) {
            Known::Yes(true) => return Known::Yes(true),
            Known::Unknown => unknown = true,
            Known::Yes(false) => {}
        }
    }
    if unknown { Known::Unknown } else { Known::Yes(false) }
}

/// Under exact matching a dynamic (`any`) argument fits only a dynamic or
/// top parameter.
fn dynamic_mismatch(expected: &NodeValue, actual: &NodeValue, values: &Values<'_>) -> Known<bool> {
    let (NodeValue::Alternatives(expected), NodeValue::Alternatives(actual)) = (expected, actual) else { return Known::Unknown };
    let mut dynamic = false;
    for &leaf in actual.iter().flatten() {
        match values.dynamic(leaf) {
            Known::Yes(true) => dynamic = true,
            Known::Unknown => return Known::Unknown,
            Known::Yes(false) => {}
        }
    }
    if !dynamic { return Known::Yes(false) }
    for row in expected {
        let mut universal = !row.is_empty();
        for &leaf in row {
            match values.top(leaf) {
                Known::Yes(true) => {}
                Known::Yes(false) => universal = false,
                Known::Unknown => return Known::Unknown,
            }
        }
        if universal { return Known::Yes(false) }
    }
    Known::Yes(true)
}

pub(super) fn selection(call: Leaf, values: &Values<'_>) -> Known<Option<Selection>> {
    select(values.calls().refined(call))
}

fn select(candidates: Answer) -> Known<Option<Selection>> {
    let groups = match candidates {
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
        // A lone signature is the call's contract even when an argument is
        // incompatible with it: TypeScript still types the arguments (their
        // expected values) from it and reports the mismatch.
        if let [Known::Yes(sole)] = &group[..] {
            selected.choices.push(sole.chosen.clone());
            continue;
        }
        let mut open = group.into_iter().filter(|candidate| !matches!(candidate,
            Known::Yes(candidate) if candidate.compatible == Known::Yes(false)));
        let first = open.next();
        // The only candidate not ruled out is the signature the call
        // resolves to, even when an argument (an untyped callback) can't be
        // checked against it yet: TypeScript types such arguments from it.
        // The same signature reached twice (in another frame) is no rival.
        let same = |other: &Known<Candidate>| match (&first, other) {
            (Some(Known::Yes(first)), Known::Yes(other)) => first.chosen.header.site.target == other.chosen.header.site.target
                && first.chosen.header.signature == other.chosen.header.signature,
            _ => false,
        };
        let alone = open.all(|other| same(&other));
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
    if let Some(result) = agreed_outputs(call, values) { return Known::Yes(Some(result)) }
    let groups = match values.calls().refined(call) {
        Known::Yes(Some(groups)) => groups,
        Known::Yes(None) => return Known::Yes(None),
        Known::Unknown => return Known::Unknown,
    };
    // No candidate at all says nothing about the result: an empty union
    // would claim the call never returns.
    if groups.is_empty() { return Known::Unknown }
    let mut results = Vec::new();
    for group in groups {
        // A lone signature types the call even when an argument doesn't fit
        // it (as in `selection`): `new Make()` missing an argument is still a
        // `Make`.
        let result = match (proven(&group), &group[..]) {
            (Some(candidate), _) => super::result::header_result(candidate.chosen.header, values),
            (None, [Known::Yes(candidate)]) if candidate.instantiated => super::result::header_result(candidate.chosen.header, values),
            (None, _) => agreement(&group, values),
        };
        let Known::Yes(NodeValue::Alternatives(arms)) = result else { return Known::Unknown };
        for arm in arms {
            if !values.question.budget.step() { return Known::Unknown }
            results.push(arm);
        }
    }
    Known::Yes(Some(NodeValue::rows(results)))
}

/// The call's result when every overload it could select has the very same
/// one (no type parameters to infer): which overload the arguments pick
/// can't change it, so none is selected. `None`: selection decides.
fn agreed_outputs(call: Leaf, values: &Values<'_>) -> Option<NodeValue> {
    let kind = kind(call, values)?;
    // Explicit type arguments rule out overloads without type parameters.
    if !values.declared_arguments(call).known()?.unwrap_or_default().is_empty() { return None }
    for invoke in &kind.calls.invokes {
        match invoke.when.map_or(Known::Yes(true), |when| values.world.condition(call, when, values.semantic)) {
            Known::Yes(false) => continue,
            Known::Unknown => return None,
            Known::Yes(true) => {}
        }
        let callee = values.evaluate(call, &invoke.item.callee);
        let invoke_kind = if invoke.item.construct { InvokeKind::Construct } else { InvokeKind::Call };
        let sets = values.calls().headers(&callee, invoke_kind).known()?;
        if sets.is_empty() { return None }
        let mut results = Vec::new();
        for set in sets {
            let mut agreed: Option<NodeValue> = None;
            for header in set {
                if !super::infer::formals(header.site, values).known()?.is_empty() { return None }
                // The frame a selected overload is read in (as `candidate`).
                // A header with no type parameters reads its result in its
                // own frame: one result per declaration, not per call site.
                let result = super::result::header_result(header, values).known()?;
                // The very same result only: mutually assignable results
                // (`void`, `undefined`) still tell overloads apart.
                match &agreed {
                    None => agreed = Some(result),
                    Some(previous) if previous == &result => {}
                    Some(_) => return None,
                }
            }
            let NodeValue::Alternatives(arms) = agreed? else { return None };
            results.extend(arms);
        }
        return Some(NodeValue::rows(results));
    }
    None
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
            if previous == &result { continue }
            // Instances of the same generics whose arguments aren't all known
            // can't be shown to agree: don't compare their whole member
            // structure both ways (`Set<?>` from two `Set` constructor
            // overloads spent a question's budget doing that).
            if instances_with_unknown_arguments(previous, &result, values) { return Known::Unknown }
            if values.calls().accepts(previous, &result) != Known::Yes(true)
                || values.calls().accepts(&result, previous) != Known::Yes(true) {
                return Known::Unknown;
            }
        } else {
            agreed = Some(result);
        }
    }
    agreed.map_or(Known::Unknown, Known::Yes)
}

/// Whether `a` and `b` are, row by row and leaf by leaf, instances of the
/// same declarations, and some instance argument is unknown on either side.
fn instances_with_unknown_arguments(a: &NodeValue, b: &NodeValue, values: &Values<'_>) -> bool {
    let (NodeValue::Alternatives(a), NodeValue::Alternatives(b)) = (a, b) else { return false };
    if a.len() != b.len() { return false }
    let mut unknown = false;
    for (left, right) in a.iter().zip(b.iter()) {
        if left.len() != right.len() { return false }
        for (&left, &right) in left.iter().zip(right) {
            if left.target != right.target { return false }
            let (Known::Yes(Some(left)), Known::Yes(Some(right))) = (values.carrier(left), values.carrier(right)) else { return false };
            if left.declaration != right.declaration { return false }
            unknown |= left.arguments.iter().chain(&right.arguments).any(|argument| *argument == NodeValue::Unknown);
        }
    }
    unknown
}

/// One overload against the call's arguments; `decide`: whether its
/// compatibility is asked (else it stays unknown, unchecked).
fn candidate(header: Header, call: Leaf, arguments: &[Leaf], receiver: Option<&NodeValue>, exact: Option<&std::cell::Cell<bool>>, decide: bool, values: &Values<'_>) -> Known<Candidate> {
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
                instantiated: false,
                pending: None,
            });
        }
    }
    let Known::Yes(classes) = values.question.owner_classes(header.site.frame, owner) else { return Known::Unknown };
    let frame = values.question.bind_slots(header.site.frame, owner, Some(call), explicit.iter().cloned().map(Some).collect(), classes);
    let header = Header { site: Leaf { frame, ..header.site }, ..header };
    let Known::Yes(parameters) = values.follow(header.site, &signature.item.parameters) else { return Known::Unknown };
    // Arity first: arguments without a parameter, or a required parameter
    // without one, rule the overload out before any argument is compared
    // or anything inferred (the overloads of `addEventListener`, `from`...).
    let mut mapping = Vec::new();
    let mut slots = Vec::new();
    let mut arity = if decide { Known::Yes(true) } else { Known::Unknown };
    let mut has_rest = false;
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
        slots.push((parameter, is_rest, actuals));
        if decide && actuals.is_empty() && !is_rest {
            match values.optional(parameter) {
                Known::Yes(false) => arity = Known::Yes(false),
                Known::Unknown if arity != Known::Yes(false) => arity = Known::Unknown,
                _ => {}
            }
        }
    }
    if decide && !has_rest && arguments.len() > parameters.len() {
        arity = Known::Yes(false);
    }
    if arity == Known::Yes(false) {
        return Known::Yes(Candidate { chosen: Chosen { header, mapping }, compatible: Known::Yes(false), instantiated: false, pending: None });
    }
    let mut compatible = if decide { Known::Yes(true) } else { Known::Unknown };
    // A declared receiver must accept the one the call passes.
    if let Some(receiver) = receiver.filter(|_| decide) {
        let answer = match values.calls().receiver(header) {
            Known::Yes(Some(declared)) => values.calls().accepts(&values.values(declared), receiver),
            Known::Yes(None) => Known::Yes(true),
            Known::Unknown => Known::Unknown,
        };
        compatible = answer;
    }
    if arity == Known::Unknown && compatible != Known::Yes(false) { compatible = Known::Unknown }
    // Inferred parameters are checked once instantiated.
    let needs_inference = !formals.is_empty() && !arguments.is_empty();
    if decide && !needs_inference {
        'slots: for (parameter, is_rest, actuals) in slots {
            for (offset, &actual) in actuals.iter().enumerate() {
                if compatible == Known::Yes(false) { break 'slots }
                let expected = if is_rest {
                    values.member(&values.values(parameter), &Key::Ordinal(offset as u32))
                } else {
                    values.values(parameter)
                };
                let actual = values.values(actual);
                let mismatch = exact.map(|exact| {
                    let mismatch = dynamic_mismatch(&expected, &actual, values);
                    if mismatch != Known::Yes(false) { exact.set(true) }
                    mismatch
                });
                let answer = match mismatch {
                    Some(Known::Yes(true)) => Known::Yes(false),
                    Some(Known::Unknown) => Known::Unknown,
                    _ => values.calls().accepts(&expected, &actual),
                };
                match answer {
                    Known::Yes(false) => compatible = Known::Yes(false),
                    Known::Unknown if compatible != Known::Yes(false) => compatible = Known::Unknown,
                    _ => {}
                }
            }
        }
    }
    // An overload already ruled out needs no instantiation.
    if compatible == Known::Yes(false) {
        return Known::Yes(Candidate { chosen: Chosen { header, mapping }, compatible, instantiated: false, pending: None });
    }
    let (header, pending) = if formals.is_empty() {
        (header, None)
    } else {
        let instantiated = match super::infer::instantiate(declaration, call, &mapping, &explicit, super::infer::Pass::Context, compatible == Known::Yes(true), values) {
            Known::Yes(Some(instantiated)) => instantiated,
            Known::Yes(None) => return Known::Yes(Candidate { chosen: Chosen { header, mapping }, compatible: Known::Yes(false), instantiated: false, pending: None }),
            Known::Unknown => return Known::Unknown,
        };
        let pending = instantiated.pending.then(|| Box::new(Pending {
            declaration, mapping: mapping.clone(), explicit, checked: compatible,
        }));
        if compatible == Known::Yes(true) { compatible = instantiated.fits }
        for (parameter, _) in &mut mapping { parameter.frame = instantiated.header.site.frame }
        (instantiated.header, pending)
    };
    Known::Yes(Candidate { chosen: Chosen { header, mapping }, compatible, instantiated: true, pending })
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
        // The frame is the first pass's (arguments typed by it) or the
        // second's (results), which maps the same arguments to the same
        // signature: the first pass answers for both, so the second never
        // waits on itself.
        let mut chosen = match values.calls().context(call) {
            Known::Yes(Some(selected)) if !selected.uncertain => selected.choices.into_iter()
                .filter(|chosen| node(chosen.header.site, values) == Some(owner)).collect::<Vec<_>>(),
            Known::Yes(None) => return Known::Yes(None),
            _ => return Known::Unknown,
        };
        let chosen = match chosen.iter().position(|chosen| chosen.header.site.frame == frame) {
            Some(exact) => chosen.swap_remove(exact),
            None if chosen.len() == 1 => chosen.remove(0),
            None => return Known::Unknown,
        };
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

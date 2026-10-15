use crate::expr::Value;
use crate::grammar::names::question::{Known, Leaf, NodeRef, NodeValue, Target};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::values::{Identity, Key, MemberOrigin};
use crate::grammar::names::calls::InvokeKind;

use super::QueryFunction;

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "value_constituents", arity: 1, call: constituents },
    QueryFunction { name: "call_signatures", arity: 1, call: signatures },
    QueryFunction { name: "expected_call_signatures", arity: 1, call: expected_signatures },
    QueryFunction { name: "call_parameters", arity: 1, call: parameters },
    QueryFunction { name: "call_results", arity: 1, call: results },
    QueryFunction { name: "member_call_parameters", arity: 2, call: member_parameters },
    QueryFunction { name: "value_member", arity: 2, call: member },
    QueryFunction { name: "value_lookup", arity: 2, call: lookup },
    QueryFunction { name: "value_has_member", arity: 3, call: has_member },
    QueryFunction { name: "value_at", arity: 2, call: path },
    QueryFunction { name: "value_expected_at", arity: 2, call: expected_path },
    QueryFunction { name: "value_expected", arity: 1, call: expected },
    QueryFunction { name: "value_has_expected", arity: 1, call: has_expected },
    QueryFunction { name: "value_widened", arity: 1, call: widened },
    QueryFunction { name: "value_accepts", arity: 4, call: accepts },
    QueryFunction { name: "value_origin", arity: 1, call: origin },
    QueryFunction { name: "global_declarations", arity: 3, call: globals },
    QueryFunction { name: "declaration_identity", arity: 1, call: declaration },
];

fn constituents(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    project(semantic, semantic.values().values(at))
}

pub(super) fn project(semantic: &Semantic<'_>, value: NodeValue) -> Known<Value<'static>> {
    project_with(semantic, value, &mut |semantic, argument| project(semantic, argument))
}

/// [`project`], with carrier arguments shown by `argument` (projected in
/// full, or as handles that leave them unread).
pub(super) fn project_with(
    semantic: &Semantic<'_>, value: NodeValue,
    argument: &mut dyn FnMut(&Semantic<'_>, NodeValue) -> Known<Value<'static>>,
) -> Known<Value<'static>> {
    let Some(_nested) = semantic.question.budget.nest() else { return Known::Unknown };
    let values = semantic.values();
    let NodeValue::Alternatives(alternatives) = value else { return Known::Unknown };
    let mut arms = Vec::with_capacity(alternatives.len());
    for alternative in alternatives {
        let mut parts = Vec::with_capacity(alternative.len());
        for leaf in alternative {
            if !semantic.question.budget.step() { return Known::Unknown }
            let mut traits = Vec::new();
            let mut uncertain = Vec::new();
            for (id, name) in semantic.tables.vocabulary.traits.iter().enumerate() {
                let Ok(id) = u16::try_from(id) else { return Known::Unknown };
                match values.fits(leaf, id) {
                    Known::Yes(true) => traits.push(Value::from(name.clone())),
                    Known::Yes(false) => {}
                    Known::Unknown => uncertain.push(Value::from(name.clone())),
                }
                if semantic.question.budget.exhausted() { return Known::Unknown }
            }
            let (identity, identity_known) = match values.identity(leaf) {
                Known::Yes(Some(Identity::Value(value))) => (Value::record(vec![
                    ("kind".into(), Value::from("value")), ("value".into(), value),
                ]), true),
                Known::Yes(Some(Identity::Declaration(node))) => (Value::record(vec![
                    ("kind".into(), Value::from("declaration")),
                    ("file".into(), Value::Num(f64::from(node.file))),
                    ("node".into(), Value::Num(f64::from(node.node))),
                ]), true),
                Known::Yes(None) => (Value::Null, true),
                Known::Unknown => (Value::Null, false),
            };
            if semantic.question.budget.exhausted() { return Known::Unknown }
            let (carrier, carrier_known) = match values.carrier(leaf) {
                Known::Yes(Some(carrier)) => {
                    let Some(arguments) = carrier.arguments.into_iter()
                        .map(|value| argument(semantic, value).known())
                        .collect::<Option<Vec<_>>>() else { return Known::Unknown };
                    (Value::record(vec![
                        ("file".into(), Value::Num(f64::from(carrier.declaration.file))),
                        ("node".into(), Value::Num(f64::from(carrier.declaration.node))),
                        ("description".into(), carrier.description),
                        ("arguments".into(), Value::list(arguments)),
                    ]), true)
                }
                Known::Yes(None) => (Value::Null, true),
                Known::Unknown => (Value::Null, false),
            };
            let (product, product_known) = match values.product_info(leaf) {
                Known::Yes(Some(product)) => (Value::record(vec![
                    ("readonly".into(), Value::Bool(product.readonly)),
                    ("elements".into(), Value::list(product.elements.into_iter().map(|element| Value::record(vec![
                        ("optional".into(), Value::Bool(element.optional)),
                        ("repeated".into(), Value::Bool(element.repeated)),
                    ])).collect())),
                ]), true),
                Known::Yes(None) => (Value::Null, true),
                Known::Unknown => (Value::Null, false),
            };
            parts.push(Value::record(vec![
                ("traits".into(), Value::list(traits)),
                ("uncertain_traits".into(), Value::list(uncertain)),
                ("identity".into(), identity),
                ("identity_known".into(), Value::Bool(identity_known)),
                ("carrier".into(), carrier),
                ("carrier_known".into(), Value::Bool(carrier_known)),
                ("product".into(), product),
                ("product_known".into(), Value::Bool(product_known)),
            ]));
        }
        arms.push(Value::list(parts));
    }
    Known::Yes(Value::list(arms))
}

fn member(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(argument) = args.first() else { return Known::Unknown };
    let Known::Yes(key) = query_key(semantic, at, argument) else { return Known::Unknown };
    let values = semantic.values();
    project(semantic, values.member(&values.values(at), &key))
}

fn query_key(semantic: &Semantic<'_>, at: Leaf, value: &Value<'static>) -> Known<Key> {
    if let Some(key) = member_key(value) { return Known::Yes(key) }
    let values = semantic.values();
    if let Value::Node(node, _) = value {
        let file = match at.target {
            Target::Node(node) => node.file,
            Target::Scope(scope) => scope.file,
            Target::Traits(_) | Target::Product(_) => return Known::Unknown,
        };
        return values.value_key(&values.values(Leaf { target: Target::Node(NodeRef { file, node: *node }), frame: at.frame }));
    }
    let Known::Yes(value) = global_value(semantic, at, value) else { return Known::Unknown };
    values.value_key(&value)
}

fn global_value(semantic: &Semantic<'_>, at: Leaf, value: &Value<'static>) -> Known<NodeValue> {
    let Value::Record(fields) = value else { return Known::Unknown };
    let field = |name: &str| fields.iter().find(|(key, _)| &**key == name).map(|(_, value)| value);
    let (Some(Value::Str(name)), Some(Value::Str(class)), Some(Value::List(path))) =
        (field("global"), field("class"), field("path")) else { return Known::Unknown };
    let classes = semantic.tables.vocabulary.declaration_class(class);
    if classes == 0 { return Known::Unknown }
    let values = semantic.values();
    let mut value = values.evaluate(at, &crate::grammar::names::Value::Instance {
        name: (&**name).into(), classes, arguments: Vec::new(),
    });
    for key in path.iter() {
        if !semantic.question.budget.step() { return Known::Unknown }
        let Some(key) = member_key(key) else { return Known::Unknown };
        value = values.member(&value, &key);
    }
    Known::Yes(value)
}

/// The node's value for member questions. A node that is its own shape (a
/// declaration, or one unwrapping to its own member scope) is read without
/// classifying it as a whole: its trait conditions ask these questions, and
/// normalising it here (which asks those traits) would cycle. Other nodes
/// (expressions) keep their full, narrowed value.
fn own_value(semantic: &Semantic<'_>, at: Leaf) -> Known<NodeValue> {
    use crate::grammar::names::Value as Recipe;
    let values = semantic.values();
    match values.unwrap_recipe(at) {
        Known::Yes(None | Some(Recipe::Current)) => Known::Yes(NodeValue::leaf(at)),
        Known::Yes(Some(recipe @ Recipe::Scope { .. })) => Known::Yes(values.evaluate(at, recipe)),
        Known::Yes(Some(_)) => Known::Yes(values.values(at)),
        Known::Unknown => Known::Unknown,
    }
}

fn has_member(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(argument) = args.first() else { return Known::Unknown };
    let Known::Yes(key) = query_key(semantic, at, argument) else { return Known::Unknown };
    let values = semantic.values();
    let receiver = match args.get(1) {
        Some(Value::Null) => match own_value(semantic, at) {
            Known::Yes(value) => value,
            Known::Unknown => return Known::Unknown,
        },
        Some(source) => match global_value(semantic, at, source) {
            Known::Yes(value) => value,
            Known::Unknown => return Known::Unknown,
        },
        None => return Known::Unknown,
    };
    match values.has_member(&receiver, &key) {
        Known::Yes(value) => Known::Yes(Value::Bool(value)),
        Known::Unknown => Known::Unknown,
    }
}

/// One member source: how the member is keyed, the node declaring it and
/// that declaration's flags.
pub(super) fn source_value(semantic: &Semantic<'_>, source: &crate::grammar::names::values::MemberSource) -> Known<Value<'static>> {
    let (kind, domain) = origin_parts(source.origin);
    let site = match source.site.target {
        Target::Node(node) => Value::record(vec![
            ("file".into(), Value::Num(f64::from(node.file))),
            ("node".into(), Value::Num(f64::from(node.node))),
        ]),
        _ => Value::Null,
    };
    let Known::Yes(flags) = semantic.values().flags(source.site) else { return Known::Unknown };
    let flags = Value::list(flags.into_iter().map(|flag| Value::from(String::from(flag))).collect());
    Known::Yes(Value::record(vec![("kind".into(), Value::from(kind)), ("domain".into(), domain), ("site".into(), site), ("flags".into(), flags)]))
}

fn origin_parts(origin: MemberOrigin) -> (&'static str, Value<'static>) {
    match origin {
        MemberOrigin::Named => ("named", Value::Null),
        MemberOrigin::Ordinal { repeated: false } => ("ordinal", Value::Null),
        MemberOrigin::Ordinal { repeated: true } => ("repeated", Value::Null),
        MemberOrigin::Indexer(domain) => ("indexer", Value::from(match domain {
            crate::config::ast::values::KeyDomain::Text => "text",
            crate::config::ast::values::KeyDomain::Ordinal => "ordinal",
            crate::config::ast::values::KeyDomain::Symbol => "symbol",
        })),
    }
}

fn lookup(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(argument) = args.first() else { return Known::Unknown };
    let Known::Yes(key) = query_key(semantic, at, argument) else { return Known::Unknown };
    lookup_in(semantic, semantic.values().values(at), &key)
}

pub(super) fn lookup_in(semantic: &Semantic<'_>, value: NodeValue, key: &Key) -> Known<Value<'static>> {
    let values = semantic.values();
    let Known::Yes(rows) = values.keyed_members(&value, key) else { return Known::Unknown };
    let mut alternatives = Vec::new();
    for row in rows {
        if !semantic.question.budget.step() { return Known::Unknown }
        let (matched, required, sources) = match row {
            Some(member) => {
                let required = member.required.known().map_or(Value::Null, Value::Bool);
                let mut sources = Vec::with_capacity(member.sources.len());
                for source in &member.sources {
                    let Known::Yes(source) = source_value(semantic, source) else { return Known::Unknown };
                    sources.push(source);
                }
                (true, required, sources)
            },
            None => (false, Value::Null, Vec::new()),
        };
        alternatives.push(Value::record(vec![
            ("matched".into(), Value::Bool(matched)), ("required".into(), required),
            ("sources".into(), Value::list(sources)),
        ]));
    }
    let result = project(semantic, values.member(&value, key)).known();
    if semantic.question.budget.exhausted() { return Known::Unknown }
    Known::Yes(Value::record(vec![
        ("value_known".into(), Value::Bool(result.is_some())),
        ("value".into(), result.unwrap_or(Value::Null)),
        ("alternatives".into(), Value::list(alternatives)),
    ]))
}

pub(super) fn member_key(value: &Value<'static>) -> Option<Key> {
    match value {
        Value::Str(key) => Some(Key::Text((&**key).into())),
        Value::Num(index) if *index >= 0.0 && *index <= f64::from(u32::MAX) && index.fract() == 0.0 => {
            Some(Key::Ordinal(*index as u32))
        }
        _ => None,
    }
}

fn path(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    project_path(semantic, semantic.values().values(at), args)
}

fn expected_path(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    match semantic.calls().expected(at) {
        Known::Yes(Some(value)) => project_path(semantic, value, args),
        Known::Yes(None) => Known::Yes(Value::Null),
        Known::Unknown => Known::Unknown,
    }
}

pub(super) fn project_path(semantic: &Semantic<'_>, mut value: NodeValue, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(Value::List(keys)) = args.first() else { return Known::Unknown };
    let values = semantic.values();
    for key in keys.iter() {
        if !semantic.question.budget.step() { return Known::Unknown }
        let Some(key) = member_key(key) else { return Known::Unknown };
        value = values.member(&value, &key);
    }
    project(semantic, value)
}

fn expected(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let value = match semantic.calls().expected(at) {
        Known::Unknown => return Known::Unknown,
        Known::Yes(value) => value,
    };
    let present = value.is_some();
    let value = match value {
        Some(value) => match project(semantic, value) {
            Known::Yes(value) => value,
            Known::Unknown => return Known::Unknown,
        },
        None => Value::Null,
    };
    Known::Yes(Value::record(vec![
        ("present".into(), Value::Bool(present)),
        ("value".into(), value),
    ]))
}

fn has_expected(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    match semantic.calls().expected(at) {
        Known::Yes(value) => Known::Yes(Value::Bool(value.is_some())),
        Known::Unknown => Known::Unknown,
    }
}

fn widened(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    project(semantic, semantic.values().evaluate(at,
        &crate::grammar::names::Value::Path(vec![crate::grammar::names::WIDENED])))
}

/// Whether every value the second node may be is one the first allows, each
/// read as its `"value"`, `"expected"` or `"widened"` value.
fn accepts(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let [Value::Node(other, _), Value::Str(allowing), Value::Str(given)] = args else { return Known::Unknown };
    let Target::Node(node) = at.target else { return Known::Unknown };
    let other = Leaf { target: Target::Node(NodeRef { node: *other, ..node }), ..at };
    let side = |at: Leaf, mode: &str| match mode {
        "value" => Known::Yes(Some(semantic.values().values(at))),
        "expected" => semantic.calls().expected(at),
        "widened" => Known::Yes(Some(semantic.values().evaluate(at,
            &crate::grammar::names::Value::Path(vec![crate::grammar::names::WIDENED])))),
        _ => Known::Unknown,
    };
    let (Known::Yes(Some(expected)), Known::Yes(Some(actual))) = (side(at, allowing), side(other, given)) else { return Known::Unknown };
    match semantic.calls().accepts(&expected, &actual) {
        Known::Yes(accepted) => Known::Yes(Value::Bool(accepted)),
        Known::Unknown => Known::Unknown,
    }
}

fn origin(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let leaf = match semantic.values().origin(at) {
        Known::Yes(Some(leaf)) => leaf,
        Known::Yes(None) => return Known::Yes(Value::Null),
        Known::Unknown => return Known::Unknown,
    };
    let Target::Node(node) = leaf.target else { return Known::Unknown };
    let Some(tree) = semantic.world.tree(node.file) else { return Known::Unknown };
    let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
    let Some(matcher) = semantic.world.grammar().matchers.get(data.matcher as usize) else { return Known::Unknown };
    Known::Yes(Value::record(vec![
        ("file".into(), Value::Num(f64::from(node.file))),
        ("node".into(), Value::Num(f64::from(node.node))),
        ("kind".into(), Value::from(matcher.name.clone())),
    ]))
}

fn globals(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let [Value::Str(name), Value::Str(class)] = args else { return Known::Unknown };
    let classes = semantic.tables.vocabulary.declaration_class(class);
    if classes == 0 { return Known::Unknown }
    let file = match at.target {
        Target::Node(node) => node.file,
        Target::Scope(scope) => scope.file,
        Target::Traits(_) | Target::Product(_) => return Known::Unknown,
    };
    let Known::Yes(targets) = semantic.modules().global(file, name, classes) else { return Known::Unknown };
    let mut declarations = Vec::new();
    for target in targets {
        if !semantic.question.budget.step() { return Known::Unknown }
        let node = match target {
            Target::Node(node) => node,
            Target::Scope(scope) => {
                let Some(info) = semantic.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                    return Known::Unknown;
                };
                NodeRef { file: scope.file, node: info.node }
            }
            Target::Traits(_) | Target::Product(_) => return Known::Unknown,
        };
        let Known::Yes(node) = semantic.values().declaration_owner(node) else { return Known::Unknown };
        let declaration = Value::record(vec![
            ("file".into(), Value::Num(f64::from(node.file))),
            ("node".into(), Value::Num(f64::from(node.node))),
        ]);
        if !declarations.contains(&declaration) { declarations.push(declaration) }
    }
    Known::Yes(Value::list(declarations))
}

fn declaration(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let node = match at.target {
        Target::Node(node) => node,
        Target::Scope(scope) => {
            let Some(info) = semantic.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                return Known::Unknown;
            };
            NodeRef { file: scope.file, node: info.node }
        }
        Target::Traits(_) | Target::Product(_) => return Known::Unknown,
    };
    let Known::Yes(node) = semantic.values().resolved_declaration(node) else { return Known::Unknown };
    Known::Yes(Value::record(vec![
        ("file".into(), Value::Num(f64::from(node.file))),
        ("node".into(), Value::Num(f64::from(node.node))),
    ]))
}

fn signatures(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    signature_groups(semantic, at, SignatureFields::All)
}

/// The call signatures of the node's expected value, per alternative;
/// alternatives that cannot be called drop out. Null when nothing is
/// expected.
fn expected_signatures(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let expected = match semantic.calls().expected(at) {
        Known::Yes(Some(expected)) => expected,
        Known::Yes(None) => return Known::Yes(Value::Null),
        Known::Unknown => return Known::Unknown,
    };
    let Known::Yes(Value::List(alternatives)) = signature_values(semantic, &expected, SignatureFields::All) else { return Known::Unknown };
    Known::Yes(Value::list(alternatives.iter().filter(|arm| !matches!(arm, Value::List(signatures) if signatures.is_empty())).cloned().collect()))
}

fn parameters(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    signature_groups(semantic, at, SignatureFields::Parameters)
}

fn results(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    signature_groups(semantic, at, SignatureFields::Results)
}

fn member_parameters(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(argument) = args.first() else { return Known::Unknown };
    let Known::Yes(key) = query_key(semantic, at, argument) else { return Known::Unknown };
    let Known::Yes(receiver) = own_value(semantic, at) else { return Known::Unknown };
    member_parameters_in(semantic, &receiver, &key)
}

/// The parameters of each call signature of `receiver`'s member `key`.
pub(super) fn member_parameters_in(semantic: &Semantic<'_>, receiver: &NodeValue, key: &Key) -> Known<Value<'static>> {
    let values = semantic.values();
    // A member a closed shape proves absent has no signatures at all.
    if values.has_member(receiver, key) == Known::Yes(false) { return Known::Yes(Value::list(Vec::new())) }
    let member = values.member(receiver, key);
    signature_values(semantic, &member, SignatureFields::Parameters)
}

enum SignatureFields { All, Parameters, Results }

fn signature_groups(semantic: &Semantic<'_>, at: Leaf, fields: SignatureFields) -> Known<Value<'static>> {
    signature_values(semantic, &semantic.values().values(at), fields)
}

fn signature_values(semantic: &Semantic<'_>, value: &NodeValue, fields: SignatureFields) -> Known<Value<'static>> {
    let values = semantic.values();
    let calls = semantic.calls();
    let Known::Yes(arms) = calls.headers(value, InvokeKind::Call) else { return Known::Unknown };
    let mut alternatives = Vec::with_capacity(arms.len());
    for arm in arms {
        let mut signatures = Vec::with_capacity(arm.len());
        for header in arm {
            if !semantic.question.budget.step() { return Known::Unknown }
            if matches!(fields, SignatureFields::Results) {
                let result = calls.header_result(header).known().and_then(|result| project(semantic, result).known());
                if semantic.question.budget.exhausted() { return Known::Unknown }
                signatures.push(Value::record(vec![
                    ("result_known".into(), Value::Bool(result.is_some())),
                    ("result".into(), result.unwrap_or(Value::Null)),
                ]));
                continue;
            }
            let (parameters, parameters_known) = match calls.parameters(header) {
                Known::Yes(parameters) => {
                    let projected = parameters.into_iter().map(|parameter| project(semantic, values.values(parameter)).known())
                        .collect::<Option<Vec<_>>>();
                    match projected {
                        Some(parameters) => (Value::list(parameters), true),
                        None => (Value::Null, false),
                    }
                }
                Known::Unknown => (Value::Null, false),
            };
            if matches!(fields, SignatureFields::Parameters) {
                if semantic.question.budget.exhausted() { return Known::Unknown }
                signatures.push(Value::record(vec![
                    ("parameters".into(), parameters),
                    ("parameters_known".into(), Value::Bool(parameters_known)),
                ]));
                continue;
            }
            let result = calls.header_result(header).known().and_then(|result| project(semantic, result).known());
            let (receiver, receiver_known, has_receiver) = match calls.receiver(header) {
                Known::Yes(Some(receiver)) => {
                    let receiver = project(semantic, values.values(receiver)).known();
                    let known = receiver.is_some();
                    (receiver.unwrap_or(Value::Null), known, Value::Bool(true))
                }
                Known::Yes(None) => (Value::Null, true, Value::Bool(false)),
                Known::Unknown => (Value::Null, false, Value::Null),
            };
            if semantic.question.budget.exhausted() { return Known::Unknown }
            signatures.push(Value::record(vec![
                ("parameters".into(), parameters),
                ("parameters_known".into(), Value::Bool(parameters_known)),
                ("result_known".into(), Value::Bool(result.is_some())),
                ("result".into(), result.unwrap_or(Value::Null)),
                ("receiver".into(), receiver),
                ("receiver_known".into(), Value::Bool(receiver_known)),
                ("has_receiver".into(), has_receiver),
            ]));
        }
        alternatives.push(Value::list(signatures));
    }
    Known::Yes(Value::list(alternatives))
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{lint, linter};

    #[test]
    fn expected_presence_does_not_expand_values_or_turn_failed_context_into_absence() {
        let linter = linter(r#"
            match unreadable /u/ { unwraps unknown }
            match container "c" argument:unreadable { carries argument using it as {} }
            in .t match present "p" context:container {
                in it expects context
                fits checked when value_has_expected(node)
                rule present warn "present" { when node always checked }
            }
            match absent /a/ {
                in it expects context
                fits checked when !value_has_expected(node)
                rule absent warn "absent" { when node always checked }
            }
            in .t match host "h" content:absent { fits context }
            in .t match failed /f/ {
                in it expects unknown
                fits invented when !value_has_expected(node)
                rule invented warn "invented" { when node always invented }
                rule unknown warn "unknown" { when value_has_expected(node) == null }
            }
        "#);
        let reports = lint(&linter, "pcu ha f");
        assert_eq!(reports.len(), 3, "{reports:#?}");
        for rule in ["present", "absent", "unknown"] {
            assert_eq!(reports.iter().filter(|report| report.rule == rule).count(), 1);
        }
    }

    #[test]
    fn member_paths_follow_each_value_and_keep_missing_answers_unknown() {
        let linter = linter(r#"
            match good /a/ { fits good }
            match other /b/ { fits other }
            match cell "i" element:(good or other) { indexed element }
            in .t match container "c" element:cell {
                indexed element
                in it expects element
                let nested value_at(node, [0, 0])
                let contextual value_expected_at(node, [0])
                let missing value_at(node, [0, 1])
                let invalid value_at(node, [true])
                rule nested warn "nested" {
                    when any(nested, arm => any(arm, part => contains(part.traits, "good")))
                }
                rule contextual warn "contextual" {
                    when any(contextual, arm => any(arm, part => contains(part.traits, "good")))
                }
                rule unknown warn "unknown" { when missing == null && invalid == null }
            }
        "#);
        let reports = lint(&linter, "cia cib");
        assert_eq!(reports.iter().filter(|report| report.rule == "nested").count(), 1);
        assert_eq!(reports.iter().filter(|report| report.rule == "contextual").count(), 1);
        assert_eq!(reports.iter().filter(|report| report.rule == "unknown").count(), 2);
    }

    #[test]
    fn parameter_checks_do_not_require_recursive_result_classification() {
        let linter = linter(r#"
            match callable /a/ { fits callable }
            in .t match producer "p" parameter:callable {
                fits callable
                signature parameter returns it
                let parameters call_parameters(node)
                fits checked when any(parameters, arm => any(arm, header => header.parameters_known))
                rule parameters warn "parameters" { when node always checked }
            }
        "#);
        assert_eq!(lint(&linter, "pa").len(), 1);
    }

    #[test]
    fn known_parameter_traits_survive_unrelated_recursive_uncertainty_on_first_query() {
        let linter = linter(r#"
            match parameter /a/ {
                fits callable
                fits incidental when parent always checked
            }
            in .t match producer "p" parameter:parameter {
                fits callable
                signature parameter returns it
                let parameters call_parameters(node)
                fits checked when any(parameters, arm => any(arm, header =>
                    header.parameters_known && any(header.parameters, parameter =>
                        any(parameter, parts => any(parts, part => contains(part.traits, "callable"))))))
                let missing if node could checked then null else { name: "missing" }
                rule missing warn missing.name { when missing != null }
                rule checked warn "checked" { when node always checked }
            }
        "#);
        let reports = lint(&linter, "pa");
        assert_eq!(reports.len(), 1, "{reports:#?}");
        assert_eq!(reports[0].rule, "checked");
    }

    #[test]
    fn known_constituents_survive_uncertain_traits_without_inventing_negative_proofs() {
        let linter = linter(r#"
            match known /a/ {
                fits good
                fits incidental when node always incidental
            }
            in .t match producer "p" parameter:known {
                fits checked when any(value_constituents(parameter.node), arm =>
                    any(arm, part => contains(part.traits, "good")
                        && contains(part.uncertain_traits, "incidental")))
                fits invented when !(parameter.node could incidental)
                rule checked warn "checked" { when node always checked }
                rule invented warn "invented" { when node always invented }
            }
        "#);
        let reports = lint(&linter, "pa");
        assert_eq!(reports.len(), 1, "{reports:#?}");
        assert_eq!(reports[0].rule, "checked");
    }

    #[test]
    fn unreadable_carrier_arguments_cannot_be_known_signature_parameters() {
        let linter = linter(r#"
            match unreadable /u/ { unwraps unknown }
            match container "c" argument:unreadable { carries argument using it as {} }
            in .t match producer "p" parameter:container {
                fits callable
                signature parameter returns it
                rule unknown warn "unknown" {
                    when any(call_parameters(node), arm => any(arm, header =>
                        !header.parameters_known && header.parameters == null))
                }
            }
        "#);
        assert_eq!(lint(&linter, "pcu").len(), 1);
    }

    #[test]
    fn result_checks_do_not_require_resolving_parameters_and_keep_unknown_results() {
        let linter = linter(r#"
            match good /b/ { fits good }
            match parameter /a/ { fits demanded when parent always checked }
            match unreadable /u/ { unwraps unknown }
            in .t match producer "p" parameter:parameter output:(good or unreadable) {
                fits callable
                signature parameter returns output
                let results call_results(node)
                fits checked when any(results, arm => any(arm, header => header.result_known
                    && any(header.result, parts => any(parts, part => contains(part.traits, "good")))))
                rule known warn "known" { when node always checked }
                rule unknown warn "unknown" {
                    when any(results, arm => any(arm, header => !header.result_known && header.result == null))
                }
            }
        "#);
        let reports = lint(&linter, "pab pau");
        assert_eq!(reports.iter().filter(|report| report.rule == "known").count(), 1);
        assert_eq!(reports.iter().filter(|report| report.rule == "unknown").count(), 1);
    }

    #[test]
    fn receiver_values_keep_declared_absence_distinct_from_unreadable_metadata() {
        let linter = linter(r#"
            match good /a/ { fits good }
            match missing /m/ { fits missing }
            match callable "f" parameter:"()"? context:good {
                fits callable
                signature parameter receiver context returns it
            }
            match absent "e" (parameter:good or "n") {
                fits callable
                signature parameter receiver parameter returns it
            }
            match unreadable "u" parameter:"()"? {
                fits callable
                signature parameter returns it
            }
            in .t match probe "q" value:(callable or absent or unreadable) {
                unwraps value.receiver ?? trait missing
                rule receiver warn "receiver" { when node always good }
                rule absent warn "absent" { when node always missing }
                rule unknown warn "unknown" { when value_constituents(node) == null }
            }
        "#);
        let reports = lint(&linter, "qfa qen qu");
        for rule in ["receiver", "absent", "unknown"] {
            assert_eq!(reports.iter().filter(|report| report.rule == rule).count(), 1, "{rule}");
        }
    }

    #[test]
    fn carrier_arguments_are_projected_after_value_expansion() {
        let linter = linter(r#"
            match good /a/ { fits good; widens trait widened }
            match other /b/ { fits other }
            match wide /w/ { fits widened }
            in .t {
                match container "c" argument:(good or other) {
                    carries argument using it.widened as { readonly: true }
                }
                match alias "r" value:container { unwraps value }
            }
            in alias {
                let parts value_constituents(node)
                rule argument warn "argument" {
                    when any(parts, arm => any(arm, part =>
                        part.carrier_known && part.carrier.description.readonly
                        && any(part.carrier.arguments, argument =>
                            any(argument, row => any(row, item => contains(item.traits, "widened"))))))
                }
            }
        "#);
        let reports = lint(&linter, "rca rcb");
        assert_eq!(reports.len(), 1);
    }

    #[test]
    fn projected_constituents_preserve_disjunction_and_conjunction() {
        let linter = linter(r#"
            match good /a/ { fits good }
            match other /b/ { fits other }
            in .t {
                match either left:good "|" right:other { unwraps left | right }
                match both left:good "&" right:other { unwraps left & right }
            }
            in either, both {
                let parts value_constituents(node)
                let first_arm first(parts)
                rule disjunction warn "union" { when len(parts) == 2 }
                rule conjunction warn "intersection" {
                    when (len(parts) == 1) && (len(first_arm) == 2)
                }
                rule traits warn "traits" {
                    when any(parts, arm => any(arm, part => contains(part.traits, "good")))
                }
            }
        "#);
        let reports = lint(&linter, "a|b a&b");
        assert_eq!(reports.iter().filter(|report| report.rule == "disjunction").count(), 1);
        assert_eq!(reports.iter().filter(|report| report.rule == "conjunction").count(), 1);
        assert_eq!(reports.iter().filter(|report| report.rule == "traits").count(), 2);
    }

    #[test]
    fn callable_projection_preserves_results_and_unknown_receivers() {
        let linter = linter(r#"
            match good /a/ { fits good }
            match other /b/ { fits other }
            in .t {
                match producer "f" parameter:"()"? output:(good or other) {
                    fits callable
                    signature parameter returns output
                }
            }
            in producer {
                let signatures call_signatures(node)
                rule result warn "result" {
                    when any(signatures, arm => any(arm, signature =>
                        signature.result_known
                        && any(signature.result, parts => any(parts, part => contains(part.traits, "good")))))
                }
                rule receiver warn "receiver" {
                    when any(signatures, arm => any(arm, signature =>
                        signature.receiver_known == false && signature.has_receiver == null))
                }
            }
        "#);
        let reports = lint(&linter, "fa fb");
        assert_eq!(reports.iter().filter(|report| report.rule == "result").count(), 1);
        assert_eq!(reports.iter().filter(|report| report.rule == "receiver").count(), 2);
    }
}

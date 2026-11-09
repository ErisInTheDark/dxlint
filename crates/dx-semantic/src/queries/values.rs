use dx_ir::value::Value;
use crate::names::question::{Known, Leaf, NodeRef, NodeValue, Target};
use crate::semantic::Semantic;
use crate::names::values::{Identity, Key, MemberOrigin};
use crate::names::calls::InvokeKind;

use super::QueryFunction;

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "value_constituents", arity: 1, call: constituents },
    QueryFunction { name: "value_constituent_traits", arity: 2, call: constituent_traits },
    QueryFunction { name: "call_signatures", arity: 1, call: signatures },
    QueryFunction { name: "expected_call_signatures", arity: 1, call: expected_signatures },
    QueryFunction { name: "call_parameters", arity: 1, call: parameters },
    QueryFunction { name: "call_results", arity: 1, call: results },
    QueryFunction { name: "member_call_parameters", arity: 2, call: member_parameters },
    QueryFunction { name: "value_member", arity: 2, call: member },
    QueryFunction { name: "value_lookup", arity: 2, call: lookup },
    QueryFunction { name: "value_member_optional", arity: 2, call: member_optional },
    QueryFunction { name: "value_key", arity: 1, call: value_key },
    QueryFunction { name: "value_keys", arity: 1, call: value_keys },
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

/// The trait ids named by a list of trait names; `None` when the argument
/// isn't one or names a trait the vocabulary lacks.
pub(super) fn trait_ids(semantic: &Semantic<'_>, names: Option<&Value<'static>>) -> Option<Vec<(u16, Value<'static>)>> {
    let Some(Value::List(names)) = names else { return None };
    let traits = &semantic.tables.vocabulary.traits;
    names.iter().map(|name| {
        let Value::Str(_) = name else { return None };
        let name = name.text();
        let id = traits.iter().position(|known| **known == *name)?;
        Some((u16::try_from(id).ok()?, Value::from(traits[id].clone())))
    }).collect()
}

/// [`project`] reduced to the traits asked for: per alternative, per leaf,
/// `{ traits, uncertain_traits }` over `wanted` only. Callers reading one or
/// two traits skip the whole vocabulary and the identity, carrier and
/// product records.
pub(super) fn project_traits(semantic: &Semantic<'_>, value: NodeValue, wanted: &[(u16, Value<'static>)]) -> Known<Value<'static>> {
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
            for (id, name) in wanted {
                match values.fits(leaf, *id) {
                    Known::Yes(true) => traits.push(name.clone()),
                    Known::Yes(false) => {}
                    Known::Unknown => uncertain.push(name.clone()),
                }
                if semantic.question.budget.exhausted() { return Known::Unknown }
            }
            parts.push(Value::record(vec![
                ("traits".into(), Value::list(traits)),
                ("uncertain_traits".into(), Value::list(uncertain)),
            ]));
        }
        arms.push(Value::list(parts));
    }
    Known::Yes(Value::list(arms))
}

fn constituent_traits(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(wanted) = trait_ids(semantic, args.first()) else { return Known::Unknown };
    project_traits(semantic, semantic.values().values(at), &wanted)
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
                        ("values".into(), Value::Bool(element.values)),
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
            Target::Traits(_) | Target::Product(_) | Target::Modified(_) | Target::Key(_) => return Known::Unknown,
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
    let mut value = values.evaluate(at, &crate::names::Value::Instance {
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
    use crate::names::Value as Recipe;
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
pub(super) fn source_value(semantic: &Semantic<'_>, source: &crate::names::values::MemberSource) -> Known<Value<'static>> {
    let (kind, domain) = origin_parts(source.origin);
    let site = match source.site.target {
        Target::Node(node) => Value::record(vec![
            ("file".into(), Value::Num(f64::from(node.file))),
            ("node".into(), Value::Num(f64::from(node.node))),
        ]),
        _ => Value::Null,
    };
    let Known::Yes(flags) = semantic.values().source_flags(source) else { return Known::Unknown };
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

/// Whether some alternative of `at`'s value has member `key` as a declared
/// (named) member that is not required: `value_lookup`'s answer to that one
/// question, without projecting the member's value.
fn member_optional(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Some(argument) = args.first() else { return Known::Unknown };
    let Known::Yes(key) = query_key(semantic, at, argument) else { return Known::Unknown };
    let values = semantic.values();
    let Known::Yes(rows) = values.keyed_members(&values.values(at), &key) else { return Known::Unknown };
    let optional = rows.iter().flatten().any(|member| member.required == Known::Yes(false)
        && member.sources.iter().any(|source| matches!(source.origin, MemberOrigin::Named)));
    Known::Yes(Value::Bool(optional))
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
    let member = values.member(&value, key);
    if semantic.question.budget.exhausted() { return Known::Unknown }
    let member = if matches!(member, NodeValue::Unknown) { Value::Null } else { super::handles::handle(semantic, member) };
    Known::Yes(Value::record(vec![
        ("member".into(), member),
        ("alternatives".into(), Value::list(alternatives)),
    ]))
}

/// A member key as rules write it: text, an ordinal, or a key record as
/// `value_key` / `value_children` give it (`{symbol: {file, node}}`,
/// `{domain: "text" | "ordinal" | "symbol"}`).
pub(super) fn member_key(value: &Value<'static>) -> Option<Key> {
    use crate::config::ast::values::KeyDomain;
    match value {
        Value::Str(key) => Some(Key::Text((&**key).into())),
        Value::Num(index) if *index >= 0.0 && *index <= f64::from(u32::MAX) && index.fract() == 0.0 => {
            Some(Key::Ordinal(*index as u32))
        }
        Value::Record(_) => {
            let number = |site: &Value<'static>, name: &str| match site.key(name) {
                Value::Num(n) if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) => Some(n as u32),
                _ => None,
            };
            match (value.key("symbol"), value.key("domain")) {
                (symbol @ Value::Record(_), _) => Some(Key::Symbol(NodeRef { file: number(&symbol, "file")?, node: number(&symbol, "node")? })),
                (_, Value::Str(domain)) => Some(Key::Domain(match &*domain {
                    "text" => KeyDomain::Text,
                    "ordinal" => KeyDomain::Ordinal,
                    "symbol" => KeyDomain::Symbol,
                    _ => return None,
                })),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The member key a key expression evaluates to (`obj[key]`'s `key`), in
/// the form `value_lookup` accepts back; null when unknown.
fn value_key(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let values = semantic.values();
    match values.value_key(&values.values(at)) {
        Known::Yes(key) => Known::Yes(super::handles::key_value(&key)),
        Known::Unknown => Known::Yes(Value::Null),
    }
}

/// Every key a key expression may evaluate to (`'a' | 'b'` gives both),
/// each as `value_key` gives it; null when unknown.
fn value_keys(semantic: &Semantic<'_>, at: Leaf, _: &[Value<'static>]) -> Known<Value<'static>> {
    let values = semantic.values();
    match values.value_keys(&values.values(at)) {
        Known::Yes(keys) => Known::Yes(Value::list(keys.iter().map(super::handles::key_value).collect())),
        Known::Unknown => Known::Yes(Value::Null),
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
        &crate::names::Value::Path(vec![crate::names::WIDENED])))
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
            &crate::names::Value::Path(vec![crate::names::WIDENED])))),
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
        Target::Traits(_) | Target::Product(_) | Target::Modified(_) | Target::Key(_) => return Known::Unknown,
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
            Target::Traits(_) | Target::Product(_) | Target::Modified(_) | Target::Key(_) => return Known::Unknown,
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
        Target::Traits(_) | Target::Product(_) | Target::Modified(_) | Target::Key(_) => return Known::Unknown,
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
            let flag = |answer: Known<bool>| match answer {
                Known::Yes(flag) => Value::Bool(flag),
                Known::Unknown => Value::Null,
            };
            let (parameters, parameters_known, rest, optional) = match calls.parameters(header) {
                Known::Yes(parameters) => {
                    let rest = Value::list(parameters.iter().map(|&parameter| flag(calls.is_rest(parameter))).collect());
                    let optional = Value::list(parameters.iter().map(|&parameter| flag(values.optional(parameter))).collect());
                    let projected = parameters.into_iter().map(|parameter| project(semantic, values.values(parameter)).known())
                        .collect::<Option<Vec<_>>>();
                    match projected {
                        Some(parameters) => (Value::list(parameters), true, rest, optional),
                        None => (Value::Null, false, rest, optional),
                    }
                }
                Known::Unknown => (Value::Null, false, Value::Null, Value::Null),
            };
            if matches!(fields, SignatureFields::Parameters) {
                if semantic.question.budget.exhausted() { return Known::Unknown }
                signatures.push(Value::record(vec![
                    ("parameters".into(), parameters),
                    ("parameters_known".into(), Value::Bool(parameters_known)),
                    ("parameter_rest".into(), rest),
                    ("parameter_optional".into(), optional),
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
                ("parameter_rest".into(), rest),
                ("parameter_optional".into(), optional),
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

use crate::expr::Value;
use crate::grammar::names::question::{Known, Leaf, Target};
use crate::grammar::names::semantic::Semantic;

use super::QueryFunction;

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "name_classes", arity: 3, call: classes },
    QueryFunction { name: "module_exports", arity: 2, call: exports },
];

fn classes(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Target::Node(node) = at.target else { return Known::Unknown };
    let [Value::Str(name), source] = args else { return Known::Unknown };
    match source {
        Value::Null => class_answers(semantic, |class| semantic.modules().binding_has_class(node, name, class)),
        Value::Str(source) => match semantic.world.module(at, source, semantic) {
            Known::Yes(Some(scope)) => class_answers(semantic, |class| semantic.modules().export_has_class(scope, name, class)),
            _ => Known::Unknown,
        },
        _ => Known::Unknown,
    }
}

fn exports(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let [Value::Str(source)] = args else { return Known::Unknown };
    let Known::Yes(Some(scope)) = semantic.world.module(at, source, semantic) else { return Known::Unknown };
    let Known::Yes(names) = semantic.modules().export_names(scope) else { return Known::Unknown };
    let mut rows = Vec::with_capacity(names.len());
    for name in names {
        let Known::Yes(classes) = class_answers(semantic, |class| semantic.modules().export_has_class(scope, &name, class)) else { return Known::Unknown };
        if let Value::Record(answers) = &classes {
            if answers.iter().all(|(_, value)| matches!(value, Value::Bool(false))) { continue }
        }
        rows.push(Value::record(vec![
            ("name".into(), Value::text_copy(&name)),
            ("classes".into(), classes),
        ]));
    }
    Known::Yes(Value::list(rows))
}

fn class_answers(semantic: &Semantic<'_>, mut ask: impl FnMut(u64) -> Known<bool>) -> Known<Value<'static>> {
    let mut answers = Vec::new();
    for (bit, name) in semantic.tables.vocabulary.declaration_classes.iter().enumerate() {
        let value = match ask(1u64 << bit) {
            Known::Yes(present) => Value::Bool(present),
            Known::Unknown => Value::Null,
        };
        if semantic.question.budget.exhausted() { return Known::Unknown }
        answers.push((name.clone().into(), value));
    }
    Known::Yes(Value::record(answers))
}

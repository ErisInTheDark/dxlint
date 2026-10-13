use crate::expr::Value;
use crate::grammar::names::question::{Known, Leaf, Target};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::modules::queries::ClassAnswers;

use super::QueryFunction;

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "name_classes", arity: 3, call: classes },
    QueryFunction { name: "module_exports", arity: 2, call: exports },
];

fn classes(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Target::Node(node) = at.target else { return Known::Unknown };
    let [Value::Str(name), source] = args else { return Known::Unknown };
    match source {
        Value::Null => class_answers(semantic, semantic.modules().binding_classes(node, name, class_mask(semantic))),
        Value::Str(source) => match semantic.world.module(at, source, semantic) {
            Known::Yes(Some(scope)) => class_answers(semantic, semantic.modules().export_classes(scope, name, class_mask(semantic))),
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
        let Known::Yes(classes) = class_answers(semantic, semantic.modules().export_classes(scope, &name, class_mask(semantic))) else { return Known::Unknown };
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

fn class_mask(semantic: &Semantic<'_>) -> u64 {
    let count = semantic.tables.vocabulary.declaration_classes.len();
    if count == 64 { u64::MAX } else { (1u64 << count) - 1 }
}

fn class_answers(semantic: &Semantic<'_>, classes: ClassAnswers) -> Known<Value<'static>> {
    let mut answers = Vec::new();
    for (bit, name) in semantic.tables.vocabulary.declaration_classes.iter().enumerate() {
        let class = 1u64 << bit;
        let value = if classes.known & class == 0 { Value::Null } else { Value::Bool(classes.present & class != 0) };
        if semantic.question.budget.exhausted() { return Known::Unknown }
        answers.push((name.clone().into(), value));
    }
    Known::Yes(Value::record(answers))
}

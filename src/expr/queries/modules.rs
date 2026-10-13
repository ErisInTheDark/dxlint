use crate::expr::Value;
use crate::grammar::names::question::{Known, Leaf, Target};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::modules::queries::ClassAnswers;

use super::QueryFunction;

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "name_classes", arity: 4, call: classes },
    QueryFunction { name: "module_exports", arity: 2, call: exports },
];

fn classes(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Target::Node(node) = at.target else { return Known::Unknown };
    let [Value::Str(name), source, filter] = args else { return Known::Unknown };
    let mask = match filter {
        Value::Null => class_mask(semantic),
        Value::List(classes) => {
            let mut mask = 0;
            for class in &**classes {
                let Value::Str(class) = class else { return Known::Unknown };
                let Some(bit) = semantic.tables.vocabulary.declaration_classes.iter().position(|name| name == &**class)
                    else { return Known::Unknown };
                mask |= 1u64 << bit;
            }
            mask
        }
        _ => return Known::Unknown,
    };
    match source {
        Value::Null => class_answers(semantic, semantic.modules().binding_classes(node, name, mask), mask),
        Value::Str(source) => match semantic.world.module(at, source, semantic) {
            Known::Yes(Some(scope)) => class_answers(semantic, semantic.modules().export_classes(scope, name, mask), mask),
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
        let mask = class_mask(semantic);
        let Known::Yes(classes) = class_answers(semantic, semantic.modules().export_classes(scope, &name, mask), mask) else { return Known::Unknown };
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

fn class_answers(semantic: &Semantic<'_>, classes: ClassAnswers, mask: u64) -> Known<Value<'static>> {
    let mut answers = Vec::new();
    for (bit, name) in semantic.tables.vocabulary.declaration_classes.iter().enumerate() {
        let class = 1u64 << bit;
        if mask & class == 0 { continue }
        let value = if classes.known & class == 0 { Value::Null } else { Value::Bool(classes.present & class != 0) };
        if semantic.question.budget.exhausted() { return Known::Unknown }
        answers.push((name.clone().into(), value));
    }
    Known::Yes(Value::record(answers))
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{lint, linter};

    #[test]
    fn class_filters_keep_selected_answers_and_reject_invalid_requests() {
        let linter = linter(r#"
            in .t match document /[\s\S]+/ { scope outer }
            in document match declaration name:/x/ /;/ {
                declares value name in nearest outer hoisted
            }
            in document match type_declaration name:/y/ /;/ {
                declares type name in nearest outer hoisted
            }
            in document match reference name:/x/ /!/ {
                resolves value name
                let selected name_classes(name.node, "x", null, ["value", "value"])
                let all_classes name_classes(name.node, "x", null, null)
                let empty name_classes(name.node, "x", null, [])
                rule selected {
                    when selected.value == true && count(keys(selected)) == 1
                    warn "selected"
                }
                rule all_classes {
                    when all_classes.value == true && count(keys(all_classes)) == 2
                    warn "all classes"
                }
                rule empty {
                    when count(keys(empty)) == 0
                    warn "empty"
                }
                rule invalid_class {
                    when name_classes(name.node, "x", null, ["missing"]) == null
                    warn "invalid class"
                }
                rule invalid_filter {
                    when name_classes(name.node, "x", null, 1) == null
                    warn "invalid filter"
                }
            }
        "#);
        let reports = lint(&linter, "x;y;x!");
        assert_eq!(reports.len(), 5, "{reports:#?}");
    }
}

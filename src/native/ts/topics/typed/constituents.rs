//! Ordered component relationships within union and intersection types.

use super::*;
use crate::fx_hash::FxHashMap;

#[derive(Default)]
struct Relationships<'s> {
    syntax: FxHashMap<(Option<u16>, Vec<&'s str>), Value<'static>>,
    types: Vec<(Ty, Value<'static>)>,
}

pub(super) fn bare(file: &NativeFile<'_>, mut node: u32) -> u32 {
    while file.syntax.is(node, Kind::ParenType) {
        let Some(inner) = file.syntax.get(node, Field::Type) else { break };
        node = inner;
    }
    node
}

pub(super) fn components(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(kind @ (Kind::Union | Kind::Intersection)) = s.kind(node) else { return Value::Null };
    let mut seen = Relationships::default();
    let mut components = Vec::new();
    let group = Value::Node(node, s.text(node).to_owned().into());
    for part in s.all(node, Field::Type) {
        collect(file, part, &group, kind, &mut seen, &mut components);
    }
    Value::list(components)
}

fn collect<'f>(file: &'f NativeFile<'_>, node: u32, group: &Value<'static>, kind: Kind, seen: &mut Relationships<'f>, components: &mut Vec<Value<'static>>) {
    let node = bare(file, node);
    let s = &file.syntax;
    let ty = file.type_of(node);
    let syntax = (s.kind(node).map(|kind| kind as u16), file.tokens(s.unparen(node)));
    let previous_syntax = seen.syntax.get(&syntax).cloned().unwrap_or(Value::Null);
    let previous_identity = seen.types.iter().find(|(previous, _)| ty.same_identity(previous))
        .map_or(Value::Null, |(_, node)| node.clone());
    let complete = !super::unsafe_types::missing_annotation(file, node);
    let node_link = Value::Node(node, s.text(node).to_owned().into());
    components.push(Value::record(vec![
        ("node".into(), node_link.clone()),
        ("group".into(), group.clone()),
        ("previous_syntax".into(), previous_syntax),
        ("previous_identity".into(), previous_identity),
        ("complete".into(), Value::Bool(complete)),
    ]));
    if complete {
        seen.syntax.entry(syntax).or_insert_with(|| node_link.clone());
        seen.types.push((ty, node_link.clone()));
    }
    if s.is(node, kind) {
        for part in s.all(node, Field::Type) {
            collect(file, part, &node_link, kind, seen, components);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use crate::expr::value::Value;

    #[test]
    fn component_relationships_keep_distinct_nested_and_unresolved_types() {
        with_native_file("type A = 'a'; type B = 'a'; type T = /*@*/A | B | (A | number) | Missing;", |file, nodes| {
            let result = super::components(file, nodes[0]);
            let parts = result.items();
            let find = |text: &str| parts.iter().find(|part|
                part.key("node").text() == text).expect("component, not only a finding");
            assert_eq!(find("A").key("complete"), Value::Bool(true));
            assert_eq!(find("B").key("previous_syntax"), Value::Null);
            assert_eq!(find("B").key("previous_identity").text(), "A");
            assert_eq!(find("number").key("previous_identity"), Value::Null);
            assert_eq!(find("Missing").key("complete"), Value::Bool(false));
            assert!(find("number").key("group").text().contains("A | number"));
        });
    }
}

use std::path::Path;

use dx_config::source::Disk;
use dx_ir::value::Value;

use crate::grammar::{Grammar, Tree};

fn grammar(source: &str) -> Grammar {
    let loaded = crate::config::load_text("shapes.dx", source, &Disk);
    assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
    let mut errors = Vec::new();
    let env = crate::expr::build_env();
    let grammar = Grammar::build(&loaded.config, &mut errors, &env);
    assert!(errors.is_empty(), "{errors:?}");
    grammar
}

fn validate(grammar: &Grammar, tree: &Tree) {
    for &id in &tree.order {
        let parent = tree.node(id).matcher;
        for edge in tree.children(id) {
            let child = tree.node(edge.node).matcher;
            assert!(grammar.children_kinds(parent).contains(&child), "{parent}: child {child}");
            assert!(grammar.parent_kinds(child).contains(&parent), "{child}: parent {parent}");
            let shape = grammar.field_targets(parent, edge.label);
            assert!(shape.any || if child == crate::grammar::TEXT { shape.text } else { shape.kinds.contains(&child) }, "{parent}.{}: child {child}", edge.label);
        }
        for label in grammar.matchers.get(parent as usize).map(|m| m.labels()).unwrap_or_default() {
            let label = grammar.labels.get(&label).unwrap();
            let shape = grammar.field_targets(parent, label);
            for child in tree.fields(id, label) {
                let child = tree.node(child).matcher;
                assert!(shape.any || if child == crate::grammar::TEXT { shape.text } else { shape.kinds.contains(&child) }, "{parent}.{label}: projected child {child}");
            }
        }
    }
}

#[test]
fn native_fields_survive_tokens_specialisations_and_pattern_grafts() {
    let grammar = grammar(r#"
import .ts
in ts_identifier match specialised like ts_identifier /x/
in ts_block match comment /\/\/[^\r\n]*/
in ts_call layer calls match call_text /f/
"#);
    for source in [
        "function f(x: number) { // note\n return x + 1; } f(2);",
        "class C<T> { x?: T; m(y: T): T { return y; } }",
        "const f = <T,>(x:T) => <div key={x}>{x}</div>;",
    ] {
        let front = &grammar.natives[0];
        let jsx = Value::Bool(true);
        let mut parsed = front.frontend.parse(source, Path::new("a.tsx"), &[("jsx", &jsx)]).unwrap();
        let every = vec![true; front.frontend.kinds().len()];
        front.frontend.attach_tokens(source, &mut parsed, &every);
        let nodes = front.remap(parsed.nodes);
        validate(&grammar, &Tree::from_native(nodes.clone(), parsed.root));
        validate(&grammar, &grammar.graft_tree(grammar.lang("js").unwrap(), source, nodes, parsed.root, None, false, None));
    }
    let specialised = grammar.matcher("specialised").unwrap();
    let name = grammar.labels.get("name").unwrap();
    let parameter = grammar.matcher("ts_parameter").unwrap();
    assert!(grammar.field_targets(parameter, name).kinds.contains(&specialised));
}

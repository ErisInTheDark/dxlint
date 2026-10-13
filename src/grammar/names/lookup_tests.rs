use crate::config::source::Disk;
use crate::expr::Value;
use crate::grammar::{Edge, Grammar, Node, Tree};

#[test]
fn outside_lookup_keeps_nested_locals_and_rejects_invalid_anchors() {
    let loaded = crate::config::load_text("lookup.zlint", r#"
        lang demo "*.demo"
        in .demo match name /x/
        in .demo match declaration name:name
        in .demo match closure parameter:declaration body:name
        in .demo match method parameter:declaration default:name decorator:name body:name closure:closure
        in .demo match root global:declaration method:method
        in root {
            scope block
            declares value global.name in nearest block hoisted
        }
        in method {
            scope function
            scope block
            declares value parameter.name in nearest function hoisted
            resolves value decorator outside node
        }
        in closure {
            scope function
            scope block
            declares value parameter.name in nearest function hoisted
            resolves value body outside parent.node
        }
        in name resolves value
    "#, &Disk);
    let mut errors = loaded.errors;
    let grammar = Grammar::build(&loaded.config, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    let matcher = |name| grammar.matcher(name).expect("matcher");
    let label = |name| grammar.labels.get(name).expect("field");
    let name = |start| Node { matcher: matcher("name"), start, end: start + 1, children: Vec::new() };
    let tree = Tree::from_native(vec![
        Node { matcher: matcher("root"), start: 0, end: 7, children: vec![
            Edge { label: label("global"), node: 1 }, Edge { label: label("method"), node: 3 },
        ] },
        Node { matcher: matcher("declaration"), start: 0, end: 1,
            children: vec![Edge { label: label("name"), node: 2 }] },
        name(0),
        Node { matcher: matcher("method"), start: 1, end: 7, children: vec![
            Edge { label: label("parameter"), node: 4 }, Edge { label: label("default"), node: 6 },
            Edge { label: label("decorator"), node: 7 }, Edge { label: label("body"), node: 8 },
            Edge { label: label("closure"), node: 9 },
        ] },
        Node { matcher: matcher("declaration"), start: 1, end: 2,
            children: vec![Edge { label: label("name"), node: 5 }] },
        name(1), name(2), name(3), name(4),
        Node { matcher: matcher("closure"), start: 5, end: 7, children: vec![
            Edge { label: label("parameter"), node: 10 }, Edge { label: label("body"), node: 12 },
        ] },
        Node { matcher: matcher("declaration"), start: 5, end: 6,
            children: vec![Edge { label: label("name"), node: 11 }] },
        name(5), name(6),
    ], 0);
    let text = "xxxxxxx";
    let class = grammar.names.vocabulary.declaration_class("value");
    for mode in 0..5 {
        let invalid = mode != 0;
        let resolved = grammar.names.resolve(&tree, text, &|name| name.into(), &|_, _| true,
            &|_, _, target| match mode {
                0 => Ok(Value::Node(3, "".into())),
                1 => Ok(Value::Node(5, "".into())),
                2 => Ok(Value::Node(target.unwrap(), "".into())),
                3 => Ok(Value::Null),
                _ => Err("read failed".into()),
            });
        assert_eq!(resolved.declaration.get(&6), Some(&5));
        assert_eq!(resolved.declaration.get(&8), Some(&5));
        if invalid {
            assert!(!resolved.failures.is_empty());
            assert!(!resolved.declaration.contains_key(&7));
            assert!(!resolved.declaration.contains_key(&12));
            assert_eq!(resolved.visible(&tree, text, &|name| name.into(), 7, "x", class), None);
        } else {
            assert!(resolved.failures.is_empty());
            assert_eq!(resolved.declaration.get(&7), Some(&2));
            assert_eq!(resolved.declaration.get(&12), Some(&11));
            assert_eq!(resolved.visible(&tree, text, &|name| name.into(), 7, "x", class), Some(2));
            assert_eq!(resolved.visible(&tree, text, &|name| name.into(), 12, "x", class), Some(11));
        }
    }
    let mut no_scopes = crate::grammar::names::Names { kinds: grammar.names.kinds.clone(), ..Default::default() };
    for kind in &mut no_scopes.kinds { kind.scopes.clear(); }
    let resolved = no_scopes.resolve(&tree, text, &|name| name.into(), &|_, _| true,
        &|_, _, _| Ok(Value::Node(3, "".into())));
    assert!(!resolved.failures.is_empty(), "an outside lookup with no active scopes must fail");
    assert!(resolved.declaration.is_empty());
}

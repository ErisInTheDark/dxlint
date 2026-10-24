use crate::config::source::Disk;
use crate::{config, engine};
use super::{Declare, KindNames, Names, OpenScope, Resolve};
use crate::grammar::{Edge, Node, Tree};

#[test]
fn keyed_operands_bind_targets_without_losing_the_declaring_items_fields() {
    let source = r#"
        lang t "*.t"
        in .t match holder key:/\w+/ {
            scope block
            declares value key keyed key.text it.text in nearest block
            resolves value keyed it.text
        }
    "#;
    let built = engine::build(
        config::load_text("keyed.dx", source, &Disk),
        std::path::PathBuf::from("/root"),
    );
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    assert!(built.linter.is_some());
}

#[test]
fn captured_declarations_and_uses_share_the_real_name_and_binding_owner() {
    use crate::grammar::parse::TEXT;
    use crate::pattern::compile::NO_LABEL;
    let tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 2, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 4 }] },
        Node { matcher: 1, start: 0, end: 1, children: vec![Edge { label: 0, node: 2 }] },
        Node { matcher: TEXT, start: 0, end: 1, children: vec![Edge { label: NO_LABEL, node: 3 }] },
        Node { matcher: 3, start: 0, end: 1, children: Vec::new() },
        Node { matcher: 2, start: 1, end: 2, children: vec![Edge { label: 0, node: 5 }] },
        Node { matcher: TEXT, start: 1, end: 2, children: vec![Edge { label: NO_LABEL, node: 6 }] },
        Node { matcher: 3, start: 1, end: 2, children: Vec::new() },
    ], 0);
    let mut names = Names { kinds: vec![KindNames::default(); 4], ..Names::default() };
    names.kinds[0].scopes.push(OpenScope { classes: 1, within: Vec::new(), when: None });
    names.kinds[1].declares.push(Declare {
        classes: 1, path: vec![0], nearest: 1, hoisted: true, enclosing: false,
        implicit: None, when: None, key: None, filter: None,
    });
    names.kinds[2].resolves.push(Resolve { classes: 1, path: vec![0], when: None, key: None, outside: None });
    let resolved = names.resolve(&tree, "xx", &|key| key.into(), &|_, _| true, &|_, _, _| Ok(crate::expr::Value::Null));
    assert_eq!(resolved.declaration.get(&6), Some(&3));
    assert_eq!(resolved.bindings.get(&3), Some(&1));
    assert_eq!(resolved.declared.get(&3).map(|entry| entry.1), Some(1));
    assert!(resolved.failures.is_empty());

    names.kinds[3].targets.push((vec![vec![super::IT]], None));
    let failed = names.resolve(&tree, "xx", &|key| key.into(), &|_, _| true, &|_, _, _| Ok(crate::expr::Value::Null));
    assert!(failed.declared.is_empty());
    assert!(failed.declaration.is_empty());
    assert_eq!(failed.failures.iter().map(|(owner, _)| *owner).collect::<Vec<_>>(), vec![1, 4]);
    assert_eq!(names.targets_of(&tree, 2, &|_, _| super::question::Known::Yes(true)), super::question::Known::Unknown);
}

#[test]
fn merged_bindings_keep_class_specific_declarations_and_references() {
    let tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 4, children: vec![
            Edge { label: 0, node: 1 }, Edge { label: 0, node: 3 },
            Edge { label: 0, node: 5 }, Edge { label: 0, node: 7 },
        ] },
        Node { matcher: 1, start: 0, end: 1, children: vec![Edge { label: 0, node: 2 }] },
        Node { matcher: 5, start: 0, end: 1, children: vec![] },
        Node { matcher: 2, start: 1, end: 2, children: vec![Edge { label: 0, node: 4 }] },
        Node { matcher: 5, start: 1, end: 2, children: vec![] },
        Node { matcher: 3, start: 2, end: 3, children: vec![Edge { label: 0, node: 6 }] },
        Node { matcher: 5, start: 2, end: 3, children: vec![] },
        Node { matcher: 4, start: 3, end: 4, children: vec![Edge { label: 0, node: 8 }] },
        Node { matcher: 5, start: 3, end: 4, children: vec![] },
    ], 0);
    let mut names = Names { kinds: vec![KindNames::default(); 6], ..Names::default() };
    names.kinds[0].scopes.push(OpenScope { classes: 1, within: vec![], when: None });
    for (kind, classes) in [(1, 1), (2, 2)] {
        names.kinds[kind].declares.push(Declare {
            classes, path: vec![0], nearest: 1, hoisted: true, enclosing: false,
            implicit: None, when: None, key: None, filter: None,
        });
    }
    for (kind, classes) in [(3, 1), (4, 2)] {
        names.kinds[kind].resolves.push(Resolve { classes, path: vec![0], when: None, key: None, outside: None });
    }
    let resolved = names.resolve(&tree, "xxxx", &|key| key.into(), &|_, _| true,
        &|_, _, _| Ok(crate::expr::Value::Null));
    assert_eq!(resolved.merged.get(&4), Some(&2));
    assert!(!resolved.declaration.contains_key(&4));
    assert_eq!(resolved.declaration.get(&6), Some(&2));
    assert_eq!(resolved.declaration.get(&8), Some(&4));
    assert_eq!(resolved.references.get(&2), Some(&vec![6]));
    assert_eq!(resolved.references.get(&4), Some(&vec![8]));
    assert!(resolved.failures.is_empty());

    names.kinds[1].resolves.push(Resolve { classes: 2, path: vec![0], when: None, key: None, outside: None });
    let dual = names.resolve(&tree, "xxxx", &|key| key.into(), &|_, _| true,
        &|_, _, _| Ok(crate::expr::Value::Null));
    assert_eq!(dual.declaration.get(&2), Some(&4));
    assert_eq!(dual.declaration_in(2, 1), 2);
    assert_eq!(dual.declaration_in(2, 2), 4);
    assert_eq!(dual.declaration_in(6, 1), 2);
    assert_eq!(dual.declaration_in(8, 2), 4);
}

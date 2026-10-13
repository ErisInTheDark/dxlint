use super::{Declare, KindNames, Names, OpenScope};
use crate::expr::Value;
use crate::grammar::{Edge, Node, Tree};
use std::cell::RefCell;

fn declarations() -> (Names, Tree) {
    let mut kinds = vec![KindNames::default(); 4];
    kinds[0].scopes.push(OpenScope { classes: 1, within: Vec::new(), when: None });
    kinds[1].declares.push(Declare {
        classes: 1, path: vec![0], nearest: 1, hoisted: true, enclosing: false,
        implicit: None, when: None, key: None, filter: Some(0),
    });
    kinds[2].targets.push((vec![vec![1]], None));
    let tree = Tree::from_native(vec![
        Node { matcher: 0, start: 0, end: 2, children: vec![Edge { label: 0, node: 1 }] },
        Node { matcher: 1, start: 0, end: 2, children: vec![Edge { label: 0, node: 2 }, Edge { label: 0, node: 3 }] },
        Node { matcher: 2, start: 0, end: 1, children: vec![Edge { label: 1, node: 4 }] },
        Node { matcher: 2, start: 1, end: 2, children: vec![Edge { label: 1, node: 5 }] },
        Node { matcher: 3, start: 0, end: 1, children: Vec::new() },
        Node { matcher: 3, start: 1, end: 2, children: Vec::new() },
    ], 0);
    (Names { kinds, ..Names::default() }, tree)
}

#[test]
fn filters_choose_classes_after_targets_expand_without_changing_the_declaring_owner() {
    let (mut names, tree) = declarations();
    let mut types = names.kinds[1].declares[0].clone();
    types.classes = 2;
    types.filter = None;
    names.kinds[1].declares.push(types);
    let calls = RefCell::new(Vec::new());
    let resolved = names.resolve(&tree, "ab", &|key| key.into(), &|_, _| true, &|owner, _, target| {
        calls.borrow_mut().push((owner, target));
        Ok(Value::Bool(target == Some(4)))
    });
    assert_eq!(*calls.borrow(), vec![(1, Some(4)), (1, Some(5))]);
    assert_eq!(resolved.declared.get(&4).map(|entry| (entry.1, entry.2)), Some((1, 3)));
    assert_eq!(resolved.declared.get(&5).map(|entry| (entry.1, entry.2)), Some((1, 2)));
    assert!(resolved.failures.is_empty());
}

#[test]
fn failed_or_nonboolean_filters_skip_targets_and_report_the_declaring_owner() {
    let (names, tree) = declarations();
    let resolved = names.resolve(&tree, "ab", &|key| key.into(), &|_, _| true, &|_, _, target| {
        if target == Some(4) { Err("filter failed".into()) } else { Ok(Value::Null) }
    });
    assert!(resolved.declared.is_empty());
    assert_eq!(resolved.failures.len(), 2);
    assert!(resolved.failures.iter().all(|(owner, _)| *owner == 1));
}

#[test]
fn implicit_declarations_use_the_same_boolean_filter_contract() {
    for (value, declared, failed) in [
        (Value::Bool(true), true, false),
        (Value::Bool(false), false, false),
        (Value::Null, false, true),
    ] {
        let (mut names, tree) = declarations();
        names.kinds[1].declares[0].implicit = Some("implicit".into());
        names.kinds[1].declares[0].path.clear();
        let resolved = names.resolve(&tree, "ab", &|key| key.into(), &|_, _| true, &|owner, _, target| {
            assert_eq!(Some(owner), target);
            Ok(value.clone())
        });
        assert_eq!(resolved.declared.contains_key(&1), declared);
        assert_eq!(!resolved.failures.is_empty(), failed);
    }
}

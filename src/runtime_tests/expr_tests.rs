//! Expression evaluation tests: node selection and walking, `fn` binding and
//! evaluation, indent recording, and static evaluation.

use std::path::Path;

use crate::config::ast::Severity;
use crate::engine::tests::{lint, linter, native_linter};
use crate::expr::selection;
use crate::grammar::{Edge, Node, Tree, ROOT, TEXT};

#[test]
fn indexed_descendants_preserve_order_fields_and_structural_membership() {
    let linter = crate::engine::tests::linter("match base \"b\"\nmatch specialised like base \"s\"\nmatch other \"o\"");
    let grammar = &linter.grammar;
    let base = grammar.matcher("base").unwrap();
    let specialised = grammar.matcher("specialised").unwrap();
    let other = grammar.matcher("other").unwrap();
    let edge = |node, label| Edge { node, label };
    let node = |matcher, start, end, children| Node { matcher, start, end, children };
    let tree = Tree::from_native(vec![
        node(TEXT, 0, 1, vec![]),
        node(specialised, 0, 4, vec![edge(0, 3)]),
        node(base, 0, 4, vec![]),
        node(other, 0, 10, vec![edge(2, 4), edge(1, 4), edge(6, 4)]),
        node(base, 2, 3, vec![]),
        node(ROOT, 0, 10, vec![edge(3, 1), edge(4, 1)]),
        node(base, 30, 31, vec![]),
        node(base, 0, 1, vec![]),
    ], 5);
    let selectors = [
        selection(grammar, &[base], None),
        selection(grammar, &[base, other, specialised], None),
        selection(grammar, &[base], Some(3)),
        selection(grammar, &[other], Some(4)),
    ];
    for selector in &selectors {
        for id in 0..tree.nodes.len() as u32 {
            assert_eq!(selector.descendants(&tree, id).collect::<Vec<_>>(), selector.walk(&tree, id));
        }
    }
    assert_eq!(selectors[0].descendants(&tree, 3).collect::<Vec<_>>(), [1, 2, 6]);
    assert_eq!(selectors[2].descendants(&tree, 3).collect::<Vec<_>>(), [0]);
    assert_eq!(selectors[1].descendants(&tree, 5).collect::<Vec<_>>(), [3, 1, 2, 4, 6]);
}

#[test]
fn shared_edges_and_nested_files_keep_walk_semantics() {
    let linter = crate::engine::tests::linter("match item \"i\"");
    let item = linter.grammar.matcher("item").unwrap();
    let edge = |node, label| Edge { node, label };
    let node = |matcher, start, end, children| Node { matcher, start, end, children };
    let tree = Tree::from_native(vec![
        node(item, 1, 2, vec![]),
        node(item, 0, 3, vec![edge(0, 1), edge(0, 2)]),
        node(item, 4, 6, vec![edge(0, 3)]),
        node(ROOT, 7, 9, vec![edge(4, 1)]),
        node(item, 7, 8, vec![]),
        node(ROOT, 0, 10, vec![edge(1, 1), edge(2, 1), edge(3, 1)]),
    ], 5);
    let selector = selection(&linter.grammar, &[item], None);
    for id in 0..tree.nodes.len() as u32 {
        assert_eq!(selector.descendants(&tree, id).collect::<Vec<_>>(), selector.walk(&tree, id));
    }
    assert_eq!(selector.descendants(&tree, 1).collect::<Vec<_>>(), [0, 0]);
    assert_eq!(selector.descendants(&tree, 2).collect::<Vec<_>>(), [0]);
    assert_eq!(selector.descendants(&tree, 3).collect::<Vec<_>>(), [4]);
    assert!(!selector.descendants(&tree, 5).any(|id| id == 4));
}

#[test]
fn first_use_bindings_keep_vm_field_coercion_and_parameter_frames() {
    use crate::expr::fold::bind;
    use crate::expr::pipeline::Stage;
    use crate::expr::{CExpr, Each, Value};
    let indexed = CExpr::Each(Box::new(Stage {
        kind: Each::Map, depth: 1, arity: 2, input: CExpr::Param(0), args: vec![],
        body: CExpr::Binary(crate::config::ast::BinOp::Add, Box::new(CExpr::Param(1)), Box::new(CExpr::Param(2))),
    }));
    let body = bind(CExpr::Const(Value::list(vec![Value::Num(10.0), Value::Num(20.0)])), 0, indexed);
    assert_eq!(crate::expr::Ctx::eval_static(&body, &[]).unwrap(), Value::list(vec![Value::Num(10.0), Value::Num(21.0)]));
    let errors = linter(r#"
        fn second(a, b) = b
        fn frames(seed) = {
            value seed
            map (item, i) { value item; then (n) n + i }
            sum
        }
        in .t match word /[a-z]+/ {
            rule first_error warn { value num(text); second (1 % 0) }
            rule lazy_error warn { value num(text); then (n) if true then "unused" else n }
            rule foreign_error warn { value num(text); then (n) text }
            rule frames warn "{frames([10, 20])}"
        }
    "#);
    let reports = lint(&errors, "a");
    let failures: Vec<_> = reports.iter().filter(|r| r.rule.ends_with("error")).collect();
    assert_eq!(failures.len(), 3);
    assert!(failures.iter().all(|r| r.message.contains("num:")));
    assert_eq!(reports.iter().find(|r| r.rule == "frames").unwrap().message, "31");
}

#[test]
fn hits_do_not_cross_rules_nodes_or_files() {
    let l = native_linter(r#"
        in ts_line rule a info "{indent_level}:{len(indent_anchor)}" { indent in ts_block by 2 }
        in ts_line rule b info "{indent_level}:{len(indent_anchor)}" { indent in ts_block by 5 }
    "#);
    for (path, indent) in [("/root/a.ts", 2), ("/root/b.ts", 6), ("/root/a.ts", 0)] {
        let source = format!("{}{{\n  action();\n}}\n", " ".repeat(indent));
        let got = l.lint(Path::new(path), &source);
        let line: Vec<_> = got.iter().filter(|d| d.start == source.find("  action").unwrap()).collect();
        assert_eq!(line.len(), 2);
        assert_eq!(line[0].message, format!("2:{indent}"));
        assert_eq!(line[1].message, format!("5:{indent}"));
        assert!(got.iter().all(|d| d.severity != Severity::Error));
    }
}

#[test]
fn nodes_keys_read_records_but_project_syntax_fields() {
    let linter = crate::engine::tests::linter(r#"
        fn entries(value) = value.nodes
        fn children(value) = value.element.nodes
        in .t match item /[ab]/
        in .t match bag "[" (element:item)* "]" {
            rule record warn "record" {
                let result { nodes: [1, 2] }
                when (len(entries(result)) == 2) && (len(result.nodes) == 2)
            }
            rule nodes warn "nodes" {
                when (len(element.nodes) == 2) && all(element.nodes, n => n is item)
            }
            rule broad warn "broad" {
                let projected children(node)
                when (len(projected) == 2) && any([node], n => len(n.element.nodes) == 2)
            }
            rule absent warn "absent" { when len(element.nodes) == 0 }
        }
    "#);
    let reports = crate::engine::tests::lint(&linter, "[ab] []");
    assert_eq!(reports.iter().filter(|report| report.rule == "record").count(), 2);
    assert_eq!(reports.iter().filter(|report| report.rule == "nodes").count(), 1, "{reports:?}");
    assert_eq!(reports.iter().filter(|report| report.rule == "broad").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "absent").count(), 1);
}

#[test]
fn node_properties_prefer_syntax_fields_but_keep_attributes_on_other_nodes() {
    let linter = crate::engine::tests::linter(r#"
        fn declarations(value) = value.declaration.nodes
        in .t match item /[ab]/
        in .t match bag "[" (declaration:item)* "]"
        in .t match marker "!"
        in item, bag, marker let declaration "attribute"
        in bag rule field {
            when len(declarations(node)) == 2 && all(declarations(node), child => child is item)
            warn "field"
        }
        in marker rule attribute {
            when node.declaration == "attribute"
            warn "attribute"
        }
        in item rule parent_field {
            when len(parent.declaration.nodes) == 2 && len(declarations(node.parent)) == 2
            warn "parent"
        }
    "#);
    let reports = crate::engine::tests::lint(&linter, "[ab] !");
    assert_eq!((
        reports.iter().filter(|report| report.rule == "field").count(),
        reports.iter().filter(|report| report.rule == "attribute").count(),
        reports.iter().filter(|report| report.rule == "parent_field").count(),
    ), (1, 1, 2));
}

#[test]
fn pipeline_labels_do_not_leak_or_resolve_before_their_step() {
    for body in [
        "let result { value 1; stored: then (value) value }\nwhen stored == 1",
        "let result { value future; future: then (value) value }",
        "let result { value stored: 1; stored: then (value) value }",
    ] {
        assert!(!crate::engine::tests::errors(&format!("in .t match word /[a-z]+/ {{ rule scoped {{ {body}\nwarn \"invalid\" }} }}")).is_empty());
    }
}

#[test]
fn static_booleans_match_runtime_truth_tables_and_skip_dead_errors() {
    use crate::config::ast::BinOp;
    use crate::expr::{CExpr, Ctx, Value};
    let boolean = |value| Box::new(CExpr::Const(Value::Bool(value)));
    for a in [false, true] {
        for b in [false, true] {
            for (op, expected) in [(BinOp::And, a && b), (BinOp::Or, a || b)] {
                let result = Ctx::eval_static(&CExpr::Binary(op, boolean(a), boolean(b)), &[]).unwrap();
                assert_eq!(result, Value::Bool(expected));
            }
        }
    }
    for (op, first) in [(BinOp::And, false), (BinOp::Or, true)] {
        let failure = CExpr::Binary(BinOp::Rem, Box::new(CExpr::Const(Value::Num(1.0))), Box::new(CExpr::Const(Value::Num(0.0))));
        assert_eq!(Ctx::eval_static(&CExpr::Binary(op, boolean(first), Box::new(failure)), &[]).unwrap(), Value::Bool(first));
    }
}

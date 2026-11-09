//! VM evaluation tests that need a linter, attributes or a test host.

use std::path::Path;

use crate::config::ast::BinOp;
use crate::expr::{functions, selection, FactOf, NodeSel};
use crate::grammar::{Edge, Grammar, Node, Tree, ROOT};
use crate::line_index::LineIndex;
use crate::vm::{eval, intern, Binder, EachKind, FileVm, Host, Seq, SeqStage, V, VExpr, Values};

#[test]
fn compound_call_arguments_keep_values_and_lambda_bindings() {
    for condition in [
        r#"upper(lower(text)) == "AB""#,
        r#"twice(len(lower(text))) == 4"#,
        r#"join(map([text], item => upper(lower(item))), ",") == "AB""#,
        r#"upper(if len(text) == 2 then lower(text) else "wrong") == "AB""#,
        r#"upper("{lower(text)}") == "AB""#,
        r#"upper({ value text; lower }) == "AB""#,
        r#"join({ value [text]; concat [lower(text)] }, ",") == "Ab,ab""#,
        r#"pick({ value: upper(lower(text)) }) == "AB""#,
    ] {
        let linter = crate::engine::tests::linter(&format!(
            "fn twice(value) = value + value\nfn pick(value) = value.value\nin .t match item /[A-Za-z]+/ {{ rule result warn \"result\" {{ when {condition} }} }}",
        ));
        assert_eq!(crate::engine::tests::lint(&linter, "Ab").len(), 1, "{condition}");
        assert_eq!(crate::engine::tests::lint(&linter, "x").len(), 0, "{condition}");
    }
}

#[test]
fn lowered_attributes_keep_cycles_and_eager_argument_errors_visible() {
    use crate::engine::tests::native_linter;
    let config = r#"
        in ts_node let cyclic cyclic
        in ts_return rule cycle info "{cyclic}"
        fn lazy_pick(flag, x) = if flag then 0 else x
        in ts_identifier let text_number num(text)
        in ts_identifier rule lazy_arg info "{lazy_pick(true, text_number)}"
        fn identity(value) = value
        fn depth(n, value) = if n == 0 then { value value; num; identity } else depth(n - 1, value)
        in ts_identifier rule depth_arg info "{depth(255, text)}"
    "#;
    let diagnostics = native_linter(config).lint(Path::new("/root/a.ts"), "function f(a) { return a; return a; }");
    assert!(diagnostics.iter().any(|d| d.rule == "cycle" && d.severity == crate::config::ast::Severity::Error));
    let failed_arguments: Vec<_> = diagnostics.iter().filter(|d| d.rule == "lazy_arg").collect();
    assert!(!failed_arguments.is_empty());
    assert!(failed_arguments.iter().all(|d| d.severity == crate::config::ast::Severity::Error));
    let depth_arguments: Vec<_> = diagnostics.iter().filter(|d| d.rule == "depth_arg").collect();
    assert_eq!(depth_arguments.len(), failed_arguments.len());
    for (deep, shallow) in depth_arguments.iter().zip(failed_arguments) {
        assert_eq!(deep.message.split_once("failed here:").map(|(_, cause)| cause), shallow.message.split_once("failed here:").map(|(_, cause)| cause));
    }
}

#[test]
fn raw_descendant_reducers_stop_without_hiding_pipeline_errors() {
    struct CountingHost<'a> {
        grammar: &'a Grammar,
        tree: Tree,
        lines: LineIndex,
        values: Values,
        file: FileVm,
        yielded: std::cell::Cell<usize>,
    }
    impl Host for CountingHost<'_> {
        fn tree(&self) -> &Tree { &self.tree }
        fn grammar(&self) -> &Grammar { self.grammar }
        fn source(&self) -> &str { "abc" }
        fn lines(&self) -> &LineIndex { &self.lines }
        fn values(&self) -> &Values { &self.values }
        fn file_vm(&self) -> Option<&FileVm> { Some(&self.file) }
        fn attr(&self, _: u32, _: u16) -> Option<V> { None }
        fn fact(&self, _: u32, _: usize, _: FactOf) -> Option<V> { None }
        fn descendants<'s>(&'s self, node: u32, sel: &NodeSel) -> impl Iterator<Item = u32> + 's {
            sel.descendants(&self.tree, node).inspect(|_| self.yielded.set(self.yielded.get() + 1))
        }
    }
    let linter = crate::engine::tests::linter("match item \"i\"");
    let item = linter.grammar.matcher("item").unwrap();
    let host = CountingHost {
        grammar: &linter.grammar,
        tree: Tree::from_native(vec![
            Node { matcher: item, start: 0, end: 1, children: vec![] },
            Node { matcher: item, start: 1, end: 2, children: vec![] },
            Node { matcher: item, start: 2, end: 3, children: vec![] },
            Node { matcher: ROOT, start: 0, end: 3, children: vec![
                Edge { node: 0, label: 1 }, Edge { node: 1, label: 1 }, Edge { node: 2, label: 1 },
            ] },
        ], 3),
        lines: LineIndex::new("abc"),
        values: Values::default(),
        file: FileVm::default(),
        yielded: Default::default(),
    };
    let source = || Box::new(Seq::Descendants(selection(host.grammar, &[item], None)));
    let bind = Binder { slot: 0, indexed: false };
    let first = VExpr::First(source());
    assert_eq!(eval(&host, 3, &first).unwrap().value, V::Node(0));
    assert_eq!(host.yielded.replace(0), 1);
    let any = |source| VExpr::Each(EachKind::Any, source, Box::new(VExpr::Const(V::Bool(true))), bind);
    assert_eq!(eval(&host, 3, &any(source())).unwrap().value, V::Bool(true));
    assert_eq!(host.yielded.replace(0), 1);
    let indexed = VExpr::Each(EachKind::Any, source(), Box::new(VExpr::Binary(
        BinOp::Eq, Box::new(VExpr::Bound(1)), Box::new(VExpr::Const(V::Num(1.0))),
    )), Binder { slot: 0, indexed: true });
    assert_eq!(eval(&host, 3, &indexed).unwrap().value, V::Bool(true));
    assert_eq!(host.yielded.replace(0), 2);
    let never = VExpr::Each(EachKind::Any, source(), Box::new(VExpr::Const(V::Bool(false))), bind);
    assert_eq!(eval(&host, 3, &never).unwrap().value, V::Bool(false));
    assert_eq!(host.yielded.replace(0), 3);
    let error = || VExpr::Call(functions::lookup("num").unwrap(), vec![VExpr::Const(V::Str(intern("bad")))].into());
    let failed = VExpr::Each(EachKind::Any, source(), Box::new(error()), bind);
    assert!(eval(&host, 3, &failed).is_none());
    assert_eq!(host.yielded.replace(0), 1);
    assert_eq!(eval(&host, 3, &VExpr::Count(source())).unwrap().value, V::Num(3.0));
    assert_eq!(host.yielded.replace(0), 3);
    let empty = Box::new(Seq::Descendants(selection(host.grammar, &[], None)));
    assert_eq!(eval(&host, 3, &VExpr::First(empty)).unwrap().value, V::Null);
    let mapped = || Box::new(Seq::Stage(SeqStage::Map, source(), Box::new(VExpr::If(
        Box::new(VExpr::Binary(BinOp::Eq, Box::new(VExpr::Bound(0)), Box::new(VExpr::Const(V::Node(1))))),
        Box::new(VExpr::Call(functions::lookup("num").unwrap(), vec![VExpr::Const(V::Str(intern("bad")))].into())),
        Box::new(VExpr::Const(V::Num(1.0))),
    )), bind));
    // A decided answer never computes later items (their errors
    // included); consumers that need every item still fail on them.
    assert_eq!(eval(&host, 3, &VExpr::First(mapped())).unwrap().value, V::Num(1.0));
    assert_eq!(eval(&host, 3, &any(mapped())).unwrap().value, V::Bool(true));
    assert!(eval(&host, 3, &VExpr::Count(mapped())).is_none());
    let walked = || Box::new(Seq::Walk(source(), Box::new(Seq::Value(Box::new(VExpr::If(
        Box::new(VExpr::Binary(BinOp::Eq, Box::new(VExpr::Bound(0)), Box::new(VExpr::Const(V::Node(1))))),
        Box::new(error()), Box::new(VExpr::Const(V::Null)),
    )))), 0));
    assert_eq!(eval(&host, 3, &VExpr::First(walked())).unwrap().value, V::Node(0));
    assert_eq!(eval(&host, 3, &any(walked())).unwrap().value, V::Bool(true));
    assert!(eval(&host, 3, &VExpr::Count(walked())).is_none());
}

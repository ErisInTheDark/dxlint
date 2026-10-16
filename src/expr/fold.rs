//! Rule expressions specialised against their final, immutable options.

use super::{CExpr, Value};
use crate::config::ast::BinOp;
use crate::grammar::UserFn;

pub(super) fn bind(value: CExpr, depth: u16, mut body: CExpr) -> CExpr {
    fn uses(expr: &CExpr, depth: u16) -> usize {
        let mut count = usize::from(matches!(expr, CExpr::Param(i) if *i == depth));
        expr.for_each_child(&mut |child| count += uses(child, depth));
        count
    }
    fn first(expr: &CExpr, depth: u16) -> bool {
        match expr {
            CExpr::Param(i) => *i == depth,
            CExpr::Call(_, args) | CExpr::UserCall(_, args) => args.first().is_some_and(|arg| first(arg, depth)),
            CExpr::Each(stage) => first(&stage.input, depth),
            CExpr::Bind(value, _, _) | CExpr::Binary(_, value, _) | CExpr::Nullish(value, _) => first(value, depth),
            CExpr::Not(value) | CExpr::Neg(value) | CExpr::NonEmpty(value) | CExpr::Key(value, _) => first(value, depth),
            _ => false,
        }
    }
    fn replace(expr: &mut CExpr, depth: u16, value: &mut Option<CExpr>) {
        match expr {
            CExpr::Param(i) if *i == depth => {
                *expr = value.take().expect("one binding use");
                return;
            }
            CExpr::Param(i) | CExpr::Bind(_, i, _) | CExpr::Walk(_, i, _) if *i > depth => *i -= 1,
            CExpr::Each(stage) if stage.depth > depth => stage.depth -= 1,
            _ => {}
        }
        expr.for_each_child_mut(&mut |child| replace(child, depth, value));
    }
    // Only the first eager use can replace a frame without moving evaluation,
    // duplicating it, or evaluating on a different node or lambda invocation.
    if uses(&body, depth) == 1 && first(&body, depth) {
        replace(&mut body, depth, &mut Some(value));
        body
    } else {
        CExpr::Bind(Box::new(value), depth, Box::new(body))
    }
}

pub(crate) fn specialise(expr: &mut CExpr, options: &[Value<'static>], functions: &[UserFn]) {
    expr.for_each_child_mut(&mut |child| specialise(child, options, functions));

    // Only evaluate known value-only forms. A failed operation remains a
    // runtime expression, including in unvisited branches and dormant rules.
    let constant = |expr: &CExpr| matches!(expr, CExpr::Const(_));
    let eligible = match expr {
        CExpr::Opt(_) => true,
        CExpr::UserCall(_, _) => true,
        CExpr::Bind(value, _, _) => constant(value),
        CExpr::Each(stage) => constant(&stage.input) && stage.args.iter().all(constant),
        CExpr::Template(items) | CExpr::List(items) | CExpr::Call(_, items) => items.iter().all(constant),
        CExpr::Record(entries) => entries.iter().all(|(_, value)| constant(value)),
        CExpr::Hole(inner, _) | CExpr::Not(inner) | CExpr::Neg(inner) | CExpr::NonEmpty(inner)
        | CExpr::Key(inner, _) | CExpr::Member(inner, _, _) | CExpr::Path(inner, _, _) => constant(inner),
        CExpr::If(condition, yes, no) => match condition.as_ref() {
            CExpr::Const(value) => constant(if value.truthy() { yes } else { no }),
            _ => false,
        },
        CExpr::Nullish(left, right) => match left.as_ref() {
            CExpr::Const(value) => !matches!(value, Value::Null) || constant(right),
            _ => false,
        },
        CExpr::Binary(op, left, right) => constant(left) && (constant(right) || match left.as_ref() {
            CExpr::Const(value) => matches!(op, BinOp::And) && !value.truthy() || matches!(op, BinOp::Or) && value.truthy(),
            _ => false,
        }),
        _ => false,
    };
    if eligible {
        if let Ok(value) = super::static_eval::StaticEval::new(options, functions).eval(expr) {
            *expr = CExpr::Const(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{lint, linter};

    #[test]
    fn first_use_bindings_keep_vm_field_coercion_and_parameter_frames() {
        use super::{bind, CExpr, Value};
        use crate::expr::Each;
        use crate::expr::pipeline::Stage;
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
}

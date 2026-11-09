//! Rule expressions specialised against their final, immutable options.

use super::{CExpr, Value};
use crate::config::ast::BinOp;
use crate::grammar::UserFn;

pub fn bind(value: CExpr, depth: u16, mut body: CExpr) -> CExpr {
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

pub fn specialise(expr: &mut CExpr, options: &[Value<'static>], functions: &[UserFn]) {
    expr.for_each_child_mut(&mut |child| specialise(child, options, functions));

    // Only evaluate known value-only forms. A failed operation remains a
    // runtime expression, including in unvisited branches and dormant rules.
    let constant = |expr: &CExpr| matches!(expr, CExpr::Const(_));
    let eligible = match expr {
        // Copies that read options at runtime specialise against none.
        CExpr::Opt(i) => *i < options.len(),
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

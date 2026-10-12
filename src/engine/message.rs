use crate::expr::{CExpr, Value};
use crate::grammar::UserFn;

/// Message branches remain structural even when their options are constant.
/// Other expressions still receive ordinary option specialisation.
pub(super) enum Plan {
    Leaf(usize),
    If(Box<Plan>, Box<Plan>),
    Nullish(Box<Plan>, Box<Plan>),
    Bind(Box<Plan>),
    Hole(Box<Plan>),
}

impl Plan {
    pub(super) fn new(expr: &mut CExpr, options: &[Value<'static>], functions: &[UserFn]) -> Self {
        fn build(expr: &mut CExpr, next: &mut usize, options: &[Value<'static>], functions: &[UserFn]) -> Plan {
            let fold = |expr: &mut CExpr| crate::expr::fold::specialise(expr, options, functions);
            match expr {
                CExpr::If(condition, yes, no) => {
                    fold(condition);
                    Plan::If(Box::new(build(yes, next, options, functions)), Box::new(build(no, next, options, functions)))
                }
                CExpr::Nullish(left, right) => Plan::Nullish(
                    Box::new(build(left, next, options, functions)), Box::new(build(right, next, options, functions))),
                CExpr::Bind(value, _, body) => {
                    fold(value);
                    Plan::Bind(Box::new(build(body, next, options, functions)))
                }
                CExpr::Hole(inner, _) => Plan::Hole(Box::new(build(inner, next, options, functions))),
                _ => {
                    fold(expr);
                    let index = *next;
                    *next += 1;
                    Plan::Leaf(index)
                }
            }
        }
        build(expr, &mut 0, options, functions)
    }

    /// The message as one expression giving `[index, text]`, for the VM.
    /// `depth`: the first free parameter slot (after the rule's `each` item).
    pub(super) fn indexed(&self, expr: &CExpr, depth: u16) -> CExpr {
        let function = |name: &str| crate::expr::functions::FUNCTIONS.iter().find(|f| f.name == name).expect("built-in function");
        match (self, expr) {
            (Self::Leaf(index), expr) => CExpr::List(vec![CExpr::Const(Value::Num(*index as f64)), expr.clone()]),
            (Self::If(yes, no), CExpr::If(condition, a, b)) => CExpr::If(
                condition.clone(), Box::new(yes.indexed(a, depth)), Box::new(no.indexed(b, depth))),
            // `a ?? b`: a's pair, unless its text is null.
            (Self::Nullish(left, right), CExpr::Nullish(a, b)) => {
                let mut right = right.indexed(b, depth);
                shift_params(&mut right, depth);
                let text = CExpr::Call(function("last"), vec![CExpr::Param(depth)]);
                let null = CExpr::Binary(crate::config::ast::BinOp::Eq,
                    Box::new(CExpr::Call(function("type_of"), vec![text])), Box::new(CExpr::Const(Value::from("null"))));
                CExpr::Bind(Box::new(left.indexed(a, depth)), depth, Box::new(CExpr::If(Box::new(null), Box::new(right), Box::new(CExpr::Param(depth)))))
            }
            (Self::Bind(body), CExpr::Bind(value, slot, inner)) => CExpr::Bind(value.clone(), *slot, Box::new(body.indexed(inner, slot + 1))),
            (Self::Hole(body), CExpr::Hole(inner, _)) => body.indexed(inner, depth),
            _ => unreachable!("message structure is preserved after indexing"),
        }
    }

}

/// Moves every parameter slot from `from` up by one, for a binding
/// inserted at `from` around `expr`.
fn shift_params(expr: &mut CExpr, from: u16) {
    match expr {
        CExpr::Param(i) | CExpr::Bind(_, i, _) | CExpr::Walk(_, i, _) if *i >= from => *i += 1,
        CExpr::Each(stage) if stage.depth >= from => stage.depth += 1,
        _ => {}
    }
    expr.for_each_child_mut(&mut |child| shift_params(child, from));
}

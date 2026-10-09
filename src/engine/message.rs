use crate::expr::{CExpr, Ctx, Value};
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

    pub(super) fn eval<'a>(&self, expr: &CExpr, ctx: &Ctx<'a>) -> Result<(Value<'a>, usize), String> {
        match (self, expr) {
            (Self::Leaf(index), expr) => Ok((ctx.eval(expr)?, *index)),
            (Self::If(yes, no), CExpr::If(condition, a, b)) => {
                if ctx.eval(condition)?.truthy() { yes.eval(a, ctx) } else { no.eval(b, ctx) }
            }
            (Self::Nullish(left, right), CExpr::Nullish(a, b)) => {
                let result = left.eval(a, ctx)?;
                if matches!(result.0, Value::Null) { right.eval(b, ctx) } else { Ok(result) }
            }
            (Self::Bind(body), CExpr::Bind(value, _, expr)) => {
                ctx.push_param(ctx.eval(value)?);
                let result = body.eval(expr, ctx);
                ctx.pop_param();
                result
            }
            (Self::Hole(body), CExpr::Hole(expr, _)) => body.eval(expr, ctx),
            _ => unreachable!("message structure is preserved after indexing"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::linter;

    #[test]
    fn message_branches_keep_source_indexes_and_binding_frames() {
        let lint = linter(r#"
            in .t match word /[a-z]+/ {
                rule branch warn if text == "a" then "same" else if text == "b" then "same" else "other"
                rule fallback warn (if text == "a" then null else "present") ?? "fallback"
                rule pipeline warn { value text; then (word) if word == "a" then "first" else "second" }
                rule constant warn if false then "first" else "second"
                rule failure warn num(text)
            }
        "#);
        let reports = lint.lint(Path::new("/root/a.t"), "a b c");
        let indexes = |rule| reports.iter().filter(|d| d.rule == rule)
            .map(|d| serde_json::to_value(d).unwrap()["message_index"].clone()).collect::<Vec<_>>();
        assert_eq!(indexes("branch"), vec![0, 1, 2]);
        assert_eq!(indexes("fallback"), vec![2, 1, 1]);
        assert_eq!(indexes("pipeline"), vec![0, 1, 1]);
        assert_eq!(indexes("constant"), vec![1, 1, 1]);
        assert!(indexes("failure").iter().all(serde_json::Value::is_null));
    }
}

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
            CExpr::Param(i) | CExpr::Bind(_, i, _) if *i > depth => *i -= 1,
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
    use std::path::Path;

    use crate::engine::tests::{lint, linter};

    #[test]
    fn first_use_bindings_keep_vm_field_coercion_and_parameter_frames() {
        use super::{bind, CExpr, Value};
        use crate::expr::functions;
        use crate::expr::Each;
        use crate::expr::pipeline::Stage;
        let call = |name, args| CExpr::Call(functions::lookup(name).unwrap(), args);
        let field = CExpr::OnField(0, Box::new(CExpr::Builtin(crate::expr::Builtin::EndOffset)));
        let last = bind(field, 0, call("last", vec![CExpr::Param(0)]));
        assert!(crate::vm::lower(&last, &[]).is_ok(), "a field projection still coerces to its whole list");
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
                rule foreign_error warn { value num(text); space_before }
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
    fn final_options_and_local_records_keep_diagnostics_and_fix_bytes() {
        let make = |dynamic: bool| {
            let value = |name: &str| if dynamic { format!("runtime({name}, node)") } else { name.into() };
            linter(&format!(r#"
                fn runtime(value, owner) = if owner == null then value else value
                in .t match word /[a-z]+/ {{
                    rule options {{
                        option replacement string = "z"
                        option enabled boolean = false
                        let policy {{ replacement: {}, enabled: {}, suffix: trim(" ! ") }}
                        when policy.enabled && text != policy.replacement
                        fix as policy.replacement
                        warn "{{text}} -> {{policy.replacement}}{{policy.suffix}}"
                    }}
                }}
                rule options {{ set replacement "b" }}
                rule options {{
                    set enabled true
                    set replacement "c"
                }}
            "#, value("replacement"), value("enabled")))
        };
        let folded = make(false);
        let dynamic = make(true);
        for (path, source) in [("/root/a.t", "a b"), ("/root/b.t", "c a")] {
            let path = Path::new(path);
            let reports = folded.lint(path, source);
            assert!(!reports.is_empty());
            assert_eq!(serde_json::to_value(&reports).unwrap(), serde_json::to_value(dynamic.lint(path, source)).unwrap());
            assert!(reports.iter().all(|d| d.message.ends_with("-> c!") && d.fix.is_some()));
            let fixed = folded.fix(path, source);
            assert_eq!(fixed.text, dynamic.fix(path, source).text);
            assert!(folded.lint(path, &fixed.text).is_empty());
        }
    }

    #[test]
    fn option_locals_stay_guarded_on_foreign_nodes() {
        let l = linter(r#"
            in .t match word /[a-z]+/ {
                rule guarded {
                    option limit number = 7
                    let policy { limit: limit }
                    when parent.policy != null
                      || previous.policy != null && previous.policy.limit != limit
                      || policy.limit != limit
                    warn "local escaped its target"
                }
            }
        "#);
        assert!(lint(&l, "a b c").is_empty());
    }

    #[test]
    fn failing_pure_values_remain_lazy_runtime_errors() {
        let l = linter(r#"
            in .t match word /[a-z]+/ {
                rule lazy {
                    option enabled boolean = false
                    when !(false && true) && (true || false)
                      && (if enabled then replace("x", "", "y") else true)
                      && !(enabled && 1 % 0 == 0)
                    warn "safe"
                }
                rule failing {
                    option enabled boolean = true
                    when enabled
                    warn "{replace('x', '', 'y')}"
                }
                rule dormant {
                    warn "{1 % 0}"
                }
            }
            rule dormant off
        "#);
        for source in ["a", "a b"] {
            let reports = lint(&l, source);
            let failures: Vec<_> = reports.iter().filter(|d| d.rule == "failing").collect();
            assert_eq!(failures.len(), source.split_whitespace().count());
            assert!(failures.iter().all(|d| d.severity == crate::config::ast::Severity::Error && d.message.contains("replace")));
            assert_eq!(reports.iter().filter(|d| d.rule == "lazy" && d.message == "safe").count(), failures.len());
            assert!(!reports.iter().any(|d| d.rule == "dormant"));
        }
    }

    #[test]
    fn user_functions_keep_eager_arguments_and_isolated_parameter_frames() {
        let l = linter(r#"
            fn inner(value) = upper(value)
            fn outer(value) = inner(value) value
            fn choose(flag, value) = if flag then "fixed" else value
            fn recurse(value) = recurse(value)
            in .t match word /[a-z]+/
            in word let cycle cycle
            in word {
                rule constants {
                    option value string = "x"
                    when outer(value) == "Aa"
                    fix as inner(value)
                    warn choose(true, node)
                }
                rule dynamic {
                    warn choose(false, node)
                }
                rule eager {
                    warn choose(true, 1 % 0)
                }
                rule recursive {
                    warn recurse("x")
                }
                rule cyclic_argument {
                    warn choose(true, cycle)
                }
                rule dormant {
                    warn choose(true, 1 % 0)
                }
            }
            rule constants { set value "a" }
            rule dormant off
        "#);
        for source in ["a", "b c"] {
            let reports = lint(&l, source);
            let count = source.split_whitespace().count();
            assert_eq!(reports.iter().filter(|d| d.rule == "constants" && d.message == "fixed"
                && d.fix.as_ref().is_some_and(|f| f.iter().all(|e| e.text == "A"))).count(), count);
            assert_eq!(reports.iter().filter(|d| d.rule == "dynamic"
                && source.split_whitespace().any(|word| word == d.message)).count(), count);
            for rule in ["eager", "recursive", "cyclic_argument"] {
                assert_eq!(reports.iter().filter(|d| d.rule == rule
                    && d.severity == crate::config::ast::Severity::Error).count(), count);
            }
            assert!(!reports.iter().any(|d| d.rule == "dormant"));
        }
    }

    #[test]
    fn shared_function_dag_can_defer_work_until_a_node_is_visited() {
        let mut config = String::from("fn f0() = 1\n");
        for i in 1..=40 {
            config.push_str(&format!("fn f{i}() = f{}() + f{}()\n", i - 1, i - 1));
        }
        let empty = linter(&format!("{config}in .t match word /[a-z]+/ {{ rule deep {{ warn f40() }} }}"));
        assert!(lint(&empty, "").is_empty());
        let active = linter(&format!("{config}in .t match word /[a-z]+/ {{ rule bounded {{ when f12() == 4096\nwarn \"ok\" }} }}"));
        let reports = lint(&active, "a");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].severity, crate::config::ast::Severity::Warn);
    }

    #[test]
    fn constant_function_chain_keeps_runtime_depth_failure() {
        let mut config = String::from("fn f0(value) = value\n");
        let last = super::super::MAX_CALL_DEPTH + 1;
        for i in 1..=last {
            config.push_str(&format!("fn f{i}(value) = f{}(value)\n", i - 1));
        }
        config.push_str(&format!("in .t match word /[a-z]+/ {{ rule deep {{ warn f{last}(\"x\") }} }}"));
        let reports = lint(&linter(&config), "a");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].severity, crate::config::ast::Severity::Error);
    }
}

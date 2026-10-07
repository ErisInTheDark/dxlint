//! Rule expressions specialised against their final, immutable options.

use super::{CExpr, Value};
use crate::config::ast::BinOp;
use crate::grammar::UserFn;

pub(crate) fn specialise(expr: &mut CExpr, options: &[Value<'static>], functions: &[UserFn]) {
    let fold = |child: &mut CExpr| specialise(child, options, functions);
    match expr {
        CExpr::Template(items) | CExpr::List(items) | CExpr::Call(_, items)
        | CExpr::FactCall(_, items) | CExpr::UserCall(_, items) => {
            items.iter_mut().for_each(fold);
        }
        CExpr::Hole(inner, _) | CExpr::Not(inner) | CExpr::Neg(inner)
        | CExpr::Test(inner, _) | CExpr::TestDfa(inner, _) | CExpr::Dbg(inner, _)
        | CExpr::Relative(_, inner) | CExpr::Is(inner, _) | CExpr::Key(inner, _)
        | CExpr::OnField(_, inner) | CExpr::OwnChild(_, _, inner) | CExpr::OnParent(inner)
        | CExpr::Position(inner) | CExpr::KindName(inner) | CExpr::NodeAt(inner)
        | CExpr::FieldElse(_, inner) | CExpr::OnAncestor(_, inner) => fold(inner),
        CExpr::Binary(_, a, b) | CExpr::Each(_, a, b) | CExpr::Walk(a, b)
        | CExpr::Coalesce(a, b) => {
            fold(a);
            fold(b);
        }
        CExpr::If(a, b, c) | CExpr::Reduce(a, b, c) => {
            fold(a);
            fold(b);
            fold(c);
        }
        CExpr::Record(entries) => {
            for (_, value) in entries { fold(value); }
        }
        CExpr::Member(base, _, part) => {
            fold(base);
            if let Some(part) = part { fold(part); }
        }
        CExpr::Path(base, _, parts) => {
            fold(base);
            parts.iter_mut().flatten().for_each(fold);
        }
        CExpr::Overlapping(_, range) => {
            if let Some(range) = range {
                fold(&mut range.0);
                fold(&mut range.1);
            }
        }
        CExpr::Selector(_, base, span) => {
            if let Some(base) = base { fold(base); }
            fold(span);
        }
        CExpr::Const(_) | CExpr::Opt(_) | CExpr::Field(_) | CExpr::Ancestor(_, _)
        | CExpr::Builtin(_) | CExpr::Fact(_, _) | CExpr::FactField(_, _) | CExpr::Attr(_)
        | CExpr::FieldAttr(_, _) | CExpr::ParentField(_) | CExpr::ParentAttr(_)
        | CExpr::Local(_) | CExpr::Param(_) | CExpr::Link(_)
        | CExpr::Descendants(_) | CExpr::Containing(_) => {}
    }

    // Only evaluate known value-only forms. A failed operation remains a
    // runtime expression, including in unvisited branches and dormant rules.
    let constant = |expr: &CExpr| matches!(expr, CExpr::Const(_));
    let eligible = match expr {
        CExpr::Opt(_) => true,
        CExpr::UserCall(_, _) => true,
        CExpr::Template(items) | CExpr::List(items) | CExpr::Call(_, items) => items.iter().all(constant),
        CExpr::Record(entries) => entries.iter().all(|(_, value)| constant(value)),
        CExpr::Hole(inner, _) | CExpr::Not(inner) | CExpr::Neg(inner)
        | CExpr::Key(inner, _) | CExpr::Member(inner, _, _) | CExpr::Path(inner, _, _) => constant(inner),
        CExpr::If(condition, yes, no) => match condition.as_ref() {
            CExpr::Const(value) => constant(if value.truthy() { yes } else { no }),
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

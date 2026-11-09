use crate::grammar::names::question::{Known, Leaf};
use crate::grammar::names::semantic::Semantic;
use crate::config::ast::Quantifier;
use crate::expr::Value;

mod flow;
mod values;
mod syntax;
mod modules;
mod names;
mod handles;
mod commands;

pub use dx_ir::expr::QueryId;
pub use handles::{handle_function, handle_of, HandleFunction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uncertainty {
    Strict,
    /// The result propagates unknowns or represents them in its schema.
    Explicit,
}

#[derive(Debug)]
pub struct Query {
    pub name: &'static str,
    pub quantifier: Quantifier,
    pub call: fn(&Semantic<'_>, Leaf, &crate::grammar::names::Value, u64) -> Known<bool>,
}

pub static QUERIES: &[Query] = &[
    Query { name: "could", quantifier: Quantifier::Could, call: |semantic, at, subject, mask| {
        let values = semantic.values();
        values.could(&values.evaluate(at, subject), mask)
    } },
    Query { name: "always", quantifier: Quantifier::Always, call: |semantic, at, subject, mask| {
        let values = semantic.values();
        values.always(&values.evaluate(at, subject), mask)
    } },
];

pub fn query(quantifier: Quantifier) -> &'static Query {
    QUERIES.iter().find(|query| query.quantifier == quantifier).expect("registered quantifier")
}

#[derive(Debug)]
pub struct QueryFunction {
    pub name: &'static str,
    pub arity: usize,
    pub call: fn(&Semantic<'_>, Leaf, &[Value<'static>]) -> Known<Value<'static>>,
}

pub static QUERY_FUNCTIONS: &[&[QueryFunction]] = &[flow::FUNCTIONS, values::FUNCTIONS, syntax::FUNCTIONS, modules::FUNCTIONS, names::FUNCTIONS, handles::FUNCTIONS, commands::FUNCTIONS];

impl QueryFunction {
    pub fn uncertainty(&self) -> Uncertainty {
        const EXPLICIT: &[&str] = &["call_signatures", "call_parameters", "call_results", "member_call_parameters", "value_constituents", "value_accepts", "value_parts", "value_alternatives", "value_union", "value_expected_handle", "value_children", "value_signatures", "value_constructors", "call_selection"];
        if EXPLICIT.contains(&self.name) { Uncertainty::Explicit } else { Uncertainty::Strict }
    }
}

pub fn lookup_function(name: &str) -> Option<QueryId> {
    functions().iter().position(|function| function.name == name).map(|id| QueryId(id as u16))
}

/// Every query function, in id order.
fn functions() -> &'static [&'static QueryFunction] {
    static FUNCTIONS: std::sync::OnceLock<Vec<&'static QueryFunction>> = std::sync::OnceLock::new();
    FUNCTIONS.get_or_init(|| QUERY_FUNCTIONS.iter().flat_map(|group| group.iter()).collect())
}

pub fn query_function(id: QueryId) -> &'static QueryFunction {
    let functions = functions();
    if let Some(function) = functions.get(id.0 as usize) {
        return *function;
    }
    #[cfg(test)]
    {
        let extra = test_functions();
        if let Some(function) = extra.get(id.0 as usize - functions.len()) {
            return *function;
        }
    }
    panic!("unknown query function id {}", id.0)
}

/// Registrations for the tests' own query functions, past the real ones.
#[cfg(test)]
fn test_functions() -> std::sync::MutexGuard<'static, Vec<&'static QueryFunction>> {
    static FUNCTIONS: std::sync::Mutex<Vec<&'static QueryFunction>> = std::sync::Mutex::new(Vec::new());
    FUNCTIONS.lock().expect("test query functions")
}

/// The id the VM's test query functions use: the same as [`lookup_function`]
/// for a real one.
#[cfg(test)]
pub fn test_query_id(function: &'static QueryFunction) -> QueryId {
    let functions = functions();
    if let Some(id) = functions.iter().position(|f| std::ptr::eq(*f, function)) {
        return QueryId(id as u16);
    }
    let mut extra = test_functions();
    if let Some(id) = extra.iter().position(|f| std::ptr::eq(*f, function)) {
        return QueryId((functions.len() + id) as u16);
    }
    extra.push(function);
    QueryId((functions.len() + extra.len() - 1) as u16)
}

pub fn semantic_step(name: &str) -> Option<u16> {
    use crate::grammar::names::{EXPECTED, INFERRED, RESULT, RETURNED, WIDENED};
    match name {
        "result" => Some(RESULT),
        "expected" => Some(EXPECTED),
        "inferred" => Some(INFERRED),
        "returned" => Some(RETURNED),
        "widened" => Some(WIDENED),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{errors, lint, linter};

    #[test]
    fn trait_queries_quantify_unions_and_intersections_and_combine_trait_masks() {
        let linter = linter(r#"
            match good /a/ { fits good }
            match other /b/ { fits other }
            in .t {
                match either left:good /\|/ right:other { unwraps left | right }
                match both left:good /&/ right:other { unwraps left & right }
            }
            in either, both {
                rule possible warn "possible" {
                    let x node
                    when x could good
                }
                rule certain warn "certain" { when node always good }
                rule masked warn "masked" {
                    when node
                        always (good, other)
                }
                rule nonnode warn "nonnode" { when (text could good) == null }
            }
        "#);
        let reports = lint(&linter, "a|b a&b");
        let count = |rule| reports.iter().filter(|report| report.rule == rule).count();
        assert_eq!(count("possible"), 2);
        assert_eq!(count("certain"), 1);
        assert_eq!(count("masked"), 1);
        assert_eq!(count("nonnode"), 2);
    }

    #[test]
    fn unknown_trait_answers_are_false_for_rules() {
        let linter = linter(r#"
            in str fits good
            in paren {
                unwraps it
                rule unknown warn "unknown" {
                    when node could good || node always good
                }
            }
        "#);
        assert!(lint(&linter, "()").is_empty());
    }

    #[test]
    fn unknown_trait_names_report_the_trait_and_suggest_a_known_one() {
        let errors = errors(r#"
            in paren fits callable
            in paren rule typo warn "typo" { when node could callabl }
        "#);
        assert!(errors.iter().any(|error| error.contains("nothing fits trait") && error.contains("callabl") && error.contains("callable")), "{errors:#?}");
    }

    #[test]
    fn semantic_result_subjects_use_declared_outputs_without_exposing_nodes() {
        let linter = linter(r#"
            match good /a/ { fits good }
            match other /b/ { fits other }
            in .t {
                match producer "f" parameter:"()"? output:(good or other) {
                    signature parameter returns output
                }
            }
            in producer {
                rule returned warn "returned" { when node.result always good }
                rule nonnode warn "nonnode" { when (text.result could good) == null }
            }
        "#);
        let reports = lint(&linter, "fa fb");
        assert_eq!(reports.iter().filter(|report| report.rule == "returned").count(), 1);
        assert_eq!(reports.iter().filter(|report| report.rule == "nonnode").count(), 2);
        assert!(!errors(r#"
            in paren rule escaped warn node.result
        "#).is_empty());
        assert!(!errors(r#"
            in paren fits good
            in paren rule escaped warn "escaped" { when node.result.node could good }
        "#).is_empty());
    }

    #[test]
    fn member_subjects_query_declared_values_without_proving_absent_members() {
        let linter = linter(r#"
            match good /a/ { fits good }
            match other /b/ { fits other }
            in .t {
                match container "[" content:(good or other) "]" {
                    scope properties
                    members properties
                    member properties "key" unwraps content
                }
            }
            in container {
                rule member warn "member" { when node.member "key" always good }
                rule absent warn "absent" {
                    when node.member "missing" could good || node.member "missing" always good
                }
            }
        "#);
        let reports = lint(&linter, "[a] [b]");
        assert_eq!(reports.iter().filter(|report| report.rule == "member").count(), 1);
        assert_eq!(reports.iter().filter(|report| report.rule == "absent").count(), 0);
    }
}

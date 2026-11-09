use crate::names::question::{Known, Leaf};
use crate::names::semantic::Semantic;
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
    pub call: fn(&Semantic<'_>, Leaf, &crate::names::Value, u64) -> Known<bool>,
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
    {
        let extra = test_functions();
        if let Some(function) = extra.get(id.0 as usize - functions.len()) {
            return *function;
        }
    }
    panic!("unknown query function id {}", id.0)
}

/// Registrations for the tests' own query functions, past the real ones.
fn test_functions() -> std::sync::MutexGuard<'static, Vec<&'static QueryFunction>> {
    static FUNCTIONS: std::sync::Mutex<Vec<&'static QueryFunction>> = std::sync::Mutex::new(Vec::new());
    FUNCTIONS.lock().expect("test query functions")
}

/// The id the VM's test query functions use: the same as [`lookup_function`]
/// for a real one.
#[doc(hidden)]
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
    use crate::names::{EXPECTED, INFERRED, RESULT, RETURNED, WIDENED};
    match name {
        "result" => Some(RESULT),
        "expected" => Some(EXPECTED),
        "inferred" => Some(INFERRED),
        "returned" => Some(RETURNED),
        "widened" => Some(WIDENED),
        _ => None,
    }
}

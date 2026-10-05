//! `zlint --profile`: where linting time goes, per matcher and rule.
//! Counters are atomics so parallel lint threads share one profile.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Default)]
pub struct Profile {
    /// Interpreter steps (instructions run) per matcher.
    pub matcher_steps: Vec<AtomicU64>,
    /// Program runs per matcher (memoised calls and overlay attempts).
    pub matcher_runs: Vec<AtomicU64>,
    /// Nanoseconds searching for each overlay matcher, and the nodes found.
    pub overlay_nanos: Vec<AtomicU64>,
    pub overlay_nodes: Vec<AtomicU64>,
    /// Nanoseconds spent evaluating each rule, and how often.
    pub rule_nanos: Vec<AtomicU64>,
    pub rule_evals: Vec<AtomicU64>,
    /// Nanoseconds spent computing each native fact (memo misses only, so
    /// a whole-file analysis counts against the fact that first needs it).
    pub fact_nanos: Vec<AtomicU64>,
    pub fact_calls: Vec<AtomicU64>,
    pub parse_nanos: AtomicU64,
    /// Parsing's parts: the native front end, its token nodes, and pattern
    /// layers (grafted in, or a whole pattern tree).
    pub native_nanos: AtomicU64,
    pub token_nanos: AtomicU64,
    pub layer_nanos: AtomicU64,
    /// Time spent running rules, all together.
    pub rules_nanos: AtomicU64,
}

fn counters(n: usize) -> Vec<AtomicU64> {
    (0..n).map(|_| AtomicU64::new(0)).collect()
}

impl Profile {
    pub fn new(matchers: usize, rules: usize) -> Profile {
        Profile {
            matcher_steps: counters(matchers),
            matcher_runs: counters(matchers),
            overlay_nanos: counters(matchers),
            overlay_nodes: counters(matchers),
            rule_nanos: counters(rules),
            rule_evals: counters(rules),
            fact_nanos: counters(crate::expr::facts::FACTS.len()),
            fact_calls: counters(crate::expr::facts::FACTS.len()),
            ..Profile::default()
        }
    }

    pub fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_time(counter: &AtomicU64, d: Duration) {
        counter.fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
    }

    pub fn get(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}

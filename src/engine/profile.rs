//! `zlint --profile`: where linting time goes, per matcher and formatter.
//! Counters are atomics so parallel lint threads share one profile.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Default)]
pub struct Profile {
    /// Interpreter steps (instructions run) per matcher.
    pub matcher_steps: Vec<AtomicU64>,
    /// Program runs per matcher (memoised calls and overlay attempts).
    pub matcher_runs: Vec<AtomicU64>,
    /// Nanoseconds spent evaluating each formatter, and how often.
    pub formatter_nanos: Vec<AtomicU64>,
    pub formatter_evals: Vec<AtomicU64>,
    pub parse_nanos: AtomicU64,
    pub format_nanos: AtomicU64,
}

fn counters(n: usize) -> Vec<AtomicU64> {
    (0..n).map(|_| AtomicU64::new(0)).collect()
}

impl Profile {
    pub fn new(matchers: usize, formatters: usize) -> Profile {
        Profile {
            matcher_steps: counters(matchers),
            matcher_runs: counters(matchers),
            formatter_nanos: counters(formatters),
            formatter_evals: counters(formatters),
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

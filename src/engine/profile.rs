//! `zlint --profile`: where linting time goes, per matcher and rule.
//! Counters are atomics so parallel lint threads share one profile.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::fx_hash::FxHashMap;

thread_local! {
    /// This thread's outermost native fact time so far, and how deeply fact
    /// computations are nested right now (a fact may read another fact).
    static FACT_TIME: Cell<(u64, u32)> = const { Cell::new((0, 0)) };
}

#[derive(Default)]
pub struct Profile {
    /// Interpreter steps (instructions run) per matcher.
    pub matcher_steps: Vec<AtomicU64>,
    /// Program runs per matcher (memoised calls and overlay attempts).
    pub matcher_runs: Vec<AtomicU64>,
    /// Nanoseconds searching for each overlay matcher, and the nodes found.
    pub overlay_nanos: Vec<AtomicU64>,
    pub overlay_nodes: Vec<AtomicU64>,
    /// Nanoseconds spent evaluating each rule (native fact time excluded),
    /// and how often.
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
    /// Resolving trees' names, and running name items' `when` conditions
    /// within that.
    pub names_nanos: AtomicU64,
    pub names_trees: AtomicU64,
    pub name_condition_nanos: AtomicU64,
    pub name_conditions: AtomicU64,
    /// Node queries by [`Query`]: nanoseconds (inside the rules' time),
    /// calls and nodes returned.
    pub query_nanos: [AtomicU64; Query::ALL.len()],
    pub query_calls: [AtomicU64; Query::ALL.len()],
    pub query_nodes: [AtomicU64; Query::ALL.len()],
    pub semantic_queries: FxHashMap<&'static str, SemanticQueryProfile>,
    pub semantic_budget_limits: AtomicU64,
    pub semantic_budget_steps_total: AtomicU64,
    pub semantic_budget_steps_max: AtomicU64,
    pub semantic_budget_steps_over_half: AtomicU64,
    pub semantic_state_discards: AtomicU64,
    pub effect_graph_builds: AtomicU64,
    pub effect_read_catalogue_builds: AtomicU64,
    pub effect_graph_build_nanos: AtomicU64,
    pub effect_read_catalogue_build_nanos: AtomicU64,
    pub effect_graph_rebuild_nanos: AtomicU64,
    pub effect_read_catalogue_rebuild_nanos: AtomicU64,
    pub effect_graph_successes: AtomicU64,
    pub effect_graph_success_nanos: AtomicU64,
    pub effect_graph_unknown_nanos: AtomicU64,
    pub effect_entry_calls: AtomicU64,
    pub effect_entry_nanos: AtomicU64,
    pub module_index_nanos: AtomicU64,
    pub module_index_calls: AtomicU64,
    pub module_plans_nanos: AtomicU64,
    pub module_plans_calls: AtomicU64,
    effect_failures: Mutex<FxHashMap<(PathBuf, u32, crate::grammar::names::effects::build::Failure), EffectFailureProfile>>,
}

#[derive(Clone, Copy, Default)]
pub struct EffectFailureProfile {
    pub calls: u64,
    pub nanos: u64,
}

#[derive(Default)]
pub struct SemanticQueryProfile {
    pub nanos: AtomicU64,
    pub calls: AtomicU64,
}

pub enum EffectBuild {
    Graph,
    ReadCatalogue,
}

/// The node queries rules run (`overlapping(...)`, `containing(...)`,
/// `descendants(...)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    Overlapping,
    Containing,
    Descendants,
}

impl Query {
    pub const ALL: [Query; 3] = [Query::Overlapping, Query::Containing, Query::Descendants];

    pub fn name(self) -> &'static str {
        match self {
            Query::Overlapping => "overlapping",
            Query::Containing => "containing",
            Query::Descendants => "descendants",
        }
    }
}

/// Work shared by every rule (a native fact, a tree's names): its time and
/// calls, and when outermost, this thread's fact clock, which rule time
/// leaves out.
fn shared<T>(nanos: &AtomicU64, calls: &AtomicU64, compute: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    FACT_TIME.with(|time| time.set((time.get().0, time.get().1 + 1)));
    let value = compute();
    let elapsed = started.elapsed();
    FACT_TIME.with(|time| {
        let (total, depth) = time.get();
        time.set((if depth == 1 { total + elapsed.as_nanos() as u64 } else { total }, depth - 1));
    });
    Profile::add_time(nanos, elapsed);
    Profile::add(calls, 1);
    value
}

fn counters(n: usize) -> Vec<AtomicU64> {
    (0..n).map(|_| AtomicU64::new(0)).collect()
}

fn timed<T>(nanos: &AtomicU64, calls: &AtomicU64, run: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = run();
    Profile::add_time(nanos, started.elapsed());
    Profile::add(calls, 1);
    value
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
            semantic_queries: crate::expr::queries::QUERY_FUNCTIONS.iter().flat_map(|group| group.iter())
                .map(|query| (query.name, SemanticQueryProfile::default())).collect(),
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

    /// Computes one native fact, counting its time against the fact and,
    /// when outermost, against this thread's fact clock.
    pub fn time_fact<T>(&self, fact: usize, compute: impl FnOnce() -> T) -> T {
        shared(&self.fact_nanos[fact], &self.fact_calls[fact], compute)
    }

    /// Resolves one tree's names: shared by every rule, so outside their time.
    pub fn time_names<T>(&self, compute: impl FnOnce() -> T) -> T {
        shared(&self.names_nanos, &self.names_trees, compute)
    }

    /// Runs one name item's `when` condition (inside the names' time).
    pub fn time_name_condition<T>(&self, run: impl FnOnce() -> T) -> T {
        timed(&self.name_condition_nanos, &self.name_conditions, run)
    }

    /// Runs one node query, counting its time, the call and the nodes found.
    pub fn time_query<T>(&self, query: Query, run: impl FnOnce() -> T, found: impl FnOnce(&T) -> usize) -> T {
        let started = Instant::now();
        let value = run();
        let at = query as usize;
        Profile::add_time(&self.query_nanos[at], started.elapsed());
        Profile::add(&self.query_calls[at], 1);
        Profile::add(&self.query_nodes[at], found(&value) as u64);
        value
    }

    pub fn time_semantic_query<T>(&self, name: &'static str, run: impl FnOnce() -> T) -> T {
        let Some(query) = self.semantic_queries.get(name) else { return run() };
        timed(&query.nanos, &query.calls, run)
    }

    pub fn time_module_index<T>(&self, run: impl FnOnce() -> T) -> T {
        timed(&self.module_index_nanos, &self.module_index_calls, run)
    }

    pub fn time_module_plans<T>(&self, run: impl FnOnce() -> T) -> T {
        timed(&self.module_plans_nanos, &self.module_plans_calls, run)
    }

    pub fn record_semantic_budget(&self, used: u64, limit: u64) {
        Self::add(&self.semantic_budget_steps_total, used);
        self.semantic_budget_steps_max.fetch_max(used, Ordering::Relaxed);
        if used > limit / 2 { Self::add(&self.semantic_budget_steps_over_half, 1); }
    }

    pub fn record_effect_failure(&self, path: &Path, node: u32,
        cause: crate::grammar::names::effects::build::Failure, elapsed: Duration) {
        let mut failures = self.effect_failures.lock().expect("effect profiling lock");
        let row = failures.entry((path.to_owned(), node, cause)).or_default();
        row.calls += 1;
        row.nanos += elapsed.as_nanos() as u64;
    }

    pub fn effect_failures(&self) -> Vec<(PathBuf, u32, crate::grammar::names::effects::build::Failure, EffectFailureProfile)> {
        let failures = self.effect_failures.lock().expect("effect profiling lock");
        let mut rows: Vec<_> = failures.iter().map(|((path, node, cause), row)| (path.clone(), *node, *cause, *row)).collect();
        rows.sort_by_key(|(_, _, _, row)| std::cmp::Reverse(row.nanos));
        rows
    }

    pub fn time_effect_build<T>(&self, kind: EffectBuild, rebuild: bool, run: impl FnOnce() -> T,
        completed: impl FnOnce(&T, Duration) -> bool) -> T {
        let (calls, nanos, rebuilt, outcomes) = match kind {
            EffectBuild::Graph => (&self.effect_graph_builds, &self.effect_graph_build_nanos, &self.effect_graph_rebuild_nanos,
                Some((&self.effect_graph_success_nanos, &self.effect_graph_unknown_nanos))),
            EffectBuild::ReadCatalogue => (&self.effect_read_catalogue_builds, &self.effect_read_catalogue_build_nanos, &self.effect_read_catalogue_rebuild_nanos, None),
        };
        Profile::add(calls, 1);
        let started = Instant::now();
        let value = run();
        let elapsed = started.elapsed();
        Profile::add_time(nanos, elapsed);
        if rebuild { Profile::add_time(rebuilt, elapsed); }
        if let Some((success, unknown)) = outcomes {
            if completed(&value, elapsed) {
                Profile::add(&self.effect_graph_successes, 1);
                Profile::add_time(success, elapsed);
            } else {
                Profile::add_time(unknown, elapsed);
            }
        }
        value
    }

    pub fn time_effect_entry<T>(&self, run: impl FnOnce() -> T) -> T {
        timed(&self.effect_entry_nanos, &self.effect_entry_calls, run)
    }

    /// Times one rule evaluation, excluding the native facts it computed:
    /// those are shared by every rule, so they count only as facts.
    pub fn time_rule<T>(&self, rule: usize, run: impl FnOnce() -> T) -> T {
        let facts_before = FACT_TIME.with(|time| time.get().0);
        let started = Instant::now();
        let value = run();
        let facts = FACT_TIME.with(|time| time.get().0) - facts_before;
        Profile::add(&self.rule_nanos[rule], (started.elapsed().as_nanos() as u64).saturating_sub(facts));
        Profile::add(&self.rule_evals[rule], 1);
        value
    }
}

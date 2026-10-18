//! `zlint --profile`: where linting time goes, per matcher and rule.
//! Counters are atomics so parallel lint threads share one profile.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::fx_hash::FxHashMap;

thread_local! {
    /// This thread's outermost native fact time so far, and how deeply fact
    /// computations are nested right now (a fact may read another fact).
    static FACT_TIME: Cell<(u64, u32)> = const { Cell::new((0, 0)) };
    static SEMANTIC_QUERY: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// Per open self-timed region (outermost first), the time its nested
    /// regions took so far.
    static NESTED_TIME: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// One self-timed region: semantic queries, value evaluations and name
/// loads, so each reports the time not spent in the others it nests.
struct Region {
    depth: usize,
    started: Instant,
}

impl Region {
    fn start() -> Self {
        let depth = NESTED_TIME.with(|stack| {
            let mut stack = stack.borrow_mut();
            stack.push(0);
            stack.len()
        });
        Region { depth, started: Instant::now() }
    }

    /// The region's total time and its self time; the total counts as
    /// nested time of the enclosing region. An evaluation abandoned out of
    /// order (an enclosing region closed first) reports no self time.
    fn finish(&self) -> (Duration, Duration) {
        let elapsed = self.started.elapsed();
        let nested = NESTED_TIME.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.len() < self.depth { return None }
            stack.truncate(self.depth);
            let nested = stack.pop().unwrap_or(0);
            if let Some(parent) = stack.last_mut() { *parent += elapsed.as_nanos() as u64 }
            Some(nested)
        });
        let own = nested.map_or(Duration::ZERO, |nested| elapsed.saturating_sub(Duration::from_nanos(nested)));
        (elapsed, own)
    }
}

#[cfg(test)]
mod tests {
    use super::{Profile, Query};

    #[test]
    fn descendants_profile_counts_consumed_results_on_drop() {
        let profile = Profile::new(0, 0);
        {
            let mut iter = Profile::iter_query(Some(&profile), Query::Descendants, || 0..10);
            assert_eq!(iter.next(), Some(0));
            assert_eq!(iter.next(), Some(1));
        }
        {
            let _unused = Profile::iter_query(Some(&profile), Query::Descendants, || 0..10);
        }
        let at = Query::Descendants as usize;
        assert_eq!(Profile::get(&profile.query_calls[at]), 2);
        assert_eq!(Profile::get(&profile.query_nodes[at]), 2);
    }

    #[test]
    fn profile_scopes_restore_attribution_and_count_abandoned_values() {
        let profile = Profile::new(2, 0);
        let names = || profile.time_names(|| ());
        names();
        profile.time_semantic_query("could", || {
            names();
            profile.time_semantic_query("always", names);
            let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                profile.time_semantic_query("always", || panic!("query failed"));
            }));
            assert!(failed.is_err());
            names();
        });
        names();
        assert_eq!(Profile::get(&profile.semantic_queries["could"].names_trees), 2);
        assert_eq!(Profile::get(&profile.semantic_queries["always"].names_trees), 1);
        assert_eq!(Profile::get(&profile.unattributed_names.names_trees), 2);
        assert_eq!(Profile::get(&profile.names_trees), 5);

        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _outer = profile.begin_value(0).unwrap();
            let _inner = profile.begin_value(1).unwrap();
            panic!("budget exit");
        }));
        assert!(failed.is_err());
        assert_eq!(Profile::get(&profile.value_calls[0]), 1);
        assert_eq!(Profile::get(&profile.value_calls[1]), 1);
        assert!(profile.begin_value(2).is_none());
    }

    #[test]
    fn trait_queries_are_counted_when_linting() {
        let source = r#"
            lang plain "*.t"
            in .plain match item /[a-z]/
            in item fits marked
            in item rule probe info text { when node could marked && node always marked }
        "#;
        let built = crate::engine::build(
            crate::config::load_text("profile.zlint", source, &crate::config::source::Disk),
            std::path::PathBuf::from("/root"),
        );
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        let mut linter = built.linter.expect("valid grammar");
        linter.enable_profile();
        assert_eq!(linter.lint(std::path::Path::new("/root/file.t"), "x").len(), 1);
        let profile = linter.profile.as_ref().unwrap();
        for query in crate::expr::queries::QUERIES {
            assert!(super::Profile::get(&profile.semantic_queries[query.name].calls) > 0, "{}", query.name);
        }
    }
}

#[derive(Default)]
pub struct Profile {
    /// Interpreter steps (instructions run) per matcher.
    pub matcher_steps: Vec<AtomicU64>,
    /// Program runs per matcher (memoised calls and overlay attempts).
    pub matcher_runs: Vec<AtomicU64>,
    pub value_nanos: Vec<AtomicU64>,
    pub value_self_nanos: Vec<AtomicU64>,
    pub value_calls: Vec<AtomicU64>,
    /// Root questions cut short by their budget, by the asking node's
    /// matcher, and the steps they spent.
    pub cut_root_calls: Vec<AtomicU64>,
    pub cut_root_steps: Vec<AtomicU64>,
    /// Per attribute (`let`): reads including memo hits, total and self
    /// time, so a shared let's cost shows on the let, not on whichever
    /// rule read it first.
    pub attr_nanos: Vec<AtomicU64>,
    pub attr_self_nanos: Vec<AtomicU64>,
    pub attr_calls: Vec<AtomicU64>,
    pub attr_entered: Vec<AtomicU64>,
    /// Per `fn`: calls, total and self time (arguments excluded).
    pub fn_nanos: Vec<AtomicU64>,
    pub fn_self_nanos: Vec<AtomicU64>,
    pub fn_calls: Vec<AtomicU64>,
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
    pub unattributed_names: SemanticQueryProfile,
    pub semantic_budget_limits: AtomicU64,
    pub semantic_budget_steps_total: AtomicU64,
    pub semantic_budget_steps_max: AtomicU64,
    pub semantic_budget_steps_over_half: AtomicU64,
    /// Steps spent in root questions their budget cut short.
    pub semantic_budget_steps_limited: AtomicU64,
    pub semantic_state_discards: AtomicU64,
    pub semantic_memo_discards: AtomicU64,
    pub program_catalogue_builds: AtomicU64,
    pub catalogue_read_validations: AtomicU64,
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
    /// Reading and parsing files other than the linted ones (once per run).
    pub unit_parse_nanos: AtomicU64,
    pub unit_parses: AtomicU64,
    /// The files whose rules took longest (wall, groups included), slowest
    /// first: what bounds a parallel run's wall time.
    slowest_files: Mutex<Vec<(u64, String)>>,
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
    /// Time not spent in nested queries, value evaluations or name loads.
    pub self_nanos: AtomicU64,
    pub calls: AtomicU64,
    /// Calls started, finished or not: shows queries a hanging run keeps
    /// entering.
    pub entered: AtomicU64,
    /// Root questions started by this query that ran out of budget.
    pub limited: AtomicU64,
    pub names_nanos: AtomicU64,
    pub names_trees: AtomicU64,
}

pub(crate) struct ValueEvaluation<'a> {
    nanos: &'a AtomicU64,
    self_nanos: &'a AtomicU64,
    calls: &'a AtomicU64,
    region: Region,
}

impl Drop for ValueEvaluation<'_> {
    fn drop(&mut self) {
        let (elapsed, own) = self.region.finish();
        Profile::add_time(self.nanos, elapsed);
        Profile::add_time(self.self_nanos, own);
        Profile::add(self.calls, 1);
    }
}

struct SemanticQueryScope(Option<&'static str>);

impl Drop for SemanticQueryScope {
    fn drop(&mut self) {
        SEMANTIC_QUERY.with(|query| query.set(self.0));
    }
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

pub(crate) struct QueryIter<'a, I: Iterator> {
    inner: I,
    profile: Option<&'a Profile>,
    query: Query,
    elapsed: Duration,
    found: u64,
}

impl<I: Iterator> Iterator for QueryIter<'_, I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<Self::Item> {
        let Some(_) = self.profile else { return self.inner.next() };
        let started = Instant::now();
        let value = self.inner.next();
        self.elapsed += started.elapsed();
        self.found += u64::from(value.is_some());
        value
    }
}

impl<I: Iterator> Drop for QueryIter<'_, I> {
    fn drop(&mut self) {
        if let Some(profile) = self.profile {
            let at = self.query as usize;
            Profile::add_time(&profile.query_nanos[at], self.elapsed);
            Profile::add(&profile.query_calls[at], 1);
            Profile::add(&profile.query_nodes[at], self.found);
        }
    }
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
            value_nanos: counters(matchers),
            value_self_nanos: counters(matchers),
            value_calls: counters(matchers),
            cut_root_calls: counters(matchers),
            cut_root_steps: counters(matchers),
            overlay_nanos: counters(matchers),
            overlay_nodes: counters(matchers),
            rule_nanos: counters(rules),
            rule_evals: counters(rules),
            fact_nanos: counters(crate::expr::facts::FACTS.len()),
            fact_calls: counters(crate::expr::facts::FACTS.len()),
            semantic_queries: crate::expr::queries::QUERY_FUNCTIONS.iter().flat_map(|group| group.iter())
                .map(|query| query.name).chain(crate::expr::queries::QUERIES.iter().map(|query| query.name))
                .chain(crate::grammar::names::calls::ProfileQuery::ALL.iter().map(|query| query.name()))
                .chain(crate::grammar::names::modules::queries::PROFILE_QUERIES.iter().copied())
                .chain(["units.native", "units.view", "modules.resolve.search", "modules.resolve.shared", "modules.resolve.shared.key", "modules.resolve.shared.select", "modules.resolve.shared.identity","modules.resolve.shared.lookup", "modules.resolve.shared.targets", "modules.resolve.shared.record", "modules.resolve.shared.publish", "modules.resolve.catalog", "modules.roots.inputs", "modules.roots.survey", "modules.roots.candidates", "modules.roots.prefetch", "modules.roots.attach"])
                .map(|name| (name, SemanticQueryProfile::default())).collect(),
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

    /// One line on the busiest semantic queries so far (by self time), for
    /// watching a run that never finishes (`ZLINT_PROFILE_EVERY`).
    /// `entered/finished` counts show what a stuck run keeps starting;
    /// `attribute` names a `let` by index.
    pub fn watch_line(&self, attribute: impl Fn(usize) -> String) -> String {
        let mut queries: Vec<_> = self.semantic_queries.iter()
            .map(|(&name, query)| (Self::get(&query.entered), Self::get(&query.calls), Self::get(&query.self_nanos), name))
            .filter(|&(entered, ..)| entered > 0).collect();
        queries.sort_by_key(|row| std::cmp::Reverse(row.0));
        let queries: Vec<String> = queries.iter().take(8)
            .map(|(entered, calls, nanos, name)| format!("{name} {entered}/{calls} {:.0}ms", *nanos as f64 / 1e6)).collect();
        let mut lets: Vec<_> = self.attr_entered.iter().enumerate()
            .map(|(index, entered)| (Self::get(entered), Self::get(&self.attr_calls[index]), index))
            .filter(|&(entered, ..)| entered > 0).collect();
        lets.sort_by_key(|row| std::cmp::Reverse(row.0));
        let lets: Vec<String> = lets.iter().take(6)
            .map(|&(entered, calls, index)| format!("{} {entered}/{calls}", attribute(index))).collect();
        format!("zlint profile: queries {} | lets {}", queries.join(", "), lets.join(", "))
    }

    const SLOWEST_FILES: usize = 10;

    pub(crate) fn note_file(&self, file: &str, d: Duration) {
        let nanos = d.as_nanos() as u64;
        let mut slowest = self.slowest_files.lock().unwrap_or_else(|e| e.into_inner());
        if slowest.len() == Self::SLOWEST_FILES && slowest.last().is_some_and(|&(least, _)| least >= nanos) { return }
        let at = slowest.partition_point(|&(other, _)| other >= nanos);
        slowest.insert(at, (nanos, file.to_string()));
        slowest.truncate(Self::SLOWEST_FILES);
    }

    pub fn slowest_files(&self) -> Vec<(u64, String)> {
        self.slowest_files.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Sizes the per-attribute counters (the linter knows its attributes
    /// only after the grammar).
    pub fn with_attributes(mut self, attributes: usize) -> Self {
        self.attr_nanos = counters(attributes);
        self.attr_self_nanos = counters(attributes);
        self.attr_calls = counters(attributes);
        self.attr_entered = counters(attributes);
        self
    }

    /// Sizes the per-`fn` counters.
    pub fn with_functions(mut self, functions: usize) -> Self {
        self.fn_nanos = counters(functions);
        self.fn_self_nanos = counters(functions);
        self.fn_calls = counters(functions);
        self
    }

    pub(crate) fn begin_function(&self, function: usize) -> Option<ValueEvaluation<'_>> {
        Some(ValueEvaluation {
            nanos: self.fn_nanos.get(function)?,
            self_nanos: self.fn_self_nanos.get(function)?,
            calls: self.fn_calls.get(function)?,
            region: Region::start(),
        })
    }

    pub(crate) fn begin_attribute(&self, attr: usize) -> Option<ValueEvaluation<'_>> {
        Self::add(self.attr_entered.get(attr)?, 1);
        Some(ValueEvaluation {
            nanos: self.attr_nanos.get(attr)?,
            self_nanos: self.attr_self_nanos.get(attr)?,
            calls: self.attr_calls.get(attr)?,
            region: Region::start(),
        })
    }

    pub(crate) fn begin_value(&self, matcher: usize) -> Option<ValueEvaluation<'_>> {
        Some(ValueEvaluation {
            nanos: self.value_nanos.get(matcher)?,
            self_nanos: self.value_self_nanos.get(matcher)?,
            calls: self.value_calls.get(matcher)?,
            region: Region::start(),
        })
    }

    /// Computes one native fact, counting its time against the fact and,
    /// when outermost, against this thread's fact clock.
    pub fn time_fact<T>(&self, fact: usize, compute: impl FnOnce() -> T) -> T {
        shared(&self.fact_nanos[fact], &self.fact_calls[fact], compute)
    }

    pub fn record_cut_root(&self, matcher: usize, steps: u64) {
        if let (Some(calls), Some(spent)) = (self.cut_root_calls.get(matcher), self.cut_root_steps.get(matcher)) {
            Self::add(calls, 1);
            Self::add(spent, steps);
        }
    }

    /// Counts a root question cut short by its budget against the semantic
    /// query that asked it.
    pub fn note_budget_limit(&self) {
        let label = SEMANTIC_QUERY.with(Cell::get);
        let query = label.and_then(|label| self.semantic_queries.get(label)).unwrap_or(&self.unattributed_names);
        Self::add(&query.limited, 1);
    }

    /// Resolves one tree's names: shared by every rule, so outside their time.
    pub fn time_names<T>(&self, compute: impl FnOnce() -> T) -> T {
        let label = SEMANTIC_QUERY.with(Cell::get);
        let query = label.and_then(|label| self.semantic_queries.get(label)).unwrap_or(&self.unattributed_names);
        let region = Region::start();
        let value = shared(&self.names_nanos, &self.names_trees, compute);
        Self::add_time(&query.names_nanos, region.finish().0);
        Self::add(&query.names_trees, 1);
        value
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

    pub(crate) fn iter_query<'a, I: Iterator>(
        profile: Option<&'a Self>, query: Query, run: impl FnOnce() -> I,
    ) -> QueryIter<'a, I> {
        let started = profile.map(|_| Instant::now());
        let inner = run();
        QueryIter { inner, profile, query, elapsed: started.map_or(Duration::ZERO, |started| started.elapsed()), found: 0 }
    }

    pub fn time_semantic_query<T>(&self, name: &'static str, run: impl FnOnce() -> T) -> T {
        let _scope = SemanticQueryScope(SEMANTIC_QUERY.with(|query| query.replace(Some(name))));
        let Some(query) = self.semantic_queries.get(name) else { return run() };
        Self::add(&query.entered, 1);
        let region = Region::start();
        let value = run();
        let (elapsed, own) = region.finish();
        Self::add_time(&query.nanos, elapsed);
        Self::add_time(&query.self_nanos, own);
        Self::add(&query.calls, 1);
        value
    }

    pub fn time_module_index<T>(&self, run: impl FnOnce() -> T) -> T {
        timed(&self.module_index_nanos, &self.module_index_calls, run)
    }

    /// Parses one unit (a file a semantic question reached), outside the
    /// self time of the query that reached it.
    pub fn time_unit_parse<T>(&self, run: impl FnOnce() -> T) -> T {
        let region = Region::start();
        let value = run();
        Self::add_time(&self.unit_parse_nanos, region.finish().0);
        Self::add(&self.unit_parses, 1);
        value
    }

    pub fn time_module_plans<T>(&self, run: impl FnOnce() -> T) -> T {
        timed(&self.module_plans_nanos, &self.module_plans_calls, run)
    }

    pub fn record_semantic_budget(&self, used: u64, limit: u64, limited: bool) {
        Self::add(&self.semantic_budget_steps_total, used);
        if limited { Self::add(&self.semantic_budget_steps_limited, used); }
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

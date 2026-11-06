//! Measuring builds only (`--features work`, run with `--jobs 1`): a work
//! count that repeats exactly whatever else the machine is doing, and a census
//! of the facts computed: per fact, its own work (nested facts excluded) by a
//! key that means the same in every linted file. Printed at exit: what was
//! computed, how many distinct answers, and how much work went on exact
//! repeats. `strict` keys name instantiations by their route, `loose` by what
//! they bind: repeats under `strict` are certain, under `loose` a ceiling.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::fx_hash::FxHashMap;

static VM: AtomicU64 = AtomicU64::new(0);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
/// Set while the census itself runs: its work is not the lint's.
static PAUSED: AtomicBool = AtomicBool::new(false);

#[inline]
pub fn vm() {
    if !PAUSED.load(Ordering::Relaxed) { VM.fetch_add(1, Ordering::Relaxed); }
}

#[inline]
pub fn alloc() {
    if !PAUSED.load(Ordering::Relaxed) {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        if SITES_ON.load(Ordering::Relaxed) { site_alloc() }
    }
}

thread_local! {
    /// Per open budgeted fact: the work of those inside it that ran out.
    static BUDGETED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}
static RAN_OUT: AtomicU64 = AtomicU64::new(0);
static RAN_OUT_FACTS: AtomicU64 = AtomicU64::new(0);

/// A budgeted fact starts; give what this returns to [`fact_end`].
pub fn fact_begin() -> u64 {
    BUDGETED.with(|open| open.borrow_mut().push(0));
    now()
}

/// That fact ended: all its work is wasted when it ran out of budget.
pub fn fact_end(started: u64, ran_out: bool) {
    let total = now() - started;
    let inner = BUDGETED.with(|open| open.borrow_mut().pop().unwrap_or(0));
    let wasted = if ran_out {
        RAN_OUT.fetch_add(total.saturating_sub(inner), Ordering::Relaxed);
        RAN_OUT_FACTS.fetch_add(1, Ordering::Relaxed);
        total
    } else { inner };
    BUDGETED.with(|open| if let Some(parent) = open.borrow_mut().last_mut() { *parent += wasted });
}

/// Budget steps of facts heavy enough to matter, against any budget worth
/// trying: one fact's own steps, nested facts excluded, decide if it passes.
static HEAVY_FACTS: Mutex<Vec<(u32, &'static str, String)>> = Mutex::new(Vec::new());
static FACTS_ENDED: AtomicU64 = AtomicU64::new(0);

/// A budgeted fact spent `steps` of its own allowance.
pub fn fact_steps(kind: &'static str, steps: u32, label: impl FnOnce() -> String) {
    PAUSED.store(true, Ordering::Relaxed);
    FACTS_ENDED.fetch_add(1, Ordering::Relaxed);
    if steps >= 50 {
        let label = label();
        keep_fact_sites(steps, kind, &label);
        HEAVY_FACTS.lock().expect("heavy facts").push((steps, kind, label));
    }
    PAUSED.store(false, Ordering::Relaxed);
}

static STEP_SITES: Mutex<Option<FxHashMap<usize, (&'static std::panic::Location<'static>, u64)>>> = Mutex::new(None);

type StepLocation = &'static std::panic::Location<'static>;

/// `DXLINT_WORK_FACT_SITES=<steps>`: facts spending this many of their own
/// steps also report where they spent them.
static FACT_SITES_FROM: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Per open fact, where its own steps went.
    static FACT_STEPS: RefCell<Vec<FxHashMap<usize, (StepLocation, u32)>>> = const { RefCell::new(Vec::new()) };
    /// Where the last fact to end spent its own steps.
    static LAST_FACT_STEPS: RefCell<FxHashMap<usize, (StepLocation, u32)>> = RefCell::new(FxHashMap::default());
}

static HEAVY_FACT_SITES: Mutex<Vec<(u32, String, Vec<(StepLocation, u32)>)>> = Mutex::new(Vec::new());

/// One budget step, charged at `at` in the source.
pub fn step_at(at: StepLocation) {
    let key = std::ptr::from_ref(at) as usize;
    if FACT_SITES_FROM.load(Ordering::Relaxed) != 0 {
        FACT_STEPS.with(|open| if let Some(top) = open.borrow_mut().last_mut() { top.entry(key).or_insert((at, 0)).1 += 1 });
    }
    let mut sites = STEP_SITES.lock().expect("step sites");
    sites.get_or_insert_with(FxHashMap::default).entry(key).or_insert((at, 0)).1 += 1;
}

pub fn fact_steps_open() {
    if FACT_SITES_FROM.load(Ordering::Relaxed) != 0 { FACT_STEPS.with(|open| open.borrow_mut().push(FxHashMap::default())) }
}

pub fn fact_steps_close() {
    if FACT_SITES_FROM.load(Ordering::Relaxed) == 0 { return }
    let top = FACT_STEPS.with(|open| open.borrow_mut().pop()).unwrap_or_default();
    LAST_FACT_STEPS.with(|last| *last.borrow_mut() = top);
}

fn keep_fact_sites(steps: u32, kind: &str, label: &str) {
    let from = FACT_SITES_FROM.load(Ordering::Relaxed);
    if from == 0 || u64::from(steps) < from { return }
    let mut sites: Vec<_> = LAST_FACT_STEPS.with(|last| last.borrow().values().copied().collect());
    sites.sort_by_key(|(_, steps)| std::cmp::Reverse(*steps));
    let kind = kind.rsplit("::").next().unwrap_or(kind);
    HEAVY_FACT_SITES.lock().expect("heavy fact sites").push((steps, format!("{kind} {label}"), sites));
}

fn report_fact_sites() {
    let mut heavy = HEAVY_FACT_SITES.lock().expect("heavy fact sites");
    heavy.sort_by_key(|(steps, ..)| std::cmp::Reverse(*steps));
    let mut lines: FxHashMap<usize, (StepLocation, u64)> = FxHashMap::default();
    for (_, _, sites) in heavy.iter() {
        for &(at, steps) in sites { lines.entry(std::ptr::from_ref(at) as usize).or_insert((at, 0)).1 += u64::from(steps) }
    }
    let mut lines: Vec<_> = lines.into_values().collect();
    lines.sort_by_key(|(_, steps)| std::cmp::Reverse(*steps));
    eprintln!("own steps of those {} facts, by where they are charged:", heavy.len());
    for (at, steps) in lines.into_iter().take(40) { eprintln!("{steps:>10}  {}:{}", at.file(), at.line()) }
    let mut seen = crate::fx_hash::FxHashSet::default();
    for (steps, label, sites) in heavy.iter().filter(|(_, label, _)| seen.insert(label.clone())).take(30) {
        let label: String = label.chars().take(120).map(|c| if c.is_control() { ' ' } else { c }).collect();
        eprintln!("{steps:>7} own steps  {label}");
        for (at, steps) in sites.iter().take(14) { eprintln!("{steps:>12}  {}:{}", at.file(), at.line()) }
    }
}

fn report_fact_steps() {
    if let Some(sites) = STEP_SITES.lock().expect("step sites").as_ref() {
        let mut rows: Vec<_> = sites.values().collect();
        rows.sort_by_key(|(_, steps)| std::cmp::Reverse(*steps));
        eprintln!("budget steps: {}, by where they are charged:", rows.iter().map(|(_, steps)| *steps).sum::<u64>());
        for (at, steps) in rows.into_iter().take(45) {
            eprintln!("{steps:>10}  {}:{}", at.file(), at.line());
        }
    }
    let heavy = HEAVY_FACTS.lock().expect("heavy facts");
    eprintln!("budgeted facts: {}", FACTS_ENDED.load(Ordering::Relaxed));
    for floor in [50, 100, 250, 500, 1000, 2500, 5000, 10000] {
        let over: Vec<_> = heavy.iter().filter(|(steps, ..)| *steps >= floor).collect();
        eprintln!("{:>9} own steps or more in {} facts, {} steps", floor, over.len(), over.iter().map(|(steps, ..)| u64::from(*steps)).sum::<u64>());
    }
    let mut named: FxHashMap<(&str, &str), (u32, u32, u64)> = FxHashMap::default();
    for (steps, kind, label) in heavy.iter() {
        let row = named.entry((*kind, label)).or_default();
        row.0 = row.0.max(*steps);
        row.1 += 1;
        row.2 += u64::from(*steps);
    }
    let mut named: Vec<_> = named.into_iter().collect();
    fn print(rows: &[((&str, &str), (u32, u32, u64))]) {
        for ((kind, label), (most, times, total)) in rows {
            let kind: String = kind.rsplit("::").next().unwrap_or(kind).chars().take(20).collect();
            let label: String = label.chars().take(110).map(|c| if c.is_control() { ' ' } else { c }).collect();
            eprintln!("{most:>7} x{times:<5} {total:>9} {kind:<20} {label}");
        }
    }
    eprintln!("facts spending most in one go, own steps (most, times, total):");
    named.sort_by_key(|(_, (most, ..))| std::cmp::Reverse(*most));
    print(&named[..named.len().min(80)]);
    eprintln!("facts spending most altogether:");
    named.sort_by_key(|(_, (.., total))| std::cmp::Reverse(*total));
    print(&named[..named.len().min(80)]);
    drop(heavy);
    report_fact_sites();
}

/// `DXLINT_WORK_SITES=1`: the census also keeps, per rule and `let`, the work
/// of each expression in it (slow).
static SITES_ON: AtomicBool = AtomicBool::new(false);

/// Reads the switches; call before linting.
pub fn start() {
    SITES_ON.store(std::env::var_os("DXLINT_WORK_SITES").is_some(), Ordering::Relaxed);
    let fact_sites = std::env::var("DXLINT_WORK_FACT_SITES").ok().and_then(|steps| steps.parse().ok());
    FACT_SITES_FROM.store(fact_sites.unwrap_or(0), Ordering::Relaxed);
}

#[derive(Default)]
struct Site { evals: u64, allocs: u64, label: String }

thread_local! {
    /// The expressions being evaluated, innermost last.
    static SITE_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    /// Where the open rule or `let` started on [`SITE_STACK`]'s owners.
    static OWNERS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

static SITES: Mutex<Option<FxHashMap<(&'static str, usize), Site>>> = Mutex::new(None);

fn site_alloc() {
    PAUSED.store(true, Ordering::Relaxed);
    let at = SITE_STACK.try_with(|stack| stack.borrow().last().copied()).ok().flatten();
    let owner = OWNERS.try_with(|owners| owners.borrow().last().copied()).ok().flatten();
    if let (Some(at), Some(owner)) = (at, owner) {
        if let Some(site) = SITES.lock().expect("sites").get_or_insert_with(FxHashMap::default).get_mut(&(owner, at)) {
            site.allocs += 1;
        }
    }
    PAUSED.store(false, Ordering::Relaxed);
}

pub struct SiteGuard(bool);

/// One evaluation of the expression at `at`, until the guard drops; `label`
/// names it when first seen.
pub fn site(at: usize, label: impl FnOnce() -> String) -> SiteGuard {
    if !SITES_ON.load(Ordering::Relaxed) || PAUSED.load(Ordering::Relaxed) { return SiteGuard(false) }
    PAUSED.store(true, Ordering::Relaxed);
    SITE_STACK.with(|stack| stack.borrow_mut().push(at));
    if let Some(owner) = OWNERS.with(|owners| owners.borrow().last().copied()) {
        let mut sites = SITES.lock().expect("sites");
        let site = sites.get_or_insert_with(FxHashMap::default).entry((owner, at)).or_default();
        if site.evals == 0 { site.label = label() }
        site.evals += 1;
    }
    PAUSED.store(false, Ordering::Relaxed);
    SiteGuard(true)
}

impl Drop for SiteGuard {
    fn drop(&mut self) {
        if !self.0 { return }
        PAUSED.store(true, Ordering::Relaxed);
        SITE_STACK.with(|stack| { stack.borrow_mut().pop(); });
        PAUSED.store(false, Ordering::Relaxed);
    }
}

/// Charges expression work to `name` until the guard drops.
pub struct OwnerGuard(bool);

pub fn owner(name: &str) -> OwnerGuard {
    if !SITES_ON.load(Ordering::Relaxed) { return OwnerGuard(false) }
    PAUSED.store(true, Ordering::Relaxed);
    let name = intern(name);
    OWNERS.with(|owners| owners.borrow_mut().push(name));
    PAUSED.store(false, Ordering::Relaxed);
    OwnerGuard(true)
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        if !self.0 { return }
        PAUSED.store(true, Ordering::Relaxed);
        OWNERS.with(|owners| { owners.borrow_mut().pop(); });
        PAUSED.store(false, Ordering::Relaxed);
    }
}

fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Option<FxHashMap<String, &'static str>>> = Mutex::new(None);
    let mut names = NAMES.lock().expect("names");
    let names = names.get_or_insert_with(FxHashMap::default);
    if let Some(&known) = names.get(name) { return known }
    let kept: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(name.to_owned(), kept);
    kept
}

fn now() -> u64 {
    VM.load(Ordering::Relaxed) + ALLOCS.load(Ordering::Relaxed)
}

thread_local! {
    /// Per open fact, its nested facts' work so far.
    static OPEN: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

struct Fact {
    calls: u64,
    own: u64,
    /// With what ran nested in it, charged as whoever asked first was.
    inclusive: u64,
    first: u64,
    label: String,
    loose: u64,
}

static FACTS: Mutex<Option<FxHashMap<(&'static str, u64), Fact>>> = Mutex::new(None);

pub struct Guard {
    kind: &'static str,
    strict: u64,
    loose: u64,
    label: String,
    start: u64,
}

/// Opens fact `kind` until the guard drops; `key` gives its strict key,
/// loose key and a label for reading the report.
pub fn enter(kind: &'static str, key: impl FnOnce() -> (u64, u64, String)) -> Guard {
    PAUSED.store(true, Ordering::Relaxed);
    let (strict, loose, label) = key();
    OPEN.with(|open| open.borrow_mut().push(0));
    PAUSED.store(false, Ordering::Relaxed);
    Guard { kind, strict, loose, label, start: now() }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let total = now() - self.start;
        PAUSED.store(true, Ordering::Relaxed);
        let nested = OPEN.with(|open| {
            let mut open = open.borrow_mut();
            let nested = open.pop().unwrap_or(0);
            if let Some(parent) = open.last_mut() { *parent += total }
            nested
        });
        let own = total.saturating_sub(nested);
        let mut facts = FACTS.lock().expect("census");
        let fact = facts.get_or_insert_with(FxHashMap::default).entry((self.kind, self.strict)).or_insert_with(|| Fact {
            calls: 0, own: 0, inclusive: 0, first: own, label: std::mem::take(&mut self.label), loose: self.loose,
        });
        fact.calls += 1;
        fact.own += own;
        fact.inclusive += total;
        drop(facts);
        PAUSED.store(false, Ordering::Relaxed);
    }
}

/// Prints the work and the census.
pub fn report() {
    PAUSED.store(true, Ordering::Relaxed);
    let (vm, allocs) = (VM.load(Ordering::Relaxed), ALLOCS.load(Ordering::Relaxed));
    eprintln!("work: {} = {vm} vm + {allocs} allocs", vm + allocs);
    eprintln!("in facts that ran out of budget: {} work, {} facts", RAN_OUT.load(Ordering::Relaxed), RAN_OUT_FACTS.load(Ordering::Relaxed));
    report_fact_steps();
    let facts = FACTS.lock().expect("census");
    let Some(facts) = facts.as_ref() else { return };
    #[derive(Default)]
    struct Kind { calls: u64, strict: u64, own: u64, strict_repeat: u64, loose: FxHashMap<u64, u64>, loose_first: FxHashMap<u64, u64> }
    let mut kinds: FxHashMap<&str, Kind> = FxHashMap::default();
    for ((name, _), fact) in facts {
        let kind = kinds.entry(name).or_default();
        kind.calls += fact.calls;
        kind.strict += 1;
        kind.own += fact.own;
        // A rule on another node, a `let` on another node: nyot the same answer.
        if !matches!(*name, "rule" | "let" | "parse") { kind.strict_repeat += fact.own - fact.first; }
        *kind.loose.entry(fact.loose).or_default() += fact.own;
        let first = kind.loose_first.entry(fact.loose).or_insert(fact.first);
        *first = (*first).max(fact.first);
    }
    let mut rows: Vec<_> = kinds.into_iter().collect();
    rows.sort_by_key(|(_, kind)| std::cmp::Reverse(kind.own));
    let attributed: u64 = rows.iter().map(|(_, kind)| kind.own).sum();
    eprintln!("census: {attributed} work in facts ({}% of all)", attributed * 100 / (vm + allocs).max(1));
    eprintln!("{:>12} {:>9} {:>9} {:>9} {:>12} {:>12}  fact", "own work", "calls", "distinct", "loose", "repeat", "loose repeat");
    let (mut repeat, mut loose_repeat) = (0, 0);
    for (name, kind) in &rows {
        let loose: u64 = kind.loose.iter().map(|(key, own)| own - kind.loose_first[key].min(*own)).sum();
        repeat += kind.strict_repeat;
        loose_repeat += loose;
        let short: String = name.chars().take(170).collect();
        eprintln!("{:>12} {:>9} {:>9} {:>9} {:>12} {:>12}  {short}", kind.own, kind.calls, kind.strict, kind.loose.len(), kind.strict_repeat, loose);
    }
    eprintln!("heaviest answers of each fact:");
    for (name, _) in rows.iter().filter(|(name, _)| !matches!(*name, "rule" | "let" | "let.compute.linted" | "let.compute.foreign" | "attribute")).take(16) {
        let mut heavy: Vec<_> = facts.iter().filter(|((kind, _), _)| kind == name).map(|(_, fact)| fact).collect();
        heavy.sort_by_key(|fact| std::cmp::Reverse(fact.own));
        let short: String = name.chars().take(60).collect();
        eprintln!("  {short}");
        for fact in heavy.into_iter().take(6) {
            let label: String = fact.label.chars().take(100).collect();
            eprintln!("{:>12} own x{:<7} {label}", fact.own, fact.calls);
        }
    }
    eprintln!("repeats: {repeat} certain, {loose_repeat} ceiling, of {attributed}");
    let mut top: Vec<_> = facts.iter().filter(|((name, _), fact)| fact.calls > 1 && !matches!(*name, "rule" | "let" | "parse")).collect();
    top.sort_by_key(|(_, fact)| std::cmp::Reverse(fact.own - fact.first));
    let mut rules: Vec<_> = facts.iter().filter(|((name, _), _)| *name == "rule").map(|(_, fact)| fact).collect();
    rules.sort_by_key(|fact| std::cmp::Reverse(fact.own));
    eprintln!("rules by their own work (inclusive: first-asker charged, nyot a cost):");
    for fact in rules.iter().take(60) {
        eprintln!("{:>12} own {:>12} inclusive {:>9} calls {:>7}/call  {}", fact.own, fact.inclusive, fact.calls, fact.own / fact.calls.max(1), fact.label);
    }
    for (kind, title) in [("attribute", "attributes under questions"), ("let.compute.linted", "`let`s of linted files"), ("let.compute.foreign", "`let`s of other files")] {
        let mut named: FxHashMap<&str, (u64, u64, u64)> = FxHashMap::default();
        for (_, fact) in facts.iter().filter(|((name, _), _)| *name == kind) {
            let row = named.entry(fact.label.split(' ').next().unwrap_or("")).or_default();
            row.0 += fact.own;
            row.1 += fact.calls;
            row.2 += 1;
        }
        let mut named: Vec<_> = named.into_iter().collect();
        named.sort_by_key(|(_, row)| std::cmp::Reverse(row.0));
        eprintln!("{title}, by their own work:");
        for (name, (own, calls, distinct)) in named.into_iter().take(60) {
            eprintln!("{own:>12} own {calls:>9} calls {distinct:>9} distinct {:>6}/call  {name}", own / calls.max(1));
        }
    }
    eprintln!("most repeated work (certain):");
    for ((name, _), fact) in top.into_iter().take(25) {
        let short: String = name.rsplit("::").next().unwrap_or(name).chars().take(24).collect();
        eprintln!("{:>10} x{:<6} {short:<24} {}", fact.own - fact.first, fact.calls, fact.label);
    }
    let sites = SITES.lock().expect("sites");
    let Some(sites) = sites.as_ref() else { return };
    let mut owners: FxHashMap<&str, Vec<&Site>> = FxHashMap::default();
    for ((owner, _), site) in sites { owners.entry(owner).or_default().push(site) }
    let cost = |site: &Site| site.evals + site.allocs;
    // What kind of expression work goes on, whoever runs it.
    let mut heads: FxHashMap<&str, (u64, u64)> = FxHashMap::default();
    for site in sites.values() {
        let head = site.label.find(['(', ' ', '.']).filter(|&at| at > 0).map_or(site.label.as_str(), |at| &site.label[..at]);
        let row = heads.entry(head).or_default();
        row.0 += site.evals;
        row.1 += site.allocs;
    }
    let mut heads: Vec<_> = heads.into_iter().collect();
    heads.sort_by_key(|(_, (evals, allocs))| std::cmp::Reverse(evals + allocs));
    eprintln!("expression kinds (evals allocs):");
    for (head, (evals, allocs)) in heads.into_iter().take(70) {
        eprintln!("{evals:>12} {allocs:>12}  {head}");
    }
    let mut owners: Vec<_> = owners.into_iter().map(|(owner, sites)| (owner, sites.iter().map(|site| cost(site)).sum::<u64>(), sites)).collect();
    owners.sort_by_key(|(_, total, _)| std::cmp::Reverse(*total));
    eprintln!("expressions, per rule and `let` (evals allocs):");
    for (owner, total, mut sites) in owners.into_iter().take(50) {
        eprintln!("{total:>12}  {owner}");
        sites.sort_by_key(|site| std::cmp::Reverse(cost(site)));
        for site in sites.into_iter().take(14) {
            eprintln!("{:>12} {:>10} {:>10}  {}", "", site.evals, site.allocs, site.label);
        }
    }
}

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

/// `DXLINT_WORK_SITES=1`: the census also keeps, per rule and `let`, the work
/// of each expression in it (slow).
static SITES_ON: AtomicBool = AtomicBool::new(false);

/// Reads the switches; call before linting.
pub fn start() {
    SITES_ON.store(std::env::var_os("DXLINT_WORK_SITES").is_some(), Ordering::Relaxed);
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

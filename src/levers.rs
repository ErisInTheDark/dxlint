//! Experimental engine levers, switched per process by
//! `ZLINT_LEVERS=<name>,<name>` (`all` turns every one on), so one build
//! can measure many changes against each other. A lever lives here only
//! until it is measured: kept levers lose their switch, rejected ones their
//! code. Off by default; with none set output is the released behaviour.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Lever {
    /// `skip narrowing` for every config (conformance runs build their own).
    NoNarrowing,
    /// Generic signatures infer nothing: their instantiation is unknown.
    NoInference,
    /// `skip selection` for every config (conformance runs build their own).
    NoConditional,
    /// Reads whose declared value has one alternative skip narrowing
    /// (parity ledger: guards on a lone `unknown`/class value are lost).
    LoneSkip,
    /// Counts root questions by outcome and time, and prints each root that
    /// ends unknown to stderr (`UNK query us file text`); no answer changes.
    Census,
}

const NAMES: &[(&str, Lever)] = &[
    ("no_narrowing", Lever::NoNarrowing),
    ("no_inference", Lever::NoInference),
    ("no_conditional", Lever::NoConditional),
    ("lone_skip", Lever::LoneSkip),
    ("census", Lever::Census),
];

/// Lever counters, printed to stderr at exit when any is nonzero.
static COUNTS: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];

pub fn count(slot: usize) {
    add(slot, 1);
}

pub fn add(slot: usize, amount: u64) {
    COUNTS[slot].fetch_add(amount, Ordering::Relaxed);
}

pub fn report() {
    let counts: Vec<u64> = COUNTS.iter().map(|count| count.load(Ordering::Relaxed)).collect();
    if counts.iter().any(|&count| count != 0) { eprintln!("lever counts {counts:?}") }
}

/// Whether `lever` is switched on for this process.
pub fn on(lever: Lever) -> bool {
    static SET: OnceLock<u64> = OnceLock::new();
    let set = *SET.get_or_init(|| {
        let Some(list) = std::env::var_os("ZLINT_LEVERS") else { return 0 };
        list.to_string_lossy().split(',').map(str::trim).fold(0, |set, name| match name {
            "all" => u64::MAX,
            name => NAMES.iter().find(|(known, _)| *known == name).map_or(set, |&(_, lever)| set | 1 << lever as u8),
        })
    });
    set & 1 << lever as u8 != 0
}

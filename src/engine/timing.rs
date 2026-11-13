//! Where a run's file work went (`--stats`): time in each phase, summed over
//! threads. Phases nest (a program's setup parses files): each moment counts
//! for the outermost phase only, so the phases add up.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Clone, Copy)]
pub enum Phase {
    /// Checking whether a file's cached result still holds.
    Cache,
    /// Parsing a file (linted or another file read for its types).
    Parse,
    /// Building a program: its input list and global names' candidates.
    Program,
    /// Waiting on a configured command.
    Commands,
}

const PHASES: usize = 4;
const NAMES: [&str; PHASES] = ["cache checks", "parsing", "program setup", "commands"];

static NANOS: [AtomicU64; PHASES] = [const { AtomicU64::new(0) }; PHASES];
static COUNTS: [AtomicU64; PHASES] = [const { AtomicU64::new(0) }; PHASES];
/// Linting whole files, phases included.
static FILES: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

/// Counts the time until it drops for its phase, unless an outer phase does.
pub struct Timed(Option<(Phase, Instant)>);

impl Drop for Timed {
    fn drop(&mut self) {
        let Some((phase, started)) = self.0 else { return };
        NANOS[phase as usize].fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        COUNTS[phase as usize].fetch_add(1, Ordering::Relaxed);
        INSIDE.with(|inside| inside.set(false));
    }
}

pub fn enter(phase: Phase) -> Timed {
    if INSIDE.with(|inside| inside.replace(true)) { return Timed(None) }
    Timed(Some((phase, Instant::now())))
}

/// Times one file's whole lint.
pub fn file<T>(lint: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let found = lint();
    FILES.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
    found
}

/// `--stats`' line: each phase's seconds (and times entered), and the rest
/// of the file work (rules and type questions).
pub fn summary() -> String {
    let seconds = |nanos: u64| nanos as f64 / 1e9;
    let mut phases = 0;
    let mut parts: Vec<String> = (0..PHASES).map(|at| {
        let nanos = NANOS[at].load(Ordering::Relaxed);
        phases += nanos;
        format!("{} {:.1} s ({})", NAMES[at], seconds(nanos), COUNTS[at].load(Ordering::Relaxed))
    }).collect();
    parts.push(format!("rules and types {:.1} s", seconds(FILES.load(Ordering::Relaxed).saturating_sub(phases))));
    format!("file work, summed over threads: {}", parts.join(", "))
}

//! Opt-in timing of declared parses' phases, for parser profiling
//! (`DXLINT_SYNTAX_PHASES`). Off unless [`set_phase_profiling`] turns it on;
//! each thread times its own parses and adds them to the process totals as
//! each parse ends.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// One phase's accumulated time: `self_ns` excludes nested phases.
#[derive(Clone, Debug)]
pub struct PhaseTotal {
    pub name: &'static str,
    pub inclusive_ns: u64,
    pub self_ns: u64,
    pub calls: u64,
}

#[derive(Default)]
struct State {
    totals: Vec<PhaseTotal>,
    /// Open phases: index into `totals`, start, time spent in nested phases.
    open: Vec<(usize, Instant, u64)>,
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static TOTALS: Mutex<Vec<PhaseTotal>> = Mutex::new(Vec::new());

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn set_phase_profiling(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    STATE.with(|state| *state.borrow_mut() = None);
}

/// The totals since profiling was enabled or last taken, every thread's.
pub fn take_phase_totals() -> Vec<PhaseTotal> {
    flush();
    std::mem::take(&mut *TOTALS.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// Adds this thread's totals to the process's, once no phase is open.
pub(super) fn flush() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut().filter(|state| state.open.is_empty()) else { return };
        let mut totals = TOTALS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for total in state.totals.drain(..) {
            match totals.iter_mut().find(|known| known.name == total.name) {
                Some(known) => {
                    known.inclusive_ns += total.inclusive_ns;
                    known.self_ns += total.self_ns;
                    known.calls += total.calls;
                }
                None => totals.push(total),
            }
        }
    });
}

fn with_state<T>(run: impl FnOnce(&mut State) -> T) -> Option<T> {
    if !ENABLED.load(Ordering::Relaxed) { return None }
    STATE.with(|state| run(state.borrow_mut().get_or_insert_with(State::default)).into())
}

/// Closes its phase when dropped, so an unwinding parse leaves no frame open.
struct Open;

impl Drop for Open {
    fn drop(&mut self) {
        with_state(|state| {
            let Some((index, start, nested)) = state.open.pop() else { return };
            let elapsed = start.elapsed().as_nanos() as u64;
            let total = &mut state.totals[index];
            total.inclusive_ns += elapsed;
            total.self_ns += elapsed.saturating_sub(nested);
            total.calls += 1;
            if let Some(parent) = state.open.last_mut() { parent.2 += elapsed }
        });
    }
}

/// Adds `amount` to a counter reported as a phase with calls but no time.
pub(super) fn count(name: &'static str, amount: u64) {
    with_state(|state| match state.totals.iter_mut().find(|total| total.name == name) {
        Some(total) => total.calls += amount,
        None => state.totals.push(PhaseTotal { name, inclusive_ns: 0, self_ns: 0, calls: amount }),
    });
}

pub(super) fn phase<T>(name: &'static str, run: impl FnOnce() -> T) -> T {
    let _open = with_state(|state| {
        let index = match state.totals.iter().position(|total| total.name == name) {
            Some(index) => index,
            None => {
                state.totals.push(PhaseTotal { name, inclusive_ns: 0, self_ns: 0, calls: 0 });
                state.totals.len() - 1
            }
        };
        state.open.push((index, Instant::now(), 0));
        Open
    });
    run()
}

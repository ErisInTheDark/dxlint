//! Opt-in timing of a declared parse's phases, per thread, for parser
//! profiling harnesses. Off unless [`set_phase_profiling`] turns it on.

use std::cell::RefCell;
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

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn set_phase_profiling(enabled: bool) {
    STATE.with(|state| *state.borrow_mut() = enabled.then(State::default));
}

/// The totals since profiling was enabled or last taken.
pub fn take_phase_totals() -> Vec<PhaseTotal> {
    STATE.with(|state| state.borrow_mut().as_mut().map(|state| std::mem::take(&mut state.totals)).unwrap_or_default())
}

/// Closes its phase when dropped, so an unwinding parse leaves no frame open.
struct Open;

impl Drop for Open {
    fn drop(&mut self) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut() else { return };
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
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else { return };
        match state.totals.iter_mut().find(|total| total.name == name) {
            Some(total) => total.calls += amount,
            None => state.totals.push(PhaseTotal { name, inclusive_ns: 0, self_ns: 0, calls: amount }),
        }
    });
}

pub(super) fn phase<T>(name: &'static str, run: impl FnOnce() -> T) -> T {
    let _open = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut()?;
        let index = match state.totals.iter().position(|total| total.name == name) {
            Some(index) => index,
            None => {
                state.totals.push(PhaseTotal { name, inclusive_ns: 0, self_ns: 0, calls: 0 });
                state.totals.len() - 1
            }
        };
        state.open.push((index, Instant::now(), 0));
        Some(Open)
    });
    run()
}

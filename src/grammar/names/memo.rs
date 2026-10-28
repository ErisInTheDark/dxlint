use std::cell::{Cell, RefCell};
use std::hash::Hash;

use crate::fx_hash::FxHashMap;

/// One kind of memoised semantic answer for a file's session. Every entry
/// is a fact computed on its own allowance ([`super::question::Budget::fact`]),
/// so it stays right whichever root question wrote it.
///
/// Each entry also keeps the linted file's bodies its computation entered
/// ([`body`] bits, a superset), so an editor session can keep every entry
/// an edit inside one body leaves standing ([`Memo::forget`]).
pub(super) struct Memo<K, V> {
    entries: RefCell<FxHashMap<K, (V, u64)>>,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self { entries: RefCell::default() }
    }
}

impl<K: Eq + Hash, V: Clone> Memo<K, V> {
    pub(super) fn get(&self, key: &K) -> Option<V> {
        let entries = self.entries.borrow();
        let (value, bodies) = entries.get(key)?;
        enter(*bodies);
        Some(value.clone())
    }

    /// Kept with the bodies entered so far by the computation it answers
    /// (and, unless it began its own, by the ones around it).
    pub(super) fn insert(&self, key: K, value: V) {
        self.entries.borrow_mut().insert(key, (value, ENTERED.with(Cell::get)));
    }

    pub(super) fn remove(&self, key: &K) {
        self.entries.borrow_mut().remove(key);
    }
}

impl<K, V> Memo<K, V> {
    /// Drops the entries whose computation may have entered a body of `bodies`.
    pub(super) fn forget(&self, bodies: u64) {
        self.entries.borrow_mut().retain(|_, (_, entered)| *entered & bodies == 0);
    }

    /// [`Memo::forget`], also dropping the values `keep` refuses.
    pub(super) fn forget_unless(&self, bodies: u64, keep: impl Fn(&V) -> bool) {
        self.entries.borrow_mut().retain(|_, (value, entered)| *entered & bodies == 0 && keep(value));
    }


}

thread_local! {
    /// The linted file's bodies the computation in progress entered.
    static ENTERED: Cell<u64> = const { Cell::new(0) };
}

/// The bit of the linted file's outermost body number `index`: bits are
/// shared modulo 63, which only makes edits forget more.
pub(crate) fn body(index: usize) -> u64 {
    1 << (index % 63)
}

/// Read a whole-file scan of the linted file (its guards' tested roots):
/// kept after an edit while the scan's answer stays the same.
pub(crate) const SCAN: u64 = 1 << 63;

/// Every body, and the scan.
pub(crate) const ALL: u64 = !0;

/// Work in progress entered these bodies.
pub(crate) fn enter(bodies: u64) {
    ENTERED.with(|entered| entered.set(entered.get() | bodies));
}

/// Starts a computation of its own; [`end`] with the returned value.
pub(super) fn begin() -> u64 {
    ENTERED.with(|entered| entered.replace(0))
}

/// Replaces what the computation in progress entered.
pub(super) fn set(bodies: u64) {
    ENTERED.with(|entered| entered.set(bodies));
}

/// Ends the computation [`begin`] started: what it entered, the work around
/// it entered too.
pub(super) fn end(outer: u64) -> u64 {
    ENTERED.with(|entered| {
        let own = entered.get();
        entered.set(outer | own);
        own
    })
}

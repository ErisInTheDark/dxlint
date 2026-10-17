use std::cell::{Cell, RefCell};
use std::hash::Hash;

use crate::fx_hash::FxHashMap;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Epoch(u64);

impl Epoch {
    pub(super) fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("semantic root epoch exhausted"))
    }
}

thread_local! {
    /// Orders every memo write on this thread, so a root cut short by its
    /// budget can tell writes made before the cut (complete answers) from
    /// writes made after it.
    static WRITES: Cell<u64> = const { Cell::new(0) };
}

/// The order number the next memo write gets.
pub(super) fn write_order() -> u64 {
    WRITES.with(Cell::get)
}

fn next_write() -> u64 {
    WRITES.with(|writes| {
        let order = writes.get();
        writes.set(order + 1);
        order
    })
}

/// Whether a memoised answer is complete: kept from a root its budget cut
/// short when written before the cut. Unknown answers (including cycle
/// placeholders) are never kept.
pub(super) trait Settled {
    fn settled(&self) -> bool;
}

impl<T> Settled for super::question::Known<T> {
    fn settled(&self) -> bool { matches!(self, super::question::Known::Yes(_)) }
}

impl Settled for super::question::NodeValue {
    fn settled(&self) -> bool { !matches!(self, super::question::NodeValue::Unknown) }
}

#[derive(Clone)]
struct Entry<V> {
    epoch: Epoch,
    value: V,
    order: u64,
}

struct Writes<K, V> {
    epoch: Epoch,
    undo: Vec<(K, Option<Entry<V>>)>,
}

pub(super) struct Memo<K, V> {
    entries: RefCell<FxHashMap<K, Entry<V>>>,
    writes: RefCell<Writes<K, V>>,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self {
            entries: RefCell::default(),
            writes: RefCell::new(Writes { epoch: Epoch::default(), undo: Vec::new() }),
        }
    }
}

impl<K: Clone + Eq + Hash, V: Clone> Memo<K, V> {
    pub(super) fn get(&self, key: &K) -> Option<V> {
        self.entries.borrow().get(key).map(|entry| entry.value.clone())
    }

    pub(super) fn insert(&self, epoch: Epoch, key: K, value: V) {
        let mut entries = self.entries.borrow_mut();
        let mut writes = self.writes.borrow_mut();
        if writes.epoch != epoch {
            writes.undo.clear();
            writes.epoch = epoch;
        }
        let entry = Entry { epoch, value, order: next_write() };
        match entries.entry(key) {
            std::collections::hash_map::Entry::Occupied(mut slot) => {
                if slot.get().epoch != epoch { writes.undo.push((slot.key().clone(), Some(slot.get().clone()))) }
                slot.insert(entry);
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                writes.undo.push((slot.key().clone(), None));
                slot.insert(entry);
            }
        }
    }

    /// Forgets `key` (undone like an insert if the root fails).
    pub(super) fn remove(&self, epoch: Epoch, key: &K) {
        let mut entries = self.entries.borrow_mut();
        let mut writes = self.writes.borrow_mut();
        if writes.epoch != epoch {
            writes.undo.clear();
            writes.epoch = epoch;
        }
        if let Some(previous) = entries.remove(key) {
            if previous.epoch != epoch { writes.undo.push((key.clone(), Some(previous))) }
        }
    }

    /// Ends a root. A root cut short by its budget at write order `cut`
    /// undoes its writes, except answers last written before the cut that
    /// `keep` accepts: they were complete before the budget ran out, and
    /// dropping them made every later root redo the same work.
    pub(super) fn finish(&self, epoch: Epoch, failed: bool, cut: u64, keep: impl Fn(&V) -> bool) -> u64 {
        let mut writes = self.writes.borrow_mut();
        if writes.epoch != epoch { return 0 }
        if !failed {
            writes.undo.clear();
            return 0;
        }
        let mut discarded = 0;
        let mut entries = self.entries.borrow_mut();
        while let Some((key, previous)) = writes.undo.pop() {
            let complete = entries.get(&key).is_some_and(|entry| entry.epoch == epoch && entry.order < cut && keep(&entry.value));
            if complete { continue }
            discarded += 1;
            match previous {
                Some(entry) => { entries.insert(key, entry); }
                None => { entries.remove(&key); }
            }
        }
        discarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::names::question::Known;

    #[test]
    fn failed_roots_undo_each_written_key_once_and_keep_committed_answers() {
        let memo = Memo::default();
        let committed = Epoch::default();
        memo.insert(committed, "changed", Known::Yes(true));
        memo.insert(committed, "untouched", Known::Yes(false));
        memo.finish(committed, false, u64::MAX, Settled::settled);

        let failed = committed.next();
        let cut = write_order();
        for key in ["changed", "new"] {
            memo.insert(failed, key, Known::Unknown);
            memo.insert(failed, key, Known::Yes(false));
        }
        assert_eq!(memo.finish(failed, true, cut, Settled::settled), 2);
        assert_eq!(memo.get(&"changed"), Some(Known::Yes(true)));
        assert_eq!(memo.get(&"untouched"), Some(Known::Yes(false)));
        assert_eq!(memo.get(&"new"), None);

        let retry = failed.next();
        memo.insert(retry, "new", Known::Yes(true));
        memo.finish(retry, false, u64::MAX, Settled::settled);
        assert_eq!(memo.finish(retry.next(), true, write_order(), Settled::settled), 0);
        assert_eq!(memo.get(&"new"), Some(Known::Yes(true)));
    }

    #[test]
    fn a_root_cut_short_keeps_complete_answers_written_before_the_cut() {
        let memo = Memo::default();
        let root = Epoch::default().next();
        memo.insert(root, "finished", Known::Yes(true));
        memo.insert(root, "gave up", Known::<bool>::Unknown);
        memo.insert(root, "rewritten", Known::Yes(false));
        let cut = write_order();
        memo.insert(root, "rewritten", Known::Yes(true));
        memo.insert(root, "after", Known::Yes(true));
        assert_eq!(memo.finish(root, true, cut, Settled::settled), 3);
        assert_eq!(memo.get(&"finished"), Some(Known::Yes(true)));
        assert_eq!(memo.get(&"gave up"), None);
        assert_eq!(memo.get(&"rewritten"), None);
        assert_eq!(memo.get(&"after"), None);
    }
}

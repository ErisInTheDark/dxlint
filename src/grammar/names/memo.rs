use std::cell::RefCell;
use std::hash::Hash;

use crate::fx_hash::FxHashMap;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Epoch(u64);

impl Epoch {
    pub(super) fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("semantic root epoch exhausted"))
    }
}

#[derive(Clone)]
struct Entry<V> {
    epoch: Epoch,
    value: V,
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
        if entries.get(&key).is_none_or(|entry| entry.epoch != epoch) {
            writes.undo.push((key.clone(), entries.get(&key).cloned()));
        }
        entries.insert(key, Entry { epoch, value });
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

    pub(super) fn finish(&self, epoch: Epoch, failed: bool) -> u64 {
        let mut writes = self.writes.borrow_mut();
        if writes.epoch != epoch { return 0 }
        if !failed {
            writes.undo.clear();
            return 0;
        }
        let discarded = writes.undo.len() as u64;
        let mut entries = self.entries.borrow_mut();
        while let Some((key, previous)) = writes.undo.pop() {
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
        memo.finish(committed, false);

        let failed = committed.next();
        for key in ["changed", "new"] {
            memo.insert(failed, key, Known::Unknown);
            memo.insert(failed, key, Known::Yes(false));
        }
        assert_eq!(memo.finish(failed, true), 2);
        assert_eq!(memo.get(&"changed"), Some(Known::Yes(true)));
        assert_eq!(memo.get(&"untouched"), Some(Known::Yes(false)));
        assert_eq!(memo.get(&"new"), None);

        let retry = failed.next();
        memo.insert(retry, "new", Known::Yes(true));
        memo.finish(retry, false);
        assert_eq!(memo.finish(retry.next(), true), 0);
        assert_eq!(memo.get(&"new"), Some(Known::Yes(true)));
    }
}

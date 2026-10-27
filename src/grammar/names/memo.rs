use std::cell::RefCell;
use std::hash::Hash;

use crate::fx_hash::FxHashMap;

/// One kind of memoised semantic answer for a file's session. Every entry
/// is a fact computed on its own allowance ([`super::question::Budget::fact`]),
/// so it stays right whichever root question wrote it.
pub(super) struct Memo<K, V> {
    entries: RefCell<FxHashMap<K, V>>,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self { entries: RefCell::default() }
    }
}

impl<K: Eq + Hash, V: Clone> Memo<K, V> {
    pub(super) fn get(&self, key: &K) -> Option<V> {
        self.entries.borrow().get(key).cloned()
    }

    pub(super) fn insert(&self, key: K, value: V) {
        self.entries.borrow_mut().insert(key, value);
    }

    pub(super) fn remove(&self, key: &K) {
        self.entries.borrow_mut().remove(key);
    }
}

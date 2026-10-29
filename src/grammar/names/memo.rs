use std::cell::RefCell;
use std::hash::Hash;

use crate::footprint::{self, Dirty, Footprint};
use crate::fx_hash::FxHashMap;

/// One kind of memoised semantic answer for a file's session. Every entry
/// is a fact computed on its own allowance ([`super::question::Budget::fact`]),
/// so it stays right whichever root question wrote it.
///
/// Each entry also keeps what its computation read of the editor's file
/// ([`crate::footprint`]), handed to every reader, so an editor session can
/// keep every entry an edit leaves standing ([`Memo::forget`]).
pub(super) struct Memo<K, V> {
    entries: RefCell<FxHashMap<K, (V, Footprint)>>,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self { entries: RefCell::default() }
    }
}

impl<K: Eq + Hash, V: Clone> Memo<K, V> {
    pub(super) fn get(&self, key: &K) -> Option<V> {
        let entries = self.entries.borrow();
        let (value, read) = entries.get(key)?;
        footprint::enter(read);
        Some(value.clone())
    }

    /// Kept with what the computation it answers read so far (and, unless
    /// it began its own, what the work around it read).
    pub(super) fn insert(&self, key: K, value: V) {
        self.entries.borrow_mut().insert(key, (value, footprint::current()));
    }

    /// Kept with `read`: what computing it read.
    pub(super) fn insert_read(&self, key: K, value: V, read: Footprint) {
        self.entries.borrow_mut().insert(key, (value, read));
    }

    pub(super) fn remove(&self, key: &K) {
        self.entries.borrow_mut().remove(key);
    }
}

impl<K, V> Memo<K, V> {
    /// Drops the entries `dirty`'s edit may change; the rest move to the
    /// edited text's positions.
    pub(super) fn forget(&self, dirty: &Dirty) {
        self.forget_unless(dirty, |_| true);
    }

    /// [`Memo::forget`], also dropping the values `keep` refuses.
    pub(super) fn forget_unless(&self, dirty: &Dirty, keep: impl Fn(&V) -> bool) {
        let mut entries = self.entries.borrow_mut();
        entries.retain(|_, (value, read)| {
            let stays = !read.meets(dirty) && keep(value);
            if stays { read.shift(dirty) }
            stays
        });
    }
}

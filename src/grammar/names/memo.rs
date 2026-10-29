use std::cell::RefCell;
use std::hash::Hash;

use crate::footprint::{self, Atom, Dirty, Footprint};
use crate::fx_hash::FxHashMap;

/// One kind of memoised semantic answer for a file's session. Every entry
/// is a fact computed on its own allowance ([`super::question::Budget::fact`]),
/// so it stays right whichever root question wrote it.
///
/// Each entry also keeps what its computation read of the editor's file
/// ([`crate::footprint`]), handed to every reader, so an editor session can
/// keep every entry an edit leaves standing ([`Memo::forget`]).
///
/// A walled memo ([`Walled`]) hands its readers one atom per entry instead:
/// an entry the edit reached is computed again ([`Memo::take_suspects`])
/// and changes its readers only when its answer does.
pub(super) struct Memo<K, V> {
    entries: RefCell<FxHashMap<K, (V, Footprint)>>,
    /// The table naming each entry's atom, when walled.
    wall: Option<u8>,
    /// Entries an edit reached, with their answers before it.
    suspects: RefCell<Vec<(K, V)>>,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self { entries: RefCell::default(), wall: None, suspects: RefCell::default() }
    }
}

impl<K: Eq + Hash, V: Clone> Memo<K, V> {
    pub(super) fn get(&self, key: &K) -> Option<V> {
        let entries = self.entries.borrow();
        let (value, read) = entries.get(key)?;
        match self.wall {
            Some(table) if !read.is_none() => footprint::enter_atom(Self::atom(table, key)),
            Some(_) => {}
            None => footprint::enter(read),
        }
        Some(value.clone())
    }

    /// The entry's answer, its reading unrecorded.
    pub(super) fn peek(&self, key: &K) -> Option<V> {
        self.entries.borrow().get(key).map(|(value, _)| value.clone())
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

    /// The atom readers of `key`'s entry record in a memo walled by `table`.
    fn atom(table: u8, key: &K) -> Atom {
        use std::hash::BuildHasher;
        Atom::table(table, crate::fx_hash::FxBuild::default().hash_one(key) as u32)
    }

    /// The atom readers of `key`'s entry record (walled memos only).
    pub(super) fn entry_atom(&self, key: &K) -> Option<Atom> {
        self.wall.map(|table| Self::atom(table, key))
    }
}

impl<K, V> Memo<K, V> {
    /// A walled memo whose entries' atoms `table` names.
    pub(super) fn walled(table: u8) -> Self {
        Self { wall: Some(table), ..Self::default() }
    }

    /// Drops the entries `dirty`'s edit may change (a walled memo keeps
    /// them as suspects); the rest move to the edited text's positions.
    pub(super) fn forget(&self, dirty: &Dirty) where K: Eq + Hash {
        self.forget_unless(dirty, |_| true);
    }

    /// [`Memo::forget`], also dropping the values `keep` refuses.
    pub(super) fn forget_unless(&self, dirty: &Dirty, keep: impl Fn(&V) -> bool) where K: Eq + Hash {
        let mut entries = self.entries.borrow_mut();
        let mut suspects = self.suspects.borrow_mut();
        let walled = self.wall.is_some();
        for (key, (value, _)) in entries.extract_if(|_, (value, read)| {
            let stays = !read.meets(dirty) && keep(value);
            if stays { read.shift(dirty) }
            !stays
        }) {
            if walled && keep(&value) { suspects.push((key, value)) }
        }
    }

    /// The entries the last edits reached, with their answers before them.
    pub(super) fn take_suspects(&self) -> Vec<(K, V)> {
        std::mem::take(&mut self.suspects.borrow_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint::tables;

    #[test]
    fn walled_answers_hand_readers_only_themselves_and_are_rechecked_when_reached() {
        let memo: Memo<u32, u32> = Memo::walled(tables::MEMBER_KEYS);
        memo.insert_read(7, 70, Footprint::atom(Atom::whole(1)));
        let outer = footprint::begin();
        assert_eq!(memo.get(&7), Some(70));
        let read = footprint::end(outer);
        let mut edit = Dirty::default();
        edit.add_atom(Atom::whole(1));
        assert!(!read.meets(&edit));
        memo.forget(&edit);
        assert_eq!(memo.peek(&7), None);
        assert_eq!(memo.take_suspects(), vec![(7, 70)]);
        let mut changed = Dirty::default();
        changed.add_atom(memo.entry_atom(&7).unwrap());
        assert!(read.meets(&changed));
    }
}

/// A [`Memo`] walled by table `TABLE` from the start.
pub(super) struct Walled<K, V, const TABLE: u8>(Memo<K, V>);

impl<K, V, const TABLE: u8> Default for Walled<K, V, TABLE> {
    fn default() -> Self {
        Self(Memo::walled(TABLE))
    }
}

impl<K, V, const TABLE: u8> std::ops::Deref for Walled<K, V, TABLE> {
    type Target = Memo<K, V>;
    fn deref(&self) -> &Memo<K, V> {
        &self.0
    }
}

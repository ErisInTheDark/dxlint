//! Values built lately from equal inputs, kept for reuse: one process
//! (fixtures, tests, a language server's reloads, multi-config runs) builds
//! the same DFAs, declared parsers, library parses and configs again and
//! again. Every such cache is a [`Recent`], so they share one eviction rule.

use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::fx_hash::FxHashMap;

/// Up to a capacity of values by key. When full, the least recently used
/// quarter goes. Values are cheap to clone (`Arc`s and the like).
pub struct Recent<K, V> {
    capacity: AtomicUsize,
    entries: Mutex<Entries<K, V>>,
}

struct Entries<K, V> {
    map: FxHashMap<K, (V, u64)>,
    /// Bumped by every use; an entry's stamp is its last use.
    clock: u64,
}

impl<K: Hash + Eq, V: Clone> Recent<K, V> {
    pub const fn new(capacity: usize) -> Self {
        Recent {
            capacity: AtomicUsize::new(capacity),
            entries: Mutex::new(Entries { map: FxHashMap::with_hasher(std::hash::BuildHasherDefault::new()), clock: 0 }),
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let mut entries = self.entries.lock().expect("recent values");
        entries.clock += 1;
        let clock = entries.clock;
        entries.map.get_mut(key).map(|(value, used)| {
            *used = clock;
            value.clone()
        })
    }

    /// The value kept for `key`, else `make`'s (built without the lock held,
    /// so builds may nest), kept. When another thread kept one meanwhile,
    /// that one wins: everyone shares a single value per key.
    pub fn get_or_insert_with<E>(&self, key: K, make: impl FnOnce() -> Result<V, E>) -> Result<V, E> {
        if let Some(value) = self.get(&key) { return Ok(value) }
        let value = make()?;
        Ok(self.insert(key, value))
    }

    /// Keeps `value` for `key` unless a value is kept for it already; returns
    /// the kept one.
    pub fn insert(&self, key: K, value: V) -> V {
        let capacity = self.capacity.load(Ordering::Relaxed);
        if capacity == 0 { return value }
        let mut entries = self.entries.lock().expect("recent values");
        entries.clock += 1;
        let clock = entries.clock;
        if let Some((kept, used)) = entries.map.get_mut(&key) {
            *used = clock;
            return kept.clone();
        }
        let keep = capacity - capacity.div_ceil(4);
        if entries.map.len() >= capacity && keep == 0 {
            entries.map.clear();
        } else if entries.map.len() >= capacity {
            // Stamps are distinct: exactly `keep` entries stay.
            let mut stamps: Vec<u64> = entries.map.values().map(|(_, used)| *used).collect();
            let gone = stamps.len() - keep;
            let (_, &mut oldest_kept, _) = stamps.select_nth_unstable(gone);
            entries.map.retain(|_, (_, used)| *used >= oldest_kept);
        }
        entries.map.insert(key, (value.clone(), clock));
        value
    }

    /// Keeps at most `capacity` values from now on; 0 keeps none (a process
    /// building each value once).
    pub fn set_capacity(&self, capacity: usize) {
        self.capacity.store(capacity, Ordering::Relaxed);
        if capacity == 0 { self.clear() }
    }

    pub fn capacity(&self) -> usize { self.capacity.load(Ordering::Relaxed) }

    pub fn clear(&self) {
        self.entries.lock().expect("recent values").map.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.lock().expect("recent values").map.len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
}

#[cfg(test)]
mod tests {
    use super::Recent;

    #[test]
    fn full_caches_drop_the_least_recently_used_first() {
        let recent = Recent::new(4);
        for key in 0..4 { recent.insert(key, key * 10); }
        assert_eq!(recent.get(&0), Some(0));
        recent.insert(4, 40);
        assert_eq!(recent.get(&1), None, "least recently used goes");
        assert_eq!(recent.get(&0), Some(0), "a value used lately stays");
        assert_eq!(recent.len(), 4);
    }

    #[test]
    fn the_first_value_kept_for_a_key_wins() {
        let recent = Recent::new(2);
        assert_eq!(recent.insert("a", 1), 1);
        assert_eq!(recent.insert("a", 2), 1);
        assert_eq!(recent.get_or_insert_with::<()>("a", || Ok(3)), Ok(1));
        assert_eq!(recent.get_or_insert_with("b", || Err("failed")), Err("failed"));
        assert_eq!(recent.get(&"b"), None, "failed builds keep nothing");
    }

    #[test]
    fn capacity_zero_keeps_nothing() {
        let recent = Recent::new(4);
        recent.insert(1, 1);
        recent.set_capacity(0);
        assert!(recent.is_empty());
        assert_eq!(recent.insert(2, 2), 2);
        assert!(recent.is_empty());
    }
}

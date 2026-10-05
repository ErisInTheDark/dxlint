//! A fast, non-cryptographic hasher (rustc's Fx) for the per-file memo maps
//! keyed by node ids and short names, where SipHash dominated lookup time.
//! Not for keys an attacker chooses (zlint's keys come from its own trees).

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_le_bytes(chunk.try_into().expect("8 bytes")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut last = [0u8; 8];
            last[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(last));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub type FxBuild = BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = HashMap<K, V, FxBuild>;
pub type FxHashSet<K> = HashSet<K, FxBuild>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_keep_distinct_keys_apart() {
        let mut map: FxHashMap<(u32, u16), u32> = FxHashMap::default();
        for node in 0..1000u32 {
            for fact in 0..4u16 {
                map.insert((node, fact), node * 4 + fact as u32);
            }
        }
        assert_eq!(map.len(), 4000);
        assert_eq!(map[&(999, 3)], 3999);
        let names: FxHashSet<&str> = ["ts_call", "ts_callee", "ts_cal", ""].into_iter().collect();
        assert_eq!(names.len(), 4);
    }
}

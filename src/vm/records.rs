use super::{intern, interned, V};
use crate::grammar::Tree;
use crate::grammar::names::question::Known;
use std::cell::RefCell;

/// A node's record cell, as stored.
enum Cell {
    Unset,
    Busy,
    /// Evaluation failed here.
    GaveUp,
    Ready(V),
}

const UNSET: u64 = 0;
const BUSY: u64 = 1;
const NULL: u64 = 2;
const FALSE: u64 = 3;
const TRUE: u64 = 4;
const GAVE_UP: u64 = 5;

/// Numbers are stored as their bits inverted, so a zeroed buffer reads as
/// unset; every NaN is stored as the canonical one, keeping the small codes
/// above free.
/// Nodes, node texts and text constants: a tag in bits 32.. over a 32-bit
/// payload. Inverted, these codes are sign-set NaNs no number encodes to.
const NODE_TAG: u64 = 1 << 32;
const NODE_TEXT_TAG: u64 = 2 << 32;
const STR_TAG: u64 = 3 << 32;
const LIST_TAG: u64 = 4 << 32;
const RECORD_TAG: u64 = 5 << 32;
const FILE_TEXT_TAG: u64 = 6 << 32;

pub(super) fn encode(v: V) -> u64 {
    match v {
        V::Null => NULL,
        V::Bool(false) => FALSE,
        V::Bool(true) => TRUE,
        V::Num(n) => !(if n.is_nan() { f64::NAN } else { n }).to_bits(),
        V::Node(id) => NODE_TAG | u64::from(id),
        V::NodeText(id) => NODE_TEXT_TAG | u64::from(id),
        V::Str(s) => STR_TAG | u64::from(intern(s).1),
        V::List(handle) => LIST_TAG | u64::from(handle.bits()),
        V::Record(shape, handle) => RECORD_TAG | (u64::from(shape) << 35) | u64::from(handle.bits()),
        V::Text(handle) => FILE_TEXT_TAG | u64::from(handle),
    }
}

fn decode(cell: u64) -> Cell {
    let payload = cell as u32;
    match cell {
        UNSET => Cell::Unset,
        BUSY => Cell::Busy,
        NULL => Cell::Ready(V::Null),
        FALSE => Cell::Ready(V::Bool(false)),
        TRUE => Cell::Ready(V::Bool(true)),
        GAVE_UP => Cell::GaveUp,
        _ if cell >> 32 == NODE_TAG >> 32 => Cell::Ready(V::Node(payload)),
        _ if cell >> 32 == NODE_TEXT_TAG >> 32 => Cell::Ready(V::NodeText(payload)),
        _ if cell >> 32 == STR_TAG >> 32 => Cell::Ready(V::Str(interned(payload))),
        _ if cell >> 32 == LIST_TAG >> 32 => Cell::Ready(V::List(super::values::Handle::from_bits(payload))),
        _ if cell >> 32 == FILE_TEXT_TAG >> 32 => Cell::Ready(V::Text(payload)),
        _ if (cell >> 32) & 7 == RECORD_TAG >> 32 && cell >> 51 == 0 => {
            Cell::Ready(V::Record((cell >> 35) as u16, super::values::Handle::from_bits(payload)))
        }
        bits => Cell::Ready(V::Num(f64::from_bits(!bits))),
    }
}

pub(super) fn stored(cell: u64) -> V {
    match decode(cell) {
        Cell::Ready(value) => value,
        _ => unreachable!("arena payload must contain a value"),
    }
}

/// One file's VM-computed attribute values. Layouts group fields by their
/// first requesting rule root; only read blocks acquire cells.
#[derive(Default)]
pub struct FileVm {
    revision: std::cell::Cell<Option<u64>>,
    error: RefCell<Option<String>>,
    attr_errors: RefCell<crate::fx_hash::FxHashMap<(u32, u16), String>>,
    storage: RefCell<Storage>,
    pub(super) values: RefCell<super::values::Arena>,
    packed_facts: RefCell<crate::fx_hash::FxHashMap<FactPackKey, V>>,
    /// Retaining the Arc prevents its address being reused for another query result.
    shared_queries: RefCell<crate::fx_hash::FxHashMap<SharedValueKey, (crate::expr::Value<'static>, V)>>,
    kept: RefCell<crate::fx_hash::FxHashMap<(u32, u16), Option<V>>>,
    /// Buffers of finished rule runs ([`super::Session`]), reused.
    pub(crate) spare: RefCell<Vec<super::Spare>>,
    fact_packs: RefCell<crate::fx_hash::FxHashMap<FactPackKey, FactPackStats>>,
    /// Attributes being evaluated without a memo (inside semantic questions).
    uncached: RefCell<Vec<(u32, u16)>>,
}

// Whole-node reads use their target node. Projections and multi-field wrappers
// stay distinct, because their first-read payload sizes need not agree.
pub(crate) type FactPackKey = (u32, usize, Option<usize>, Option<u16>);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SharedValueKey {
    List(usize),
    Record(usize),
}

#[derive(Default)]
struct FactPackStats {
    calls: usize,
    bytes: usize,
    first_bytes: usize,
}

#[derive(Default)]
struct Storage {
    offsets: Vec<u32>,
    blocks: Pages<u32, 1024>,
    cells: Pages<u64, 512>,
}

/// Fixed-size pages keep allocation peaks independent of existing payload
/// size. Positions stay stable while either arena grows.
#[derive(Default)]
struct Pages<T, const N: usize> {
    pages: Vec<Box<[T; N]>>,
    len: usize,
}

impl<T: Copy, const N: usize> Pages<T, N> {
    fn allocate(&mut self, count: usize, empty: T) -> usize {
        let start = self.len;
        self.len += count;
        while self.pages.len() < self.len.div_ceil(N) {
            self.pages.push(Box::new([empty; N]));
        }
        start
    }

    fn get(&self, at: usize) -> T {
        self.pages[at / N][at % N]
    }

    fn set(&mut self, at: usize, value: T) {
        self.pages[at / N][at % N] = value;
    }

    fn iter(&self) -> impl Iterator<Item = &T> {
        self.pages.iter().flat_map(|page| page.iter()).take(self.len)
    }

    fn bytes(&self) -> usize {
        self.pages.len() * N * std::mem::size_of::<T>()
    }

    fn pointer_bytes(&self) -> usize {
        self.pages.capacity() * std::mem::size_of::<Box<[T; N]>>()
    }
}

const BLOCK: usize = 4;

/// A node without a directory, or a block without cells yet.
const NO_RECORD: u32 = u32::MAX;

/// What [`FileVm::begin`] found.
pub enum Begin {
    Ready(V),
    /// Busy, or failed before.
    GiveUp,
    /// Marked busy at this cell: evaluate, then [`FileVm::finish`].
    Compute(usize),
}

impl FileVm {
    pub(crate) fn sync_revision(&self, revision: u64, allow_reset: bool) -> bool {
        match self.revision.get() {
            Some(current) if current == revision => return true,
            Some(_) if !allow_reset => return false,
            Some(_) => {
                self.storage.replace(Storage::default());
                self.kept.borrow_mut().clear();
                self.shared_queries.borrow_mut().clear();
                self.attr_errors.borrow_mut().clear();
                self.clear_error();
            }
            None => {}
        }
        self.revision.set(Some(revision));
        true
    }

    pub(crate) fn fail(&self, message: String) {
        let mut error = self.error.borrow_mut();
        if error.is_none() { *error = Some(message); }
    }

    pub(crate) fn clear_error(&self) {
        self.error.borrow_mut().take();
    }

    pub(crate) fn error(&self) -> String {
        self.error.borrow().clone().unwrap_or_else(|| "expression evaluation failed".into())
    }

    pub(crate) fn attr_failure(&self, node: u32, attr: u16, name: &str) {
        let error = self.attr_errors.borrow().get(&(node, attr)).cloned()
            .unwrap_or_else(|| format!("attribute `{name}` depends on itself"));
        self.fail(error);
    }

    pub(crate) fn remember_attr_failure(&self, node: u32, attr: u16) {
        self.attr_errors.borrow_mut().insert((node, attr), self.error());
    }

    pub(crate) fn pack(&self, value: &crate::expr::Value, config: &super::Values, source: &str) -> Option<V> {
        self.packing(config.try_value_into(value, &mut self.values.borrow_mut(), super::values::Region::File, source))
    }

    pub(super) fn packing(&self, result: Result<V, super::values::PackError>) -> Option<V> {
        match result {
            Ok(value) => Some(value),
            Err(error) => { self.fail(error.to_string()); None }
        }
    }
    pub(crate) fn shared_query(&self, value: &crate::expr::Value<'static>, config: &super::Values, source: &str) -> Option<V> {
        let key = match value {
            crate::expr::Value::List(items) => SharedValueKey::List(items.as_ptr() as usize),
            crate::expr::Value::Record(entries) => SharedValueKey::Record(entries.as_ptr() as usize),
            _ => return self.pack(value, config, source),
        };
        if let Some((_, packed)) = self.shared_queries.borrow().get(&key) { return Some(*packed); }
        let packed = self.pack(value, config, source)?;
        self.shared_queries.borrow_mut().insert(key, (value.clone(), packed));
        Some(packed)
    }
    /// An attribute on a node kind without a record field for it: computed
    /// once per (node, attribute) by `compute` (persisted to the file arena).
    /// A read while it is being computed (a cycle) or after it failed is
    /// `None`.
    pub(crate) fn memo(&self, node: u32, attr: u16, compute: impl FnOnce() -> Known<Option<V>>) -> Option<V> {
        if let Some(&kept) = self.kept.borrow().get(&(node, attr)) { return kept; }
        self.kept.borrow_mut().insert((node, attr), None);
        match compute() {
            Known::Yes(kept) => {
                self.kept.borrow_mut().insert((node, attr), kept);
                kept
            }
            Known::Unknown => {
                self.kept.borrow_mut().remove(&(node, attr));
                None
            }
        }
    }

    /// Evaluates (node, attribute) without a memo, as attributes read inside
    /// semantic questions are; a read while it is in progress (a cycle) is
    /// `None`, like [`FileVm::memo`]'s.
    pub(crate) fn uncached(&self, node: u32, attr: u16, compute: impl FnOnce() -> Option<V>) -> Option<V> {
        if self.uncached.borrow().contains(&(node, attr)) { return None }
        self.uncached.borrow_mut().push((node, attr));
        let value = compute();
        self.uncached.borrow_mut().pop();
        value
    }

    pub(crate) fn fact_value(&self, value: &crate::expr::Value, config: &super::Values, source: &str, key: FactPackKey, measure: bool) -> Option<V> {
        if let Some(value) = self.packed_facts.borrow().get(&key) { return Some(*value); }
        let mut arena = self.values.borrow_mut();
        let before = measure.then(|| arena.used_bytes());
        let result = self.packing(config.try_fact_value(value, key.1, key.2.is_some(), key.3.is_some() && key.2.is_none(), &mut arena, source));
        if let Some(value @ (V::List(_) | V::Record(..) | V::Text(_))) = result {
            self.packed_facts.borrow_mut().insert(key, value);
        }
        if let Some(before) = before {
            let bytes = arena.used_bytes() - before;
            let mut packs = self.fact_packs.borrow_mut();
            let stats = packs.entry(key).or_default();
            if stats.calls == 0 { stats.first_bytes = bytes; }
            stats.calls += 1;
            stats.bytes += bytes;
        }
        result
    }

    pub(crate) fn gave_up_slots(&self, tree: &Tree, layout: &[u16]) -> Vec<(u32, u16)> {
        let storage = self.storage.borrow();
        let mut slots = Vec::new();
        for (node, &offset) in storage.offsets.iter().enumerate() {
            if offset == NO_RECORD { continue; }
            let width = layout[tree.node(node as u32).matcher as usize];
            for slot in 0..width {
                let block = storage.blocks.get(offset as usize + slot as usize / BLOCK);
                if block != NO_RECORD && storage.cells.get(block as usize + slot as usize % BLOCK) == GAVE_UP {
                    slots.push((node as u32, slot));
                }
            }
        }
        slots
    }

    pub(crate) fn report_stats(&self, tree: &Tree, layout: &[u16], file: &str) {
        let packs = self.fact_packs.borrow();
        for kind in ["full", "projection", "wrapper", "missing"] {
            let selected: Vec<_> = packs.iter().filter(|((node, _, projection, field), _)| match kind {
                "missing" => *node == u32::MAX,
                "wrapper" => *node != u32::MAX && field.is_some(),
                "projection" => *node != u32::MAX && field.is_none() && projection.is_some(),
                _ => *node != u32::MAX && field.is_none() && projection.is_none(),
            }).collect();
            let calls: usize = selected.iter().map(|(_, stats)| stats.calls).sum();
            let bytes: usize = selected.iter().map(|(_, stats)| stats.bytes).sum();
            let first_bytes: usize = selected.iter().map(|(_, stats)| stats.first_bytes).sum();
            eprintln!("zlint vm fact packs: {file} kind={kind} calls={calls} distinct={} bytes={bytes} distinct_bytes={first_bytes}", selected.len());
        }
        let storage = self.storage.borrow();
        let nodes = storage.offsets.iter().filter(|&&offset| offset != NO_RECORD).count();
        let unset = storage.cells.iter().filter(|&&cell| cell == UNSET).count();
        let gave_up = storage.cells.iter().filter(|&&cell| cell == GAVE_UP).count();
        let cell_bytes = storage.cells.bytes();
        let directory_bytes = storage.blocks.bytes();
        let pointer_bytes = storage.cells.pointer_bytes() + storage.blocks.pointer_bytes();
        let offset_bytes = storage.offsets.capacity() * std::mem::size_of::<u32>();
        let values_bytes = self.values.borrow().bytes();
        let texts_bytes = self.values.borrow().text_bytes();
        eprintln!("zlint vm memory: {file} cells_bytes={cell_bytes} offsets_bytes={offset_bytes} directory_bytes={directory_bytes} pointer_bytes={pointer_bytes} values_bytes={values_bytes} texts_bytes={texts_bytes} slots={} unset={unset} gave_up={gave_up} record_nodes={nodes}", storage.cells.len);
        for size in [4, 8, 16] {
            let mut directory = 0;
            let mut touched = 0;
            let mut occupied = 0;
            for (node, &offset) in storage.offsets.iter().enumerate() {
                if offset == NO_RECORD { continue; }
                let width = layout[tree.node(node as u32).matcher as usize] as usize;
                directory += width.div_ceil(size);
                for start in (0..width).step_by(size) {
                    let used = (start..(start + size).min(width)).filter(|&slot| {
                        let block = storage.blocks.get(offset as usize + slot / BLOCK);
                        block != NO_RECORD && storage.cells.get(block as usize + slot % BLOCK) != UNSET
                    }).count();
                    if used != 0 {
                        touched += 1;
                        occupied += used;
                    }
                }
            }
            let payload_bytes = touched * size * std::mem::size_of::<u64>();
            let directory_bytes = directory * std::mem::size_of::<u32>();
            let projected_bytes = payload_bytes + directory_bytes + offset_bytes;
            eprintln!("zlint vm blocks: {file} size={size} touched={touched} occupied={occupied} directory_entries={directory} payload_bytes={payload_bytes} directory_bytes={directory_bytes} projected_bytes={projected_bytes}");
        }
    }

    pub fn begin(&self, tree: &Tree, layout: &[u16], node: u32, slot: u16) -> Begin {
        let mut storage = self.storage.borrow_mut();
        if storage.offsets.is_empty() {
            storage.offsets = vec![NO_RECORD; tree.nodes.len()];
        }
        let mut directory = storage.offsets[node as usize];
        if directory == NO_RECORD {
            let width = layout[tree.node(node).matcher as usize] as usize;
            directory = storage.blocks.allocate(width.div_ceil(BLOCK), NO_RECORD) as u32;
            storage.offsets[node as usize] = directory;
        }
        let block_at = directory as usize + slot as usize / BLOCK;
        let mut block = storage.blocks.get(block_at);
        if block == NO_RECORD {
            block = storage.cells.allocate(BLOCK, UNSET) as u32;
            storage.blocks.set(block_at, block);
        }
        let at = block as usize + slot as usize % BLOCK;
        match decode(storage.cells.get(at)) {
            Cell::Ready(v) => Begin::Ready(v),
            Cell::Busy | Cell::GaveUp => Begin::GiveUp,
            Cell::Unset => {
                storage.cells.set(at, BUSY);
                Begin::Compute(at)
            }
        }
    }

    pub fn finish(&self, at: usize, answer: Known<Option<V>>) {
        let value = match answer {
            Known::Yes(value) => value.map_or(GAVE_UP, encode),
            Known::Unknown => UNSET,
        };
        self.storage.borrow_mut().cells.set(at, value);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_query_payloads_pack_once_without_merging_distinct_values() {
        use crate::expr::Value;
        let config = super::super::Values::default();
        let vm = super::FileVm::default();
        for value in [
            Value::list(vec![Value::Node(1, "".into()), Value::Node(2, "".into())]),
            Value::record(vec![("references".into(), Value::list(vec![Value::Node(3, "".into())]))]),
        ] {
            let first = vm.shared_query(&value, &config, "").unwrap();
            let bytes = vm.values.borrow().used_bytes();
            for _ in 0..10 {
                let alias = value.clone();
                assert_eq!(vm.shared_query(&alias, &config, ""), Some(first));
                assert_eq!(vm.values.borrow().used_bytes(), bytes);
            }
        }
        let a = Value::list(vec![Value::Node(4, "".into())]);
        let b = Value::list(vec![Value::Node(5, "".into())]);
        assert_ne!(vm.shared_query(&a, &config, ""), vm.shared_query(&b, &config, ""));
    }

    use super::*;
    use crate::grammar::Node;

    fn tree(kinds: impl IntoIterator<Item = u32>) -> Tree {
        Tree::from_native(kinds.into_iter().map(|matcher| Node { matcher, start: 0, end: 0, children: Vec::new() }).collect(), 0)
    }

    #[test]
    fn allocating_fact_queries_share_payload_without_caching_free_results() {
        use crate::expr::{facts, Value};
        let fact = facts::fact("list_layout").expect("list fact");
        let mut config = super::super::Values::default();
        config.register_fact(fact).expect("declared shape");
        let vm = FileVm::default();
        let keys = ["offset", "end", "comments", "prev", "next", "spaced", "broken", "kind", "brackets", "count", "single",
            "node_single", "first_broken", "empty_comments", "mode"];
        let value = Value::list(vec![Value::record(keys.iter().map(|&key| (key.into(), Value::from("owned text"))).collect::<Vec<_>>())]);
        let key = (7, fact, None, None);
        assert!(vm.fact_value(&value, &config, "", key, true).is_some());
        let bytes = vm.values.borrow().used_bytes();
        assert!(vm.fact_value(&value, &config, "", key, true).is_some());
        assert_eq!(vm.values.borrow().used_bytes(), bytes, "repeat must share its packed payload");
        let distinct = (8, fact, None, None);
        assert!(vm.fact_value(&value, &config, "", distinct, true).is_some());
        assert!(vm.values.borrow().used_bytes() > bytes, "different node is a separate query");
        let projected = (7, fact, Some(0), None);
        assert!(matches!(vm.fact_value(&Value::from("projected"), &config, "", projected, false), Some(V::Text(_))));
        let wrapper = (7, fact, None, Some(3));
        let wrapped = Value::list(vec![value.clone()]);
        assert!(vm.fact_value(&wrapped, &config, "", wrapper, false).is_some(), "field wrapper retains its own list shape");
        // A value the fact never declared still packs (as it comes), as its
        // own query rather than the first fact's result.
        let another = (7, facts::fact("implicit_coercion").expect("record fact"), None, None);
        let bytes = vm.values.borrow().used_bytes();
        assert!(vm.fact_value(&value, &config, "", another, false).is_some(), "an undeclared shape still packs");
        assert!(vm.values.borrow().used_bytes() > bytes, "different fact must not reuse the first result");
        let cached = vm.packed_facts.borrow().len();
        for value in [Value::Null, Value::Bool(true), Value::Num(3.0)] {
            let free = (20, fact, None, None);
            assert!(vm.fact_value(&value, &config, "", free, false).is_some());
        }
        assert_eq!(vm.packed_facts.borrow().len(), cached, "free results must not grow a second fact memo");
    }

    #[test]
    fn sparse_reads_do_not_allocate_a_whole_wide_record() {
        let tree = tree([0]);
        let vm = FileVm::default();
        let width = u16::MAX - 1;
        let Begin::Compute(at) = vm.begin(&tree, &[width], 0, width - 1) else { panic!("first read"); };
        vm.finish(at, Known::Yes(Some(V::Num(7.0))));
        let storage = vm.storage.borrow();
        let bytes = storage.cells.bytes() + storage.blocks.bytes() + storage.offsets.capacity() * 4
            + storage.cells.pointer_bytes() + storage.blocks.pointer_bytes();
        drop(storage);
        assert!(bytes < width as usize * 8 / 2, "sparse storage used {bytes} bytes");
        assert!(matches!(vm.begin(&tree, &[width], 0, width - 1), Begin::Ready(V::Num(7.0))));
    }

    #[test]
    fn pending_handles_and_fallbacks_survive_other_nodes_and_storage_growth() {
        let tree = tree((0..3000).map(|n| n % 2));
        let layout = [7, 19];
        let vm = FileVm::default();
        let Begin::Compute(pending) = vm.begin(&tree, &layout, 0, 6) else { panic!("first read"); };
        assert!(matches!(vm.begin(&tree, &layout, 0, 6), Begin::GiveUp));
        let Begin::Compute(gave_up) = vm.begin(&tree, &layout, 1, 18) else { panic!("first read"); };
        vm.finish(gave_up, Known::Yes(None));
        for node in 2..3000 {
            let slot = layout[node as usize % 2] - 1;
            let Begin::Compute(at) = vm.begin(&tree, &layout, node, slot) else { panic!("first read"); };
            vm.finish(at, Known::Yes(Some(V::Node(node))));
        }
        vm.finish(pending, Known::Yes(Some(V::Bool(false))));
        assert!(matches!(vm.begin(&tree, &layout, 0, 6), Begin::Ready(V::Bool(false))));
        assert!(matches!(vm.begin(&tree, &layout, 1, 18), Begin::GiveUp));
        for node in 2..3000 {
            let slot = layout[node as usize % 2] - 1;
            assert!(matches!(vm.begin(&tree, &layout, node, slot), Begin::Ready(V::Node(value)) if value == node));
        }
        let Begin::Compute(neighbour) = vm.begin(&tree, &layout, 0, 5) else { panic!("unread neighbour"); };
        vm.finish(neighbour, Known::Yes(Some(V::Null)));
        assert!(matches!(vm.begin(&tree, &layout, 0, 5), Begin::Ready(V::Null)));
    }

    #[test]
    fn stored_values_round_trip_and_stay_distinct_from_states() {
        for v in [
            V::Null, V::Bool(false), V::Bool(true), V::Num(0.0), V::Num(-0.0), V::Num(1.5), V::Num(f64::INFINITY),
            V::Num(f64::NEG_INFINITY), V::Num(-1e300), V::Node(0), V::Node(u32::MAX), V::NodeText(7), V::Str(intern("a").0), V::Str(intern("").0),
            V::List(super::super::values::Handle::from_bits(0x8000_0000)),
            V::Record(u16::MAX, super::super::values::Handle::from_bits(0xbfff_ffff)),
            V::Text(u32::MAX),
        ] {
            match (decode(encode(v)), v) {
                (Cell::Ready(V::Num(back)), V::Num(n)) => assert_eq!(back.to_bits(), n.to_bits(), "{v:?}"),
                (Cell::Ready(back), _) => assert_eq!(back, v),
                _ => panic!("{v:?} decoded as a state"),
            }
        }
        assert!(matches!(decode(encode(V::Num(f64::NAN))), Cell::Ready(V::Num(n)) if n.is_nan()));
        assert!(matches!(decode(UNSET), Cell::Unset));
        assert!(matches!(decode(BUSY), Cell::Busy));
        assert!(matches!(decode(GAVE_UP), Cell::GaveUp));
    }
}

use super::{intern, interned, V};
use crate::grammar::Tree;
use std::cell::RefCell;

/// A node's record cell, as stored.
enum Cell {
    Unset,
    Busy,
    /// The VM gave up here once; the interpreter owns this value.
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

fn encode(v: V) -> u64 {
    match v {
        V::Null => NULL,
        V::Bool(false) => FALSE,
        V::Bool(true) => TRUE,
        V::Num(n) => !(if n.is_nan() { f64::NAN } else { n }).to_bits(),
        V::Node(id) => NODE_TAG | u64::from(id),
        V::NodeText(id) => NODE_TEXT_TAG | u64::from(id),
        V::Str(s) => STR_TAG | u64::from(intern(s).1),
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
        bits => Cell::Ready(V::Num(f64::from_bits(!bits))),
    }
}

/// One file's VM-computed attribute values. Layouts group fields by their
/// first requesting rule root; only read blocks acquire cells.
#[derive(Default)]
pub struct FileVm {
    storage: RefCell<Storage>,
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
    /// Busy, or given up before: the interpreter handles it.
    GiveUp,
    /// Marked busy at this cell: evaluate, then [`FileVm::finish`].
    Compute(usize),
}

impl FileVm {
    pub(crate) fn report_stats(&self, tree: &Tree, layout: &[u16], file: &str) {
        let storage = self.storage.borrow();
        let nodes = storage.offsets.iter().filter(|&&offset| offset != NO_RECORD).count();
        let unset = storage.cells.iter().filter(|&&cell| cell == UNSET).count();
        let gave_up = storage.cells.iter().filter(|&&cell| cell == GAVE_UP).count();
        let cell_bytes = storage.cells.bytes();
        let directory_bytes = storage.blocks.bytes();
        let pointer_bytes = storage.cells.pointer_bytes() + storage.blocks.pointer_bytes();
        let offset_bytes = storage.offsets.capacity() * std::mem::size_of::<u32>();
        eprintln!("zlint vm memory: {file} cells_bytes={cell_bytes} offsets_bytes={offset_bytes} directory_bytes={directory_bytes} pointer_bytes={pointer_bytes} slots={} unset={unset} gave_up={gave_up} record_nodes={nodes}", storage.cells.len);
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

    /// Publishes a value, or marks the cell given up so later reads go
    /// straight to the interpreter.
    pub fn finish(&self, at: usize, value: Option<V>) {
        self.storage.borrow_mut().cells.set(at, value.map_or(GAVE_UP, encode));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Node;

    fn tree(kinds: impl IntoIterator<Item = u32>) -> Tree {
        Tree::from_native(kinds.into_iter().map(|matcher| Node { matcher, start: 0, end: 0, children: Vec::new() }).collect(), 0)
    }

    #[test]
    fn sparse_reads_do_not_allocate_a_whole_wide_record() {
        let tree = tree([0]);
        let vm = FileVm::default();
        let width = u16::MAX - 1;
        let Begin::Compute(at) = vm.begin(&tree, &[width], 0, width - 1) else { panic!("first read"); };
        vm.finish(at, Some(V::Num(7.0)));
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
        vm.finish(gave_up, None);
        for node in 2..3000 {
            let slot = layout[node as usize % 2] - 1;
            let Begin::Compute(at) = vm.begin(&tree, &layout, node, slot) else { panic!("first read"); };
            vm.finish(at, Some(V::Node(node)));
        }
        vm.finish(pending, Some(V::Bool(false)));
        assert!(matches!(vm.begin(&tree, &layout, 0, 6), Begin::Ready(V::Bool(false))));
        assert!(matches!(vm.begin(&tree, &layout, 1, 18), Begin::GiveUp));
        for node in 2..3000 {
            let slot = layout[node as usize % 2] - 1;
            assert!(matches!(vm.begin(&tree, &layout, node, slot), Begin::Ready(V::Node(value)) if value == node));
        }
        let Begin::Compute(neighbour) = vm.begin(&tree, &layout, 0, 5) else { panic!("unread neighbour"); };
        vm.finish(neighbour, Some(V::Null));
        assert!(matches!(vm.begin(&tree, &layout, 0, 5), Begin::Ready(V::Null)));
    }

    #[test]
    fn stored_values_round_trip_and_stay_distinct_from_states() {
        for v in [
            V::Null, V::Bool(false), V::Bool(true), V::Num(0.0), V::Num(-0.0), V::Num(1.5), V::Num(f64::INFINITY),
            V::Num(f64::NEG_INFINITY), V::Num(-1e300), V::Node(0), V::Node(u32::MAX), V::NodeText(7), V::Str(intern("a").0), V::Str(intern("").0),
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

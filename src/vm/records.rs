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

/// One file's VM-computed attribute values. Each node kind has a record
/// of exactly the attributes the enabled rules can read on it (sizes per
/// matcher: `layout`, decided at config load); a node gets its record in
/// one shared buffer when one of its values is first read.
#[derive(Default)]
pub struct FileVm {
    offsets: RefCell<Vec<u32>>,
    cells: RefCell<Vec<u64>>,
}

/// A node without a record yet.
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
    pub(crate) fn report_stats(&self, file: &str) {
        let offsets = self.offsets.borrow();
        let cells = self.cells.borrow();
        let nodes = offsets.iter().filter(|&&offset| offset != NO_RECORD).count();
        let unset = cells.iter().filter(|&&cell| cell == UNSET).count();
        let gave_up = cells.iter().filter(|&&cell| cell == GAVE_UP).count();
        let cell_bytes = cells.capacity() * std::mem::size_of::<u64>();
        let offset_bytes = offsets.capacity() * std::mem::size_of::<u32>();
        eprintln!("zlint vm memory: {file} cells_bytes={cell_bytes} offsets_bytes={offset_bytes} slots={} unset={unset} gave_up={gave_up} record_nodes={nodes}", cells.len());
    }

    fn record(&self, tree: &Tree, layout: &[u16], node: u32) -> usize {
        let mut offsets = self.offsets.borrow_mut();
        if offsets.is_empty() {
            *offsets = vec![NO_RECORD; tree.nodes.len()];
        }
        let at = &mut offsets[node as usize];
        if *at == NO_RECORD {
            let mut cells = self.cells.borrow_mut();
            let start = cells.len();
            *at = start as u32;
            let size = layout.get(tree.node(node).matcher as usize).copied().unwrap_or(0);
            cells.resize(start + size as usize, UNSET);
        }
        *at as usize
    }

    pub fn begin(&self, tree: &Tree, layout: &[u16], node: u32, slot: u16) -> Begin {
        let at = self.record(tree, layout, node) + slot as usize;
        let mut cells = self.cells.borrow_mut();
        match decode(cells[at]) {
            Cell::Ready(v) => Begin::Ready(v),
            Cell::Busy | Cell::GaveUp => Begin::GiveUp,
            Cell::Unset => {
                cells[at] = BUSY;
                Begin::Compute(at)
            }
        }
    }

    /// Publishes a value, or marks the cell given up so later reads go
    /// straight to the interpreter.
    pub fn finish(&self, at: usize, value: Option<V>) {
        self.cells.borrow_mut()[at] = value.map_or(GAVE_UP, encode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

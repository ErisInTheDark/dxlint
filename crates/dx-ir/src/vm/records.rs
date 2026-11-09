//! One-word encoding of [`V`] values into record cells.

use super::handle::Handle;
use super::{interned, V};

/// A node's record cell, as stored.
pub enum Cell {
    Unset,
    Busy,
    /// Evaluation failed here.
    GaveUp,
    Ready(V),
}

pub const UNSET: u64 = 0;
pub const BUSY: u64 = 1;
const NULL: u64 = 2;
const FALSE: u64 = 3;
const TRUE: u64 = 4;
pub const GAVE_UP: u64 = 5;

/// Numbers are stored as their bits inverted, so a zeroed buffer reads as
/// unset; every NaN is stored as the canonical one, keeping the small codes
/// above free.
/// Nodes, node texts and text constants: a tag in bits 32.. over a 32-bit
/// payload. Inverted, these codes are sign-set NaNs no number encodes to.
const NODE_TAG: u64 = 1 << 32;
const SOURCE_TAG: u64 = 2 << 32;
const STR_TAG: u64 = 3 << 32;
const LIST_TAG: u64 = 4 << 32;
const RECORD_TAG: u64 = 5 << 32;
const FILE_TEXT_TAG: u64 = 6 << 32;

pub fn encode(v: V) -> u64 {
    match v {
        V::Null => NULL,
        V::Bool(false) => FALSE,
        V::Bool(true) => TRUE,
        V::Num(n) => !(if n.is_nan() { f64::NAN } else { n }).to_bits(),
        V::Node(id) => NODE_TAG | u64::from(id),
        V::Source(id) => SOURCE_TAG | u64::from(id),
        V::Str(s) => STR_TAG | u64::from(s.index()),
        V::List(handle) => LIST_TAG | u64::from(handle.bits()),
        V::Record(shape, handle) => RECORD_TAG | (u64::from(shape) << 35) | u64::from(handle.bits()),
        V::Text(handle) => FILE_TEXT_TAG | u64::from(handle),
    }
}

/// The value [`encode`] gave `cell`.
pub(super) fn unpack(cell: u64) -> V {
    match decode(cell) {
        Cell::Ready(v) => v,
        Cell::Unset | Cell::Busy | Cell::GaveUp => unreachable!("encoded values are never cell states"),
    }
}

pub fn decode(cell: u64) -> Cell {
    let payload = cell as u32;
    match cell {
        UNSET => Cell::Unset,
        BUSY => Cell::Busy,
        NULL => Cell::Ready(V::Null),
        FALSE => Cell::Ready(V::Bool(false)),
        TRUE => Cell::Ready(V::Bool(true)),
        GAVE_UP => Cell::GaveUp,
        _ if cell >> 32 == NODE_TAG >> 32 => Cell::Ready(V::Node(payload)),
        _ if cell >> 32 == STR_TAG >> 32 => Cell::Ready(V::Str(interned(payload))),
        _ if cell >> 32 == LIST_TAG >> 32 => Cell::Ready(V::List(Handle::from_bits(payload))),
        _ if cell >> 32 == FILE_TEXT_TAG >> 32 => Cell::Ready(V::Text(payload)),
        _ if cell >> 32 == SOURCE_TAG >> 32 => Cell::Ready(V::Source(payload)),
        _ if (cell >> 32) & 7 == RECORD_TAG >> 32 && cell >> 51 == 0 => {
            Cell::Ready(V::Record((cell >> 35) as u16, Handle::from_bits(payload)))
        }
        bits => Cell::Ready(V::Num(f64::from_bits(!bits))),
    }
}

pub fn stored(cell: u64) -> V {
    match decode(cell) {
        Cell::Ready(value) => value,
        _ => unreachable!("arena payload must contain a value"),
    }
}

//! Packed composite value handles: config constants, evaluation scratch,
//! and per-file escapes. A handle never owns or borrows a payload.

/// A stored text's region: a config constant, evaluation scratch, or a file's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Config = 0,
    Scratch = 1,
    File = 2,
}

pub const CELL_OFFSET_MASK: u32 = (1 << 30) - 1;

/// The bit marking a [`crate::vm::V::Text`] handle as evaluation scratch, not file.
pub const SCRATCH_TEXT: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(u32);

impl Handle {
    pub fn bits(self) -> u32 { self.0 }
    pub fn from_bits(bits: u32) -> Self { Self(bits) }
    pub fn region(self) -> Region {
        match self.0 >> 30 {
            0 => Region::Config,
            1 => Region::Scratch,
            2 => Region::File,
            _ => unreachable!("invalid VM value region"),
        }
    }
    pub fn offset(self) -> usize { (self.0 & CELL_OFFSET_MASK) as usize }
    pub fn is_scratch(self) -> bool { self.region() == Region::Scratch }
}

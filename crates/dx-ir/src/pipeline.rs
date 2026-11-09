//! Item-stage values: a stage's kind, depth and compiled parts.

use crate::expr::{Each, CExpr};

#[derive(Clone, Debug)]
pub struct Stage {
    pub kind: Each,
    pub depth: u16,
    pub arity: u8,
    pub input: CExpr,
    pub args: Vec<CExpr>,
    pub body: CExpr,
}

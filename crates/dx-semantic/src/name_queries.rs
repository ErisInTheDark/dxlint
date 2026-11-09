//! Answers kept for the `name_*` queries, each with what working it out read
//! of the editor's file, handed to every later reader.

use std::cell::RefCell;

use dx_core::fx_hash::FxHashMap;
use dx_ir::value::Value;
use dx_tree::footprint::Footprint;

use super::question::{NodeRef, ScopeRef};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassKind {
    Scope,
    Declaration,
}

/// Answers kept for the lint, each with what working it out read of the
/// editor's file, handed to every later reader.
#[derive(Default)]
pub struct Memo {
    pub classes: RefCell<FxHashMap<(u32, ClassKind, u64), Value<'static>>>,
    pub scopes: Kept<ScopeRef>,
    pub ancestors: Kept<(ScopeRef, Option<u32>)>,
    pub bindings: Kept<NodeRef>,
    pub references: Kept<NodeRef>,
}

pub type Kept<K> = RefCell<FxHashMap<K, (Value<'static>, Footprint)>>;

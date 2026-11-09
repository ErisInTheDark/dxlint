//! What the runtime may ask a lint about its files: type questions, attribute
//! caching and name resolution. The engine's world implements it.

use std::sync::{Arc, OnceLock};

use crate::expr::Value;
use crate::names::question::{Handle, Known};
use crate::names::{Resolved, Value as NameValue};
use crate::reads::Cached;
use crate::vm::{FileVm, V};
use dx_ir::expr::{HandleId, QueryId, QuerySpec};
use dx_ir::ops::Quantifier;

/// The lint's files behind type questions and semantic attributes.
pub trait TypeWorld<'a> {
    fn query(&'a self, file: u32, node: u32, subject: &NameValue, quantifier: Quantifier, mask: u64) -> bool;
    fn query_function(&'a self, file: u32, node: u32, query: QueryId, args: &[Value<'static>]) -> Value<'static>;
    fn query_handle(&'a self, file: u32, handle: Handle, query: HandleId, args: &[Value<'static>]) -> Value<'static>;
    /// What a query function is, for displays and census labels.
    fn query_spec(&self, _query: QueryId) -> Option<&'static QuerySpec> { None }
    fn name_cache(&'a self, file: u32) -> Option<&'a OnceLock<Cached<Arc<Resolved>>>>;
    fn cache_attribute(&'a self, memo: &FileVm) -> bool;
    fn note_cycle(&'a self);
    fn semantic_attribute(&'a self, file: u32, node: u32, attr: u16, compute: &mut dyn FnMut() -> Option<V>) -> Option<V>;
    fn mark_unknown(&'a self);
    fn complete_value(&'a self, run: &mut dyn FnMut() -> Option<V>) -> Known<Option<V>>;
    fn complete_unit(&'a self, run: &mut dyn FnMut()) -> bool;
}
